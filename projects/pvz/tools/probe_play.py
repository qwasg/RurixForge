# probe_play.py — 带输入的完整玩法取证 + 崩溃点定位
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
KEYS = ["fire_pea", "spawn_sun", "sun_gained", "zombie_died", "loseline", "LEVEL_WIN", "LEVEL_LOSE",
        "plant_done", "CARD_SELECTED", "call_error", "unsupported", "panic", "crash"]
def scan(ev, acc):
    if isinstance(ev, dict):
        ev = ev.get("events", [])
    for e in ev:
        if not isinstance(e, dict):
            continue
        txt = json.dumps(e, ensure_ascii=False)
        for k in KEYS:
            if k in txt:
                acc.append(e)
                break
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    print("entered")
    acc = []
    time.sleep(2.0)
    # 种向日葵 r3c2、豌豆射手 r3c4
    m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 2 * 10000 + 302)})
    time.sleep(1.0)
    m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 1 * 10000 + 304)})
    print("planted")
    for i in range(10):
        time.sleep(4.0)
        try:
            ev = m.call("host_events_drain", {})
            scan(ev, acc)
            print(f"t+{(i+1)*4}s ok, key events so far={len(acc)}")
        except Exception as ex:
            print(f"t+{(i+1)*4}s HOST GONE: {ex}")
            break
    print("=== KEY EVENTS ===")
    for e in acc[:40]:
        print(json.dumps(e, ensure_ascii=False)[:240])
    try:
        m.call("play_exit", {})
    except Exception:
        pass
finally:
    m.close()
