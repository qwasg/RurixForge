# pvz_assets.py — asset-pipeline-mcp 的 pvz 作用域 stdio 客户端 + 占位贴图生成
import json, pathlib, subprocess, os, struct, zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPO = ROOT.parent.parent
BIN = REPO / "target" / "debug" / "asset-pipeline-mcp.exe"

class AssetMcp:
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
                raise RuntimeError("asset mcp closed")
            line = line.strip()
            if line:
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

def write_png(path, width, height, rgba):
    """最小 PNG 编码:纯色 RGBA。"""
    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    raw = b""
    row = bytes(rgba)
    for _ in range(height):
        raw += b"\x00" + row * width
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw))
           + chunk(b"IEND", b""))
    pathlib.Path(path).write_bytes(png)

# 占位调色板(品红底纪律不适用——这些是纯色占位,非角色图集)
PLACEHOLDERS = {
    "PZ_Lawn.png": (64, 64, (46, 125, 50, 255)),       # 草坪绿
    "PZ_Cell.png": (64, 64, (67, 160, 71, 255)),       # 格浅绿
    "PZ_Peashooter.png": (64, 64, (27, 94, 32, 255)),  # 深绿
    "PZ_Sunflower.png": (64, 64, (251, 192, 45, 255)), # 葵黄
    "PZ_Zombie.png": (64, 96, (120, 144, 156, 255)),   # 僵尸灰
    "PZ_Pea.png": (32, 32, (76, 175, 80, 255)),        # 豌豆绿
    "PZ_Sun.png": (48, 48, (255, 235, 59, 255)),       # 阳光黄
    "PZ_Mower.png": (64, 48, (211, 47, 47, 255)),      # 推车红
    "PZ_HUD_Bar.png": (32, 16, (255, 249, 196, 255)),  # HUD 米
    "PZ_Card.png": (48, 64, (141, 110, 99, 255)),      # 卡槽棕
}

def gen_placeholders():
    out = ROOT / "Content" / "Textures"
    out.mkdir(parents=True, exist_ok=True)
    for name, (w, h, rgba) in PLACEHOLDERS.items():
        write_png(out / name, w, h, rgba)
    return list(PLACEHOLDERS)

if __name__ == "__main__":
    names = gen_placeholders()
    print("placeholders:", len(names))
    m = AssetMcp()
    try:
        r = m.call("asset_import", {"sourcePaths": [f"Content/Textures/{n}" for n in names], "destFolder": "Textures"})
        for it in r.get("imported", []):
            print(f"  {it['assetPath']} -> {it['guid']}")
        for f in r.get("failed", []):
            print("  FAIL", f)
    finally:
        m.close()
