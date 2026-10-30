//! Drop-in Rust replacement for /var/www/guestbook/api.php
//!
//! Same table (`messages`), same JSON shapes, same status codes, same limits.
//! The handler is path-agnostic: nginx decides which URL reaches it.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, Method, StatusCode},
    response::Response,
    Router,
};
use rusqlite::{types::Type, Connection};
use serde_json::{json, Value};

const ALLOWED_ORIGIN: &str = "https://ianto-cannon.github.io";
const MAX_BODY: usize = 8192;
const MAX_JSON_DEPTH: usize = 16;
const MAX_NAME_CHARS: usize = 50;
const MAX_MESSAGE_CHARS: usize = 500;

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    insert_sql: Arc<String>,
}

// ---------- response helpers (CORS + no-store on every response) ----------

fn builder(status: StatusCode) -> axum::http::response::Builder {
    axum::http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, ALLOWED_ORIGIN)
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(header::CACHE_CONTROL, "no-store")
}

fn reply(status: StatusCode, value: Value) -> Response {
    builder(status)
        .body(Body::from(value.to_string()))
        .expect("valid response")
}

fn fail(status: StatusCode, msg: &str) -> Response {
    reply(status, json!({ "error": msg }))
}

fn server_error(e: impl std::fmt::Display) -> Response {
    eprintln!("Guestbook API database or server error: {e}");
    fail(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
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
    l.match_indices("http")
        .filter(|(i, _)| {
            let rest = &l[i + 4..];
            rest.starts_with("://") || rest.starts_with("s://")
        })
        .count()
}

/// Read a TEXT column, substituting invalid UTF-8 (like JSON_INVALID_UTF8_SUBSTITUTE).
fn text(r: &rusqlite::Row, i: usize) -> rusqlite::Result<String> {
    let bytes = r
        .get_ref(i)?
        .as_bytes()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, Type::Text, Box::new(e)))?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

// ---------- handlers ----------

async fn handle(State(state): State<AppState>, req: Request) -> Response {
    let method = req.method().clone();

    if method == Method::OPTIONS {
        return builder(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .expect("valid response");
    }
    if method == Method::GET {
        return list(state).await;
    }
    if method == Method::POST {
        return create(state, req).await;
    }

    let mut resp = fail(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
    resp.headers_mut()
        .insert(header::ALLOW, "GET, POST, OPTIONS".parse().unwrap());
    resp
}

async fn list(state: AppState) -> Response {
    let res = tokio::task::spawn_blocking(move || -> rusqlite::Result<Vec<Value>> {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = db.prepare(
            "SELECT name, message, ts FROM messages ORDER BY id DESC LIMIT 1000",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(json!({
                "name": text(r, 0)?,
                "message": text(r, 1)?,
                "ts": text(r, 2)?,
            }))
        })?;
        rows.collect()
    })
    .await;

    match res {
        Ok(Ok(rows)) => reply(StatusCode::OK, Value::Array(rows)),
        Ok(Err(e)) => server_error(e),
        Err(e) => server_error(e),
    }
}

async fn create(state: AppState, req: Request) -> Response {
    let bad = StatusCode::BAD_REQUEST;

    // Grab the content type before the body is consumed.
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    // 1. Reject oversized request bodies.
    let raw = match to_bytes(req.into_body(), MAX_BODY + 1).await {
        Ok(b) if b.len() <= MAX_BODY => b,
        _ => return fail(StatusCode::PAYLOAD_TOO_LARGE, "Request too large"),
    };

    // 2. Require JSON.
    if content_type != "application/json" {
        return fail(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
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

    // Simple spam deterrent, not a security boundary.
    if count_links(message) > 1 {
        return fail(bad, "Too many links in message");
    }

    // 6. Insert.
    let (name, message) = (name.to_owned(), message.to_owned());
    let res = tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.execute(state.insert_sql.as_str(), rusqlite::params![name, message])?;
        Ok(())
    })
    .await;

    match res {
        Ok(Ok(())) => reply(StatusCode::OK, json!({ "ok": true })),
        Ok(Err(e)) => server_error(e),
        Err(e) => server_error(e),
    }
}

// ---------- startup ----------

#[tokio::main]
async fn main() {
    let db_path = std::env::var("GUESTBOOK_DB")
        .unwrap_or_else(|_| "/var/lib/guestbook/guestbook.db".into());
    let conn = Connection::open(&db_path)
        .unwrap_or_else(|e| panic!("failed to open database {db_path}: {e}"));
    conn.busy_timeout(Duration::from_millis(3000))
        .expect("failed to set busy_timeout");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            message TEXT NOT NULL,
            ts TEXT NOT NULL
        )",
    )
    .expect("failed to create schema");

    // PHP's date() used the PHP timezone; SQLite defaults to UTC.
    // Set GUESTBOOK_LOCALTIME=1 to use the system's local timezone instead.
    let ts_args = if std::env::var("GUESTBOOK_LOCALTIME").as_deref() == Ok("1") {
        "'now', 'localtime'"
    } else {
        "'now'"
    };
    let insert_sql = format!(
        "INSERT INTO messages (name, message, ts) \
         VALUES (?1, ?2, strftime('%Y-%m-%d %H:%M:%S', {ts_args}))"
    );

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        insert_sql: Arc::new(insert_sql),
    };

    let app = Router::new().fallback(handle).with_state(state);

    let addr = std::env::var("GUESTBOOK_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!("guestbook listening on http://{addr}");
    axum::serve(listener, app).await.expect("server error");
}
