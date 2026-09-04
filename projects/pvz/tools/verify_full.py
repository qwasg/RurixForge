# verify_full.py — 1-1 完整验证:经济→种植→发射→命中→击杀→(小推车/判负)
import json, pathlib, sys, time, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    t0 = time.time()
    msgs = collections.Counter()
    # 步骤时间线(秒)
    steps = [
        (1.0, "plant_at", 2 * 1000000 + 2 * 10000 + 302),   # 向日葵 r3c2
        (3.0, "plant_at", 2 * 1000000 + 2 * 10000 + 303),   # 向日葵 r3c3(阳光够?50-50=0,不够→拒)
        (10.0, "collect_sun", 3 * 1000000 + 1),
        (22.0, "collect_sun", 3 * 1000000 + 1),
        (34.0, "collect_sun", 3 * 1000000 + 1),
        (36.0, "plant_at", 2 * 1000000 + 1 * 10000 + 305),  # 豌豆 r3c5
        (46.0, "collect_sun", 3 * 1000000 + 1),
        (58.0, "collect_sun", 3 * 1000000 + 1),
    ]
    done = set()
    while time.time() - t0 < 95.0:
        el = time.time() - t0
        for i, (ts, act, val) in enumerate(steps):
            if i not in done and el > ts:
                m.call("logic_inject_input", {"action": act, "value": float(val)})
                done.add(i)
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict) and e.get("event") == "logic.message":
                msgs[e.get("name", "?")] += 1
        time.sleep(0.2)
    print("=== messages over 95s ===")
    for k, c in msgs.most_common():
        print(f"{c:5d}  {k}")
    print("=== 实体终态 ===")
    r = m.call("entity_list", {})
    ents = r.get("entities") if isinstance(r, dict) else r
    if isinstance(ents, list):
        for e in ents:
            n = e.get("name", "?")
            if any(k in n for k in ["Zombie_Pool", "Peashooter_Pool_1", "Sunflower_Pool_1", "Mower"]):
                t = e.get("translation") or e.get("transform", {}).get("translation")
                print(f"{n}: {[round(x, 2) for x in t]}")
    m.call("play_exit", {})
finally:
    m.close()
