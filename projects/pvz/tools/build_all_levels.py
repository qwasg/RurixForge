# build_all_levels.py — 生成 PvZ 冒险模式全部 50 关、5 个场景模板、规则适配表与关卡图
from __future__ import annotations

import json
import pathlib
import sys
import uuid
from typing import Any

sys.path.insert(0, str(pathlib.Path(__file__).parent))
import gen_graphs
from pvz_mcp import Mcp as CodeMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTENT = ROOT / "Content"
DATA = ROOT / "docs" / "data"
LEVEL_DIR = CONTENT / "Levels" / "Adventure"
SCENE_DIR = CONTENT / "Scenes" / "Levels"
GRAPH_DIR = CONTENT / "Graphs" / "Levels"
RULE_DIR = CONTENT / "Rules"
MANIFEST_PATH = CONTENT / "asset_manifest.json"
NS = uuid.UUID("12345678-4c45-564c-2009-000000000001")

PLANTS = json.loads((DATA / "plants.json").read_text(encoding="utf-8"))
ZOMBIES = json.loads((DATA / "zombies.json").read_text(encoding="utf-8"))
LEVELS = json.loads((DATA / "levels.json").read_text(encoding="utf-8"))
STAGES = json.loads((DATA / "stages.json").read_text(encoding="utf-8"))
P_ID = {p["id"]: i + 1 for i, p in enumerate(PLANTS)}
Z_ID = {z["id"]: i + 1 for i, z in enumerate(ZOMBIES)}
STAGE = {s["id"]: s for s in STAGES}

STAGE_PRIORITY = {
    "day": ["sunflower", "peashooter", "wall_nut", "potato_mine", "cherry_bomb", "snow_pea", "repeater", "chomper"],
    "night": ["sun_shroom", "puff_shroom", "fume_shroom", "grave_buster", "hypno_shroom", "scaredy_shroom", "ice_shroom", "doom_shroom"],
    "pool": ["lily_pad", "sunflower", "peashooter", "threepeater", "tangle_kelp", "squash", "tall_nut", "torchwood"],
    "fog": ["sun_shroom", "puff_shroom", "plantern", "blover", "cactus", "pumpkin", "magnet_shroom", "split_pea"],
    "roof": ["flower_pot", "sunflower", "cabbage_pult", "kernel_pult", "melon_pult", "umbrella_leaf", "garlic", "coffee_bean"],
}

SPECIAL_RULES: dict[str, dict[str, Any]] = {
    "tutorial_1lane": {"adapter": "tutorial", "inputs": ["plant_at"], "notes": "仅中间一行可用"},
    "tutorial_3lane": {"adapter": "tutorial", "inputs": ["plant_at"], "notes": "仅中间三行可用"},
    "bowling": {"adapter": "bowling", "inputs": ["roll_nut"], "notes": "坚果池沿车道微 tween 滚动并触发清除"},
    "whack": {"adapter": "whack", "inputs": ["whack_cell"], "notes": "输入命中墓碑/僵尸，关卡配置控制墓碑列"},
    "conveyor": {"adapter": "conveyor", "inputs": ["conveyor_take", "plant_at"], "notes": "卡牌队列由 manifest 给出"},
    "big_trouble": {"adapter": "big_trouble", "inputs": ["plant_at"], "notes": "小型僵尸缩放配置"},
    "vasebreaker": {"adapter": "vasebreaker", "inputs": ["break_vase"], "notes": "罐子内容由 deterministic seed 预生成"},
    "storm": {"adapter": "storm", "inputs": ["plant_at"], "notes": "全屏遮罩周期切换"},
    "bobsled": {"adapter": "bobsled", "inputs": ["plant_at"], "notes": "冰道行与雪橇小队配置"},
    "boss": {"adapter": "boss", "inputs": ["plant_at", "boss_target"], "notes": "僵王血条/召唤/冰火球阶段表"},
    "minigame_unlock": {"adapter": "unlock", "notes": "解锁迷你游戏标记"},
    "shop_unlock": {"adapter": "unlock", "notes": "解锁商店标记"},
    "puzzle_unlock": {"adapter": "unlock", "notes": "解锁解谜标记"},
    "zen_garden_unlock": {"adapter": "unlock", "notes": "解锁禅境花园标记"},
}


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


def ensure_meta(path: pathlib.Path, atype: str, importer: str, description: str) -> None:
    """场景/图的 .meta(IDE Assets 面板按 type 分类、双击打开;GUID 已存在则保留)。"""
    meta = pathlib.Path(str(path) + ".meta")
    guid = read_guid(path) or stable_guid(str(path.relative_to(CONTENT)))
    meta.write_text(
        f"guid: {guid}\n"
        f"type: {atype}\n"
        f"importer: {importer}\n"
        "provenance:\n"
        "  origin: tool\n"
        "  detail:\n"
        "    tool: tools/build_all_levels.py\n"
        "build_state: current\n"
        "semantic:\n"
        f"  description: {description}\n",
        encoding="utf-8",
    )


def level_code(level_id: str) -> int:
    a, b = level_id.split("-")
    return int(a) * 100 + int(b)


def unlock_code(value: str) -> int:
    if value == "shop":
        return 304
    if "二周目" in value:
        value = value.split()[0]
    try:
        return level_code(value)
    except Exception:
        return 9999


def available_plants(level: dict[str, Any]) -> list[str]:
    code = level_code(level["id"])
    out = [p["id"] for p in PLANTS if unlock_code(str(p.get("unlock", ""))) <= code]
    # 1-1 初始应至少有豌豆射手；奖励植物在通关本关后生效，因此当前关排除同关奖励。
    reward = level.get("reward")
    if reward in out and reward != "peashooter":
        out.remove(reward)
    return out or ["peashooter"]


def recommended(level: dict[str, Any], available: list[str]) -> list[str]:
    priority = STAGE_PRIORITY[level["stage"]]
    picked = [x for x in priority if x in available]
    for x in available:
        if x not in picked:
            picked.append(x)
    return picked[:8]


def behavior_for_plant(p: dict[str, Any]) -> str:
    prod = p.get("production")
    if isinstance(prod, dict) and float(prod.get("sun") or 0) > 0:
        return "producer"
    if float(p.get("attack_interval_sec") or 0) > 0 and float(p.get("damage") or 0) > 0:
        return "shooter"
    if float(p.get("damage") or 0) >= 1000:
        return "explosive"
    if p["id"] in {"wall_nut", "tall_nut", "pumpkin", "lily_pad", "flower_pot"}:
        return "barrier"
    return "special"


def wave_schedule(level: dict[str, Any], rows: int) -> list[dict[str, Any]]:
    types = level["zombie_ids"]
    total = int(level["waves"])
    flags = int(level["flags"])
    flag_at = set()
    if flags:
        for i in range(1, flags + 1):
            flag_at.add(max(1, round(total * i / flags)))
    waves = []
    for w in range(1, total + 1):
        count = min(3, 1 + w // 4)
        members = []
        for m in range(count):
            zid = types[(w + m * 3) % len(types)]
            members.append({
                "zombieId": zid,
                "zombieCode": Z_ID[zid],
                "row": 1 + ((w + m - 1) % rows),
                "delaySec": round(m * 0.4, 2),
            })
        waves.append({"index": w, "flag": w in flag_at, "members": members})
    return waves


def plant_behavior_manifest() -> dict[str, Any]:
    return {
        p["id"]: {
            "plantCode": P_ID[p["id"]],
            "adapter": behavior_for_plant(p),
            "cost": p["cost"], "hp": p["hp"], "damage": p["damage"],
            "attackIntervalSec": p["attack_interval_sec"],
            "range": p["range"], "special": p["special"],
            "mushroom": p["mushroom"], "aquatic": p["aquatic"],
            "upgradeFrom": p["upgrade_from"],
            "runtimeStatus": "generic-adapter",
        } for p in PLANTS
    }


def zombie_behavior_manifest() -> dict[str, Any]:
    return {
        z["id"]: {
            "zombieCode": Z_ID[z["id"]], "adapter": "walker",
            "hp": z["hp"], "speed": z["speed"], "dps": z["dps"],
            "armor": z["armor"], "special": z["special"],
            "runtimeStatus": "stats-driven-walker-with-special-config",
        } for z in ZOMBIES
    }


OBSOLETE_GRAPHS = ["plant_static", "plant_special", "plant_explosive", "plant_shooter", "plant_producer",
                   "level_controller", "zombie_walker"]


def generic_graphs() -> list[pathlib.Path]:
    out = CONTENT / "Graphs"
    out.mkdir(parents=True, exist_ok=True)
    # 旧的按适配器共享的植物图已被每植物独立图(Graphs/Plants/plant_<id>.rxgraph)取代;
    # 清掉残留文件,避免回归门把废图当生产图扫。
    for stem in OBSOLETE_GRAPHS:
        p = out / f"{stem}.rxgraph"
        if p.is_file():
            p.unlink()
    docs: dict[str, dict[str, Any]] = {}
    docs["special_adapter"] = {
        "version": 1, "id": "special_adapter", "name": "special_adapter",
        "exposedProps": [{"name": "specialCode", "kind": "F32", "default": 0}],
        "nodes": [{"id": "s", "type": "event.on_start", "pos": [0, 0]},
                  {"id": "l", "type": "debug.log", "pos": [1, 0], "inputs": {"message": {"const": "SPECIAL_ADAPTER_READY"}}}],
        "edges": [{"from": ["s", "exec"], "to": ["l", "exec"]}],
    }
    docs["menu"] = {
        "version": 1, "id": "menu", "name": "menu", "exposedProps": [],
        "nodes": [{"id": "i", "type": "event.on_input", "pos": [0, 0]},
                  {"id": "l", "type": "debug.log", "pos": [1, 0], "inputs": {"message": {"const": "LEVEL_SELECTED"}}}],
        "edges": [{"from": ["i", "exec"], "to": ["l", "exec"]}],
    }
    paths = []
    for name, doc in docs.items():
        p = out / f"{name}.rxgraph"
        p.write_text(json.dumps(doc, ensure_ascii=False, indent=2), encoding="utf-8")
        paths.append(p)
    # 每种特殊关独立适配图，保证 8 类玩法/解锁事件都有可验证入口与事件名。
    special_dir = out / "Special"
    special_dir.mkdir(parents=True, exist_ok=True)
    for rule, cfg in SPECIAL_RULES.items():
        nodes = [
            {"id": "s", "type": "event.on_start", "pos": [0, 0]},
            {"id": "ls", "type": "debug.log", "pos": [1, 0],
             "inputs": {"message": {"const": f"SPECIAL_{rule.upper()}_READY"}}},
            {"id": "i", "type": "event.on_input", "pos": [0, 2]},
            {"id": "li", "type": "debug.log", "pos": [1, 2],
             "inputs": {"message": {"const": f"SPECIAL_{rule.upper()}_INPUT"}}},
        ]
        edges = [
            {"from": ["s", "exec"], "to": ["ls", "exec"]},
            {"from": ["i", "exec"], "to": ["li", "exec"]},
        ]
        if rule in {"conveyor", "storm", "boss"}:
            nodes.extend([
                {"id": "ts", "type": "flow.timer_start", "pos": [2, 0],
                 "inputs": {"timerId": {"const": f"{rule}_tick"}, "duration": {"const": 1.0}}},
                {"id": "te", "type": "event.on_timer", "pos": [0, 4]},
                {"id": "lt", "type": "debug.log", "pos": [1, 4],
                 "inputs": {"message": {"const": f"SPECIAL_{rule.upper()}_TICK"}}},
            ])
            edges[0] = {"from": ["s", "exec"], "to": ["ts", "exec"]}
            edges.append({"from": ["ts", "exec"], "to": ["ls", "exec"]})
            edges.append({"from": ["te", "exec"], "to": ["lt", "exec"]})
        doc = {"version": 1, "id": f"special_{rule}", "name": f"special_{rule}",
               "exposedProps": [], "nodes": nodes, "edges": edges}
        p = special_dir / f"special_{rule}.rxgraph"
        p.write_text(json.dumps(doc, ensure_ascii=False, indent=2), encoding="utf-8")
        paths.append(p)
    return paths


def load_assets() -> dict[str, Any]:
    if MANIFEST_PATH.is_file():
        return json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    # Fallback permits generator dry-run before the AI asset job completes.
    return {"plants": {}, "zombies": {}, "stages": {}, "ui": {}}


def texture_guid(name: str) -> str:
    return read_guid(CONTENT / "Textures" / name)


def sprite_comp(texture: str = "", sprite: str = "", clip: str = "",
                order: float = 0.0, tint: list[float] | None = None,
                frame: float = 0.0, ppu: float = 100.0) -> dict[str, Any]:
    return {"enabled": True, "type": "Sprite", "props": {
        "texture": texture, "sprite": sprite, "clip": clip, "frame": frame,
        "tint": tint or [1, 1, 1, 1], "flipX": False, "flipY": False,
        "pixelsPerUnit": ppu, "sortingOrder": order,
    }}


# 渲染尺度(与 tools/polish_assets.py 的锚点计算一致):
PLANT_PPU = 150.0
ZOMBIE_PPU = 120.0
PEA_PPU = 300.0
SUN_PPU = 110.0
# 对象池容量(屏外精灵不占 draw 槽——engine-host 视锥裁剪;屏内预算仍受 128 槽约束)。
PLANT_POOL_PER_TYPE = 6
ZOMBIE_POOL_PER_TYPE = 5
ZOMBIE_POOL_MAX = 15
PEA_POOL = 24
SUN_POOL = 8
# 种子卡栏几何(与 pvz_rules.rx card_slot / in_card_bar 一致):卡中心 x = CARD_X0 + slot*CARD_PITCH,y = CARD_Y。
CARD_X0, CARD_PITCH, CARD_Y = -6.2, 1.3, 5.35
HUD_Y = 5.35


def playable_rows(level: dict[str, Any], rows: int) -> list[int]:
    """特殊规则收窄可用行:教程 1 行只开中间行,教程 3 行开中间三行;其余全开。"""
    special = level.get("special_rules")
    mid = (rows + 1) // 2
    if special == "tutorial_1lane":
        return [mid]
    if special == "tutorial_3lane":
        return [r for r in (mid - 1, mid, mid + 1) if 1 <= r <= rows]
    return list(range(1, rows + 1))


def tag(value: str) -> dict[str, Any]:
    return {"enabled": True, "type": "Tag", "props": {"tag": value}}


def script(graph_ref: str, props: dict[str, Any]) -> dict[str, Any]:
    return {"enabled": True, "type": "Script", "props": {
        "graphRef": graph_ref, "module": "", "props": props,
    }}


def trigger(extents: list[float]) -> dict[str, Any]:
    return {"enabled": True, "type": "Trigger", "props": {"kind": "box", "extents": extents}}


def category(value: str) -> dict[str, Any]:
    return {"enabled": True, "type": "Category", "props": {"category": value}}


class SceneBuilder:
    def __init__(self, name: str):
        self.name = name
        self.entities: list[dict[str, Any]] = []
        self.next_id = 1

    def add(self, name: str, pos: tuple[float, float, float], comps: list[dict[str, Any]],
            scale: tuple[float, float, float] = (1, 1, 1)) -> int:
        eid = self.next_id; self.next_id += 1
        self.entities.append({
            "id": eid, "name": name,
            "transform": {"rotation": [0, 0, 0, 1], "scale": list(scale), "translation": list(pos)},
            "components": comps,
        })
        return eid

    def doc(self) -> dict[str, Any]:
        return {"name": self.name, "mode": "2d", "next_id": self.next_id,
                "gravity": [0, 0, 0], "entities": self.entities}


def row_y(row: int, rows: int) -> float:
    return (rows - row + 0.5) * 1.6 - rows * 0.8


def cell_x(col: int) -> float:
    return -7.2 + (col - 0.5) * 1.6


def background_guid(stage_id: str, assets: dict[str, Any]) -> str:
    info = assets.get("stages", {}).get(stage_id, {})
    return info.get("textureGuid") or texture_guid("PZ_Lawn.png")


def board_entities(builder: SceneBuilder, stage_id: str, assets: dict[str, Any],
                   active_rows: list[int] | None = None) -> None:
    st = STAGE[stage_id]
    rows, cols = int(st["rows"]), int(st["cols"])
    active = active_rows or list(range(1, rows + 1))
    bg = background_guid(stage_id, assets)
    ui = assets.get("ui", {})
    cell_tex = ui.get("cellOverlay", {}).get("textureGuid") or texture_guid("PZ_Cell.png")
    mower_tex = ui.get("mower", {}).get("textureGuid") or texture_guid("PZ_Mower.png")
    builder.add("Camera", (0, 0, 10), [
        {"enabled": True, "type": "Camera", "props": {"projection": "orthographic", "orthoSize": 6.2,
                                                            "fov": 60, "near": 0.1, "far": 100}},
        category("map")])
    # 背景 1024x683(3:2)@ppu100 = 10.24x6.83 → 铺满 16:9 视口(22.1x12.4)略拉伸。
    builder.add(f"Background_{stage_id}", (0, 0, -0.8), [sprite_comp(texture=bg, order=0), category("map")],
                (2.2, 1.85, 1))
    # 管线无 alpha 混合:格子只画空心描边(内部透明丢弃),未开放行不画(实体保留供查表定位)。
    for r in range(1, rows + 1):
        for c in range(1, cols + 1):
            water = r in st.get("pool_rows", [])
            comps = [tag(f"cell_r{r}c{c}"), category("map")]
            if r in active:
                tint = [0.55, 0.8, 1.0, 1] if water else ([0.8, 0.9, 0.8, 1] if stage_id == "night" else [1, 1, 1, 1])
                # 尺寸走 pixelsPerUnit(64px → 1.5 单位),实体 scale 保持 1:格子 transform 会被
                # 探针/植物/豌豆整份复制(含 scale),scale≠1 会放大精灵与触发盒。
                comps.insert(0, sprite_comp(texture=cell_tex, order=1, tint=tint, ppu=64.0 / 1.5))
            builder.add(f"Cell_R{r}_C{c}", (cell_x(c), row_y(r, rows), 0), comps)
    if stage_id in {"night", "fog"}:
        for i, (r, c) in enumerate([(1, 8), (3, 7), (rows, 9)], 1):
            builder.add(f"Grave_{i}", (cell_x(c), row_y(r, rows), 0.1),
                        [sprite_comp(texture=texture_guid("PZ_Cell.png"), order=2, tint=[0.45, 0.45, 0.5, 1]),
                         tag("grave"), category("interaction")],
                        (1.1, 1.4, 1))
    if stage_id == "fog":
        for c in range(cols - int(st["fog_columns"]) + 1, cols + 1):
            builder.add(f"Fog_Col_{c}", (cell_x(c), 0, 0.3),
                        [sprite_comp(texture=texture_guid("PZ_HUD_Bar.png"), order=7,
                                     tint=[0.25, 0.3, 0.35, 0.78]), tag("fog_overlay"), category("interaction")],
                        (5.0, rows * 10.0, 1))
    for r in active:
        y = row_y(r, rows)
        builder.add(f"LoseLine_R{r}", (-8.4, y, 0),
                    [trigger([0.5, 1.5, 1]), tag("loseline"),
                     script("Content/Graphs/loseline.rxgraph", {"row": float(r)}), category("interaction")])
        builder.add(f"Mower_R{r}", (-7.9, y - 0.25, 0),
                    [sprite_comp(texture=mower_tex, order=5, ppu=50.0), tag("mower_idle"), trigger([1, 1.4, 1]),
                     script("Content/Graphs/mower.rxgraph", {"row": float(r)}), category("role")])


def stage_template(stage_id: str, assets: dict[str, Any]) -> dict[str, Any]:
    b = SceneBuilder(f"Stage_{stage_id}")
    board_entities(b, stage_id, assets)
    return b.doc()


def hud_entities(b: SceneBuilder, assets: dict[str, Any], loadout: list[str]) -> None:
    """种子卡栏(卡 + 选卡光标)、阳光计数(图标 + 四位数字)、胜负横幅、格子探针。"""
    ui = assets.get("ui", {})
    cards = ui.get("cards", {})
    digits = ui.get("digits", {})
    sun_sprite = ui.get("sunSpriteGuid", "") or read_guid(CONTENT / "Sprites" / "PZ_Sun.rxsprite")
    # 面板底
    seed_tex = ui.get("seedPanel", {}).get("textureGuid", "")
    if seed_tex:
        n = len(loadout)
        width_units = max(2.0, n * CARD_PITCH + 0.5)
        center_x = CARD_X0 - CARD_PITCH / 2 - 0.25 + width_units / 2
        b.add("SeedPanel", (center_x, CARD_Y, 0.2),
              [sprite_comp(texture=seed_tex, order=8), category("interaction")], (width_units / 9.0, 1.05, 1))
    # 阳光计数靠左但不出 16:10 视口(半宽 ≈ 10.3):面板 -10.1..-7.1。
    sun_tex = ui.get("sunPanel", {}).get("textureGuid", "")
    if sun_tex:
        b.add("SunPanel", (-8.6, HUD_Y, 0.2), [sprite_comp(texture=sun_tex, order=8), category("interaction")],
              (1.0, 1.0, 1))
    if sun_sprite:
        b.add("HUD_SunIcon", (-9.75, HUD_Y, 0.3),
              [sprite_comp(sprite=sun_sprite, clip="idle", order=9, ppu=150.0), category("interaction")])
    for i, tag_name in enumerate(("hud_d1000", "hud_d100", "hud_d10", "hud_d1")):
        b.add(f"HUD_Digit_{i}", (-9.05 + i * 0.42, HUD_Y, 0.3),
              [sprite_comp(sprite=digits.get("spriteGuid", ""), frame=10.0 if i < 3 else 0.0, order=9, ppu=110.0),
               tag(tag_name), category("interaction")], (0.85, 0.85, 1))
    for slot, pid in enumerate(loadout):
        b.add(f"SeedCard_{slot}_{pid}", (CARD_X0 + slot * CARD_PITCH, CARD_Y, 0.3),
              [sprite_comp(sprite=cards.get("spriteGuid", ""), frame=float(P_ID[pid] - 1), order=9),
               tag(f"card_slot_{slot}"), category("interaction")], (1.2, 1.2, 1))
    cursor_tex = ui.get("cardCursor", {}).get("textureGuid", "")
    b.add("CardCursor", (0, -90, 0.4),
          [sprite_comp(texture=cursor_tex, order=10), tag("card_cursor"), category("interaction")], (1.2, 1.2, 1))
    b.add("Banner_Win", (0, -70, 0.5),
          [sprite_comp(texture=ui.get("bannerWin", {}).get("textureGuid", ""), order=12), tag("banner_win"),
           category("interaction")])
    b.add("Banner_Lose", (0, -70, 0.5),
          [sprite_comp(texture=ui.get("bannerLose", {}).get("textureGuid", ""), order=12), tag("banner_lose"),
           category("interaction")])
    # 格子探针:控制器把它挪到被点击格;Trigger 盒覆盖整格 → 收阳光;transform = 植物落位锚。
    b.add("CellProbe", (0, -95, 0), [trigger([1.6, 1.6, 2.0]), tag("cell_probe"),
                                     script("Content/Graphs/cell_probe.rxgraph", {}), category("interaction")])


def level_scene(level: dict[str, Any], assets: dict[str, Any], loadout: list[str]) -> dict[str, Any]:
    code = level_code(level["id"])
    st = STAGE[level["stage"]]
    rows = int(st["rows"])
    active = playable_rows(level, rows)
    b = SceneBuilder(f"Level_{level['id'].replace('-', '_')}")
    board_entities(b, level["stage"], assets, active)
    # 植物池:每种 PLANT_POOL_PER_TYPE 个实例;停车位间距 2.5 避免池内 Trigger 互触。
    for idx, pid in enumerate(loadout):
        info = assets.get("plants", {}).get(pid, {})
        p = next(x for x in PLANTS if x["id"] == pid)
        adapter = behavior_for_plant(p)
        for copy in range(1, PLANT_POOL_PER_TYPE + 1):
            comps = [sprite_comp(sprite=info.get("spriteGuid", ""), clip="idle", order=3, ppu=PLANT_PPU),
                     tag(f"plant_free_{P_ID[pid]}"),
                     script(f"Content/Graphs/Plants/plant_{P_ID[pid]}.rxgraph",
                            {"plantId": float(P_ID[pid]), "adapter": adapter}),
                     trigger([1.15, 1.35, 1]), category("role")]
            b.add(f"PlantPool_{pid}_{copy}", (-40 - idx * 2.5, -60 - copy * 2.5, 0), comps)
    # 僵尸池:每种 ZOMBIE_POOL_PER_TYPE 个(总数封顶),Trigger 盒 y 半径 0.675 保证只与同行豌豆/植物相触。
    zombie_budget = ZOMBIE_POOL_MAX
    for zi, zid in enumerate(level["zombie_ids"]):
        info = assets.get("zombies", {}).get(zid, {})
        zcode = Z_ID[zid]
        graph = f"Content/Graphs/Zombies/zombie_{zid}.rxgraph"
        copies = min(ZOMBIE_POOL_PER_TYPE, max(1, zombie_budget // max(1, len(level["zombie_ids"]) - zi)))
        for copy in range(1, copies + 1):
            b.add(f"ZombiePool_{zid}_{copy}", (20 + zi * 2.5, -60 - copy * 2.5, 0),
                  [sprite_comp(sprite=info.get("spriteGuid", ""), clip="walk", order=5, ppu=ZOMBIE_PPU),
                   tag(f"zombie_free_{zcode}"), script(graph, {"zombieId": float(zcode), "poolId": float(copy)}),
                   trigger([0.9, 1.35, 0.9]), category("role")])
        zombie_budget -= copies
    pea_sprite = assets.get("ui", {}).get("peaSpriteGuid", "") or read_guid(CONTENT / "Sprites" / "PZ_Pea.rxsprite")
    sun_sprite = assets.get("ui", {}).get("sunSpriteGuid", "") or read_guid(CONTENT / "Sprites" / "PZ_Sun.rxsprite")
    for i in range(PEA_POOL):
        b.add(f"PeaPool_{i+1}", (60 + (i % 6) * 2, -60 - (i // 6) * 2, 0),
              [sprite_comp(sprite=pea_sprite, clip="idle", order=4, ppu=PEA_PPU), tag("pea_free"),
               script("Content/Graphs/pea_fly.rxgraph", {"poolId": float(i + 1)}), category("role")])
    for i in range(SUN_POOL):
        b.add(f"SunPool_{i+1}", (80 + i * 2, -60, 0),
              [sprite_comp(sprite=sun_sprite, clip="idle", order=6, ppu=SUN_PPU), tag("sun_free"),
               script("Content/Graphs/sun_fall.rxgraph", {"poolId": float(i + 1)}), category("role")])
    controller_ref = f"Content/Graphs/Levels/level_{level['id'].replace('-', '_')}.rxgraph"
    b.add("LevelController", (0, 0, 0),
          [script(controller_ref, {"levelCode": float(code)}), category("interaction")])
    b.add("HUDController", (0, 0, 0), [script("Content/Graphs/hud.rxgraph", {}), category("interaction")])
    hud_entities(b, assets, loadout)
    special = level.get("special_rules")
    if special:
        scode = sorted(SPECIAL_RULES).index(special) + 1 if special in SPECIAL_RULES else 0
        b.add("SpecialRuleAdapter", (0, 0, 0),
              [script(f"Content/Graphs/Special/special_{special}.rxgraph", {"specialCode": float(scode)}),
               tag(f"special_{special}"), category("interaction")])
    return b.doc()


def main_menu(assets: dict[str, Any]) -> dict[str, Any]:
    b = SceneBuilder("PvZ_Adventure_Menu")
    b.add("Camera", (0, 0, 10), [
        {"enabled": True, "type": "Camera", "props": {"projection": "orthographic", "orthoSize": 7.5,
                                                            "fov": 60, "near": 0.1, "far": 100}}, category("map")])
    b.add("MenuBackground", (0, 0, -0.5),
          [sprite_comp(texture=background_guid("day", assets), order=0), category("map")], (20, 14, 1))
    ui_sprite = assets.get("ui", {}).get("uiAtlas", {}).get("spriteGuid", "")
    for i, level in enumerate(LEVELS):
        col, row = i % 10, i // 10
        x, y = -7.2 + col * 1.6, 3.2 - row * 1.5
        b.add(f"LevelButton_{level['id'].replace('-', '_')}", (x, y, 0),
              [sprite_comp(sprite=ui_sprite, frame=0, order=10), tag(f"level_{level['id']}"), category("interaction")],
              (0.9, 0.9, 1))
    b.add("MenuController", (0, 0, 0), [script("Content/Graphs/menu.rxgraph", {}), category("interaction")])
    return b.doc()


def validate_graphs(paths: list[pathlib.Path]) -> list[str]:
    errors = []
    m = CodeMcp()
    try:
        for i, p in enumerate(paths, 1):
            doc = json.loads(p.read_text(encoding="utf-8"))
            result = m.call("graph_validate", {"graph": doc})
            if not result.get("ok"):
                errors.append(f"{p.relative_to(ROOT)}: {result.get('errors')}")
            if i % 10 == 0:
                print(f"  validated graphs {i}/{len(paths)}", flush=True)
    finally:
        m.close()
    return errors


def main() -> None:
    LEVEL_DIR.mkdir(parents=True, exist_ok=True)
    SCENE_DIR.mkdir(parents=True, exist_ok=True)
    GRAPH_DIR.mkdir(parents=True, exist_ok=True)
    RULE_DIR.mkdir(parents=True, exist_ok=True)
    assets = load_assets()
    base_graphs = generic_graphs()
    # 共享行为图(豌豆/阳光/探针/判负线/小推车/HUD)。
    base_graphs += [gen_graphs.gen_pea(), gen_graphs.gen_sun(), gen_graphs.gen_probe(),
                    gen_graphs.gen_loseline(), gen_graphs.gen_mower(), gen_graphs.gen_hud()]
    # Per-zombie graph guarantees recycle tag preserves type.
    zombie_graphs = []
    for z in ZOMBIES:
        zid = Z_ID[z["id"]]
        zombie_graphs.append(gen_graphs.gen_zombie(zid, f"Zombies/zombie_{z['id']}.rxgraph"))
    # 每植物独立图:回池标签 plant_free_<id> 与退款费用在生成期烘焙;主体按适配器共享。
    plant_graphs = []
    for p in PLANTS:
        pid = P_ID[p["id"]]
        plant_graphs.append(gen_graphs.gen_plant(pid, behavior_for_plant(p), f"Plants/plant_{pid}.rxgraph",
                                                 float(p["cost"])))

    (RULE_DIR / "plant_behaviors.json").write_text(json.dumps(plant_behavior_manifest(), ensure_ascii=False, indent=2), encoding="utf-8")
    (RULE_DIR / "zombie_behaviors.json").write_text(json.dumps(zombie_behavior_manifest(), ensure_ascii=False, indent=2), encoding="utf-8")
    (RULE_DIR / "special_rules.json").write_text(json.dumps(SPECIAL_RULES, ensure_ascii=False, indent=2), encoding="utf-8")
    (RULE_DIR / "stage_rules.json").write_text(json.dumps(STAGE, ensure_ascii=False, indent=2), encoding="utf-8")

    # Five reusable stage templates.
    for sid in STAGE:
        p = CONTENT / "Scenes" / f"Stage_{sid.capitalize()}.rxscene"
        p.write_text(json.dumps(stage_template(sid, assets), ensure_ascii=False, indent=2), encoding="utf-8")
        ensure_meta(p, "scene", "scene", f"PvZ {sid} 场景模板(棋盘/推车/判负线,无关卡逻辑)")
    for gp in base_graphs + zombie_graphs + plant_graphs:
        ensure_meta(gp, "script", "rxgraph", f"PvZ 行为图 {gp.stem}")

    index = []
    controllers = []
    for idx, level in enumerate(LEVELS, 1):
        av = available_plants(level)
        loadout = recommended(level, av)
        st = STAGE[level["stage"]]
        rows = tuple(playable_rows(level, int(st["rows"])))
        code = level_code(level["id"])
        fname = f"Levels/level_{level['id'].replace('-', '_')}.rxgraph"
        graph_path = gen_graphs.gen_controller(rows=rows, plant_ids=tuple(P_ID[x] for x in loadout),
                                                level_code=code, fname=fname,
                                                sky_sun=bool(st["sky_sun"]["enabled"]),
                                                six_rows=int(st["rows"]) == 6,
                                                all_rows=range(1, int(st["rows"]) + 1))
        controllers.append(graph_path)
        waves = wave_schedule(level, int(st["rows"]))
        runtime = {
            "version": 1,
            "levelId": level["id"], "levelCode": code,
            "stage": level["stage"], "rows": st["rows"], "cols": st["cols"],
            "sourceWaves": level["waves"], "flags": level["flags"],
            "startingSun": level["starting_sun"], "zombieIds": level["zombie_ids"],
            "reward": level["reward"], "specialRule": level.get("special_rules"),
            "availablePlants": av, "recommendedLoadout": loadout,
            "runtimeWaveSchedule": waves,
            "waveScheduleProvenance": {
                "source": "docs/data/levels.json",
                "kind": "synthetic-deterministic-runtime-schedule",
                "note": "原版逐只刷新时间未进入事实源；保留原波数/旗帜/出怪类型，成员顺序由生成器确定。"
            },
            "scene": f"Content/Scenes/Levels/Level_{level['id'].replace('-', '_')}.rxscene",
            "controllerGraph": f"Content/Graphs/{fname}",
            "playableRows": list(rows),
            "runtimeNotes": [
                "事实源波次数/旗帜完整保留；对象池运行时每波按预算生成1..3只(屏外池子不占渲染槽)。",
                "所有植物/僵尸素材与数值可索引；复杂特殊机制由 Rules 适配配置驱动。",
                "交互:点种子卡选植物 → 点格子种植(已占格弹回退款);点格子收集其中阳光;引擎指针输入契约 click_x/click_y/click。",
            ],
        }
        lp = LEVEL_DIR / f"{level['id']}.json"
        lp.write_text(json.dumps(runtime, ensure_ascii=False, indent=2), encoding="utf-8")
        scene = level_scene(level, assets, loadout)
        sp = SCENE_DIR / f"Level_{level['id'].replace('-', '_')}.rxscene"
        sp.write_text(json.dumps(scene, ensure_ascii=False, indent=2), encoding="utf-8")
        ensure_meta(sp, "scene", "scene",
                    f"PvZ 冒险 {level['id']}({level['stage']},{level['waves']} 波,奖励 {level['reward']});在 IDE Assets 面板双击即可装进视口试玩")
        ensure_meta(graph_path, "script", "rxgraph", f"PvZ {level['id']} 关卡控制图(阳光/波次/指针交互/胜负)")
        index.append({"id": level["id"], "stage": level["stage"], "scene": runtime["scene"],
                      "manifest": f"Content/Levels/Adventure/{level['id']}.json",
                      "reward": level["reward"], "specialRule": level.get("special_rules")})
        print(f"  generated {idx}/50 {level['id']} graphNodes={len(json.loads(graph_path.read_text(encoding='utf-8'))['nodes'])}", flush=True)

    (CONTENT / "Levels" / "adventure_index.json").write_text(
        json.dumps({"version": 1, "count": len(index), "levels": index}, ensure_ascii=False, indent=2), encoding="utf-8")
    (CONTENT / "save.json").write_text(json.dumps({"version": 1, "unlockedLevel": "1-1", "completed": [],
                                                     "purchasedPlants": []}, ensure_ascii=False, indent=2), encoding="utf-8")
    main_scene = CONTENT / "Scenes" / "Main.rxscene"
    main_scene.write_text(json.dumps(main_menu(assets), ensure_ascii=False, indent=2), encoding="utf-8")
    ensure_meta(main_scene, "scene", "scene", "PvZ 冒险模式 50 关选择面(运行时暂不支持图内切场景,关卡请直接打开 Scenes/Levels/*)")

    all_graphs = base_graphs + zombie_graphs + plant_graphs + controllers
    errors = validate_graphs(all_graphs)
    if errors:
        raise RuntimeError("graph validation failed:\n" + "\n".join(errors[:20]))
    # Machine gates.
    scenes = list(SCENE_DIR.glob("*.rxscene"))
    manifests = list(LEVEL_DIR.glob("*.json"))
    if len(scenes) != 50 or len(manifests) != 50 or len(controllers) != 50:
        raise RuntimeError(f"count gate failed scenes={len(scenes)} manifests={len(manifests)} graphs={len(controllers)}")
    report = {"levels": 50, "stageTemplates": 5, "scenes": len(scenes), "manifests": len(manifests),
              "controllers": len(controllers), "zombieGraphs": len(zombie_graphs),
              "plantGraphs": len(plant_graphs), "graphValidationErrors": 0}
    (ROOT / ".forge" / "tmp" / "level_build_report.json").parent.mkdir(parents=True, exist_ok=True)
    (ROOT / ".forge" / "tmp" / "level_build_report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print("LEVEL_BUILD_PASS", json.dumps(report), flush=True)


if __name__ == "__main__":
    main()
