# probe_min_input.py — 最小 on_input→call_function 链路验证
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp
from pvz_mcp import Mcp as CodeMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 最小图:on_input → var.set(seen, value) → call unpack_action(seen) → debug.log
graph = {
    "version": 1, "id": "probe_input", "name": "probe_input",
    "exposedProps": [],
    "nodes": [
        {"id": "i", "type": "event.on_input", "pos": [0, 0]},
        {"id": "vs", "type": "var.set", "pos": [1, 0],
         "inputs": {"name": {"const": "seen"}, "value": {"node": "i", "pin": "value"}}},
        {"id": "cf", "type": "call.call_function", "pos": [2, 0],
         "inputs": {"module": {"const": "Content/Scripts/pvz_rules.rx"}, "fn": {"const": "unpack_action"},
                    "args": {"node": "vg", "pin": "out"}}},
        {"id": "vg", "type": "var.get", "pos": [1, 1], "inputs": {"name": {"const": "seen"}}},
        {"id": "dl", "type": "debug.log", "pos": [3, 0], "inputs": {"message": {"node": "cf", "pin": "result"}}},
    ],
    "edges": [
        {"from": ["i", "exec"], "to": ["vs", "exec"]},
        {"from": ["vs", "exec"], "to": ["cf", "exec"]},
        {"from": ["cf", "exec"], "to": ["dl", "exec"]},
    ],
}

# 落盘图(经 pvz 作用域 code-forge 校验+创建)
cm = CodeMcp()
try:
    v = cm.call("graph_validate", {"graph": graph})
    print("validate:", json.dumps(v, ensure_ascii=False)[:300])
    c = cm.call("graph_create", {"name": "probe_input", "graph": graph})
    print("create:", json.dumps(c, ensure_ascii=False)[:300])
finally:
    cm.close()

# 建场景 + 实体挂图 + play + inject
m = EngineMcp()
try:
    m.call("scene_new", {"name": "probe"})
    r = m.call("entity_create", {"name": "probe", "components": [
        {"type": "Script", "props": {"graphRef": "Content/Graphs/probe_input.rxgraph", "module": "", "props": {}}}],
        "translation": [0.0, 0.0, 0.0]})
    print("entity:", json.dumps(r, ensure_ascii=False)[:200])
    print("enter:", json.dumps(m.call("play_enter", {}), ensure_ascii=False)[:200])
    time.sleep(0.5)
    m.call("logic_inject_input", {"action": "plant_at", "value": 2020302.0})
    time.sleep(1.0)
    ev = m.call("host_events_drain", {})
    if isinstance(ev, dict):
        ev = ev.get("events", [])
    for e in ev:
        if isinstance(e, dict) and e.get("event") in ("logic.input", "logic.call", "logic.log", "logic.call_error", "logic.unsupported"):
            print(json.dumps(e, ensure_ascii=False)[:280])
    m.call("play_exit", {})
finally:
    m.close()
