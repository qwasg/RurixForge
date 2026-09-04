# probe_graph.py — 追踪 level_controller 的 plant_at 链路断点
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
    tg = n.get("inputs", {}).get("tag", {})
    if isinstance(tg, dict) and tg.get("const"):
        t += f"[{tg['const']}]"
    return t

# 找 afford_packed 的 branch
afcall = next(n["id"] for n in g["nodes"]
              if n["type"] == "call.call_function" and n.get("inputs", {}).get("fn", {}).get("const") == "afford_packed")
br_af = next(n["id"] for n in g["nodes"]
             if isinstance(n.get("inputs", {}).get("condition"), dict) and n["inputs"]["condition"].get("node") == afcall)
print("br_af:", br_af, "edges:", [(p, t, ntype(t)) for p, t in adj.get(br_af, [])])
# 沿 then 走 5 层
def walk(start, depth, prefix=""):
    if depth > 5:
        return
    for pin, tgt in adj.get(start, []):
        print(f"{prefix}{pin} -> {tgt} {ntype(tgt)}")
        walk(tgt, depth + 1, prefix + "  ")
walk(br_af, 0)
print("=== 从 br_af BFS 到首个 plant_free finder 的路径 ===")
# BFS 找 plant_free finder
target = next(n["id"] for n in g["nodes"]
              if n["type"] == "entity.find_by_tag" and "plant_free" in str(n.get("inputs", {}).get("tag", {})))
import collections
prev = {}
q = collections.deque([br_af])
seen = {br_af}
while q:
    x = q.popleft()
    if x == target:
        break
    for pin, y in adj.get(x, []):
        if y not in seen:
            seen.add(y); prev[y] = x; q.append(y)
if target in seen:
    path = []
    x = target
    while x != br_af:
        path.append(x); x = prev[x]
    path.append(br_af)
    path.reverse()
    print("PATH:", " -> ".join(f"{n}({ntype(n)})" for n in path))
else:
    print("plant_free finder UNREACHABLE from br_af")
    # 找断点:从 target 反向找谁引用它
    refs = [e["from"] for e in g["edges"] if e["to"][0] == target]
    print("finder's incoming edges from:", [(f[0], f[1], ntype(f[0])) for f in refs])
