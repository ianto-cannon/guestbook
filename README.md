# Guestbook

A simple PHP guestbook API using SQLite, served by Nginx and exposed through Cloudflare Tunnel.

## Features

- Retrieve guestbook messages via `GET /api.php`
- Submit messages via `POST /api.php`
- SQLite storage
- Input validation and length limits
- Prepared SQL statements
- Honeypot field for basic bot detection
- Reject messages containing more than one HTTP/HTTPS link
- Limit message listings to the 100 most recent entries
- JSON error responses
- Restrict cross-origin browser access to the configured website

## API

### Retrieve messages

```http
GET /api.php
```

Returns up to 100 messages, newest first.

### Submit a message

```http
POST /api.php
Content-Type: application/json
```

Example request:

```json
{
  "name": "Alice",
  "message": "Hello, world!",
  "honeypot": ""
}
```

Successful response:

```json
{
  "ok": true
}
```

The name is limited to 50 characters and the message to 500 characters. Empty fields, invalid JSON, and messages containing more than one HTTP/HTTPS link are rejected.

## Requirements

- Linux server
- Nginx
- PHP-FPM with PDO SQLite and `mbstring`
- SQLite
- Cloudflare Tunnel (optional)

## Deployment

1. Place `api.php` in the web root.
2. Configure Nginx to pass `/api.php` to PHP-FPM.
3. Ensure the PHP-FPM user has appropriate access to `/var/lib/guestbook/guestbook.db` and its directory.
4. Configure Cloudflare Tunnel to route the guestbook hostname to the local Nginx service.
5. Configure rate limiting at Cloudflare to reduce automated submission abuse.

Keep the SQLite database, tunnel credentials, and private server configuration out of the repository.

## Security notes

- The API uses prepared statements for database writes.
- The API's CORS policy restricts browser access to the configured frontend origin. CORS is not authentication and does not prevent direct HTTP requests.
- The honeypot and link limit provide basic spam deterrence, not comprehensive bot protection.
- Configure rate limiting separately; application-level validation alone does not prevent request flooding.
- The frontend must safely render user-submitted content, for example using JavaScript `textContent` rather than `innerHTML`.
- Configure PHP to log errors privately rather than expose internal details to visitors.
