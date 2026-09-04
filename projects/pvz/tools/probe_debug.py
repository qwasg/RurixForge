# probe_debug.py — 输入与控制器调试:logic.input / logic.log / logic.message 取证
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    print("entered")
    time.sleep(1.0)
    m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 2 * 10000 + 302)})
    print("injected plant_at sunflower r3c2")
    time.sleep(2.0)
    ev = m.call("host_events_drain", {})
    if isinstance(ev, dict):
        ev = ev.get("events", [])
    dist = {}
    for e in ev:
        if isinstance(e, dict):
            t = e.get("event") or "?"
            dist[t] = dist.get(t, 0) + 1
    for t, c in sorted(dist.items(), key=lambda x: -x[1])[:15]:
        print(f"{c:5d}  {t}")
    print("--- input/log/message/call samples ---")
    shown = 0
    for e in ev:
        if isinstance(e, dict) and e.get("event") in ("logic.input", "logic.log", "logic.message", "logic.call_error", "logic.call"):
            print(json.dumps(e, ensure_ascii=False)[:280])
            shown += 1
            if shown > 15:
                break
    m.call("play_exit", {})
finally:
    m.close()
