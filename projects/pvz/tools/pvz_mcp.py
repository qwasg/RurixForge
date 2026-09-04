# pvz_mcp.py — 以 pvz 项目根为作用域的 code-forge-mcp stdio 客户端
# 用途:编排侧直接校验/落盘图与脚本,绕过 agentd REST 的默认 demo 根限制。
import json, pathlib, subprocess, sys, os

ROOT = pathlib.Path(__file__).resolve().parent.parent          # projects/pvz
REPO = ROOT.parent.parent                                       # 仓库根
BIN = REPO / "target" / "debug" / "code-forge-mcp.exe"

class Mcp:
    def __init__(self):
        env = dict(os.environ)
        env["FORGE_CODE_FORGE_PROJECT"] = str(ROOT)
        self.p = subprocess.Popen(
            [str(BIN)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, env=env, text=True, encoding="utf-8", errors="replace",
        )
        self._id = 0
        self._req("initialize", {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "pvz-orchestrator", "version": "0.1.0"},
        })
        self._notify("notifications/initialized", {})

    def _send(self, obj):
        self.p.stdin.write(json.dumps(obj, ensure_ascii=False) + "\n")
        self.p.stdin.flush()

    def _recv(self):
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("mcp server closed")
            line = line.strip()
            if not line:
                continue
            return json.loads(line)

    def _req(self, method, params):
        self._id += 1
        self._send({"jsonrpc": "2.0", "id": self._id, "method": method, "params": params})
        while True:
            msg = self._recv()
            if msg.get("id") == self._id:
                return msg

    def _notify(self, method, params):
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def call(self, tool, args):
        r = self._req("tools/call", {"name": tool, "arguments": args})
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
    # 用法: python pvz_mcp.py <tool> <args-json-file>
    m = Mcp()
    try:
        args = json.loads(pathlib.Path(sys.argv[2]).read_text(encoding="utf-8-sig"))
        print(json.dumps(m.call(sys.argv[1], args), ensure_ascii=False, indent=1))
    finally:
        m.close()
