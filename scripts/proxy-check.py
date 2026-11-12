#!/usr/bin/env python3
"""反向代理（S-16）的驗證用工具，由 scripts/verify-proxy.sh 呼叫。只用標準函式庫。

  proxy-check.py backend                  假後端：在容器內聽 3000，扮演 compose 的 `app`
  proxy-check.py check up   <base-url>    後端在線時，經代理的行為
  proxy-check.py check down <base-url>    後端停掉時，經代理的行為

假後端只回合成資料；驗證不碰真的資料庫或 AI。
"""

import base64
import hashlib
import http.client
import json
import socket
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
SSE_EVENTS = 4
SSE_INTERVAL = 1.0


# ---------------------------------------------------------------- 假後端
class Backend(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):  # 安靜
        pass

    def _json(self, status, body):
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.send_header("X-Backend", "fake")
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/health":
            self._json(200, {"status": "ok"})
        elif path == "/echo":
            self._json(
                200,
                {
                    "host": self.headers.get("Host"),
                    "xff": self.headers.get("X-Forwarded-For"),
                    "proto": self.headers.get("X-Forwarded-Proto"),
                    "peer": self.client_address[0],
                },
            )
        elif path == "/maintenance":
            # 後端自己的 503（維護模式，S-15）：代理必須原樣轉發
            self._json(503, {"error": {"code": "maintenance", "request_id": "r1"}})
        elif path == "/sse":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            for i in range(1, SSE_EVENTS + 1):
                data = f"data: {i}\n\n".encode()
                self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
                self.wfile.flush()
                time.sleep(SSE_INTERVAL)
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
        elif path.startswith("/api/conversations/") and path.endswith("/voice"):
            self._websocket()
        else:
            self._json(404, {"error": {"code": "not_found", "request_id": "r1"}})

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length)
        self._json(200, {"received": len(body)})

    def _websocket(self):
        key = self.headers.get("Sec-WebSocket-Key")
        if (self.headers.get("Upgrade") or "").lower() != "websocket" or not key:
            self._json(400, {"error": {"code": "bad_request", "request_id": "r1"}})
            return
        accept = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
        self.wfile.write(
            (
                "HTTP/1.1 101 Switching Protocols\r\n"
                "Upgrade: websocket\r\nConnection: Upgrade\r\n"
                f"Sec-WebSocket-Accept: {accept}\r\n\r\n"
            ).encode()
        )
        self.wfile.write(b"\x81\x05hello")  # 一個未遮罩的文字框
        self.wfile.flush()
        self.close_connection = True
        time.sleep(1)


def run_backend():
    server = ThreadingHTTPServer(("0.0.0.0", 3000), Backend)
    server.daemon_threads = True
    server.serve_forever()


# ---------------------------------------------------------------- 驗證
failures = []


def check(ok, label, detail=""):
    print(("  ok   " if ok else "  FAIL ") + label + (f"  ({detail})" if detail and not ok else ""))
    if not ok:
        failures.append(label)


def request(base, method, path, headers=None, body=None, timeout=15):
    u = urlparse(base)
    conn = http.client.HTTPConnection(u.hostname, u.port, timeout=timeout)
    conn.request(method, path, body=body, headers=headers or {})
    resp = conn.getresponse()
    data = resp.read()
    return resp.status, {k.lower(): v for k, v in resp.getheaders()}, data


def read_until(sock, marker, timeout=10):
    sock.settimeout(timeout)
    buf = b""
    while marker not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            break
        buf += chunk
    return buf


def websocket(base, path):
    u = urlparse(base)
    s = socket.create_connection((u.hostname, u.port), timeout=10)
    key = base64.b64encode(b"0123456789abcdef").decode()
    s.sendall(
        (
            f"GET {path} HTTP/1.1\r\nHost: {u.hostname}:{u.port}\r\n"
            f"Upgrade: websocket\r\nConnection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        ).encode()
    )
    return s, key


def check_up(base):
    print("後端在線：")
    s, h, b = request(base, "GET", "/health")
    check(s == 200 and json.loads(b) == {"status": "ok"}, "一般請求原樣轉發", f"{s} {b[:80]!r}")

    s, h, b = request(
        base,
        "GET",
        "/echo",
        {"X-Forwarded-For": "203.0.113.7", "X-Forwarded-Proto": "https", "Host": "chat.example.test"},
    )
    seen = json.loads(b)
    check(seen["host"] == "chat.example.test", "Host 原樣傳給後端", str(seen))
    check(
        seen["xff"].startswith("203.0.113.7, ") and len(seen["xff"].split(",")) == 2,
        "X-Forwarded-For 保留原有的來源並追加代理看到的對端",
        str(seen),
    )
    check(seen["proto"] == "https", "上游帶來的 X-Forwarded-Proto 保留", str(seen))
    s, h, b = request(base, "GET", "/echo")
    seen = json.loads(b)
    check(seen["proto"] == "http" and "," not in seen["xff"], "沒有上游標頭時，由代理補上對端與協定", str(seen))

    s, h, b = request(base, "GET", "/maintenance")
    body = json.loads(b)
    check(
        s == 503 and h.get("x-backend") == "fake" and body["error"]["code"] == "maintenance",
        "後端自己的 503（維護模式）原樣轉發，不被停機頁取代",
        f"{s} {b[:80]!r}",
    )

    size = 3 * 1024 * 1024
    s, h, b = request(base, "POST", "/upload", {"Content-Type": "application/octet-stream"}, b"x" * size)
    check(s == 200 and json.loads(b)["received"] == size, "大於預設 1 MB 的請求（語音上傳）可通過", f"{s}")

    u = urlparse(base)
    sock = socket.create_connection((u.hostname, u.port), timeout=10)
    sock.sendall(b"GET /sse HTTP/1.1\r\nHost: x\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n")
    start = time.monotonic()
    buf = read_until(sock, b"data: 1\n\n")
    first = time.monotonic() - start
    check(b"data: 1" in buf, "SSE 收得到第一段")
    read_until(sock, b"data: %d\n\n" % SSE_EVENTS, timeout=SSE_EVENTS * SSE_INTERVAL + 10)
    total = time.monotonic() - start
    sock.close()
    print(f"  info SSE 第一段 {first:.2f}s，全部 {total:.2f}s")
    check(
        first < SSE_INTERVAL * 0.8 and total - first >= SSE_INTERVAL * (SSE_EVENTS - 1) * 0.8,
        "SSE 逐段送達（沒有被緩衝到結束才一起送出）",
        f"第一段 {first:.2f}s，全部 {total:.2f}s",
    )

    sock, key = websocket(base, "/api/conversations/abc/voice")
    head = read_until(sock, b"\r\n\r\n")
    accept = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
    check(head.startswith(b"HTTP/1.1 101"), "WebSocket 可升級（101）", head[:60].decode(errors="replace"))
    check(accept.encode() in head, "Sec-WebSocket-Accept 正確")
    rest = head.split(b"\r\n\r\n", 1)[1]
    while len(rest) < 7:
        rest += sock.recv(64)
    check(rest[:7] == b"\x81\x05hello", "升級後的資料能穿過代理", repr(rest[:12]))
    sock.close()


def check_down(base):
    print("後端停掉：")
    s, h, b = request(base, "GET", "/conversations/1")
    text = b.decode("utf-8", errors="replace")
    check(s == 503, "回 503", str(s))
    check("text/html" in h.get("content-type", ""), "內容是 HTML", h.get("content-type", ""))
    for phrase in ("系統維護中，請稍後再試", "under maintenance", "en mantenimiento"):
        check(phrase.lower() in text.lower(), f"頁面含「{phrase}」")
    check(h.get("cache-control", "").startswith("no-store"), "停機頁不被瀏覽器快取", h.get("cache-control", ""))
    check("nginx/" not in h.get("server", "") and "nginx/" not in text, "不洩漏代理版本")
    check("x-backend" not in h, "不是後端的回應")

    s, h, b = request(base, "POST", "/api/auth/login", {"Content-Type": "application/json"}, b"{}")
    check(s == 503 and "text/html" in h.get("content-type", ""), "API 請求也得到 503 停機頁", str(s))

    sock, _ = websocket(base, "/api/conversations/abc/voice")
    head = read_until(sock, b"\r\n\r\n")
    sock.close()
    check(head.startswith(b"HTTP/1.1 503"), "WebSocket 升級請求得到 503，不是卡住", head[:40].decode(errors="replace"))

    s, h, b = request(base, "GET", "/proxy-health")
    check(s == 200, "代理自己的存活檢查在後端停掉時仍回 200", str(s))


def main():
    if len(sys.argv) >= 2 and sys.argv[1] == "backend":
        run_backend()
        return
    if len(sys.argv) == 4 and sys.argv[1] == "check" and sys.argv[2] in ("up", "down"):
        base = sys.argv[3]
        (check_up if sys.argv[2] == "up" else check_down)(base)
        if failures:
            print(f"\n{len(failures)} 項失敗")
            sys.exit(1)
        print("\n全部通過")
        return
    print(__doc__)
    sys.exit(2)


if __name__ == "__main__":
    main()
