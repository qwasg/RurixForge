# verify_combat.py — 1-1 战斗全链验证:经济→种植→发射→击杀→小推车
import json, pathlib, sys, time, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    t0 = time.time()
    msgs = collections.Counter()
    phase = 0
    # 策略:先种向日葵,攒阳光收阳光,再种豌豆射手,等僵尸
    while time.time() - t0 < 75.0:
        el = time.time() - t0
        if phase == 0 and el > 1.0:
            m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 2 * 10000 + 302)})  # 向日葵 r3c2
            phase += 1
        elif phase == 1 and el > 8.0:
            m.call("logic_inject_input", {"action": "collect_sun", "value": float(3 * 1000000 + 1)})  # 收阳光
            phase += 1
        elif phase == 2 and el > 20.0:
            m.call("logic_inject_input", {"action": "collect_sun", "value": float(3 * 1000000 + 1)})
            phase += 1
        elif phase == 3 and el > 25.0:
            m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 1 * 10000 + 305)})  # 豌豆 r3c5
            phase += 1
        elif phase == 4 and el > 40.0:
            m.call("logic_inject_input", {"action": "collect_sun", "value": float(3 * 1000000 + 1)})
            phase += 1
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict) and e.get("event") == "logic.message":
                msgs[e.get("name", "?")] += 1
        time.sleep(0.2)
    print("=== messages over 75s ===")
    for k, c in msgs.most_common():
        print(f"{c:5d}  {k}")
    # 僵尸位置(验证行走/死亡)
    r = m.call("entity_list", {})
    ents = r.get("entities") if isinstance(r, dict) else r
    if isinstance(ents, list):
        for e in ents:
            n = e.get("name", "?")
            if "Zombie_Pool" in n or "Pea_Pool_1" in n or "Peashooter_Pool_1" in n:
                t = e.get("translation") or e.get("transform", {}).get("translation")
                print(f"{n}: {[round(x,2) for x in t]}")
    m.call("play_exit", {})
finally:
    m.close()
