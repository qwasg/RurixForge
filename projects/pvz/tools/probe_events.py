# probe_events.py — 事件类型分布 + 错误/崩溃取证
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    time.sleep(4.0)
    ev = m.call("host_events_drain", {})
    if isinstance(ev, dict):
        ev = ev.get("events", [])
    dist = {}
    samples = {}
    for e in ev:
        if isinstance(e, dict):
            t = e.get("event") or e.get("type") or "?"
            dist[t] = dist.get(t, 0) + 1
            samples.setdefault(t, e)
    for t, c in sorted(dist.items(), key=lambda x: -x[1]):
        print(f"{c:5d}  {t}")
    print("--- samples ---")
    for t, e in samples.items():
        print(t, "=>", json.dumps(e, ensure_ascii=False)[:260])
    m.call("play_exit", {})
finally:
    m.close()
