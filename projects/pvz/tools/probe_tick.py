# probe_tick.py — 最小 on_update 每帧打点验证
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp
from pvz_mcp import Mcp as CodeMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent

graph = {
    "version": 1, "id": "probe_tick", "name": "probe_tick", "exposedProps": [],
    "nodes": [
        {"id": "u", "type": "event.on_update", "pos": [0, 0]},
        {"id": "va", "type": "var.add", "pos": [1, 0],
         "inputs": {"name": {"const": "t"}, "value": {"node": "u", "pin": "dt"}}},
        {"id": "dl", "type": "debug.log", "pos": [2, 0], "inputs": {"message": {"const": "TICK"}}},
    ],
    "edges": [{"from": ["u", "exec"], "to": ["va", "exec"]}, {"from": ["va", "exec"], "to": ["dl", "exec"]}],
}
cm = CodeMcp()
try:
    print("validate:", cm.call("graph_validate", {"graph": graph}).get("ok"))
    print("create:", cm.call("graph_create", {"name": "probe_tick", "graph": graph}))
finally:
    cm.close()

scene = {"mode": "2d", "name": "probe_tick", "entities": [
    {"id": 1, "name": "T", "transform": {"rotation": [0,0,0,1], "scale": [1,1,1], "translation": [0,0,0]},
     "components": [{"enabled": True, "type": "Script",
                     "props": {"graphRef": "Content/Graphs/probe_tick.rxgraph", "module": "", "props": {}}}]},
]}
(ROOT / "Content" / "Scenes" / "probe_tick.rxscene").write_text(json.dumps(scene), encoding="utf-8")

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/probe_tick.rxscene"})
    m.call("play_enter", {})
    time.sleep(3.0)
    ev = m.call("host_events_drain", {})
    if isinstance(ev, dict):
        ev = ev.get("events", [])
    ticks = [e for e in ev if isinstance(e, dict) and e.get("event") == "logic.log" and e.get("message") == "TICK"]
    upds = [e for e in ev if isinstance(e, dict) and e.get("event") == "logic.update"]
    print(f"over 3s: logic.update={len(upds)}  TICK logs={len(ticks)}")
    m.call("play_exit", {})
finally:
    m.close()
