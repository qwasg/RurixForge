# probe_state.py — 注入后读实体实际位置,验证种植/出怪是否发生
import json, pathlib, sys, time
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

m = EngineMcp()
try:
    m.call("scene_load", {"path": "Content/Scenes/battle_1_1.rxscene"})
    m.call("play_enter", {})
    time.sleep(1.0)
    m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 2 * 10000 + 302)})
    time.sleep(1.0)
    m.call("logic_inject_input", {"action": "plant_at", "value": float(2 * 1000000 + 1 * 10000 + 304)})
    time.sleep(14.0)  # 等首波
    # 读全部实体位置
    r = m.call("entity_list", {})
    ents = r.get("entities") if isinstance(r, dict) else r
    if not isinstance(ents, list):
        print("entity_list:", json.dumps(r, ensure_ascii=False)[:400])
    else:
        for e in ents:
            name = e.get("name", "?")
            if any(k in name for k in ["Pool", "Zombie", "Pea_", "Sun_", "Mower"]):
                t = e.get("translation") or e.get("transform", {}).get("translation")
                print(f"{name}: {t}")
    m.call("play_exit", {})
finally:
    m.close()
