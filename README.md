# Guestbook

A lightweight PHP and SQLite guestbook API served by Nginx and exposed through Cloudflare Tunnel. See it live at https://ianto-cannon.github.io/guestbook.html

## Features

- `GET /api.php`: retrieve the 100 most recent messages.
- `POST /api.php`: submit a message.
- Input validation, prepared SQL statements, and basic spam protection.

## Requirements

Debian, Nginx, PHP-FPM 8.4 with SQLite and `mbstring`, and optionally Cloudflare Tunnel.

## Deployment

### 1. Install dependencies

```bash
sudo apt update
sudo apt install nginx php8.4-fpm php8.4-sqlite3 php8.4-mbstring
```

### 2. Install the API

```bash
sudo install -d -o root -g root -m 755 /var/www/guestbook
sudo install -o root -g root -m 644 api.php /var/www/guestbook/api.php
```

### 3. Set up the database directory

PHP-FPM runs as `www-data` by default on Debian. The directory must be writable so SQLite can create the database and auxiliary files.

```bash
sudo install -d -o www-data -g www-data -m 750 /var/lib/guestbook
```

The API creates the database and table automatically.

### 4. Configure Nginx

Install and enable the configuration:

```bash
sudo install -o root -g root -m 644 guestbook.conf /etc/nginx/sites-available/guestbook
sudo ln -s /etc/nginx/sites-available/guestbook /etc/nginx/sites-enabled/guestbook
sudo nginx -t && sudo systemctl reload nginx
```

The configuration must use the correct hostname and PHP-FPM socket (`/run/php/php8.4-fpm.sock`).

### 5. Configure Cloudflare Tunnel (optional)

Route `guestbook.maricakes.de` to `http://127.0.0.1:80`. Ensure the Nginx `server_name` matches the hostname.

### 6. Test

```bash
php -l /var/www/guestbook/api.php
curl -i -H 'Host: guestbook.maricakes.de' http://127.0.0.1/api.php
```

Configure Cloudflare rate limiting for `POST /api.php` to reduce automated submissions.

## API

Submit a message using `POST /api.php` with `Content-Type: application/json`:

```json
{
  "name": "Alice",
  "message": "Hello, world!",
  "honeypot": ""
}
```

Names are limited to 50 characters and messages to 500 characters. The API rejects empty fields and messages containing more than one HTTP/HTTPS link.

