# probe_update.py — 打印 on_update 链路与 br_phase 接线
import json, pathlib

g = json.loads(pathlib.Path(__file__).resolve().parent.parent.joinpath(
    "Content/Graphs/level_controller.rxgraph").read_text(encoding="utf-8"))
nodes = {n["id"]: n for n in g["nodes"]}
adj = {}
for e in g["edges"]:
    adj.setdefault(e["from"][0], []).append((e["from"][1], e["to"][0]))

def ntype(nid):
    n = nodes.get(nid)
    if not n:
        return "MISSING"
    t = n["type"]
    fn = n.get("inputs", {}).get("fn", {})
    if isinstance(fn, dict) and fn.get("const"):
        t += f"({fn['const']})"
    return t

oi = next(n["id"] for n in g["nodes"] if n["type"] == "event.on_update")
print("on_update:", oi, "->", [(p, t, ntype(t)) for p, t in adj.get(oi, [])])
# 走 10 层
def walk(start, depth, prefix=""):
    if depth > 6:
        return
    for pin, tgt in adj.get(start, []):
        print(f"{prefix}{pin} -> {tgt} {ntype(tgt)}")
        if pin in ("exec", "seq0", "seq1", "then", "else"):
            walk(tgt, depth + 1, prefix + "  ")
walk(oi, 0)
print("=== deep walk from n26 ===")
def walk2(start, depth, prefix=""):
    if depth > 14:
        return
    for pin, tgt in adj.get(start, []):
        print(f"{prefix}{pin} -> {tgt} {ntype(tgt)}")
        walk2(tgt, depth + 1, prefix + "  ")
walk2("n26", 0)
# 找 n24 是什么
print("n24 =", ntype("n24"), "inputs:", json.dumps(nodes["n24"].get("inputs", {}), ensure_ascii=False)[:200])
print("n24 edges:", adj.get("n24"))
