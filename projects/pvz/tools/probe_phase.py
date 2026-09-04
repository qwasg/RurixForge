# probe_phase.py — 检查 br_phase 条件引用与 on_start 链
import json, pathlib

g = json.loads(pathlib.Path(__file__).resolve().parent.parent.joinpath(
    "Content/Graphs/level_controller.rxgraph").read_text(encoding="utf-8"))
nodes = {n["id"]: n for n in g["nodes"]}
adj = {}
for e in g["edges"]:
    adj.setdefault(e["from"][0], []).append((e["from"][1], e["to"][0]))

# 找 on_update 的 branch(phase 门)
upd = next(n["id"] for n in g["nodes"] if n["type"] == "event.on_update")
print("on_update:", upd)
cur = adj.get(upd, [(None, None)])[0][1]
for i in range(8):
    if cur is None:
        break
    n = nodes.get(cur)
    fn = n.get("inputs", {}).get("fn", {})
    fns = fn.get("const", "") if isinstance(fn, dict) else ""
    name = n.get("inputs", {}).get("name", {})
    names = name.get("const", "") if isinstance(name, dict) else ""
    cond = n.get("inputs", {}).get("condition", {})
    conds = ""
    if isinstance(cond, dict) and cond.get("node"):
        conds = f"cond={cond['node']}.{cond.get('pin')}"
    print(f"{cur} {n['type']} fn={fns} name={names} {conds} -> {adj.get(cur)}")
    nxt = [t for p, t in adj.get(cur, []) if p == "exec"]
    cur = nxt[0] if nxt else None

# on_start 链
print("=== on_start chain ===")
st = next(n["id"] for n in g["nodes"] if n["type"] == "event.on_start")
cur = adj.get(st, [(None, None)])[0][1]
for i in range(20):
    if cur is None:
        print("END")
        break
    n = nodes.get(cur)
    name = n.get("inputs", {}).get("name", {})
    names = name.get("const", "") if isinstance(name, dict) else ""
    val = n.get("inputs", {}).get("value", {})
    vals = ""
    if isinstance(val, dict):
        vals = val.get("const", val.get("node", ""))
    print(f"{cur} {n['type']} name={names} val={vals}")
    nxt = [t for p, t in adj.get(cur, []) if p == "exec"]
    cur = nxt[0] if nxt else None
