# probe_unsupported.py — 捕获 logic.unsupported 详情定位未实现节点调用
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    time.sleep(3.0)
    ev = m.call("host_events_drain", {})
    if isinstance(ev, dict):
        ev = ev.get("events", [])
    seen = {}
    for e in ev:
        if isinstance(e, dict):
            t = e.get("event") or e.get("type")
            if t == "logic.unsupported":
                p = e.get("payload") or e
                key = json.dumps(p, ensure_ascii=False)
                seen[key] = seen.get(key, 0) + 1
    for k, v in list(seen.items())[:10]:
        print(f"x{v} {k[:300]}")
    print("total events:", len(ev))
    m.call("play_exit", {})
finally:
    m.close()
