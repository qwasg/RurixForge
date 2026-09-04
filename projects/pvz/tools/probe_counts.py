# probe_counts.py — 按 nodeId 统计 logic.call 频次 + 捕获消息/错误,跑 16s 观察波次
import json, pathlib, sys, time, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/probe_ctrl.rxscene"})
    m.call("play_enter", {})
    counts = collections.Counter()
    fns = collections.Counter()
    msgs = []
    t0 = time.time()
    while time.time() - t0 < 16.0:
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict):
                if e.get("event") == "logic.call":
                    counts[e.get("nodeId", "?")] += 1
                    fns[e.get("fn", "?")] += 1
                elif e.get("event") in ("logic.message", "logic.log", "logic.call_error", "logic.unsupported"):
                    msgs.append(e)
        time.sleep(0.2)
    print("=== logic.call by fn ===")
    for f, c in fns.most_common(20):
        print(f"{c:5d}  {f}")
    print("=== logic.call by nodeId ===")
    for nid, c in counts.most_common(12):
        print(f"{c:5d}  {nid}")
    print("=== messages/logs/errors ===")
    seen = set()
    for e in msgs:
        k = (e.get("event"), e.get("name", ""), e.get("message", ""))
        if k not in seen:
            print(json.dumps(e, ensure_ascii=False)[:240])
            seen.add(k)
    m.call("play_exit", {})
finally:
    m.close()
