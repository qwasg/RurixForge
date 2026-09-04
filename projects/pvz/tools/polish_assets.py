# polish_assets.py — 可重放的素材后处理(在 build_all_assets.py 之后跑):
#   1. 修正 AI 切帧失败的精灵(太阳只切到碎点 → 按左右两半非品红 bbox 重切);
#   2. 统一锚点:植物/僵尸脚底落格底沿(pivot 由 frame_0 可见 bbox 底边算出),豌豆/太阳居中;
#   3. 生成 HUD 与交互素材:数字图集(0-9+空)、种子卡图集(卡底 + 植物缩略图 + 阳光费)、
#      选卡光标、胜/负横幅、阳光计数图标;
#   4. 重切五张场景背景(源图集为 2 列 x 3 行,旧脚本按 3x2 切错位);
#   5. 把新增素材 GUID 写回 Content/asset_manifest.json 的 ui 段。
# 所有产物确定性生成(纯 PIL,无 AI 调用),.meta 用 uuid5 稳定 GUID,可反复执行。
from __future__ import annotations

import json
import pathlib
import uuid
from typing import Any

from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTENT = ROOT / "Content"
TEXTURES = CONTENT / "Textures"
SPRITES = CONTENT / "Sprites"
DATA = ROOT / "docs" / "data"
MANIFEST = CONTENT / "asset_manifest.json"
NS = uuid.UUID("12345678-4c45-564c-2009-000000000001")
MAGENTA = (255, 0, 255, 255)

PLANTS = json.loads((DATA / "plants.json").read_text(encoding="utf-8"))
P_ID = {p["id"]: i + 1 for i, p in enumerate(PLANTS)}

# 渲染尺度约定(与 build_all_levels.py 的 Sprite.pixelsPerUnit 一致):
PLANT_PPU = 150.0    # 256px 帧 → 1.71 世界单位,可见植物约 0.9~1.1 单位(格 1.6)
ZOMBIE_PPU = 120.0   # 256x512 帧 → 2.13 x 4.27,可见僵尸约 2.3 单位高(≈1.5 格,贴近原版比例)
FEET_BELOW_CENTER_PLANT = 0.7   # 植物脚底落在格心下方 0.7(格底沿 0.8 略上)
FEET_BELOW_CENTER_ZOMBIE = 0.8

FONT_BOLD = r"C:\Windows\Fonts\arialbd.ttf"
FONT_IMPACT = r"C:\Windows\Fonts\impact.ttf"


def stable_guid(rel: str) -> str:
    return str(uuid.uuid5(NS, rel.replace("\\", "/").lower()))


def read_guid(path: pathlib.Path) -> str:
    meta = pathlib.Path(str(path) + ".meta")
    if not meta.is_file():
        return ""
    for line in meta.read_text(encoding="utf-8").splitlines():
        if line.startswith("guid:"):
            return line.split(":", 1)[1].strip()
    return ""


def write_meta(path: pathlib.Path, guid: str, atype: str, importer: str, pipeline: str) -> None:
    meta = pathlib.Path(str(path) + ".meta")
    if meta.is_file():
        # 已有 GUID 一律保留(场景/清单已引用);只补写 provenance。
        existing = read_guid(path)
        if existing:
            guid = existing
    meta.write_text(
        f"guid: {guid}\n"
        f"type: {atype}\n"
        f"importer: {importer}\n"
        "provenance:\n"
        "  origin: tool\n"
        "  detail:\n"
        "    tool: tools/polish_assets.py\n"
        f"    pipeline: {pipeline}\n"
        "build_state: current\n",
        encoding="utf-8",
    )


def is_key(px: tuple[int, int, int, int]) -> bool:
    """品红色键(AI 图集约定;容差覆盖 JPEG 样噪点)。"""
    r, g, b, a = px
    return a == 0 or (r > 200 and b > 200 and g < 90)


def visible_bbox(im: Image.Image, box: tuple[int, int, int, int]) -> tuple[int, int, int, int] | None:
    """box 内非色键像素的 bbox(绝对坐标 x0,y0,x1,y1);全空返回 None。"""
    crop = im.crop(box).convert("RGBA")
    px = crop.load()
    w, h = crop.size
    x0, y0, x1, y1 = w, h, -1, -1
    for y in range(h):
        for x in range(w):
            if not is_key(px[x, y]):
                if x < x0:
                    x0 = x
                if x > x1:
                    x1 = x
                if y < y0:
                    y0 = y
                if y > y1:
                    y1 = y
    if x1 < 0:
        return None
    return (box[0] + x0, box[1] + y0, box[0] + x1 + 1, box[1] + y1 + 1)


def load_sprite(name: str) -> tuple[pathlib.Path, dict[str, Any]] | None:
    p = SPRITES / f"{name}.rxsprite"
    if not p.is_file():
        return None
    return p, json.loads(p.read_text(encoding="utf-8"))


def save_sprite(p: pathlib.Path, doc: dict[str, Any]) -> None:
    p.write_text(json.dumps(doc, ensure_ascii=False, indent=2), encoding="utf-8")


def texture_of(doc: dict[str, Any]) -> pathlib.Path | None:
    guid = doc.get("texture", "")
    for meta in TEXTURES.glob("*.png.meta"):
        if read_guid(pathlib.Path(str(meta)[:-5])) == guid:
            return pathlib.Path(str(meta)[:-5])
    return None


# ---------- 1/2. 精灵切帧与锚点 ----------

def fix_sun() -> None:
    loaded = load_sprite("PZ_Sun")
    if not loaded:
        return
    p, doc = loaded
    tex = texture_of(doc) or (TEXTURES / "PZ_Sun.png")
    im = Image.open(tex).convert("RGBA")
    w, h = im.size
    frames: dict[str, Any] = {}
    for i, box in enumerate([(0, 0, w // 2, h), (w // 2, 0, w, h)]):
        bb = visible_bbox(im, box)
        if bb is None:
            continue
        # 两帧取同尺寸(以较大者为准)避免 idle 闪缩;这里先记 bbox,下面统一。
        frames[f"frame_{i}"] = bb
    if not frames:
        return
    fw = max(b[2] - b[0] for b in frames.values())
    fh = max(b[3] - b[1] for b in frames.values())
    out_frames: dict[str, Any] = {}
    for name, (x0, y0, x1, y1) in frames.items():
        cx, cy = (x0 + x1) // 2, (y0 + y1) // 2
        bx = max(0, min(w - fw, cx - fw // 2))
        by = max(0, min(h - fh, cy - fh // 2))
        out_frames[name] = {"bbox": [bx, by, fw, fh]}
    doc["frames"] = out_frames
    doc["clips"] = {"idle": {"fps": 4, "frames": list(out_frames), "loop": True}}
    doc["pivot"] = [0.5, 0.5]
    save_sprite(p, doc)
    print(f"[sun] frames={out_frames}")


def fix_pea() -> None:
    loaded = load_sprite("PZ_Pea")
    if not loaded:
        return
    p, doc = loaded
    doc["pivot"] = [0.5, 0.5]
    save_sprite(p, doc)


def feet_pivot(doc: dict[str, Any], tex: pathlib.Path, ppu: float, feet_below: float) -> float:
    """frame_0 可见底边占帧高比例 b;锚点 y = b - feet_below / (帧高/ppu),钳在 [0.5, 1]。"""
    im = Image.open(tex).convert("RGBA")
    first = next(iter(doc["frames"].values()))["bbox"]
    x, y, w, h = first
    bb = visible_bbox(im, (x, y, x + w, y + h))
    bottom = (bb[3] - y) / h if bb else 0.95
    units_h = h / ppu
    return max(0.5, min(1.0, bottom - feet_below / units_h))


def fix_characters() -> None:
    for p in sorted(SPRITES.glob("*.rxsprite")):
        name = p.stem
        doc = json.loads(p.read_text(encoding="utf-8"))
        tex = texture_of(doc)
        if not tex or not doc.get("frames"):
            continue
        if name.startswith("PZ_Zombie"):
            py = feet_pivot(doc, tex, ZOMBIE_PPU, FEET_BELOW_CENTER_ZOMBIE)
        elif name.startswith("PZ_Plant_") or name in {"PZ_Peashooter", "PZ_Sunflower", "PZ_Wallnut"}:
            py = feet_pivot(doc, tex, PLANT_PPU, FEET_BELOW_CENTER_PLANT)
        else:
            continue
        doc["pivot"] = [0.5, round(py, 3)]
        save_sprite(p, doc)
    print("[characters] pivots aligned to cell bottom")


# ---------- 3. HUD / 交互素材 ----------

def font(path: str, size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    try:
        return ImageFont.truetype(path, size)
    except OSError:
        return ImageFont.load_default()


def outlined_text(draw: ImageDraw.ImageDraw, xy: tuple[int, int], text: str, fnt: Any,
                  fill: tuple[int, int, int, int], outline: tuple[int, int, int, int], width: int = 3,
                  anchor: str = "mm") -> None:
    x, y = xy
    for dx in range(-width, width + 1):
        for dy in range(-width, width + 1):
            if dx * dx + dy * dy <= width * width:
                draw.text((x + dx, y + dy), text, font=fnt, fill=outline, anchor=anchor)
    draw.text((x, y), text, font=fnt, fill=fill, anchor=anchor)


def build_digits() -> dict[str, Any]:
    """数字图集:11 帧(0-9 + 空白),每帧 48x64。
    帧名零填充 f00..f10:引擎按帧名字典序取 frame 下标(BTreeMap),这样 set_frame(index=数字)
    直接命中对应数字,index=10 为空白(前导零消隐)。"""
    fw, fh = 48, 64
    atlas = Image.new("RGBA", (fw * 11, fh), (0, 0, 0, 0))
    draw = ImageDraw.Draw(atlas)
    fnt = font(FONT_BOLD, 50)
    frames: dict[str, Any] = {}
    for i in range(10):
        outlined_text(draw, (i * fw + fw // 2, fh // 2 + 2), str(i), fnt,
                      (255, 255, 255, 255), (40, 30, 10, 255), 3)
        frames[f"f{i:02d}"] = {"bbox": [i * fw, 0, fw, fh]}
    frames["f10"] = {"bbox": [10 * fw, 0, fw, fh]}
    tex_rel, spr_rel = "Textures/PZ_Digits.png", "Sprites/PZ_Digits.rxsprite"
    atlas.save(TEXTURES / "PZ_Digits.png")
    write_meta(TEXTURES / "PZ_Digits.png", stable_guid(tex_rel), "texture", "png", "pil-digit-atlas")
    tex_guid = read_guid(TEXTURES / "PZ_Digits.png")
    doc = {"version": 1, "texture": tex_guid, "pivot": [0.5, 0.5], "frames": frames, "clips": {}}
    save_sprite(SPRITES / "PZ_Digits.rxsprite", doc)
    write_meta(SPRITES / "PZ_Digits.rxsprite", stable_guid(spr_rel), "sprite", "sprite", "pil-digit-atlas")
    return {"texture": tex_rel, "textureGuid": tex_guid, "sprite": spr_rel,
            "spriteGuid": read_guid(SPRITES / "PZ_Digits.rxsprite"), "frames": list(frames)}


def plant_thumbnail(pid: str, size: int) -> Image.Image | None:
    """植物 frame_0 的可见部分裁切并缩放到 size 方框(透明底)。"""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8")) if MANIFEST.is_file() else {}
    info = manifest.get("plants", {}).get(pid, {})
    spr = info.get("sprite")
    if not spr or not (CONTENT / spr).is_file():
        return None
    doc = json.loads((CONTENT / spr).read_text(encoding="utf-8"))
    tex = texture_of(doc)
    if not tex:
        return None
    im = Image.open(tex).convert("RGBA")
    x, y, w, h = next(iter(doc["frames"].values()))["bbox"]
    bb = visible_bbox(im, (x, y, x + w, y + h)) or (x, y, x + w, y + h)
    crop = im.crop(bb)
    px = crop.load()
    for yy in range(crop.height):
        for xx in range(crop.width):
            if is_key(px[xx, yy]):
                px[xx, yy] = (0, 0, 0, 0)
    crop.thumbnail((size, size), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.alpha_composite(crop, ((size - crop.width) // 2, (size - crop.height) // 2))
    return out


def build_cards() -> dict[str, Any]:
    """种子卡图集:49 张(植物 id 序),7 列 x 7 行,每张 96x128;帧名 card_<plantCode 两位>。"""
    cw, ch = 96, 128
    cols = 7
    rows = (len(PLANTS) + cols - 1) // cols
    atlas = Image.new("RGBA", (cw * cols, ch * rows), (0, 0, 0, 0))
    fnt = font(FONT_BOLD, 22)
    frames: dict[str, Any] = {}
    for i, p in enumerate(PLANTS):
        cx, cy = (i % cols) * cw, (i // cols) * ch
        card = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
        d = ImageDraw.Draw(card)
        d.rounded_rectangle((2, 2, cw - 3, ch - 3), radius=10, fill=(214, 178, 106, 255), outline=(92, 62, 24, 255), width=3)
        d.rounded_rectangle((8, 8, cw - 9, ch - 36), radius=8, fill=(238, 222, 170, 255), outline=(150, 112, 56, 255), width=2)
        thumb = plant_thumbnail(p["id"], 68)
        if thumb is not None:
            card.alpha_composite(thumb, ((cw - 68) // 2, 14))
        d.rectangle((8, ch - 32, cw - 9, ch - 8), fill=(80, 56, 20, 255))
        outlined_text(d, (cw // 2, ch - 20), str(int(p["cost"])), fnt, (255, 236, 120, 255), (30, 20, 5, 255), 2)
        atlas.alpha_composite(card, (cx, cy))
        # 零填充:帧名字典序 = 植物 id 序,场景侧 Sprite.frame = plantCode-1 直接命中。
        frames[f"card_{P_ID[p['id']]:02d}"] = {"bbox": [cx, cy, cw, ch]}
    tex_rel, spr_rel = "Textures/PZ_Cards.png", "Sprites/PZ_Cards.rxsprite"
    atlas.save(TEXTURES / "PZ_Cards.png")
    write_meta(TEXTURES / "PZ_Cards.png", stable_guid(tex_rel), "texture", "png", "pil-seed-card-atlas")
    tex_guid = read_guid(TEXTURES / "PZ_Cards.png")
    doc = {"version": 1, "texture": tex_guid, "pivot": [0.5, 0.5], "frames": frames, "clips": {}}
    save_sprite(SPRITES / "PZ_Cards.rxsprite", doc)
    write_meta(SPRITES / "PZ_Cards.rxsprite", stable_guid(spr_rel), "sprite", "sprite", "pil-seed-card-atlas")
    return {"texture": tex_rel, "textureGuid": tex_guid, "sprite": spr_rel,
            "spriteGuid": read_guid(SPRITES / "PZ_Cards.rxsprite"), "cardSize": [cw, ch]}


def build_simple_texture(name: str, size: tuple[int, int], paint, pipeline: str) -> dict[str, str]:
    im = Image.new("RGBA", size, (0, 0, 0, 0))
    paint(ImageDraw.Draw(im), im)
    path = TEXTURES / f"{name}.png"
    im.save(path)
    rel = f"Textures/{name}.png"
    write_meta(path, stable_guid(rel), "texture", "png", pipeline)
    return {"texture": rel, "textureGuid": read_guid(path)}


def build_ui_textures() -> dict[str, Any]:
    out: dict[str, Any] = {}

    # 渲染管线无 alpha 混合(只有 alpha<0.02 丢弃):半透明一律画成「空心框」,内部全透明。
    def cursor(d: ImageDraw.ImageDraw, im: Image.Image) -> None:
        d.rounded_rectangle((0, 0, im.width - 1, im.height - 1), radius=12,
                            fill=(0, 0, 0, 0), outline=(255, 220, 40, 255), width=7)
    out["cardCursor"] = build_simple_texture("PZ_CardCursor", (104, 136), cursor, "pil-card-cursor")

    def banner(text: str, fill: tuple[int, int, int, int], bg: tuple[int, int, int, int]):
        def paint(d: ImageDraw.ImageDraw, im: Image.Image) -> None:
            d.rounded_rectangle((0, 0, im.width - 1, im.height - 1), radius=28, fill=bg,
                                outline=(255, 255, 255, 230), width=6)
            outlined_text(d, (im.width // 2, im.height // 2), text, font(FONT_IMPACT, 96), fill, (20, 10, 0, 255), 5)
        return paint
    out["bannerWin"] = build_simple_texture("PZ_Banner_Win", (720, 180),
                                            banner("LEVEL COMPLETE!", (255, 236, 96, 255), (40, 110, 40, 230)), "pil-banner")
    out["bannerLose"] = build_simple_texture("PZ_Banner_Lose", (720, 180),
                                             banner("THE ZOMBIES ATE YOUR BRAINS!", (255, 230, 230, 255), (120, 20, 20, 235)), "pil-banner")

    def sunbar(d: ImageDraw.ImageDraw, im: Image.Image) -> None:
        d.rounded_rectangle((0, 0, im.width - 1, im.height - 1), radius=14,
                            fill=(214, 178, 106, 245), outline=(92, 62, 24, 255), width=4)
    out["sunPanel"] = build_simple_texture("PZ_SunPanel", (300, 76), sunbar, "pil-sun-panel")

    def seedbar(d: ImageDraw.ImageDraw, im: Image.Image) -> None:
        d.rounded_rectangle((0, 0, im.width - 1, im.height - 1), radius=14,
                            fill=(120, 84, 40, 235), outline=(60, 40, 16, 255), width=4)
    out["seedPanel"] = build_simple_texture("PZ_SeedPanel", (900, 150), seedbar, "pil-seed-panel")

    def cell(d: ImageDraw.ImageDraw, im: Image.Image) -> None:
        d.rectangle((0, 0, im.width - 1, im.height - 1), fill=(0, 0, 0, 0), outline=(235, 250, 220, 255), width=2)
    out["cellOverlay"] = build_simple_texture("PZ_CellOverlay", (64, 64), cell, "pil-cell-overlay")

    def mower(d: ImageDraw.ImageDraw, im: Image.Image) -> None:
        d.rounded_rectangle((6, 14, 58, 40), radius=6, fill=(200, 40, 40, 255), outline=(60, 10, 10, 255), width=3)
        d.rectangle((14, 6, 50, 18), fill=(90, 90, 100, 255), outline=(30, 30, 30, 255), width=2)
        for x in (14, 46):
            d.ellipse((x - 8, 34, x + 8, 50), fill=(30, 30, 30, 255), outline=(200, 200, 200, 255), width=2)
    out["mower"] = build_simple_texture("PZ_MowerCart", (64, 52), mower, "pil-mower")
    return out


# ---------- 4. 场景背景重切(源图集 2 列 x 3 行) ----------

def recrop_stages() -> None:
    src = TEXTURES / "SourceAtlases" / "PZ_StageBackgrounds.png"
    if not src.is_file():
        return
    im = Image.open(src).convert("RGBA")
    w, h = im.size
    cw, ch = w // 2, h // 3
    order = {"day": (0, 0), "night": (1, 0), "pool": (0, 1), "fog": (1, 1), "roof": (0, 2)}
    for stage, (c, r) in order.items():
        crop = im.crop((c * cw, r * ch, (c + 1) * cw, (r + 1) * ch))
        # 统一放大到 1024 宽,保留 3:2 源比例(场景里按视口比例轻微拉伸)。
        crop = crop.resize((1024, int(1024 * ch / cw)), Image.Resampling.LANCZOS)
        dst = TEXTURES / f"PZ_Stage_{stage.capitalize()}.png"
        crop.save(dst)
        if not pathlib.Path(str(dst) + ".meta").is_file():
            write_meta(dst, stable_guid(f"Textures/PZ_Stage_{stage.capitalize()}.png"), "texture", "png", "stage-recrop")
    print("[stages] recropped 5 backgrounds from 2x3 source grid")


# ---------- 5. 清单回写 ----------

def update_manifest(extra_ui: dict[str, Any]) -> None:
    man = json.loads(MANIFEST.read_text(encoding="utf-8")) if MANIFEST.is_file() else {}
    ui = man.setdefault("ui", {})
    ui.update(extra_ui)
    ui["sunSpriteGuid"] = read_guid(SPRITES / "PZ_Sun.rxsprite")
    ui["peaSpriteGuid"] = read_guid(SPRITES / "PZ_Pea.rxsprite")
    ui["renderScale"] = {"plantPpu": PLANT_PPU, "zombiePpu": ZOMBIE_PPU, "peaPpu": 300.0, "sunPpu": 110.0,
                         "cardPpu": 100.0, "digitPpu": 100.0}
    man["polish"] = {"tool": "tools/polish_assets.py", "version": 1}
    MANIFEST.write_text(json.dumps(man, ensure_ascii=False, indent=2), encoding="utf-8")


def main() -> None:
    fix_sun()
    fix_pea()
    fix_characters()
    recrop_stages()
    ui: dict[str, Any] = {}
    ui["digits"] = build_digits()
    ui["cards"] = build_cards()
    ui.update(build_ui_textures())
    update_manifest(ui)
    print("POLISH_PASS", json.dumps({k: (v.get("textureGuid") or v.get("spriteGuid")) for k, v in ui.items()}, ensure_ascii=False))


if __name__ == "__main__":
    main()
