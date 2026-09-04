# probe_zombie.py — 僵尸行走定点追踪
import json, pathlib, sys, time, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    t0 = time.time()
    zcalls = collections.Counter()
    # 等首波僵尸出来(12s+)
    while time.time() - t0 < 16.0:
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict) and e.get("graphId") == "zombie_walker" and e.get("event") == "logic.call":
                zcalls[e.get("fn", "?")] += 1
        time.sleep(0.2)
    print("=== zombie_walker calls (first 16s) ===")
    for k, c in zcalls.most_common():
        print(f"{c:5d}  {k}")
    # 追踪僵尸 x
    r = m.call("entity_list", {})
    ents = r.get("entities") if isinstance(r, dict) else r
    for e in ents:
        if "Zombie_Pool_1" in e.get("name", ""):
            print("Zombie_Pool_1 pos:", e.get("translation") or e.get("transform", {}).get("translation"))
    time.sleep(3.0)
    r = m.call("entity_list", {})
    ents = r.get("entities") if isinstance(r, dict) else r
    for e in ents:
        if "Zombie_Pool_1" in e.get("name", ""):
            print("Zombie_Pool_1 pos +3s:", e.get("translation") or e.get("transform", {}).get("translation"))
    m.call("play_exit", {})
finally:
    m.close()
