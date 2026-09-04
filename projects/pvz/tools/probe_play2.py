# probe_play2.py — 高频 drain 捕获稀疏关键事件(每 300ms 排空防环缓冲逐出)
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

KEYS = ["logic.input", "logic.message", "logic.log", "plant_done", "fire_pea", "spawn_sun",
        "sun_gained", "zombie_died", "loseline", "LEVEL_WIN", "LEVEL_LOSE", "call_error", "unsupported", "inject_input"]

def run():
    m = EngineMcp()
    acc = []
    def drain():
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict):
                txt = json.dumps(e, ensure_ascii=False)
                if any(k in txt for k in KEYS):
                    acc.append(e)
    try:
        m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
        m.call("play_enter", {})
        print("entered; planting sunflower r3c2 + peashooter r3c4")
        t0 = time.time()
        planted = 0
        while time.time() - t0 < 45.0:
            # 早期种两株
            el = time.time() - t0
            if planted == 0 and el > 1.0:
                m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 2 * 10000 + 302)})
                planted += 1
            elif planted == 1 and el > 2.0:
                m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 1 * 10000 + 304)})
                planted += 1
            drain()
            time.sleep(0.3)
        print(f"=== captured {len(acc)} key events over 45s ===")
        seen = {}
        for e in acc:
            k = e.get("event", "?")
            seen[k] = seen.get(k, 0) + 1
        for k, c in sorted(seen.items(), key=lambda x: -x[1]):
            print(f"{c:4d}  {k}")
        print("--- samples ---")
        shown = set()
        for e in acc:
            k = e.get("event", "?")
            if k not in shown:
                print(json.dumps(e, ensure_ascii=False)[:260])
                shown.add(k)
        try:
            m.call("play_exit", {})
        except Exception:
            pass
    finally:
        m.close()

if __name__ == "__main__":
    run()
