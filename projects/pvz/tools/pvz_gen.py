# pvz_gen.py — gen-image-mcp 的 pvz 作用域 stdio 客户端
import json, pathlib, subprocess, os

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPO = ROOT.parent.parent
BIN = REPO / "target" / "debug" / "gen-image-mcp.exe"

class GenMcp:
    def __init__(self):
        self.p = subprocess.Popen(
            [str(BIN), "--project", str(ROOT)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, encoding="utf-8", errors="replace",
        )
        self._id = 0
        self._req("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                 "clientInfo": {"name": "pvz-orch", "version": "0.1.0"}})
        self._notify("notifications/initialized", {})
    def _send(self, obj):
        self.p.stdin.write(json.dumps(obj, ensure_ascii=False) + "\n"); self.p.stdin.flush()
    def _recv(self):
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("gen mcp closed")
            line = line.strip()
            if line:
                return json.loads(line)
    def _req(self, method, params, timeout=400):
        self._id += 1
        self._send({"jsonrpc": "2.0", "id": self._id, "method": method, "params": params})
        while True:
            msg = self._recv()
            if msg.get("id") == self._id:
                return msg
    def _notify(self, method, params):
        self._send({"jsonrpc": "2.0", "method": method, "params": params})
    def call(self, tool, args, timeout=400):
        r = self._req("tools/call", {"name": tool, "arguments": args}, timeout)
        result = r.get("result", {})
        content = result.get("content") or []
        if content and isinstance(content[0], dict) and "text" in content[0]:
            try:
                return json.loads(content[0]["text"])
            except Exception:
                return {"text": content[0]["text"]}
        return result
    def close(self):
        try:
            self.p.terminate()
        except Exception:
            pass
