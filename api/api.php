<?php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: https://ianto-cannon.github.io");
//header("Access-Control-Allow-Origin: *");
header("Access-Control-Allow-Methods: GET, POST, OPTIONS");
header("Access-Control-Allow-Headers: Content-Type");
if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    http_response_code(204);
    exit;
}
$db = new PDO('sqlite:/var/lib/guestbook/guestbook.db');
$db->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
$db->exec("
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    message TEXT NOT NULL,
    ts TEXT NOT NULL
)
");
if ($_SERVER['REQUEST_METHOD'] === 'GET') {
    $stmt = $db->query("SELECT name, message, ts FROM messages ORDER BY id DESC");
    echo json_encode($stmt->fetchAll(PDO::FETCH_ASSOC));
    exit;
}
if ($_SERVER['REQUEST_METHOD'] === 'POST') {
    $last = $db->query("SELECT ts FROM messages ORDER BY id DESC LIMIT 1")
               ->fetchColumn();
    if ($last && time() - strtotime($last) < 10) {
        http_response_code(429);
        echo json_encode(["error" => "Please wait a few seconds"]);
        exit;
    }
    $input = json_decode(file_get_contents("php://input"), true);
    $honeypot = trim($input["honeypot"] ?? "");
    if ($honeypot !== "") {
        http_response_code(400);
        echo json_encode(["error" => "Bot detected"]);
        exit;
    }
    $name = trim($input["name"] ?? "");
    $message = trim($input["message"] ?? "");
    if ($name === "" || $message === "") {
        http_response_code(400);
        echo json_encode(["error" => "Missing name or message"]);
        exit;
    }
    $name = substr($name, 0, 50);
    $message = substr($message, 0, 500);
    // 1. Block if the message contains more than 1 link
    if (substr_count($message, 'http') > 1) {
      http_response_code(400);
      echo json_encode(["error" => "Too many links in message."]);
      exit;
    }
    $ts = date("Y-m-d H:i:s");
    $stmt = $db->prepare("INSERT INTO messages (name, message, ts) VALUES (?, ?, ?)");
    $stmt->execute([$name, $message, $ts]);
    echo json_encode(["ok" => true]);
    exit;
}
http_response_code(405);
echo json_encode(["error" => "Method not allowed"]);

