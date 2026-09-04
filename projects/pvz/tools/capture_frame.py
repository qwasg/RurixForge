# capture_frame.py — 游戏中截图取证
import json, pathlib, sys, time, base64
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT.parent.parent / "evidence"
OUT.mkdir(exist_ok=True)

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    time.sleep(1.0)
    # 种两株向日葵 + 等僵尸出来
    m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 2 * 10000 + 302)})
    time.sleep(14.0)  # 等首波僵尸
    # 截图
    fr = m.call("viewport_frame", {})
    print("frame keys:", list(fr.keys()) if isinstance(fr, dict) else type(fr))
    if isinstance(fr, dict):
        fp = fr.get("framePath")
        print("framePath:", fp, "size:", fr.get("width"), "x", fr.get("height"), "draws:", fr.get("draws"))
        if fp and pathlib.Path(fp).is_file():
            import shutil
            dst = OUT / "pvz_1_1_slice.png"
            shutil.copy(fp, dst)
            print("saved:", dst)
        elif fr.get("pixelsB64"):
            (OUT / "pvz_1_1_slice.png").write_bytes(base64.b64decode(fr["pixelsB64"]))
            print("saved from b64")
    m.call("play_exit", {})
finally:
    m.close()
