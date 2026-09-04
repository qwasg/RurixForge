# gen_scene_battle.py — 生成 battle_1_1.rxscene(对象池全量预置)
# 场景即事实:所有池实体/格子/小推车/判负线/HUD 静态预置,运行时只激活。
import json, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEX = ROOT / "Content" / "Textures"
SCENE_OUT = ROOT / "Content" / "Scenes" / "battle_1_1.rxscene"

def tex_guid(name):
    # .meta 为 YAML:取首行 guid
    for line in (TEX / f"{name}.meta").read_text(encoding="utf-8").splitlines():
        if line.startswith("guid:"):
            return line.split(":", 1)[1].strip()
    raise KeyError(f"no guid in {name}.meta")

def read_sprite_guids():
    out = {}
    sdir = ROOT / "Content" / "Sprites"
    if not sdir.is_dir():
        return out
    for f in sdir.glob("*.rxsprite"):
        meta = f.with_suffix(f.suffix + ".meta")
        if meta.is_file():
            for line in meta.read_text(encoding="utf-8").splitlines():
                if line.startswith("guid:"):
                    out[f.stem] = line.split(":", 1)[1].strip()
    return out

def cell_x(col):  # 与 pvz_rules.cell_x 一致
    return -7.2 + (col - 0.5) * 1.6
ROW3_Y = 0.0

class B:
    def __init__(self):
        self.entities = []
        self._id = 0
    def ent(self, name, pos, comps, scale=(1.0, 1.0, 1.0)):
        self._id += 1
        self.entities.append({
            "id": self._id, "name": name,
            "transform": {"rotation": [0.0, 0.0, 0.0, 1.0],
                          "scale": [float(scale[0]), float(scale[1]), float(scale[2])],
                          "translation": [float(pos[0]), float(pos[1]), float(pos[2])]},
            "components": comps,
        })
        return self._id

def sprite(tex, order=0.0, extra=None):
    props = {"texture": tex, "sprite": "", "clip": "", "frame": 0.0,
             "tint": [1.0, 1.0, 1.0, 1.0], "flipX": False, "flipY": False,
             "pixelsPerUnit": 100.0, "sortingOrder": float(order)}
    if extra:
        props.update(extra)
    return {"enabled": True, "props": props, "type": "Sprite"}

def sprite_anim(rxsprite_guid, clip, order=0.0):
    # .rxsprite 图集 + 手动 clip 播放(宿主动画运行时驱动帧)
    return {"enabled": True, "props": {"texture": "", "sprite": rxsprite_guid, "clip": clip, "frame": 0.0,
            "tint": [1.0, 1.0, 1.0, 1.0], "flipX": False, "flipY": False,
            "pixelsPerUnit": 100.0, "sortingOrder": float(order)}, "type": "Sprite"}

def tag(t):
    return {"enabled": True, "props": {"tag": t}, "type": "Tag"}

def script(graph, props):
    return {"enabled": True, "props": {"graphRef": f"Content/Graphs/{graph}", "module": "", "props": props}, "type": "Script"}

def trigger(ext):
    return {"enabled": True, "props": {"extents": ext, "kind": "box"}, "type": "Trigger"}

def cat(c="map"):
    return {"enabled": True, "props": {"category": c}, "type": "Category"}

def build():
    b = B()
    g_lawn = tex_guid("PZ_Lawn.png"); g_cell = tex_guid("PZ_Cell.png")
    g_pea_shooter = tex_guid("PZ_Peashooter.png"); g_sunflower = tex_guid("PZ_Sunflower.png")
    g_zombie = tex_guid("PZ_Zombie.png"); g_pea = tex_guid("PZ_Pea.png"); g_sun = tex_guid("PZ_Sun.png")
    g_mower = tex_guid("PZ_Mower.png"); g_hud = tex_guid("PZ_HUD_Bar.png"); g_card = tex_guid("PZ_Card.png")

    # 相机(正交,orthoSize=6 覆盖草坪+HUD)
    b.ent("Camera", (0.0, 0.0, 10.0), [
        {"enabled": True, "props": {"far": 100.0, "fov": 60.0, "near": 0.1,
                                    "orthoSize": 6.0, "projection": "orthographic"}, "type": "Camera"},
        cat("map")])
    # 背景草坪(整幅)
    b.ent("Lawn", (0.0, 0.0, -0.5), [sprite(g_lawn, 0.0), cat("map")], scale=(23.0, 12.0, 1.0))
    # 9 格子(行 3)
    for col in range(1, 10):
        b.ent(f"Cell_R3_C{col}", (cell_x(col), ROW3_Y, 0.0),
              [sprite(g_cell, 1.0), tag(f"cell_r3c{col}"), cat("map")], scale=(1.5, 1.5, 1.0))
    # 判负线 + 小推车(行 3)
    b.ent("LoseLine_R3", (-8.4, ROW3_Y, 0.0),
          [trigger([0.5, 1.6, 1.6]), tag("loseline"), script("loseline.rxgraph", {"row": 3.0}), cat("interaction")])
    b.ent("Mower_R3", (-7.9, ROW3_Y, 0.0),
          [sprite(g_mower, 4.0), tag("mower_idle"), trigger([1.0, 1.4, 1.0]),
           script("mower.rxgraph", {"row": 3.0}), cat("role")])
    # 真实图集 GUID(若已生成)
    sp = read_sprite_guids()
    # 植物池:豌豆射手×3 / 向日葵×3(场外)
    for i in range(1, 4):
        comp = sprite_anim(sp["PZ_Peashooter"], "idle", 2.0) if "PZ_Peashooter" in sp else sprite(g_pea_shooter, 2.0)
        b.ent(f"Peashooter_Pool_{i}", (-20.0, -60.0 - i * 2.0, 0.0),
              [comp, tag("plant_free_1"), script("plant_shooter.rxgraph", {"plantId": 1.0}),
               trigger([1.2, 1.4, 1.0]), cat("role")])
    for i in range(1, 4):
        comp = sprite_anim(sp["PZ_Sunflower"], "idle", 2.0) if "PZ_Sunflower" in sp else sprite(g_sunflower, 2.0)
        b.ent(f"Sunflower_Pool_{i}", (-24.0, -60.0 - i * 2.0, 0.0),
              [comp, tag("plant_free_2"), script("plant_producer.rxgraph", {"plantId": 2.0}),
               trigger([1.2, 1.4, 1.0]), cat("role")])
    # 僵尸池 ×5(普通僵尸 zombieId=1)
    for i in range(1, 6):
        comp = sprite_anim(sp["PZ_Zombie"], "walk", 4.0) if "PZ_Zombie" in sp else sprite(g_zombie, 4.0)
        b.ent(f"Zombie_Pool_{i}", (20.0, -60.0 - i * 2.5, 0.0),
              [comp, tag("zombie_free_1"),
               script("zombie_walker.rxgraph", {"poolId": float(i), "zombieId": 1.0}),
               trigger([0.9, 1.4, 0.9]), cat("role")])
    # 豌豆池 ×8
    for i in range(1, 9):
        comp = sprite_anim(sp["PZ_Pea"], "idle", 3.0) if "PZ_Pea" in sp else sprite(g_pea, 3.0)
        b.ent(f"Pea_Pool_{i}", (24.0, -60.0 - i * 1.5, 0.0),
              [comp, tag("pea_free"), script("pea_fly.rxgraph", {"poolId": float(i)}), cat("role")],
              scale=(0.5, 0.5, 1.0))
    # 阳光池 ×6
    for i in range(1, 7):
        comp = sprite_anim(sp["PZ_Sun"], "idle", 5.0) if "PZ_Sun" in sp else sprite(g_sun, 5.0)
        b.ent(f"Sun_Pool_{i}", (28.0, -60.0 - i * 1.5, 0.0),
              [comp, tag("sun_free"), script("sun_fall.rxgraph", {"poolId": float(i)}), cat("role")])
    # 控制器 + HUD
    b.ent("LevelController", (0.0, 0.0, 0.0),
          [script("level_controller.rxgraph", {"levelCode": 101.0}), cat("interaction")])
    b.ent("HUDController", (0.0, 0.0, 0.0), [script("hud.rxgraph", {}), cat("interaction")])
    b.ent("HUD_SunBar", (-8.0, 4.6, 0.0), [sprite(g_hud, 10.0), tag("hud_sunbar"), cat("interaction")])
    # 卡槽(切片 2 张)
    b.ent("HUD_Card_1", (-6.4, 4.6, 0.0), [sprite(g_card, 10.0), tag("hud_card_1"), cat("interaction")])
    b.ent("HUD_Card_2", (-5.2, 4.6, 0.0), [sprite(g_card, 10.0), tag("hud_card_2"), cat("interaction")])

    doc = {"entities": b.entities, "mode": "2d", "name": "battle_1_1"}
    SCENE_OUT.write_text(json.dumps(doc, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"wrote {SCENE_OUT} entities={len(b.entities)}")

if __name__ == "__main__":
    build()
