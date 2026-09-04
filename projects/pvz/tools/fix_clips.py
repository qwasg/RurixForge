# fix_clips.py — 对已生成的 .rxsprite 按实际帧名重设 clips(不重新生成图)
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_assets import AssetMcp
from gen_cast import PILOT

m = AssetMcp()
try:
    for spec in PILOT:
        name = spec["name"]
        sprite_path = f"Sprites/{name}.rxsprite"
        sg = m.call("sprite_get", {"assetPath": sprite_path})
        doc = sg.get("doc") if isinstance(sg, dict) else None
        if not doc:
            print(f"{name}: no doc, skip")
            continue
        actual = sorted(doc.get("frames", {}).keys())
        def remap():
            out = {}
            for cname, cdef in spec["clips"].items():
                idxs = []
                for f in cdef["frames"]:
                    i = int(f.split("_")[1])
                    if i < len(actual):
                        idxs.append(actual[i])
                if idxs:
                    c2 = dict(cdef); c2["frames"] = idxs
                    out[cname] = c2
            return out
        doc["clips"] = remap()
        ss = m.call("sprite_set", {"assetPath": sprite_path, "doc": doc})
        ok = isinstance(ss, dict) and ss.get("ok")
        print(f"{name}: frames={len(actual)} clips={list(doc['clips'].keys())} ok={ok}" + ("" if ok else " " + json.dumps(ss, ensure_ascii=False)[:200]))
finally:
    m.close()
