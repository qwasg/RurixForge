# gen_cast.py — PvZ 角色图集生成驱动:gen_image → accept → autoslice → sprite_create → clips
# 遵守 game-2d-kit 纪律:整表一次生成、纯品红底 #FF00FF、containment、侧视朝右。
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_gen import GenMcp
from pvz_assets import AssetMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent

MAGENTA = "solid magenta background (#FF00FF), flat uniform, no gradients, no shadows on background"
STYLE = "Plants vs Zombies style cartoon game sprite, bold outlines, flat cel shading, side view facing right, full body visible"

# 试点卡司:(名字, 网格列x行, 帧描述, clip 定义)
PILOT = [
    {"name": "PZ_Peashooter", "cols": 2, "rows": 2, "size": 512,
     "prompt": f"peashooter plant character, green pea plant with tube head shooting peas, {STYLE}, 2x2 grid, exactly 4 frames: top row 2 idle bobbing frames, bottom row 2 shooting recoil frames, same character same scale same pose base, frames separated by pure background gaps, no text no borders no gridlines, {MAGENTA}",
     "clips": {"idle": {"frames": ["frame_0", "frame_1"], "fps": 6, "loop": True},
               "attack": {"frames": ["frame_2", "frame_3"], "fps": 8, "loop": False, "onFinish": "first"}}},
    {"name": "PZ_Sunflower", "cols": 2, "rows": 2, "size": 512,
     "prompt": f"sunflower plant character, yellow petals brown center smiling, {STYLE}, 2x2 grid, exactly 4 frames: top row 2 idle sway frames, bottom row 2 sun-production glow frames, same character same scale, frames separated by pure background gaps, no text no borders no gridlines, {MAGENTA}",
     "clips": {"idle": {"frames": ["frame_0", "frame_1"], "fps": 6, "loop": True},
               "produce": {"frames": ["frame_2", "frame_3"], "fps": 8, "loop": False, "onFinish": "first"}}},
    {"name": "PZ_Wallnut", "cols": 2, "rows": 1, "size": 512,
     "prompt": f"wall-nut plant character, round brown walnut with calm face, {STYLE}, 1x2 grid, exactly 2 frames side by side: 2 idle subtle wobble frames, same character same scale, frames separated by pure background gap, no text no borders no gridlines, {MAGENTA}",
     "clips": {"idle": {"frames": ["frame_0", "frame_1"], "fps": 4, "loop": True}}},
    {"name": "PZ_Zombie", "cols": 4, "rows": 2, "size": 1024,
     "prompt": f"cartoon zombie character, gray-green skin tattered brown coat, arms outstretched forward, {STYLE}, 2 rows x 4 columns grid, exactly 8 frames: top row 4 walking gait cycle frames (last frame returning toward first for seamless loop), bottom row first 2 frames eating chomping pose, last 2 frames collapsing death, same character same scale same direction, frames separated by pure background gaps, no text no borders no gridlines, {MAGENTA}",
     "clips": {"walk": {"frames": ["frame_0", "frame_1", "frame_2", "frame_3"], "fps": 8, "loop": True},
               "eat": {"frames": ["frame_4", "frame_5"], "fps": 6, "loop": True},
               "die": {"frames": ["frame_6", "frame_7"], "fps": 6, "loop": False, "onFinish": "hold"}}},
    {"name": "PZ_Pea", "cols": 1, "rows": 1, "size": 256,
     "prompt": f"single round green pea projectile with slight motion shine, {STYLE}, single frame centered, {MAGENTA}",
     "clips": {"idle": {"frames": ["frame_0"], "fps": 1, "loop": True}}},
    {"name": "PZ_Sun", "cols": 2, "rows": 1, "size": 256,
     "prompt": f"glowing sun orb, bright yellow with warm rays and sparkle, {STYLE}, 1x2 grid, exactly 2 frames side by side: 2 pulsing glow frames, same size, separated by pure background gap, no text no borders, {MAGENTA}",
     "clips": {"idle": {"frames": ["frame_0", "frame_1"], "fps": 4, "loop": True}}},
]

def gen_one(gen, assets, spec, retries=3):
    name = spec["name"]
    print(f"=== {name} ===", flush=True)
    cands = None
    for attempt in range(retries):
        r = gen.call("gen_image", {"prompt": spec["prompt"], "size": spec["size"], "n": 1})
        if isinstance(r, dict) and r.get("candidates"):
            cands = r["candidates"]
            break
        print(f"  gen attempt {attempt+1} failed: {json.dumps(r, ensure_ascii=False)[:200]}", flush=True)
        time.sleep(5)
    if not cands:
        return {"name": name, "ok": False, "error": "gen_image failed after retries"}
    ref = cands[0]["imageFileRef"]
    acc = gen.call("gen_accept", {"imageFileRef": ref, "destFolder": "Textures", "name": name})
    if not isinstance(acc, dict) or not acc.get("guid"):
        return {"name": name, "ok": False, "error": f"accept failed: {json.dumps(acc, ensure_ascii=False)[:200]}"}
    guid = acc["guid"]
    tex_path = acc["assetPath"]
    # autoslice 预览
    prev = assets.call("sprite_autoslice", {"assetPath": tex_path})
    boxes = prev.get("boxes", []) if isinstance(prev, dict) else []
    expect = spec["cols"] * spec["rows"]
    print(f"  guid={guid[:8]} autoslice boxes={len(boxes)} expect={expect}", flush=True)
    # sprite_create(autoslice)
    sc = assets.call("sprite_create", {"name": name, "texture": guid, "autoslice": True})
    if not isinstance(sc, dict) or sc.get("error"):
        return {"name": name, "ok": False, "error": f"sprite_create: {json.dumps(sc, ensure_ascii=False)[:200]}"}
    sprite_path = sc.get("assetPath") or f"Sprites/{name}.rxsprite"
    # sprite_set 写 clips
    sg = assets.call("sprite_get", {"assetPath": sprite_path})
    doc = sg.get("doc") if isinstance(sg, dict) else None
    clip_result = None
    if doc:
        # 实际帧名(零填充 frame_00..)按位置重映射 clip 引用
        actual = sorted(doc.get("frames", {}).keys())
        def remap(frames_spec):
            # clips 里的 frame_<i> → 实际第 i 个帧名(存在才映射)
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
        doc["clips"] = remap(spec["clips"])
        ss = assets.call("sprite_set", {"assetPath": sprite_path, "doc": doc})
        clip_result = ss
    return {"name": name, "ok": True, "guid": guid, "boxes": len(boxes), "expect": expect,
            "sprite": sprite_path, "frames": len((doc or {}).get("frames", {})), "clip_set": clip_result}

def main():
    only = sys.argv[1] if len(sys.argv) > 1 else None
    gen = GenMcp()
    assets = AssetMcp()
    results = []
    try:
        for spec in PILOT:
            if only and spec["name"] != only:
                continue
            results.append(gen_one(gen, assets, spec))
    finally:
        gen.close(); assets.close()
    print("=== RESULTS ===", flush=True)
    for r in results:
        print(json.dumps(r, ensure_ascii=False), flush=True)
    (ROOT / ".forge" / "tmp" / "gen_cast_results.json").write_text(
        json.dumps(results, ensure_ascii=False, indent=1), encoding="utf-8")

if __name__ == "__main__":
    main()
