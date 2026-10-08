"""Local OpenAI-compatible gateway that proxies every model of an upstream provider.

Stdlib only (Python 3.9+). Binds to 127.0.0.1 by default and requires a local
bearer token, so the upstream key never leaves this process.

Env / .env (same directory):
  UPSTREAM_BASE     upstream base URL (default https://sg-api.kcne.top)
  UPSTREAM_API_KEY  upstream key (required)
  LOCAL_API_KEY     token clients must send as "Authorization: Bearer ..." (required)
  HOST / PORT       bind address (default 127.0.0.1:8787)
"""
from __future__ import annotations

import hmac
import json
import os
import sys
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load_dotenv(path: Path) -> None:
    if not path.exists():
        return
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))


load_dotenv(HERE / ".env")
UPSTREAM_BASE = os.environ.get("UPSTREAM_BASE", "https://sg-api.kcne.top").rstrip("/")
UPSTREAM_API_KEY = os.environ.get("UPSTREAM_API_KEY", "")
LOCAL_API_KEY = os.environ.get("LOCAL_API_KEY", "")
HOST = os.environ.get("HOST", "127.0.0.1")
PORT = int(os.environ.get("PORT", "8787"))
TIMEOUT = float(os.environ.get("UPSTREAM_TIMEOUT", "300"))
MAX_BODY = 32 * 1024 * 1024  # 32 MiB request cap

# Only these client headers are forwarded upstream (allowlist). Browser headers such as
# Origin/Referer/Cookie/Sec-Fetch-* must never leak to the provider (and made codex hang).
FWD_REQ = {"content-type", "accept", "openai-beta", "openai-organization", "openai-project",
           "user-agent", "x-stainless-lang"}
DROP_RESP = {"content-length", "connection", "transfer-encoding", "keep-alive",
             "content-encoding", "server", "set-cookie"}

_models_cache: dict = {"ts": 0.0, "body": None}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "local-model-gateway/1.0"

    # ---- helpers -------------------------------------------------------
    def log_message(self, fmt, *args):  # concise access log, no headers/bodies
        sys.stderr.write("%s %s\n" % (self.log_date_time_string(), fmt % args))

    def _json(self, status: int, obj) -> None:
        data = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _authorized(self) -> bool:
        auth = self.headers.get("Authorization", "")
        token = auth[7:] if auth.lower().startswith("bearer ") else ""
        return bool(LOCAL_API_KEY) and hmac.compare_digest(token.encode(), LOCAL_API_KEY.encode())

    def _read_body(self) -> bytes | None:
        n = int(self.headers.get("Content-Length") or 0)
        if n > MAX_BODY:
            self._json(413, {"error": {"message": "request body too large"}})
            return None
        return self.rfile.read(n) if n else b""

    # ---- routing -------------------------------------------------------
    def do_GET(self):
        if self.path == "/health":
            return self._json(200, {"status": "ok", "upstream": UPSTREAM_BASE})
        if self.path in ("/", "/playground"):
            # Static test page; every API call it makes still needs the local token.
            data = (HERE / "playground.html").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        self._dispatch("GET")

    def do_POST(self):
        self._dispatch("POST")

    def do_DELETE(self):
        self._dispatch("DELETE")

    def _dispatch(self, method: str) -> None:
        if not self.path.startswith("/v1/"):
            return self._json(404, {"error": {"message": "not found"}})
        if not self._authorized():
            return self._json(401, {"error": {"message": "invalid or missing local API key"}})
        body = self._read_body() if method != "GET" else b""
        if body is None:
            return
        if method == "GET" and self.path.split("?")[0] == "/v1/models":
            cached = _models_cache["body"]
            if cached and time.time() - _models_cache["ts"] < 60:
                return self._json(200, cached)
        self._proxy(method, body)

    def _proxy(self, method: str, body: bytes) -> None:
        headers = {k: v for k, v in self.headers.items() if k.lower() in FWD_REQ}
        if "user-agent" not in {k.lower() for k in headers} or "Mozilla" in headers.get("User-Agent", ""):
            headers = {k: v for k, v in headers.items() if k.lower() != "user-agent"}
            headers["User-Agent"] = "local-model-gateway/1.0"
        headers["Authorization"] = f"Bearer {UPSTREAM_API_KEY}"
        req = urllib.request.Request(UPSTREAM_BASE + self.path, data=body or None,
                                     headers=headers, method=method)
        try:
            resp = urllib.request.urlopen(req, timeout=TIMEOUT)
        except urllib.error.HTTPError as e:
            resp = e  # upstream error: relay status + body verbatim
        except Exception as e:  # network failure
            return self._json(502, {"error": {"message": f"upstream unreachable: {e.__class__.__name__}: {e}"}})

        status = resp.status if hasattr(resp, "status") else resp.code
        self.send_response(status)
        for k, v in resp.headers.items():
            if k.lower() not in DROP_RESP:
                self.send_header(k, v)
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()

        capture = method == "GET" and self.path.split("?")[0] == "/v1/models" and status == 200
        buf = bytearray()
        try:
            while True:
                chunk = resp.read1(65536) if hasattr(resp, "read1") else resp.read(65536)
                if not chunk:
                    break
                if capture:
                    buf += chunk
                self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk))
                self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
        except (BrokenPipeError, ConnectionResetError):
            pass
        finally:
            resp.close()
        if capture:
            try:
                _models_cache.update(ts=time.time(), body=json.loads(buf))
            except ValueError:
                pass


def main() -> None:
    if not UPSTREAM_API_KEY or not LOCAL_API_KEY:
        sys.exit("UPSTREAM_API_KEY and LOCAL_API_KEY must be set (env or .env)")
    srv = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"local-model-gateway on http://{HOST}:{PORT} -> {UPSTREAM_BASE}", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
