# probe_verify.py — 玩法验证:种植/产阳光/发射/出怪/击杀 全链证据
import json, pathlib, sys, time, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    t0 = time.time()
    planted = 0
    msgnames = collections.Counter()
    entity_pos = {}
    while time.time() - t0 < 50.0:
        el = time.time() - t0
        if planted == 0 and el > 1.0:
            m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 2 * 10000 + 302)})
            planted += 1
        elif planted == 1 and el > 2.0:
            m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 1 * 10000 + 304)})
            planted += 1
        elif planted == 2 and el > 30.0:
            m.call("logic_inject_input", {"action": "collect_sun", "value": float(3 * 1000000 + 1)})
            planted += 1
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict) and e.get("event") == "logic.message":
                msgnames[e.get("name", "?")] += 1
        time.sleep(0.2)
    print("=== logic.message counts ===")
    for k, c in msgnames.most_common():
        print(f"{c:5d}  {k}")
    # 读实体位置验证
    r = m.call("entity_list", {})
    ents = r.get("entities") if isinstance(r, dict) else r
    if isinstance(ents, list):
        for e in ents:
            n = e.get("name", "?")
            if any(k in n for k in ["Sunflower_Pool_1", "Peashooter_Pool_1", "Zombie_Pool_1", "Zombie_Pool_2"]):
                t = e.get("translation") or e.get("transform", {}).get("translation")
                print(f"{n}: {t}")
    m.call("play_exit", {})
finally:
    m.close()
