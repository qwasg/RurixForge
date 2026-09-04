# validate_graphs.py — 用 pvz 作用域 code-forge-mcp 校验全部图
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_mcp import Mcp

ROOT = pathlib.Path(__file__).resolve().parent.parent
NAMES = ["zombie_walker", "pea_fly", "sun_fall", "plant_shooter", "plant_producer",
         "level_controller", "hud", "loseline", "mower"]

m = Mcp()
all_ok = True
for name in NAMES:
    p = ROOT / "Content" / "Graphs" / f"{name}.rxgraph"
    if not p.is_file():
        continue
    g = json.loads(p.read_text(encoding="utf-8"))
    r = m.call("graph_validate", {"graph": g})
    ok = r.get("ok")
    all_ok = all_ok and bool(ok)
    line = f"{name}: nodes={len(g['nodes'])} edges={len(g['edges'])} ok={ok}"
    if not ok:
        line += " ERRS=" + json.dumps((r.get("errors") or [])[:3], ensure_ascii=False)
    print(line)
m.close()
print("ALL_OK" if all_ok else "HAS_FAILURES")
