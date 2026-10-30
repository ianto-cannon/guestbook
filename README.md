# Guestbook

A lightweight Rust and SQLite guestbook API served by Nginx. The API runs as a systemd service and can be exposed through Cloudflare Tunnel or another reverse proxy.

See it live at [https://ianto-cannon.github.io/guestbook.html](https://ianto-cannon.github.io/guestbook.html).

## Features

- `GET /api`: retrieve the 1000 most recent messages.
- `POST /api`: submit a message.
- SQLite storage with prepared SQL statements.
- Input validation and basic spam protection.
- Request size and JSON nesting limits.
- CORS support and JSON responses.
- Runs as an unprivileged systemd service.

## Requirements

- Debian or another Linux distribution.
- Rust toolchain and Cargo.
- Nginx.
- SQLite development libraries, if required by the selected Rust SQLite build.
- Optional: Cloudflare Tunnel.

## Deployment

The following instructions assume the API listens on `127.0.0.1:3001` and uses `/var/lib/guestbook/guestbook.db`.

### 1. Install dependencies

```bash
sudo apt update
sudo apt install nginx build-essential pkg-config libsqlite3-dev
```

Install Rust and Cargo using [rustup](https://rustup.rs/), if they are not already installed.

### 2. Build and install the API

Clone the repository and build the release binary:

```bash
git clone https://github.com/yourusername/guestbook-rust.git
cd guestbook-rust

cargo build --release
```

Install the binary:

```bash
sudo install -o root -g root -m 755 \
    target/release/guestbook-api /usr/local/bin/guestbook-api
```

### 3. Set up the database directory

The API runs as `www-data`. Give it permission to create and write the SQLite database:

```bash
sudo install -d -o www-data -g www-data -m 750 /var/lib/guestbook
```

The API creates the database and table automatically.

### 4. Configure systemd

Copy the example service file:

```bash
sudo install -o root -g root -m 644 \
    guestbook.service /etc/systemd/system/guestbook.service
```

The service configuration should include:

```ini
[Unit]
Description=Guestbook API (Rust)
After=network.target

[Service]
User=www-data
Group=www-data
ExecStart=/usr/local/bin/guestbook-api
Environment=GUESTBOOK_DB=/var/lib/guestbook/guestbook.db
Environment=GUESTBOOK_ADDR=127.0.0.1:3001
Restart=on-failure
RestartSec=2
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/var/lib/guestbook

[Install]
WantedBy=multi-user.target
```

Enable and start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now guestbook
```

Check its status and logs:

```bash
sudo systemctl status guestbook
sudo journalctl -u guestbook -f
```

### 5. Configure Nginx

Create an Nginx server configuration for your hostname:

```nginx
server {
    listen 127.0.0.1:80;
    server_name guestbook.yoururl.com;

    location = /api {
        limit_req zone=guestbook burst=5 nodelay;
        limit_req_status 429;

        proxy_pass http://127.0.0.1:3001;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

Define the rate-limit zone in the Nginx `http` context:

```nginx
limit_req_zone $binary_remote_addr zone=guestbook:10m rate=2r/s;
```

Install and enable the configuration:

```bash
sudo install -o root -g root -m 644 guestbook.conf \
    /etc/nginx/sites-available/guestbook

sudo ln -s /etc/nginx/sites-available/guestbook \
    /etc/nginx/sites-enabled/guestbook

sudo nginx -t && sudo systemctl reload nginx
```

Adjust the listening address, hostname and rate limits to suit your setup. If Nginx is directly exposed to the internet, configure its public-facing listener and TLS accordingly.

### 6. Configure Cloudflare Tunnel (optional)

Route `guestbook.yoururl.com` to the local Nginx listener:

```text
http://127.0.0.1:80
```

Ensure the Nginx `server_name` matches the hostname.

### 7. Test

Check the API locally through Nginx:

```bash
curl -i -H 'Host: guestbook.yoururl.com' \
    http://127.0.0.1/api
```

Submit a test message:

```bash
curl -i -X POST \
    -H 'Host: guestbook.yoururl.com' \
    -H 'Content-Type: application/json' \
    -d '{"name":"Alice","message":"Hello, world!","honeypot":""}' \
    http://127.0.0.1/api
```

## API

### Retrieve messages

`GET /api` returns the 1000 most recent messages.

### Submit a message

Send a JSON object using `POST /api`:

```json
{
  "name": "Alice",
  "message": "Hello, world!",
  "honeypot": ""
}
```

Names are limited to 50 characters and messages to 500 characters. Empty fields and messages containing more than one HTTP/HTTPS link are rejected.

## Configuration

The API supports these environment variables:

| Variable | Default |
|---|---|
| `GUESTBOOK_DB` | `guestbook.db` |
| `GUESTBOOK_ADDR` | `127.0.0.1:8080` |

The systemd service overrides both defaults for deployment.

The allowed CORS origin is configured in the Rust source. Change it to match your frontend before deploying your own instance.

## Security

- Keep the Rust API bound to loopback rather than exposing its port directly.
- Run the service as an unprivileged user.
- Keep the database outside the web root.
- Use Nginx for reverse proxying and request rate limiting.
- Do not commit the live SQLite database, backups, logs or secrets to version control.
