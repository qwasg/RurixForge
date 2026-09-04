# run_level.py — PvZ 冒险关卡交互式启动器
# 用法: python tools/run_level.py 1-1
# 命令: plant <plant_id> <row> <col> | collect | frame | events | status | quit
from __future__ import annotations

import base64
import json
import pathlib
import sys
import time

from PIL import Image

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent
PLANTS = json.loads((ROOT / "docs/data/plants.json").read_text(encoding="utf-8"))
P_ID = {p["id"]: i + 1 for i, p in enumerate(PLANTS)}


def save_frame(frame: dict, path: pathlib.Path) -> None:
    raw = base64.b64decode(frame["pixelsB64"])
    Image.frombytes("RGBA", (int(frame["width"]), int(frame["height"])), raw).save(path)


def main() -> None:
    level = sys.argv[1] if len(sys.argv) > 1 else "1-1"
    if level not in {f"{a}-{b}" for a in range(1, 6) for b in range(1, 11)}:
        raise SystemExit("level must be 1-1 .. 5-10")
    scene = f"Content/Scenes/Levels/Level_{level.replace('-', '_')}.rxscene"
    m = EngineMcp()
    try:
        print("load:", m.call("scene_load", {"path": scene}))
        print("play:", m.call("play_enter", {}))
        print("commands: plant <id> <row> <col> | collect | frame | events | status | quit")
        print("available plant ids:", ", ".join(P_ID))
        while True:
            try:
                line = input(f"pvz {level}> ").strip()
            except EOFError:
                line = "quit"
            if not line:
                continue
            args = line.split()
            cmd = args[0].lower()
            if cmd in {"quit", "exit"}:
                break
            if cmd == "plant" and len(args) == 4:
                pid, row_s, col_s = args[1], args[2], args[3]
                if pid not in P_ID:
                    print("unknown plant", pid); continue
                row, col = int(row_s), int(col_s)
                payload = P_ID[pid] * 10000 + row * 100 + col
                value = 2 * 1000000 + payload
                print(m.call("logic_inject_input", {"action": "plant_at", "value": float(value)}))
            elif cmd == "collect":
                print(m.call("logic_inject_input", {"action": "collect_sun", "value": 3000001.0}))
            elif cmd == "frame":
                frame = m.call("viewport_frame", {})
                if frame.get("pixelsB64"):
                    out = ROOT / ".forge/tmp" / f"level_{level.replace('-', '_')}.png"
                    out.parent.mkdir(parents=True, exist_ok=True)
                    save_frame(frame, out)
                    print("saved", out, {k: frame.get(k) for k in ("width", "height", "draws", "truncated")})
                else:
                    print(frame)
            elif cmd == "events":
                events = m.call("host_events_drain", {})
                if isinstance(events, list):
                    for event in events[-40:]:
                        if isinstance(event, dict) and event.get("event") != "logic.update":
                            print(json.dumps(event, ensure_ascii=False))
                else:
                    print(events)
            elif cmd == "status":
                print(m.call("scene_summary", {}))
            elif cmd == "wait" and len(args) == 2:
                time.sleep(float(args[1]))
            else:
                print("commands: plant <plant_id> <row> <col> | collect | frame | events | status | wait <sec> | quit")
    finally:
        try:
            m.call("play_exit", {})
        except Exception:
            pass
        m.close()


if __name__ == "__main__":
    main()
