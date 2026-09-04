# playtest_pointer.py — 用引擎指针输入(logic_inject_pointer)完整试玩 1-1:
#   选卡 → 点格种植 → 点格收阳光 → 观察出怪/射击/击杀/胜负,逐步存帧到 evidence/pvz/web-playtest。
# 用法: python tools/playtest_pointer.py [level] [seconds]
from __future__ import annotations

import base64
import json
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT.parent.parent / "evidence" / "pvz" / "web-playtest"
OUT.mkdir(parents=True, exist_ok=True)
W, H = 960, 540
ORTHO = 6.2
HALF_W = ORTHO * W / H
CARD_X0, CARD_PITCH, CARD_Y = -6.2, 1.3, 5.35
T0 = time.time()
LOG: list[str] = []


def say(msg: str) -> None:
    line = f"[t+{time.time() - T0:6.1f}s] {msg}"
    print(line, flush=True)
    LOG.append(line)


def norm(wx: float, wy: float) -> tuple[float, float]:
    return ((wx / HALF_W + 1.0) / 2.0, (1.0 - wy / ORTHO) / 2.0)


def cell_center(row: int, col: int, rows: int = 5) -> tuple[float, float]:
    return (-7.2 + (col - 0.5) * 1.6, (rows - row + 0.5) * 1.6 - rows * 0.8)


class Game:
    def __init__(self) -> None:
        self.m = EngineMcp()

    def click_world(self, wx: float, wy: float, label: str) -> None:
        nx, ny = norm(wx, wy)
        r = self.m.call("logic_inject_pointer", {"x": nx, "y": ny, "width": W, "height": H})
        say(f"click {label} world=({wx:.2f},{wy:.2f}) → {json.dumps(r, ensure_ascii=False)[:140]}")

    def click_card(self, slot: int) -> None:
        self.click_world(CARD_X0 + slot * CARD_PITCH, CARD_Y, f"card#{slot}")

    def click_cell(self, row: int, col: int, rows: int = 5) -> None:
        x, y = cell_center(row, col, rows)
        self.click_world(x, y, f"cell r{row}c{col}")

    def frame(self, name: str) -> None:
        f = self.m.call("viewport_frame", {"width": W, "height": H})
        if not f.get("pixelsB64"):
            say(f"frame {name} FAILED: {json.dumps(f, ensure_ascii=False)[:200]}")
            return
        from PIL import Image
        raw = base64.b64decode(f["pixelsB64"])
        Image.frombytes("RGBA", (int(f["width"]), int(f["height"])), raw).save(OUT / name)
        say(f"[frame] {name} draws={f.get('draws')} truncated={f.get('truncated')}")

    def pos(self, eid: int):
        t = self.m.call("transform_get", {"id": eid})
        tr = t.get("translation") if isinstance(t, dict) else None
        return [round(x, 2) for x in tr] if tr else t

    def find(self, name_prefix: str) -> list[tuple[int, str, list[float]]]:
        r = self.m.call("entity_list", {})
        out = []
        for e in r.get("entities", []):
            if e["name"].startswith(name_prefix):
                out.append((e["id"], e["name"], [round(x, 2) for x in e["transform"]["translation"]]))
        return out

    def onscreen(self, name_prefix: str) -> list[tuple[int, str, list[float]]]:
        return [x for x in self.find(name_prefix) if x[2][1] > -20]

    def drain_logs(self) -> list[str]:
        ev = self.m.call("host_events_drain", {})
        evs = ev if isinstance(ev, list) else (ev.get("events") if isinstance(ev, dict) else None) or []
        return [e.get("message") for e in evs if isinstance(e, dict) and e.get("event") == "logic.log"]

    def close(self) -> None:
        try:
            self.m.call("play_exit", {})
        except Exception:
            pass
        self.m.close()


def main() -> None:
    level = sys.argv[1] if len(sys.argv) > 1 else "1-1"
    total = float(sys.argv[2]) if len(sys.argv) > 2 else 150.0
    scene = f"Content/Scenes/Levels/Level_{level.replace('-', '_')}.rxscene"
    g = Game()
    try:
        say("load: " + json.dumps(g.m.call("scene_load", {"path": scene}), ensure_ascii=False))
        say("play: " + json.dumps(g.m.call("play_enter", {}), ensure_ascii=False))
        time.sleep(1.0)
        g.frame("P01_start.png")
        say(f"logs: {g.drain_logs()[:10]}")
        # 选豌豆射手(卡 0)但阳光不够(50<100):点格无事发生
        g.click_card(0)
        time.sleep(0.3)
        g.click_cell(3, 2)
        time.sleep(0.5)
        say(f"logs: {g.drain_logs()}  plants on lawn: {g.onscreen('PlantPool_')}")
        # 等第 1 颗天降阳光落地(3s 生成,从 y=6.6 落到行 1: 约 2.1s)后点它所在格收集
        time.sleep(5.5)
        suns = g.onscreen("SunPool_")
        say(f"suns on screen: {suns}")
        g.frame("P02_sun_falling.png")
        for _, _, (sx, sy, _) in suns:
            col = int((sx + 7.2) // 1.6) + 1
            row = int((4.0 - sy) // 1.6) + 1
            if 1 <= row <= 5 and 1 <= col <= 9:
                g.click_cell(row, col)
                time.sleep(0.4)
        say(f"logs: {g.drain_logs()}  suns after click: {g.onscreen('SunPool_')}")
        # 第 2 颗阳光(约 t=12s)
        time.sleep(6.5)
        suns = g.onscreen("SunPool_")
        say(f"suns on screen: {suns}")
        for _, _, (sx, sy, _) in suns:
            col = int((sx + 7.2) // 1.6) + 1
            row = int((4.0 - sy) // 1.6) + 1
            if 1 <= row <= 5 and 1 <= col <= 9:
                g.click_cell(row, col)
                time.sleep(0.4)
        say(f"logs: {g.drain_logs()}")
        # 现在应有 100 阳光:选卡 + 点 r3c2 种植
        g.click_card(0)
        time.sleep(0.3)
        g.click_cell(3, 2)
        time.sleep(0.6)
        say(f"logs: {g.drain_logs()}  plants on lawn: {g.onscreen('PlantPool_')}")
        g.frame("P03_planted.png")
        # 再种一次到同一格 → 应弹回并退款
        g.click_card(0)
        time.sleep(0.3)
        g.click_cell(3, 2)
        time.sleep(0.8)
        say(f"logs(after replant same cell): {g.drain_logs()}  plants: {g.onscreen('PlantPool_')}")
        # 战斗观察:每 10s 收一次屏上阳光、记录僵尸/植物,尝试补种第二株到 r3c4
        planted_second = False
        elapsed = 0.0
        while elapsed < total:
            time.sleep(10.0)
            elapsed += 10.0
            for _, _, (sx, sy, _) in g.onscreen("SunPool_"):
                col = int((sx + 7.2) // 1.6) + 1
                row = int((4.0 - sy) // 1.6) + 1
                if 1 <= row <= 5 and 1 <= col <= 9:
                    g.click_cell(row, col)
                    time.sleep(0.3)
            if not planted_second and elapsed >= 30:
                g.click_card(0)
                time.sleep(0.3)
                g.click_cell(3, 4)
                time.sleep(0.5)
                planted_second = True
            logs = g.drain_logs()
            banners = g.onscreen("Banner_")
            say(f"+{elapsed:.0f}s zombies={g.onscreen('ZombiePool_')} plants={g.onscreen('PlantPool_')} peas={len(g.onscreen('PeaPool_'))} suns={len(g.onscreen('SunPool_'))} logs={[l for l in logs if l not in ('SUN_COLLECT_INPUT',)][:8]}")
            if elapsed in (20.0, 50.0, 90.0, 130.0):
                g.frame(f"P0{4 + [20.0, 50.0, 90.0, 130.0].index(elapsed)}_battle_{int(elapsed)}s.png")
            # 事件环被 logic.call 冲刷,胜负以横幅实体是否进屏为准(HUD 图收到 level_win/game_over 才挪进来)。
            if banners or any(l in ("LEVEL_WIN", "LEVEL_LOSE") for l in logs):
                say(f"*** terminal: banners={banners} logs={[l for l in logs if l in ('LEVEL_WIN', 'LEVEL_LOSE')]}")
                time.sleep(1.0)
                g.frame("P09_end.png")
                break
        say("summary: " + json.dumps(g.m.call("scene_summary", {}), ensure_ascii=False)[:300])
    finally:
        g.close()
        (OUT / "playtest_pointer.log").write_text("\n".join(LOG) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
