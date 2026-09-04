# verify_pea_iso.py — 豌豆战斗隔离验证:预置激活豌豆射手+僵尸,看 fire_pea→命中→扣血
import json, pathlib, sys, time, collections
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent
def guid(name):
    for line in (ROOT / "Content" / "Textures" / f"{name}.meta").read_text(encoding="utf-8").splitlines():
        if line.startswith("guid:"):
            return line.split(":", 1)[1].strip()

gp = guid("PZ_Peashooter.png"); gz = guid("PZ_Zombie.png"); gpea = guid("PZ_Pea.png")
def sprite(g, order):
    return {"enabled": True, "type": "Sprite", "props": {"texture": g, "sprite": "", "clip": "", "frame": 0.0,
            "tint": [1, 1, 1, 1], "flipX": False, "flipY": False, "pixelsPerUnit": 100.0, "sortingOrder": float(order)}}
def tag(t): return {"enabled": True, "type": "Tag", "props": {"tag": t}}
def script(gr, props): return {"enabled": True, "type": "Script", "props": {"graphRef": f"Content/Graphs/{gr}", "module": "", "props": props}}
def trig(e): return {"enabled": True, "type": "Trigger", "props": {"extents": e, "kind": "box"}}
def ent(i, n, pos, comps, scale=(1, 1, 1)):
    return {"id": i, "name": n, "transform": {"rotation": [0,0,0,1], "scale": list(scale), "translation": list(pos)}, "components": comps}

# 场景:豌豆射手(激活态)在 x=0,僵尸在 x=6 向左走,豌豆池 2 个
scene = {"mode": "2d", "name": "pea_iso", "entities": [
    ent(1, "Shooter", (0.0, 0.0, 0.0), [sprite(gp, 2), tag("plant_active"), script("plant_shooter.rxgraph", {"plantId": 1.0}), trig([1.2, 1.4, 1.0])]),
    ent(2, "Zombie", (6.0, 0.0, 0.0), [sprite(gz, 4), tag("zombie_active"), script("zombie_walker.rxgraph", {"poolId": 1.0, "zombieId": 1.0}), trig([0.9, 1.4, 0.9])]),
    ent(3, "Pea1", (0.0, -60.0, 0.0), [sprite(gpea, 3), tag("pea_free"), script("pea_fly.rxgraph", {"poolId": 1.0})], scale=(0.5, 0.5, 1)),
    ent(4, "Pea2", (0.0, -62.0, 0.0), [sprite(gpea, 3), tag("pea_free"), script("pea_fly.rxgraph", {"poolId": 2.0})], scale=(0.5, 0.5, 1)),
    # 迷你控制器:只处理 fire_pea(池激活)
    ent(9, "PoolMgr", (0.0, 0.0, 0.0), [script("pool_mgr.rxgraph", {})]),
]}
(ROOT / "Content" / "Scenes" / "pea_iso.rxscene").write_text(json.dumps(scene), encoding="utf-8")

# pool_mgr 图:on_message fire_pea(payload=transform)→ find pea_free → set_transform → add_tag pea_active
mgr = {"version": 1, "id": "pool_mgr", "name": "pool_mgr", "exposedProps": [],
       "nodes": [
           {"id": "om", "type": "event.on_message", "pos": [0, 0]},
           {"id": "f", "type": "entity.find_by_tag", "pos": [1, 1], "inputs": {"tag": {"const": "pea_free"}}},
           {"id": "st", "type": "entity.set_transform", "pos": [2, 0],
            "inputs": {"entity": {"node": "f", "pin": "entity"}, "transform": {"node": "om", "pin": "payload"}}},
           {"id": "at", "type": "entity.add_tag", "pos": [3, 0],
            "inputs": {"entity": {"node": "f", "pin": "entity"}, "tag": {"const": "pea_active"}}},
       ],
       "edges": [{"from": ["om", "exec"], "to": ["st", "exec"]}, {"from": ["st", "exec"], "to": ["at", "exec"]}]}
(ROOT / "Content" / "Graphs" / "pool_mgr.rxgraph").write_text(json.dumps(mgr), encoding="utf-8")

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/pea_iso.rxscene"})
    m.call("play_enter", {})
    t0 = time.time()
    msgs = collections.Counter()
    trigger_hits = collections.Counter()
    pea_x = []
    while time.time() - t0 < 40.0:
        el = time.time() - t0
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict) and e.get("event") == "logic.message":
                msgs[e.get("name", "?")] += 1
            if isinstance(e, dict) and e.get("event") == "logic.trigger":
                trigger_hits[(e.get("entityId"), e.get("otherEntity"), e.get("phase"))] += 1
        r = m.call("entity_list", {})
        ents = r.get("entities") if isinstance(r, dict) else r
        if isinstance(ents, list):
            for e in ents:
                if e.get("name") == "Pea1":
                    t = e.get("translation") or e.get("transform", {}).get("translation")
                    pea_x.append((round(el, 1), round(t[0], 2)))
                if e.get("name") == "Zombie":
                    t = e.get("translation") or e.get("transform", {}).get("translation")
                    if int(el * 2) % 6 == 0:
                        print(f"t={round(el,1)}s zombie x={round(t[0],2)}")
        time.sleep(0.2)
    print("=== messages ===")
    for k, c in msgs.most_common():
        print(f"{c:5d}  {k}")
    print("=== trigger hits ===")
    for k, c in trigger_hits.most_common():
        print(f"{c:5d}  {k}")
    print("=== Pea1 x 轨迹 ===")
    seen = set()
    for el, x in pea_x:
        k = round(x, 0)
        if k not in seen:
            print(f"t={el}s x={x}"); seen.add(k)
    m.call("play_exit", {})
finally:
    m.close()
