//! Guestbook API as a CGI program, storing one text file per message.
//!
//! File name:    YYYY-MM-DD_HH-MM-SS_Name.txt   (UTC; spaces in Name stored as '_')
//! File content: the message text
//!
//! Run per request by fcgiwrap. Request comes in via CGI environment
//! variables (REQUEST_METHOD, CONTENT_TYPE, CONTENT_LENGTH) and stdin; the
//! response goes to stdout. JSON shapes, status codes and limits follow the
//! PHP version, plus a stricter name rule and a 409 on collisions.

use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};

const MAX_BODY: usize = 8192;
const MAX_JSON_DEPTH: usize = 16;
const MAX_NAME_CHARS: usize = 50;
const MAX_MESSAGE_CHARS: usize = 500;
const SHOWN: usize = 100;
const DEFAULT_DIR: &str = "/var/lib/guestbook/messages";

/// (status line, extra header lines each ending in \r\n, body)
type Reply = (&'static str, &'static str, String);

fn fail(status: &'static str, msg: &str) -> Reply {
    (status, "", json!({ "error": msg }).to_string())
}

fn server_error(e: impl std::fmt::Display) -> Reply {
    eprintln!("Guestbook API storage or server error: {e}");
    fail("500 Internal Server Error", "Internal server error")
}

// ---------- small PHP-compat helpers ----------

/// PHP's trim() default character set.
fn php_trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'))
}

/// True if JSON nesting exceeds `max` (PHP: json_decode depth argument).
fn exceeds_depth(raw: &[u8], max: usize) -> bool {
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    for &b in raw {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > max {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// Equivalent of preg_match_all('~https?://~i', ...).
fn count_links(s: &str) -> usize {
    let l = s.to_ascii_lowercase();
    l.matches("http://").count() + l.matches("https://").count()
}

// ---------- names and file names ----------

/// Letters (any language), digits, space, hyphen and period; at least one
/// letter or digit. Everything else is rejected, so a name can never contain
/// a path separator, control character or leading-dot trick, and '_' is free
/// to stand for a space in file names.
fn name_ok(n: &str) -> bool {
    n.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '.'))
        && n.chars().any(char::is_alphanumeric)
}

/// "YYYY-MM-DD_HH-MM-SS" (UTC) for a Unix timestamp.
fn stamp_from_secs(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Days since 1970-01-01 -> civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// Split "YYYY-MM-DD_HH-MM-SS_Name.txt" into ("YYYY-MM-DD HH:MM:SS", "Name").
/// Anything that doesn't match (temp files, stray files) gives None.
fn parse_filename(f: &str) -> Option<(String, String)> {
    let stem = f.strip_suffix(".txt")?;
    let (stamp, rest) = (stem.get(..19)?, stem.get(19..)?);
    let name = rest.strip_prefix('_')?;
    let well_formed = stamp.bytes().enumerate().all(|(i, b)| match i {
        4 | 7 | 13 | 16 => b == b'-',
        10 => b == b'_',
        _ => b.is_ascii_digit(),
    });
    if !well_formed || name.is_empty() {
        return None;
    }
    let ts = format!("{} {}", &stamp[..10], stamp[11..].replace('-', ":"));
    Some((ts, name.replace('_', " ")))
}

// ---------- storage ----------

fn messages_dir() -> PathBuf {
    // GUESTBOOK_DIR can be set with `fastcgi_param` (handy for testing).
    PathBuf::from(env::var("GUESTBOOK_DIR").unwrap_or_else(|_| DEFAULT_DIR.into()))
}

fn newest_messages() -> io::Result<Vec<Value>> {
    let dir = messages_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found: Vec<(String, String, String)> = entries
        .filter_map(|e| {
            let file = e.ok()?.file_name().into_string().ok()?;
            let (ts, name) = parse_filename(&file)?;
            Some((file, ts, name))
        })
        .collect();
    // The timestamp comes first and is fixed-width, so name order = time order.
    found.sort_unstable_by(|a, b| b.0.cmp(&a.0));

    Ok(found
        .into_iter()
        .filter_map(|(file, ts, name)| {
            // A file deleted between listing and reading is simply skipped.
            let bytes = fs::read(dir.join(file)).ok()?;
            let message = String::from_utf8_lossy(&bytes);
            Some(json!({ "name": name, "message": message.trim_end(), "ts": ts }))
        })
        .take(SHOWN)
        .collect())
}

/// Write the message to a temp file, then publish it under its final name
/// with a hard link. Unlike rename, a hard link never replaces an existing
/// file, so a collision fails with AlreadyExists and nothing is overwritten.
/// Readers never see a half-written message, since only complete files get a
/// final name, and temp files don't match parse_filename().
fn store_message(name: &str, message: &str) -> io::Result<()> {
    let dir = messages_dir();
    fs::create_dir_all(&dir)?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let stamp = stamp_from_secs(now.as_secs());
    let target = dir.join(format!("{stamp}_{}.txt", name.replace(' ', "_")));
    let tmp = dir.join(format!(".tmp-{}-{}", std::process::id(), now.subsec_nanos()));

    let result = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(message.as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        fs::hard_link(&tmp, &target)
    })();
    let _ = fs::remove_file(&tmp);
    result
}

// ---------- handlers ----------

fn list() -> Reply {
    match newest_messages() {
        Ok(rows) => ("200 OK", "", Value::Array(rows).to_string()),
        Err(e) => server_error(e),
    }
}

fn create() -> Reply {
    let bad = "400 Bad Request";

    // 1. Reject oversized request bodies.
    let len: usize = env::var("CONTENT_LENGTH")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        return fail("413 Payload Too Large", "Request too large");
    }
    let mut raw = Vec::with_capacity(len);
    if io::stdin().take(len as u64).read_to_end(&mut raw).is_err() {
        return fail("413 Payload Too Large", "Request too large");
    }

    // 2. Require JSON.
    let content_type = env::var("CONTENT_TYPE").unwrap_or_default();
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if content_type != "application/json" {
        return fail(
            "415 Unsupported Media Type",
            "Content-Type must be application/json",
        );
    }

    // 3. Parse (depth-limited).
    if exceeds_depth(&raw, MAX_JSON_DEPTH) {
        return fail(bad, "Invalid JSON");
    }
    let input: Value = match serde_json::from_slice(&raw) {
        Ok(v @ (Value::Object(_) | Value::Array(_))) => v,
        _ => return fail(bad, "Invalid JSON"),
    };

    // 4. Honeypot: absent/null is fine, non-string is rejected, non-empty means bot.
    let honeypot = match input.get("honeypot") {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return fail(bad, "Invalid fields"),
    };
    if !php_trim(honeypot).is_empty() {
        return fail(bad, "Bot detected");
    }

    // 5. Field validation.
    let (name, message) = match (input.get("name"), input.get("message")) {
        (Some(Value::String(n)), Some(Value::String(m))) => (php_trim(n), php_trim(m)),
        _ => return fail(bad, "Invalid fields"),
    };
    if name.is_empty() || message.is_empty() {
        return fail(bad, "Name and message are required");
    }
    if name.chars().count() > MAX_NAME_CHARS || message.chars().count() > MAX_MESSAGE_CHARS {
        return fail(bad, "Name or message is too long");
    }
    if !name_ok(name) {
        return fail(
            bad,
            "Name may only contain letters, numbers, spaces, hyphens and periods",
        );
    }

    // Simple spam deterrent, not a security boundary.
    if count_links(message) > 1 {
        return fail(bad, "Too many links in message");
    }

    // 6. Store. A same-name post within the same second is refused, never overwritten.
    match store_message(name, message) {
        Ok(()) => ("200 OK", "", json!({ "ok": true }).to_string()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => fail(
            "409 Conflict",
            "A message from this name was just posted, please try again in a moment",
        ),
        Err(e) => server_error(e),
    }
}

fn handle() -> Reply {
    match env::var("REQUEST_METHOD").unwrap_or_default().as_str() {
        "OPTIONS" => ("204 No Content", "", String::new()),
        "GET" => list(),
        "POST" => create(),
        _ => (
            "405 Method Not Allowed",
            "Allow: GET, POST, OPTIONS\r\n",
            json!({ "error": "Method not allowed" }).to_string(),
        ),
    }
}

fn main() {
    let (status, extra, body) = handle();
    let out = format!(
        "Status: {status}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Access-Control-Allow-Origin: https://ianto-cannon.github.io\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Cache-Control: no-store\r\n\
         {extra}\r\n{body}"
    );
    let mut stdout = io::stdout();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_match_known_dates() {
        assert_eq!(stamp_from_secs(0), "1970-01-01_00-00-00");
        assert_eq!(stamp_from_secs(951_782_400), "2000-02-29_00-00-00");
        assert_eq!(stamp_from_secs(1_000_000_000), "2001-09-09_01-46-40");
        assert_eq!(stamp_from_secs(1_709_164_800), "2024-02-29_00-00-00");
        assert_eq!(stamp_from_secs(4_102_444_799), "2099-12-31_23-59-59");
    }

    #[test]
    fn name_rules() {
        for ok in ["Ianto", "Mary Ann", "Zoë Müller", "J. R.", "a-b", "Åsa 2"] {
            assert!(name_ok(ok), "{ok}");
        }
        for bad in ["", "../x", "a/b", "a\\b", "a_b", "x\ny", "---", "...", "a:b", "<b>", "a\0b"] {
            assert!(!name_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn filename_round_trip() {
        let f = format!("{}_{}.txt", stamp_from_secs(1_000_000_000), "Mary Ann".replace(' ', "_"));
        assert_eq!(f, "2001-09-09_01-46-40_Mary_Ann.txt");
        assert_eq!(
            parse_filename(&f),
            Some(("2001-09-09 01:46:40".into(), "Mary Ann".into()))
        );
        for junk in [".tmp-1-2", "notes.txt", "2001-09-09_01-46-40_.txt", "2001-09-09_01-46-40.txt", "x.txt"] {
            assert_eq!(parse_filename(junk), None, "{junk}");
        }
    }
}
