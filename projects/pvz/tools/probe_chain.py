# probe_chain.py — 打印 on_input 起始链的节点与边
import json, pathlib

g = json.loads(pathlib.Path(__file__).resolve().parent.parent.joinpath(
    "Content/Graphs/level_controller.rxgraph").read_text(encoding="utf-8"))
nodes = {n["id"]: n for n in g["nodes"]}
adj = {}
for e in g["edges"]:
    adj.setdefault(e["from"][0], []).append((e["from"][1], e["to"][0]))

oi = next(n["id"] for n in g["nodes"] if n["type"] == "event.on_input")
print("on_input:", oi, "->", adj.get(oi))
cur = adj.get(oi, [(None, None)])[0][1]
for i in range(12):
    if cur is None:
        print("CHAIN ENDS")
        break
    n = nodes.get(cur)
    if not n:
        print(cur, "MISSING NODE")
        break
    fn = n.get("inputs", {}).get("fn", {})
    fns = fn.get("const", "") if isinstance(fn, dict) else ""
    name = n.get("inputs", {}).get("name", {})
    names = name.get("const", "") if isinstance(name, dict) else ""
    print(f"{cur} {n['type']} fn={fns} name={names} -> {adj.get(cur)}")
    nxt = [t for p, t in adj.get(cur, []) if p == "exec"]
    cur = nxt[0] if nxt else None
