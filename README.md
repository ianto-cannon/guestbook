# Guestbook

A small Rust guestbook API. Nginx runs it as a CGI program through fcgiwrap, and each message is saved as a text file.

Live at [https://ianto-cannon.github.io/guestbook.html](https://ianto-cannon.github.io/guestbook.html).

## API

- `GET /api` returns the 100 newest messages as `[{"name", "message", "ts"}]`.
- `POST /api` with `{"name": "...", "message": "..."}` adds one.

Names: up to 50 characters, letters, numbers, spaces, `-` and `.`. Messages: up to 500 characters, at most one link. Errors are JSON: `{"error": "..."}`.

## Install

```bash
sudo apt install nginx fcgiwrap cargo
cargo build --release
sudo install -d /usr/local/lib/guestbook
sudo install target/release/guestbook-api /usr/local/lib/guestbook/api.cgi
sudo install -d -o www-data -g www-data -m 750 /var/lib/guestbook/messages
```

Add to your Nginx server block, and define the rate limit zone
`limit_req_zone $binary_remote_addr zone=guestbook:10m rate=2r/s;` in the `http` context:

```nginx
location = /api {
    limit_req zone=guestbook burst=5 nodelay;
    client_max_body_size 16k;
    include fastcgi_params;
    fastcgi_param SCRIPT_FILENAME /usr/local/lib/guestbook/api.cgi;
    fastcgi_pass unix:/run/fcgiwrap.socket;
}
```

Then `sudo nginx -t && sudo systemctl reload nginx`.

## Notes

- Messages live in `/var/lib/guestbook/messages` as `YYYY-MM-DD_HH-MM-SS_Name.txt`. Delete a file to delete a message.
- Set the allowed CORS origin in `src/main.rs` before using your own frontend.
