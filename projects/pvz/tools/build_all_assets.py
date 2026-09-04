# build_all_assets.py — 全量 PvZ 冒险素材生成与规范化
# 目标:49 植物 + 26 僵尸 + 5 场景背景 + UI/7x7 种子卡图集。
# 生成策略:每组 8 个角色由 gpt-image-2 一次生成一致风格源图；随后确定性裁格、
# 生成精确网格帧动画与 .rxsprite。所有派生资产保留 gen-image provenance。
from __future__ import annotations

import json
import math
import pathlib
import sys
import time
import uuid
from typing import Any

from PIL import Image, ImageEnhance

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_gen import GenMcp
from pvz_assets import AssetMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTENT = ROOT / "Content"
TEXTURES = CONTENT / "Textures"
SPRITES = CONTENT / "Sprites"
SOURCE = TEXTURES / "SourceAtlases"
DATA = ROOT / "docs" / "data"
MANIFEST_PATH = CONTENT / "asset_manifest.json"
NS = uuid.UUID("12345678-5356-505a-2009-000000000001")
MAGENTA = (255, 0, 255, 255)

PLANTS = json.loads((DATA / "plants.json").read_text(encoding="utf-8"))
ZOMBIES = json.loads((DATA / "zombies.json").read_text(encoding="utf-8"))
STAGES = json.loads((DATA / "stages.json").read_text(encoding="utf-8"))

PILOT_PLANTS = {
    "peashooter": "PZ_Peashooter",
    "sunflower": "PZ_Sunflower",
    "wall_nut": "PZ_Wallnut",
}
PILOT_ZOMBIES = {"zombie": "PZ_Zombie"}

PLANT_VISUAL = {
    "peashooter": "green pea-shooting plant with tube head",
    "sunflower": "smiling yellow sunflower with brown center",
    "cherry_bomb": "pair of red cherries with fuse and determined faces",
    "wall_nut": "round brown walnut barrier with calm face",
    "potato_mine": "small potato land mine with red detonator",
    "snow_pea": "icy blue pea shooter plant",
    "chomper": "purple carnivorous flytrap plant with teeth",
    "repeater": "green double-barrel pea shooter",
    "puff_shroom": "tiny purple puff mushroom",
    "sun_shroom": "small golden sun mushroom",
    "fume_shroom": "large purple fume mushroom",
    "grave_buster": "hungry pale grave-eating plant",
    "hypno_shroom": "spiral-eyed rainbow hypno mushroom",
    "scaredy_shroom": "tall timid purple mushroom",
    "ice_shroom": "frosty cyan mushroom",
    "doom_shroom": "dark explosive mushroom with red eyes",
    "lily_pad": "green water lily pad with face",
    "squash": "angry green squash vegetable",
    "threepeater": "three-headed pea shooter plant",
    "tangle_kelp": "green underwater kelp tendrils",
    "jalapeno": "angry red jalapeno pepper",
    "spikeweed": "low spiky grass trap",
    "torchwood": "burning tree stump with face",
    "tall_nut": "very tall brown walnut wall",
    "sea_shroom": "small blue aquatic mushroom",
    "plantern": "glowing lantern flower",
    "cactus": "green cactus plant with pink flower",
    "blover": "green four-leaf clover fan plant",
    "split_pea": "two-headed front-and-back pea shooter",
    "starfruit": "yellow star fruit plant",
    "pumpkin": "orange protective pumpkin shell",
    "magnet_shroom": "red horseshoe magnet mushroom",
    "cabbage_pult": "green cabbage catapult plant",
    "kernel_pult": "yellow corn kernel catapult plant",
    "coffee_bean": "small brown coffee bean with face",
    "garlic": "white garlic bulb with strong smell",
    "umbrella_leaf": "broad green umbrella leaf plant",
    "marigold": "orange marigold coin-producing flower",
    "melon_pult": "green watermelon catapult plant",
    "gatling_pea": "four-barrel military gatling pea plant",
    "twin_sunflower": "two-headed smiling sunflower",
    "gloom_shroom": "dark purple gloom mushroom with fumes",
    "cattail": "pink cattail cat plant on lily pad",
    "winter_melon": "icy blue winter melon catapult",
    "gold_magnet": "gold coin magnet mushroom",
    "spikerock": "jagged stone spike trap",
    "cob_cannon": "large twin corn cob cannon",
    "imitater": "gray mime potato imitating a plant",
    "flower_pot": "terracotta flower pot with face",
}

ZOMBIE_VISUAL = {
    "zombie": "basic gray-green zombie in torn brown suit",
    "flag_zombie": "zombie carrying a red brain flag",
    "conehead_zombie": "zombie wearing orange traffic cone",
    "pole_vaulting_zombie": "athletic zombie carrying vaulting pole",
    "buckethead_zombie": "zombie wearing metal bucket helmet",
    "newspaper_zombie": "old zombie holding newspaper",
    "screen_door_zombie": "zombie carrying screen door shield",
    "football_zombie": "red-uniform armored football zombie",
    "dancing_zombie": "disco dancing zombie in white suit",
    "backup_dancer": "backup dancer zombie",
    "ducky_tube_zombie": "zombie wearing yellow duck swim ring",
    "snorkel_zombie": "zombie with snorkel and goggles",
    "zomboni": "zombie driving ice resurfacing machine",
    "bobsled_team": "four zombies riding red bobsled",
    "dolphin_rider_zombie": "zombie riding cartoon dolphin",
    "jack_in_the_box_zombie": "zombie holding jack-in-the-box",
    "balloon_zombie": "zombie floating under red balloon",
    "digger_zombie": "miner zombie with pickaxe and helmet",
    "pogo_zombie": "zombie bouncing on pogo stick",
    "zombie_yeti": "white furry yeti zombie",
    "bungee_zombie": "zombie descending on bungee cord",
    "ladder_zombie": "zombie carrying wooden ladder",
    "catapult_zombie": "zombie driving basketball catapult",
    "gargantuar": "huge muscular gargantuar zombie with club",
    "imp": "tiny imp zombie",
    "dr_zomboss": "mad zombie scientist inside giant robot head",
}


def camel(s: str) -> str:
    return "".join(x.capitalize() for x in s.split("_"))


def stable_guid(rel: str) -> str:
    return str(uuid.uuid5(NS, rel.replace("\\", "/").lower()))


def write_meta(path: pathlib.Path, guid: str, atype: str, importer: str,
               source_asset: str, detail: str) -> None:
    text = (
        f"guid: {guid}\n"
        f"type: {atype}\n"
        f"importer: {importer}\n"
        "provenance:\n"
        "  origin: gen-image\n"
        "  detail:\n"
        "    backendId: remote-openai-compatible\n"
        f"    sourceAsset: {source_asset}\n"
        f"    pipeline: {detail}\n"
        "build_state: current\n"
    )
    pathlib.Path(str(path) + ".meta").write_text(text, encoding="utf-8")


def source_asset_ready(name: str) -> pathlib.Path | None:
    p = SOURCE / f"{name}.png"
    return p if p.is_file() and p.stat().st_size > 1024 else None


def generate_source(gen: GenMcp, name: str, prompt: str, seed: int) -> pathlib.Path:
    SOURCE.mkdir(parents=True, exist_ok=True)
    ready = source_asset_ready(name)
    if ready:
        print(f"[reuse] {name}", flush=True)
        return ready
    last: Any = None
    for attempt in range(1, 5):
        print(f"[gen] {name} attempt={attempt}", flush=True)
        last = gen.call("gen_image", {
            "prompt": prompt,
            "negativePrompt": "text, labels, borders, grid lines, photorealism, cropped body, duplicate character, transparent checkerboard",
            "size": 1024,
            "n": 1,
            "seed": seed,
            "backend": "remote-openai-compatible",
        })
        cands = last.get("candidates") if isinstance(last, dict) else None
        if cands:
            acc = gen.call("gen_accept", {
                "imageFileRef": cands[0]["imageFileRef"],
                "destFolder": "Textures/SourceAtlases",
                "name": name,
            })
            if isinstance(acc, dict) and acc.get("assetPath"):
                p = CONTENT / acc["assetPath"]
                if p.is_file():
                    return p
        print(f"  failed: {json.dumps(last, ensure_ascii=False)[:240]}", flush=True)
        time.sleep(4 * attempt)
    raise RuntimeError(f"generation failed: {name}: {last}")


def fit_character(crop: Image.Image, size: int = 210) -> Image.Image:
    src = crop.convert("RGBA")
    # Preserve the AI magenta key background; fit crop into a square cell.
    src.thumbnail((size, size), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (256, 256), MAGENTA)
    out.alpha_composite(src, ((256 - src.width) // 2, 256 - src.height - 12))
    return out


def animate_plant(base: Image.Image) -> Image.Image:
    sheet = Image.new("RGBA", (512, 512), MAGENTA)
    transforms = [(0, 0, 0), (0, -6, 1), (5, -2, 3), (-5, 0, -3)]
    for i, (dx, dy, ang) in enumerate(transforms):
        f = base.rotate(ang, resample=Image.Resampling.BICUBIC, expand=False, fillcolor=MAGENTA)
        cell = Image.new("RGBA", (256, 256), MAGENTA)
        cell.alpha_composite(f, (dx, dy))
        sheet.alpha_composite(cell, ((i % 2) * 256, (i // 2) * 256))
    return sheet


def animate_zombie(base: Image.Image) -> Image.Image:
    sheet = Image.new("RGBA", (1024, 512), MAGENTA)
    specs = [(-6, 0, -2), (0, -5, 1), (6, 0, 2), (0, -2, -1),
             (10, 0, 2), (14, -2, -2), (0, 20, 25), (4, 42, 70)]
    for i, (dx, dy, ang) in enumerate(specs):
        f = base.rotate(ang, resample=Image.Resampling.BICUBIC, expand=False, fillcolor=MAGENTA)
        cell = Image.new("RGBA", (256, 256), MAGENTA)
        cell.alpha_composite(f, (dx, dy))
        sheet.alpha_composite(cell, ((i % 4) * 256, (i // 4) * 256))
    return sheet


def plant_clips(p: dict[str, Any]) -> dict[str, Any]:
    clips: dict[str, Any] = {
        "idle": {"frames": ["frame_0", "frame_1"], "fps": 6, "loop": True}
    }
    if p.get("production") is not None:
        clips["produce"] = {"frames": ["frame_2", "frame_3"], "fps": 8, "loop": False, "onFinish": "first"}
    if float(p.get("damage") or 0) > 0 or float(p.get("attack_interval_sec") or 0) > 0:
        clips["attack"] = {"frames": ["frame_2", "frame_3"], "fps": 8, "loop": False, "onFinish": "first"}
    clips["special"] = {"frames": ["frame_2", "frame_3"], "fps": 7, "loop": False, "onFinish": "hold"}
    return clips


def zombie_clips(z: dict[str, Any]) -> dict[str, Any]:
    return {
        "walk": {"frames": ["frame_0", "frame_1", "frame_2", "frame_3"], "fps": 8, "loop": True},
        "eat": {"frames": ["frame_4", "frame_5"], "fps": 6, "loop": True},
        "die": {"frames": ["frame_6", "frame_7"], "fps": 6, "loop": False, "onFinish": "hold"},
        "special": {"frames": ["frame_4", "frame_5"], "fps": 7, "loop": False, "onFinish": "first"},
    }


def write_sprite(asset_name: str, texture_name: str, frames: dict[str, Any],
                 clips: dict[str, Any], source_asset: str) -> dict[str, Any]:
    tex_rel = f"Textures/{texture_name}.png"
    spr_rel = f"Sprites/{asset_name}.rxsprite"
    tex_guid = stable_guid(tex_rel)
    spr_guid = stable_guid(spr_rel)
    tex_path = CONTENT / tex_rel
    spr_path = CONTENT / spr_rel
    write_meta(tex_path, tex_guid, "texture", "png", source_asset, "ai-atlas-grid-crop")
    # Clip 引用必须落在实际帧集合；单帧/双帧先导素材缺动作帧时回落首帧，
    # 不保留悬空引用（SPRITE_INVALID）。
    frame_names = set(frames)
    safe_clips: dict[str, Any] = {}
    first_frame = next(iter(frames))
    for cname, cdef in clips.items():
        names = [f for f in cdef.get("frames", []) if f in frame_names]
        c2 = dict(cdef)
        c2["frames"] = names or [first_frame]
        safe_clips[cname] = c2
    doc = {
        "version": 1,
        "texture": tex_guid,
        "pivot": [0.5, 1.0],
        "frames": frames,
        "clips": safe_clips,
    }
    spr_path.write_text(json.dumps(doc, ensure_ascii=False, indent=2), encoding="utf-8")
    write_meta(spr_path, spr_guid, "sprite", "sprite", source_asset, "deterministic-frame-sheet")
    return {"texture": tex_rel, "textureGuid": tex_guid, "sprite": spr_rel,
            "spriteGuid": spr_guid, "clips": sorted(clips)}


def normalize_pilot() -> dict[str, Any]:
    result: dict[str, Any] = {}
    specs = {
        "peashooter": ("PZ_Peashooter", 2, 2, plant_clips(PLANTS[0])),
        "sunflower": ("PZ_Sunflower", 2, 2, plant_clips(PLANTS[1])),
        "wall_nut": ("PZ_Wallnut", 2, 1, plant_clips(next(p for p in PLANTS if p["id"] == "wall_nut"))),
    }
    for pid, (name, cols, rows, clips) in specs.items():
        tex = TEXTURES / f"{name}.png"
        if not tex.is_file():
            continue
        im = Image.open(tex).convert("RGBA")
        fw, fh = im.width // cols, im.height // rows
        frames = {f"frame_{i}": {"bbox": [(i % cols) * fw, (i // cols) * fh, fw, fh]}
                  for i in range(cols * rows)}
        result[pid] = write_sprite(name, name, frames, clips,
                                   f"Textures/{name}.png")
    # Existing regular zombie.
    tex = TEXTURES / "PZ_Zombie.png"
    if tex.is_file():
        im = Image.open(tex).convert("RGBA")
        frames = {f"frame_{i}": {"bbox": [(i % 4) * (im.width // 4), (i // 4) * (im.height // 2),
                                                    im.width // 4, im.height // 2]}
                  for i in range(8)}
        result["zombie"] = write_sprite("PZ_Zombie", "PZ_Zombie", frames,
                                         zombie_clips(ZOMBIES[0]), "Textures/PZ_Zombie.png")
    return result


def grouped(items: list[Any], n: int) -> list[list[Any]]:
    return [items[i:i+n] for i in range(0, len(items), n)]


def group_prompt(kind: str, group: list[dict[str, Any]], cols: int, rows: int) -> str:
    desc = []
    table = PLANT_VISUAL if kind == "plant" else ZOMBIE_VISUAL
    for i, item in enumerate(group, 1):
        desc.append(f"{i}:{table.get(item['id'], item['name_zh'])}")
    role = "garden defense plant icon" if kind == "plant" else "invading zombie full-body icon"
    return (f"Plants-vs-Zombies-inspired original cartoon game sprites, {cols}x{rows} equal grid, "
            f"row-major distinct {role}s: {'; '.join(desc)}. Bold dark outlines, cel shading, "
            "side view facing right, same scale, each centered inside its own cell, full body visible, "
            "solid #FF00FF magenta background in every cell, generous empty gaps, no text, no borders, no grid lines.")


def build_character_groups(gen: GenMcp, kind: str, items: list[dict[str, Any]],
                           existing: dict[str, Any]) -> dict[str, Any]:
    todo = [x for x in items if x["id"] not in existing]
    out = dict(existing)
    for gi, group in enumerate(grouped(todo, 8), 1):
        cols, rows = 4, 2
        atlas_name = f"PZ_{kind.capitalize()}Group_{gi:02d}"
        atlas = generate_source(gen, atlas_name, group_prompt(kind, group, cols, rows),
                                seed=200900 + (0 if kind == "plant" else 1000) + gi)
        src = Image.open(atlas).convert("RGBA")
        cw, ch = src.width // cols, src.height // rows
        for i, item in enumerate(group):
            crop = src.crop(((i % cols) * cw, (i // cols) * ch,
                             (i % cols + 1) * cw, (i // cols + 1) * ch))
            base = fit_character(crop)
            if kind == "plant":
                sheet = animate_plant(base)
                name = f"PZ_Plant_{camel(item['id'])}"
                frames = {f"frame_{j}": {"bbox": [(j % 2) * 256, (j // 2) * 256, 256, 256]}
                          for j in range(4)}
                clips = plant_clips(item)
            else:
                sheet = animate_zombie(base)
                name = f"PZ_Zombie_{camel(item['id'])}"
                frames = {f"frame_{j}": {"bbox": [(j % 4) * 256, (j // 4) * 256, 256, 256]}
                          for j in range(8)}
                clips = zombie_clips(item)
            tex_path = TEXTURES / f"{name}.png"
            sheet.save(tex_path)
            out[item["id"]] = write_sprite(name, name, frames, clips,
                                             f"Textures/SourceAtlases/{atlas_name}.png")
            print(f"  [{kind}] {item['id']} -> {name}", flush=True)
    return out


def build_stage_backgrounds(gen: GenMcp) -> dict[str, Any]:
    prompt = (
        "Plants-vs-Zombies-inspired original cartoon battlefield backgrounds in a 3x2 equal grid, "
        "row-major panels: sunny front lawn, moonlit graveyard lawn, backyard pool with six lanes, "
        "foggy pool at night, sloped tiled rooftop, empty bonus panel. Wide game boards, no characters, "
        "nine planting columns, bold friendly shapes, no text, no borders, each panel visually distinct."
    )
    atlas_name = "PZ_StageBackgrounds"
    atlas = generate_source(gen, atlas_name, prompt, seed=200950)
    src = Image.open(atlas).convert("RGB")
    cols, rows = 3, 2
    cw, ch = src.width // cols, src.height // rows
    out: dict[str, Any] = {}
    for i, stage in enumerate(STAGES):
        crop = src.crop(((i % cols) * cw, (i // cols) * ch,
                         (i % cols + 1) * cw, (i // cols + 1) * ch))
        crop = crop.resize((1024, 576), Image.Resampling.LANCZOS).convert("RGBA")
        name = f"PZ_Stage_{camel(stage['id'])}"
        p = TEXTURES / f"{name}.png"
        crop.save(p)
        guid = stable_guid(f"Textures/{name}.png")
        write_meta(p, guid, "texture", "png", f"Textures/SourceAtlases/{atlas_name}.png",
                   "ai-background-grid-crop")
        out[stage["id"]] = {"texture": f"Textures/{name}.png", "textureGuid": guid}
    return out


def build_ui(gen: GenMcp, plants_manifest: dict[str, Any]) -> dict[str, Any]:
    prompt = (
        "Plants-vs-Zombies-inspired original game UI icon atlas, 4x4 equal grid: wooden seed card, "
        "metal shovel, yellow sun counter, green wave meter, play button, pause button, lock, trophy, "
        "coin, watering can, almanac book, selection glow, conveyor belt, warning flag, win ribbon, "
        "game-over stone tablet. Bold cartoon icons, dark outlines, #FF00FF background, no text or borders."
    )
    atlas_name = "PZ_UI_Atlas"
    atlas = generate_source(gen, atlas_name, prompt, seed=200960)
    # Keep accepted atlas as UI texture and write exact 4x4 sprite document.
    im = Image.open(atlas)
    frames = {}
    names = ["seed_card", "shovel", "sun_counter", "wave_meter", "play", "pause", "lock", "trophy",
             "coin", "watering_can", "almanac", "selection", "conveyor", "warning_flag", "win", "game_over"]
    cw, ch = im.width // 4, im.height // 4
    for i, fname in enumerate(names):
        frames[fname] = {"bbox": [(i % 4) * cw, (i // 4) * ch, cw, ch]}
    ui_guid = stable_guid("Sprites/PZ_UI_Atlas.rxsprite")
    tex_guid = None
    meta = pathlib.Path(str(atlas) + ".meta")
    if meta.is_file():
        for line in meta.read_text(encoding="utf-8").splitlines():
            if line.startswith("guid:"):
                tex_guid = line.split(":", 1)[1].strip()
                break
    if not tex_guid:
        tex_guid = stable_guid("Textures/SourceAtlases/PZ_UI_Atlas.png")
    ui_doc = {"version": 1, "texture": tex_guid, "pivot": [0.5, 0.5],
              "frames": frames, "clips": {}}
    ui_path = SPRITES / "PZ_UI_Atlas.rxsprite"
    ui_path.write_text(json.dumps(ui_doc, ensure_ascii=False, indent=2), encoding="utf-8")
    write_meta(ui_path, ui_guid, "sprite", "sprite", "Textures/SourceAtlases/PZ_UI_Atlas.png",
               "exact-4x4-ui-grid")

    # 7x7 seed cards composed from each plant's generated frame 0.
    seed = Image.new("RGBA", (7 * 128, 7 * 128), (80, 55, 30, 255))
    seed_frames: dict[str, Any] = {}
    for i, plant in enumerate(PLANTS):
        info = plants_manifest[plant["id"]]
        tex_path = CONTENT / info["texture"]
        spr_doc = json.loads((CONTENT / info["sprite"]).read_text(encoding="utf-8"))
        box = spr_doc["frames"][sorted(spr_doc["frames"])[0]]["bbox"]
        tex = Image.open(tex_path).convert("RGBA")
        icon = tex.crop((box[0], box[1], box[0] + box[2], box[1] + box[3]))
        icon.thumbnail((104, 92), Image.Resampling.LANCZOS)
        cell = Image.new("RGBA", (128, 128), (121, 83, 45, 255))
        cell.alpha_composite(icon, ((128 - icon.width) // 2, 8))
        x, y = (i % 7) * 128, (i // 7) * 128
        seed.alpha_composite(cell, (x, y))
        seed_frames[plant["id"]] = {"bbox": [x, y, 128, 128]}
    seed_path = TEXTURES / "PZ_SeedCards.png"
    seed.save(seed_path)
    seed_tex_guid = stable_guid("Textures/PZ_SeedCards.png")
    write_meta(seed_path, seed_tex_guid, "texture", "png", "generated plant sprite collection",
               "ai-derived-7x7-seed-card-atlas")
    seed_spr_path = SPRITES / "PZ_SeedCards.rxsprite"
    seed_spr_guid = stable_guid("Sprites/PZ_SeedCards.rxsprite")
    seed_spr_path.write_text(json.dumps({"version": 1, "texture": seed_tex_guid, "pivot": [0.5, 0.5],
                                         "frames": seed_frames, "clips": {}},
                                        ensure_ascii=False, indent=2), encoding="utf-8")
    write_meta(seed_spr_path, seed_spr_guid, "sprite", "sprite", "Textures/PZ_SeedCards.png",
               "exact-7x7-seed-card-grid")
    return {
        "uiAtlas": {"texture": "Textures/SourceAtlases/PZ_UI_Atlas.png",
                    "sprite": "Sprites/PZ_UI_Atlas.rxsprite", "spriteGuid": ui_guid},
        "seedCards": {"texture": "Textures/PZ_SeedCards.png", "textureGuid": seed_tex_guid,
                      "sprite": "Sprites/PZ_SeedCards.rxsprite", "spriteGuid": seed_spr_guid},
    }


def validate_sprites() -> list[str]:
    errors: list[str] = []
    assets = AssetMcp()
    try:
        for p in sorted(SPRITES.glob("*.rxsprite")):
            rel = f"Sprites/{p.name}"
            r = assets.call("sprite_get", {"assetPath": rel})
            if not isinstance(r, dict) or r.get("error") or not r.get("doc"):
                errors.append(f"{rel}: {r}")
    finally:
        assets.close()
    return errors


def main() -> None:
    TEXTURES.mkdir(parents=True, exist_ok=True)
    SPRITES.mkdir(parents=True, exist_ok=True)
    SOURCE.mkdir(parents=True, exist_ok=True)
    manifest: dict[str, Any] = {
        "version": 1,
        "generator": "tools/build_all_assets.py",
        "plants": {}, "zombies": {}, "stages": {}, "ui": {},
    }
    manifest["plants"].update(normalize_pilot())
    # normalize_pilot returns zombie in same dict; split it back out.
    if "zombie" in manifest["plants"]:
        manifest["zombies"]["zombie"] = manifest["plants"].pop("zombie")

    gen = GenMcp()
    try:
        manifest["plants"] = build_character_groups(gen, "plant", PLANTS, manifest["plants"])
        manifest["zombies"] = build_character_groups(gen, "zombie", ZOMBIES, manifest["zombies"])
        manifest["stages"] = build_stage_backgrounds(gen)
        manifest["ui"] = build_ui(gen, manifest["plants"])
    finally:
        gen.close()

    manifest["counts"] = {
        "plants": len(manifest["plants"]),
        "zombies": len(manifest["zombies"]),
        "stages": len(manifest["stages"]),
        "spriteDocs": len(list(SPRITES.glob("*.rxsprite"))),
    }
    MANIFEST_PATH.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    errors = validate_sprites()
    if errors:
        raise RuntimeError("sprite validation failed:\n" + "\n".join(errors))
    expected = (49, 26, 5)
    actual = (manifest["counts"]["plants"], manifest["counts"]["zombies"], manifest["counts"]["stages"])
    if actual != expected:
        raise RuntimeError(f"count mismatch expected={expected} actual={actual}")
    print("ASSET_BUILD_PASS", json.dumps(manifest["counts"], ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
