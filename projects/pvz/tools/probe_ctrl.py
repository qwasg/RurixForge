# probe_ctrl.py — 最小场景单挂 LevelController,验证 on_input 链是否执行
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent
# 最小场景:只有 LevelController
scene = {
    "mode": "2d", "name": "probe_ctrl",
    "entities": [
        {"id": 1, "name": "LevelController",
         "transform": {"rotation": [0, 0, 0, 1], "scale": [1, 1, 1], "translation": [0, 0, 0]},
         "components": [{"enabled": True, "type": "Script",
                         "props": {"graphRef": "Content/Graphs/level_controller.rxgraph", "module": "", "props": {"levelCode": 101.0}}}]},
    ],
}
(ROOT / "Content" / "Scenes" / "probe_ctrl.rxscene").write_text(
    json.dumps(scene, ensure_ascii=False), encoding="utf-8")

m = EngineMcp()
try:
    print("load:", json.dumps(m.call("scene_load", {"path": "Content/Scenes/probe_ctrl.rxscene"}), ensure_ascii=False)[:200])
    print("enter:", json.dumps(m.call("play_enter", {}), ensure_ascii=False)[:200])
    time.sleep(0.5)
    m.call("logic_inject_input", {"action": "plant_at", "value": 2020302.0})
    acc = []
    t0 = time.time()
    while time.time() - t0 < 3.0:
        ev = m.call("host_events_drain", {})
        if isinstance(ev, dict):
            ev = ev.get("events", [])
        for e in ev:
            if isinstance(e, dict) and e.get("event") != "logic.update":
                acc.append(e)
        time.sleep(0.15)
    print(f"captured {len(acc)}")
    for e in acc:
        print(json.dumps(e, ensure_ascii=False)[:280])
    m.call("play_exit", {})
finally:
    m.close()
