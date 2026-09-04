# probe_gate.py — 最小复现:phase 门 + branch 每帧是否都进 then
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp
from pvz_mcp import Mcp as CodeMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent

# on_update → cmpT=0 → cmpT+=phase → cmpT+=-1 → is_zero(cmpT) → branch(then→log IN, else→log OUT)
# on_start → phase=1
graph = {
    "version": 1, "id": "probe_gate", "name": "probe_gate",
    "exposedProps": [],
    "nodes": [
        {"id": "st", "type": "event.on_start", "pos": [0, 0]},
        {"id": "sp", "type": "var.set", "pos": [1, 0], "inputs": {"name": {"const": "phase"}, "value": {"const": 1.0}}},
        {"id": "u", "type": "event.on_update", "pos": [0, 2]},
        {"id": "z", "type": "var.set", "pos": [1, 2], "inputs": {"name": {"const": "cmpT"}, "value": {"const": 0.0}}},
        {"id": "a1", "type": "var.add", "pos": [2, 2], "inputs": {"name": {"const": "cmpT"}, "value": {"node": "gp", "pin": "out"}}},
        {"id": "gp", "type": "var.get", "pos": [1, 3], "inputs": {"name": {"const": "phase"}}},
        {"id": "a2", "type": "var.add", "pos": [3, 2], "inputs": {"name": {"const": "cmpT"}, "value": {"const": -1.0}}},
        {"id": "iz", "type": "call.call_function", "pos": [4, 2],
         "inputs": {"module": {"const": "Content/Scripts/pvz_rules.rx"}, "fn": {"const": "is_zero"},
                    "args": {"node": "gc", "pin": "out"}}},
        {"id": "gc", "type": "var.get", "pos": [3, 3], "inputs": {"name": {"const": "cmpT"}}},
        {"id": "br", "type": "flow.branch", "pos": [5, 2], "inputs": {"condition": {"node": "iz", "pin": "result"}}},
        {"id": "lin", "type": "debug.log", "pos": [6, 1], "inputs": {"message": {"const": "IN"}}},
        {"id": "lout", "type": "debug.log", "pos": [6, 3], "inputs": {"message": {"const": "OUT"}}},
    ],
    "edges": [
        {"from": ["st", "exec"], "to": ["sp", "exec"]},
        {"from": ["u", "exec"], "to": ["z", "exec"]},
        {"from": ["z", "exec"], "to": ["a1", "exec"]},
        {"from": ["a1", "exec"], "to": ["a2", "exec"]},
        {"from": ["a2", "exec"], "to": ["iz", "exec"]},
        {"from": ["iz", "exec"], "to": ["br", "exec"]},
        {"from": ["br", "then"], "to": ["lin", "exec"]},
        {"from": ["br", "else"], "to": ["lout", "exec"]},
    ],
}
cm = CodeMcp()
try:
    print("validate:", cm.call("graph_validate", {"graph": graph}))
    cm.call("graph_create", {"name": "probe_gate", "graph": graph})
finally:
    cm.close()

scene = {"mode": "2d", "name": "probe_gate", "entities": [
    {"id": 1, "name": "T", "transform": {"rotation": [0,0,0,1], "scale": [1,1,1], "translation": [0,0,0]},
     "components": [{"enabled": True, "type": "Script",
                     "props": {"graphRef": "Content/Graphs/probe_gate.rxgraph", "module": "", "props": {}}}]},
]}
(ROOT / "Content" / "Scenes" / "probe_gate.rxscene").write_text(json.dumps(scene), encoding="utf-8")

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/probe_gate.rxscene"})
    m.call("play_enter", {})
    time.sleep(3.0)
    ev = m.call("host_events_drain", {})
    if isinstance(ev, dict):
        ev = ev.get("events", [])
    ins = [e for e in ev if isinstance(e, dict) and e.get("message") == "IN"]
    outs = [e for e in ev if isinstance(e, dict) and e.get("message") == "OUT"]
    calls = [e for e in ev if isinstance(e, dict) and e.get("event") == "logic.call"]
    print(f"over 3s: IN={len(ins)} OUT={len(outs)} calls={len(calls)}")
    if calls:
        print("sample call:", json.dumps(calls[0], ensure_ascii=False)[:240])
    m.call("play_exit", {})
finally:
    m.close()
