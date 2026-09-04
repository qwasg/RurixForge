# probe_input.py — 输入链路定点验证:inject 响应 + 立即取事件
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    print("load:", json.dumps(m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"}), ensure_ascii=False)[:200])
    print("enter:", json.dumps(m.call("play_enter", {}), ensure_ascii=False)[:200])
    time.sleep(0.5)
    r = m.call("logic_inject_input", {"action": "plant_at", "value": 2020302.0})
    print("inject:", json.dumps(r, ensure_ascii=False)[:300])
    time.sleep(1.0)
    ev = m.call("host_events_drain", {})
    if isinstance(ev, dict):
        ev = ev.get("events", [])
    inp = [e for e in ev if isinstance(e, dict) and "input" in str(e.get("event", ""))]
    logs = [e for e in ev if isinstance(e, dict) and e.get("event") in ("logic.log", "logic.message")]
    print(f"input events: {len(inp)}; log/message events: {len(logs)}")
    for e in inp[:5]:
        print("IN:", json.dumps(e, ensure_ascii=False)[:250])
    for e in logs[:10]:
        print("LOG:", json.dumps(e, ensure_ascii=False)[:250])
    m.call("play_exit", {})
finally:
    m.close()
