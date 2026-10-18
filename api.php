<?php
declare(strict_types=1);

header("Content-Type: application/json; charset=utf-8");
header("Access-Control-Allow-Origin: https://your-username.github.io");
header("Access-Control-Allow-Methods: GET, POST, OPTIONS");
header("Access-Control-Allow-Headers: Content-Type");
header("Cache-Control: no-store");

function respond(int $status, array $data): never {
    http_response_code($status);
    echo json_encode($data, JSON_INVALID_UTF8_SUBSTITUTE);
    exit;
}

$method = $_SERVER["REQUEST_METHOD"] ?? "";

if ($method === "OPTIONS") {
    http_response_code(204);
    exit;
}

if (!in_array($method, ["GET", "POST"], true)) {
    header("Allow: GET, POST, OPTIONS");
    respond(405, ["error" => "Method not allowed"]);
}

try {
    $db = new PDO("sqlite:/var/lib/guestbook/guestbook.db");
    $db->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
    $db->exec("PRAGMA busy_timeout = 3000");

    $db->exec("
        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            message TEXT NOT NULL,
            ts TEXT NOT NULL
        )
    ");

    if ($method === "GET") {
        $stmt = $db->query(
            "SELECT name, message, ts
             FROM messages ORDER BY id DESC LIMIT 1000"
        );
        echo json_encode(
            $stmt->fetchAll(PDO::FETCH_ASSOC),
            JSON_INVALID_UTF8_SUBSTITUTE
        );
        exit;
    }

    // Reject oversized request bodies.
    $raw = file_get_contents("php://input");

    if ($raw === false || strlen($raw) > 8192) {
        respond(413, ["error" => "Request too large"]);
    }

    // Require JSON.
    $contentType = strtolower(trim(
        explode(";", $_SERVER["CONTENT_TYPE"] ?? "")[0]
    ));

    if ($contentType !== "application/json") {
        respond(415, ["error" => "Content-Type must be application/json"]);
    }

    try {
        $input = json_decode($raw, true, 16, JSON_THROW_ON_ERROR);
    } catch (JsonException $e) {
        respond(400, ["error" => "Invalid JSON"]);
    }

    if (!is_array($input)) {
        respond(400, ["error" => "Invalid JSON"]);
    }

    // Honeypot for simple automated submissions.
    if (!is_string($input["honeypot"] ?? "")) {
        respond(400, ["error" => "Invalid fields"]);
    }

    if (trim($input["honeypot"] ?? "") !== "") {
        respond(400, ["error" => "Bot detected"]);
    }

    $name = $input["name"] ?? null;
    $message = $input["message"] ?? null;

    if (!is_string($name) || !is_string($message)) {
        respond(400, ["error" => "Invalid fields"]);
    }

    $name = trim($name);
    $message = trim($message);

    if ($name === "" || $message === "") {
        respond(400, ["error" => "Name and message are required"]);
    }

    if (!mb_check_encoding($name, "UTF-8") ||
        !mb_check_encoding($message, "UTF-8")) {
        respond(400, ["error" => "Invalid text encoding"]);
    }

    if (mb_strlen($name) > 50 || mb_strlen($message) > 500) {
        respond(400, ["error" => "Name or message is too long"]);
    }

    // Simple spam deterrent, not a security boundary.
    if (preg_match_all('~https?://~i', $message) > 1) {
        respond(400, ["error" => "Too many links in message"]);
    }

    $stmt = $db->prepare(
        "INSERT INTO messages (name, message, ts)
         VALUES (?, ?, ?)"
    );
    $stmt->execute([
        $name,
        $message,
        date("Y-m-d H:i:s")
    ]);

    respond(200, ["ok" => true]);

} catch (Throwable $e) {
    error_log("Guestbook API database or server error: " . $e->getMessage());
    respond(500, ["error" => "Internal server error"]);
}
