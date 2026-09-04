# probe_plant.py — 单株种植全事件捕获(紧循环 drain)
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    time.sleep(1.0)
    m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 2 * 10000 + 302)})
    acc = []
    t0 = time.time()
    while time.time() - t0 < 4.0:
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict):
                t = e.get("event", "")
                if t not in ("logic.update",):  # 滤掉帧洪
                    acc.append(e)
        time.sleep(0.15)
    print(f"captured {len(acc)} non-update events")
    dist = {}
    for e in acc:
        dist[e.get("event", "?")] = dist.get(e.get("event", "?"), 0) + 1
    for k, c in sorted(dist.items(), key=lambda x: -x[1]):
        print(f"{c:4d}  {k}")
    print("--- call/message/log/error samples ---")
    shown = 0
    for e in acc:
        if e.get("event") in ("logic.call", "logic.message", "logic.log", "logic.call_error", "logic.input", "logic.unsupported"):
            txt = json.dumps(e, ensure_ascii=False)
            # 只打印 controller 的或错误的
            if e.get("graphId") == "level_controller" or "error" in e.get("event", "") or e.get("event") in ("logic.input", "logic.message", "logic.log"):
                print(txt[:300])
                shown += 1
                if shown > 25:
                    break
    m.call("play_exit", {})
finally:
    m.close()
