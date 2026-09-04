# pvz_engine.py — engine-scene-mcp 的 pvz 作用域 stdio 客户端(自带 engine-host)
import json, pathlib, subprocess, os, sys, time

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPO = ROOT.parent.parent
BIN = REPO / "target" / "debug" / "engine-scene-mcp.exe"

class EngineMcp:
    def __init__(self):
        env = dict(os.environ)
        env["FORGE_PROJECT_ROOT"] = str(ROOT)
        # PVZ_MCP_STDERR=<file>:把 engine-scene-mcp 的 stderr 落盘(排查子进程崩溃);缺省丢弃。
        err_path = os.environ.get("PVZ_MCP_STDERR")
        stderr = open(err_path, "a", encoding="utf-8") if err_path else subprocess.DEVNULL
        self.p = subprocess.Popen(
            [str(BIN)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=stderr, env=env, text=True, encoding="utf-8", errors="replace",
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
                raise RuntimeError("engine mcp closed")
            line = line.strip()
            if line:
                return json.loads(line)
    def _req(self, method, params, timeout=120):
        self._id += 1
        self._send({"jsonrpc": "2.0", "id": self._id, "method": method, "params": params})
        t0 = time.time()
        while time.time() - t0 < timeout:
            msg = self._recv()
            if msg.get("id") == self._id:
                return msg
        raise TimeoutError(method)
    def _notify(self, method, params):
        self._send({"jsonrpc": "2.0", "method": method, "params": params})
    def call(self, tool, args=None, timeout=120):
        r = self._req("tools/call", {"name": tool, "arguments": args or {}}, timeout)
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

if __name__ == "__main__":
    m = EngineMcp()
    try:
        print(json.dumps(m.call(sys.argv[1], json.loads(sys.argv[2]) if len(sys.argv) > 2 else {}),
                          ensure_ascii=False)[:2000])
    finally:
        m.close()
