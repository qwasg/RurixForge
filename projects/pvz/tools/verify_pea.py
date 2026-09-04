# verify_pea.py — 豌豆战斗链验证:攒阳光→种豌豆→fire_pea→豌豆飞行→命中扣血→击杀
import json, pathlib, sys, time, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    t0 = time.time()
    msgs = collections.Counter()
    steps = [
        (1.0, "plant_at", 2 * 1000000 + 2 * 10000 + 302),   # 向日葵 r3c2
        (8.0, "collect_sun", 3 * 1000000 + 1),
        (20.0, "collect_sun", 3 * 1000000 + 1),
        (30.0, "collect_sun", 3 * 1000000 + 1),
        (31.0, "plant_at", 2 * 1000000 + 1 * 10000 + 306),  # 豌豆射手 r3c6(僵尸必经)
        (44.0, "collect_sun", 3 * 1000000 + 1),
        (56.0, "collect_sun", 3 * 1000000 + 1),
    ]
    done = set()
    pea_positions = []
    while time.time() - t0 < 80.0:
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
        # 追踪豌豆位置
        if int(el * 2) % 4 == 0:
            r = m.call("entity_list", {})
            ents = r.get("entities") if isinstance(r, dict) else r
            if isinstance(ents, list):
                for e in ents:
                    if "Pea_Pool_1" in e.get("name", ""):
                        t = e.get("translation") or e.get("transform", {}).get("translation")
                        pea_positions.append((round(el, 1), round(t[0], 2)))
        time.sleep(0.2)
    print("=== messages ===")
    for k, c in msgs.most_common():
        print(f"{c:5d}  {k}")
    print("=== Pea_Pool_1 x 轨迹(采样) ===")
    seen = set()
    for el, x in pea_positions:
        key = round(x, 0)
        if key not in seen:
            print(f"t={el}s x={x}")
            seen.add(key)
    print("=== Peashooter_Pool_1 终态 ===")
    r = m.call("entity_list", {})
    ents = r.get("entities") if isinstance(r, dict) else r
    for e in ents:
        if "Peashooter_Pool_1" in e.get("name", ""):
            print(e.get("translation") or e.get("transform", {}).get("translation"))
    m.call("play_exit", {})
finally:
    m.close()
