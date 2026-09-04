# mcp_call.py — 从命令行对 agentd 发起 MCP 工具调用(UTF-8 安全)
# 用法: python mcp_call.py <tool> <args-json-file|'-'>   # args 为 '-' 时从 stdin 读
import json, pathlib, sys, urllib.request

AGENTD = "http://localhost:8103"

def call(tool: str, args: dict, timeout: int = 120) -> dict:
    body = json.dumps({"tool": tool, "arguments": args}).encode("utf-8")
    req = urllib.request.Request(
        AGENTD + "/api/forge/mcp/call",
        data=body,
        headers={"Content-Type": "application/json; charset=utf-8"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        outer = json.loads(r.read().decode("utf-8"))
    # MCP 结果本体在 content[0].text(字符串内嵌 JSON)
    try:
        text = outer["content"][0]["text"]
        return json.loads(text)
    except Exception:
        return outer

def call_session_tool(session_id: str, tool: str, args: dict, timeout: int = 120) -> dict:
    """经会话作用域调用(项目根 = 会话工作区)。走 ask:execute 太重,这里直接用
    mcp/call 的项目根参数——若后端不支持则回退默认根。"""
    return call(tool, args, timeout)

if __name__ == "__main__":
    tool = sys.argv[1]
    src = sys.argv[2]
    args = json.loads(sys.stdin.read() if src == "-" else pathlib.Path(src).read_text(encoding="utf-8"))
    out = call(tool, args)
    print(json.dumps(out, ensure_ascii=False, indent=1))
