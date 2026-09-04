# probe_graph2.py — 区分动作/纯节点的真实可达性 + 数据引用完整性
import json, pathlib, collections

g = json.loads(pathlib.Path(__file__).resolve().parent.parent.joinpath(
    "Content/Graphs/level_controller.rxgraph").read_text(encoding="utf-8"))
nodes = {n["id"]: n for n in g["nodes"]}
PURE = {"var.get", "entity.has_tag", "entity.find_by_tag", "entity.get_transform", "transform.lerp",
        "physics.cast_ray", "physics.overlap"}
adj = {}
for e in g["edges"]:
    adj.setdefault(e["from"][0], []).append(e["to"][0])

def is_pure(nid):
    return nodes[nid]["type"] in PURE

# 从全部事件源 BFS(仅沿 exec 边;纯节点经数据引用挂接,不算 exec 可达)
starts = [n["id"] for n in g["nodes"] if n["type"].startswith("event.")]
seen = set()
q = collections.deque(starts)
while q:
    x = q.popleft()
    if x in seen:
        continue
    seen.add(x)
    q.extend(adj.get(x, []))

# 检查:需要 exec 的节点(动作/流控)是否都可达
bad = []
for n in g["nodes"]:
    t = n["type"]
    if t in PURE or t.startswith("event."):
        continue
    if n["id"] not in seen:
        bad.append((n["id"], t))
print("非纯节点不可达:", len(bad))
for nid, t in bad[:20]:
    print("  UNREACH:", nid, t)

# 检查纯节点是否被引用(数据边)
referenced = set()
for n in g["nodes"]:
    for inp in (n.get("inputs") or {}).values():
        if isinstance(inp, dict) and inp.get("node"):
            referenced.add(inp["node"])
orphan_pure = [n["id"] for n in g["nodes"] if n["type"] in PURE and n["id"] not in referenced]
print("纯节点未被引用(孤儿):", len(orphan_pure), orphan_pure[:10])
