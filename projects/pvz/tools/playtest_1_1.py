# playtest_1_1.py — 1-1 垂直切片端到端试玩验证
# 流程:加载场景 → play_enter → 种向日葵 → 种豌豆射手 → 等波次 → 观察事件与帧 → play_exit
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

OUT = pathlib.Path(__file__).resolve().parent.parent / ".forge" / "tmp"
OUT.mkdir(parents=True, exist_ok=True)

def drain_events(m, label):
    ev = m.call("host_events_drain", {})
    if isinstance(ev, list):
        return ev
    if isinstance(ev, dict):
        return ev.get("events") or ev.get("text") or [ev]
    return [ev]

def main():
    m = EngineMcp()
    log = []
    try:
        # 加载场景
        r = m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
        log.append(("scene_load", r))
        print("scene_load:", json.dumps(r, ensure_ascii=False)[:300])
        # play
        r = m.call("play_enter", {})
        print("play_enter:", json.dumps(r, ensure_ascii=False)[:300])
        log.append(("play_enter", r))
        time.sleep(2.0)
        # 种向日葵(plantId=2)到 r3c2:action=2, payload=2*10000+302 → value=2*1000000+20302
        plant_sunflower = 2 * 1000000 + 2 * 10000 + 302
        m.call("logic_inject_input", {"action": "plant_at", "value": float(plant_sunflower)})
        time.sleep(1.0)
        # 种豌豆射手(plantId=1)到 r3c4
        plant_pea = 2 * 1000000 + 1 * 10000 + 304
        m.call("logic_inject_input", {"action": "plant_at", "value": float(plant_pea)})
        time.sleep(1.0)
        # 收集阳光(等向日葵产出)
        time.sleep(26.0)  # 向日葵 24s 产阳光 + 天降 5s
        m.call("logic_inject_input", {"action": "collect_sun", "value": 3 * 1000000 + 1})
        time.sleep(2.0)
        # 等首波僵尸(12s 首波,应该已出)
        for i in range(6):
            time.sleep(5.0)
            ev = drain_events(m, f"t{i}")
            txt = json.dumps(ev, ensure_ascii=False)
            hits = [k for k in ["zombie_died", "loseline_hit", "fire_pea", "spawn_sun", "sun_gained", "LEVEL_WIN", "LEVEL_LOSE", "logic.unsupported", "call_error"] if k in txt]
            print(f"t+{(i+1)*5}s events={len(ev) if isinstance(ev, list) else '?'} hits={hits}")
        # 截图
        fr = m.call("viewport_frame", {})
        print("viewport_frame:", json.dumps(fr, ensure_ascii=False)[:200])
        log.append(("viewport_frame", fr))
        # 场景状态
        ss = m.call("scene_summary", {})
        print("scene_summary:", json.dumps(ss, ensure_ascii=False)[:300])
        m.call("play_exit", {})
    finally:
        m.close()
    (OUT / "playtest_1_1_log.json").write_text(json.dumps(log, ensure_ascii=False, indent=1), encoding="utf-8")
    print("log saved")

if __name__ == "__main__":
    main()
