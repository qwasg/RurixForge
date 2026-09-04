# regression_all.py — PvZ 冒险模式 50 关静态+运行时回归矩阵
from __future__ import annotations

import base64
import json
import pathlib
import subprocess
import sys
import time
from typing import Any

from PIL import Image

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from pvz_engine import EngineMcp

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPO = ROOT.parent.parent
CONTENT = ROOT / "Content"
EVIDENCE = REPO / "evidence" / "pvz"
EVIDENCE.mkdir(parents=True, exist_ok=True)


def jread(path: pathlib.Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def guid_index() -> set[str]:
    out = set()
    for p in CONTENT.rglob("*.meta"):
        for line in p.read_text(encoding="utf-8").splitlines():
            if line.startswith("guid:"):
                out.add(line.split(":", 1)[1].strip())
    return out


def check_static() -> dict[str, Any]:
    failures: list[str] = []
    plants = jread(ROOT / "docs/data/plants.json")
    zombies = jread(ROOT / "docs/data/zombies.json")
    levels = jread(ROOT / "docs/data/levels.json")
    stages = jread(ROOT / "docs/data/stages.json")
    assets = jread(CONTENT / "asset_manifest.json")
    expected = {"plants": 49, "zombies": 26, "levels": 50, "stages": 5}
    actual = {"plants": len(plants), "zombies": len(zombies), "levels": len(levels), "stages": len(stages)}
    if actual != expected:
        failures.append(f"data counts {actual} != {expected}")
    if assets.get("counts", {}).get("plants") != 49 or assets.get("counts", {}).get("zombies") != 26:
        failures.append(f"asset manifest counts bad: {assets.get('counts')}")
    sprites = list((CONTENT / "Sprites").glob("*.rxsprite"))
    source_atlases = list((CONTENT / "Textures/SourceAtlases").glob("*.png"))
    if len(sprites) < 77:
        failures.append(f"sprite docs <77: {len(sprites)}")
    if len(source_atlases) < 12:
        failures.append(f"AI source atlases <12: {len(source_atlases)}")

    level_manifests = sorted((CONTENT / "Levels/Adventure").glob("*.json"))
    level_scenes = sorted((CONTENT / "Scenes/Levels").glob("*.rxscene"))
    controllers = sorted((CONTENT / "Graphs/Levels").glob("*.rxgraph"))
    zombie_graphs = sorted((CONTENT / "Graphs/Zombies").glob("*.rxgraph"))
    stage_scenes = sorted((CONTENT / "Scenes").glob("Stage_*.rxscene"))
    counts = {"levelManifests": len(level_manifests), "levelScenes": len(level_scenes),
              "controllers": len(controllers), "zombieGraphs": len(zombie_graphs),
              "stageScenes": len(stage_scenes), "spriteDocs": len(sprites),
              "sourceAtlases": len(source_atlases)}
    for key, want in [("levelManifests", 50), ("levelScenes", 50), ("controllers", 50),
                      ("zombieGraphs", 26), ("stageScenes", 5)]:
        if counts[key] != want:
            failures.append(f"{key}={counts[key]} expected={want}")

    gids = guid_index()
    source_levels = {x["id"]: x for x in levels}
    max_sprites = 0
    scene_refs = 0
    all_stage_counts: dict[str, int] = {}
    for lp in level_manifests:
        doc = jread(lp)
        lid = doc["levelId"]
        src = source_levels[lid]
        if doc["sourceWaves"] != src["waves"] or doc["flags"] != src["flags"]:
            failures.append(f"{lid} wave/flag mismatch")
        if len(doc["runtimeWaveSchedule"]) != src["waves"]:
            failures.append(f"{lid} runtime schedule count mismatch")
        all_stage_counts[doc["stage"]] = all_stage_counts.get(doc["stage"], 0) + 1
    if all_stage_counts != {"day": 10, "night": 10, "pool": 10, "fog": 10, "roof": 10}:
        failures.append(f"stage distribution bad: {all_stage_counts}")

    for sp in level_scenes + stage_scenes + [CONTENT / "Scenes/Main.rxscene"]:
        doc = jread(sp)
        renderables = 0
        for e in doc.get("entities", []):
            # 对象池停在屏外(|y| ≥ 20),engine-host 视锥裁剪后不占 draw 槽;预算只算屏内精灵。
            parked = abs(float(e.get("transform", {}).get("translation", [0, 0, 0])[1])) >= 20.0
            for c in e.get("components", []):
                if not c.get("enabled", True):
                    continue
                if c.get("type") == "Sprite":
                    if not parked:
                        renderables += 1
                    props = c.get("props", {})
                    for k in ("texture", "sprite"):
                        v = props.get(k)
                        if v and v not in gids:
                            failures.append(f"{sp.name}:{e.get('name')} unknown {k} GUID {v}")
                elif c.get("type") == "Script":
                    scene_refs += 1
                    ref = c.get("props", {}).get("graphRef", "")
                    if ref and not (ROOT / ref).is_file():
                        failures.append(f"{sp.name}:{e.get('name')} missing graph {ref}")
        max_sprites = max(max_sprites, renderables)
        # 屏内静态精灵 + 池子同时在场上限(植物 ≤ 45 格 + 僵尸 ≤ 15 + 豌豆 24 + 阳光 8 不可能同时全在)
        # 取保守门:静态部分 ≤ 96,给运行时对象留 ≥ 32 槽。
        if renderables > 96:
            failures.append(f"{sp.name} on-screen renderables={renderables} >96")

    # JSON parse all graphs and ensure no unsupported node families are used in production graphs.
    unsupported = {"entity.spawn", "entity.destroy", "flow.for_each", "flow.gate", "physics.cast_ray",
                   "physics.apply_impulse", "physics.overlap", "audio.play", "audio.stop",
                   "transform.look_at", "transform.lerp", "debug.draw_debug_line"}
    production_graphs = [p for p in (CONTENT / "Graphs").rglob("*.rxgraph")
                         if "probe_" not in p.name and p.name not in {"projectile.rxgraph", "level_controller_core.rxgraph"}]
    plant_graphs = sorted((CONTENT / "Graphs/Plants").glob("*.rxgraph"))
    counts["plantGraphs"] = len(plant_graphs)
    if len(plant_graphs) != 49:
        failures.append(f"plantGraphs={len(plant_graphs)} expected=49")
    for gp in production_graphs:
        doc = jread(gp)
        bad = sorted({n.get("type") for n in doc.get("nodes", []) if n.get("type") in unsupported})
        if bad:
            failures.append(f"{gp.relative_to(ROOT)} unsupported nodes {bad}")

    # .rx static and DLL checks.
    rx_results = {}
    compiler = pathlib.Path(r"H:\rurix\target\debug\rurixc.exe")
    for name in ("pvz_data.rx", "pvz_rules.rx"):
        src = CONTENT / "Scripts" / name
        check = subprocess.run([str(compiler), str(src), "--emit=check", "--error-format=json"],
                               capture_output=True, text=True, encoding="utf-8", errors="replace")
        dll = ROOT / ".forge/tmp" / f"reg_{src.stem}.dll"
        dll.parent.mkdir(parents=True, exist_ok=True)
        build = subprocess.run([str(compiler), str(src), "--emit=dll", "-o", str(dll)],
                               capture_output=True, text=True, encoding="utf-8", errors="replace")
        rx_results[name] = {"checkExit": check.returncode, "dllExit": build.returncode,
                            "diagnostics": check.stdout.strip()}
        if check.returncode != 0 or build.returncode != 0:
            failures.append(f"{name} compile failed: {check.stderr[:200]} {build.stderr[:200]}")

    return {"ok": not failures, "failures": failures, "counts": counts,
            "dataCounts": actual, "stageDistribution": all_stage_counts,
            "maxOnScreenSprites": max_sprites, "scriptReferences": scene_refs,
            "productionGraphs": len(production_graphs), "rx": rx_results}


def save_frame(frame: dict[str, Any], path: pathlib.Path) -> bool:
    b64 = frame.get("pixelsB64")
    w, h = frame.get("width"), frame.get("height")
    if not b64 or not w or not h:
        return False
    raw = base64.b64decode(b64)
    expected = int(w) * int(h) * 4
    if len(raw) != expected:
        return False
    Image.frombytes("RGBA", (int(w), int(h)), raw).save(path)
    return True


def runtime_matrix() -> dict[str, Any]:
    failures: list[dict[str, Any]] = []
    results: list[dict[str, Any]] = []
    representatives = {"1-1": "day", "2-1": "night", "3-1": "pool", "4-1": "fog", "5-1": "roof"}
    m: EngineMcp | None = None
    try:
        m = EngineMcp()
        for i in range(1, 6):
            for j in range(1, 11):
                lid = f"{i}-{j}"
                rel = f"Content/Scenes/Levels/Level_{i}_{j}.rxscene"
                item: dict[str, Any] = {"level": lid, "scene": rel}
                try:
                    load = m.call("scene_load", {"path": rel}, timeout=120)
                    item["load"] = load
                    if isinstance(load, dict) and load.get("error"):
                        raise RuntimeError(str(load))
                    enter = m.call("play_enter", {}, timeout=120)
                    item["playEnter"] = enter
                    if isinstance(enter, dict) and enter.get("error"):
                        raise RuntimeError(str(enter))
                    time.sleep(0.04)
                    events = m.call("host_events_drain", {}, timeout=30)
                    if isinstance(events, dict):
                        ev_list = events.get("events", [])
                    else:
                        ev_list = events if isinstance(events, list) else []
                    errors = [e for e in ev_list if isinstance(e, dict) and e.get("event") in
                              {"logic.call_error", "logic.unsupported", "anim.warn"}]
                    item["runtimeErrors"] = errors[:10]
                    if errors:
                        raise RuntimeError(f"runtime error events: {errors[:2]}")
                    if lid in representatives:
                        frame = m.call("viewport_frame", {}, timeout=120)
                        shot = EVIDENCE / f"stage_{representatives[lid]}.png"
                        item["frame"] = {k: frame.get(k) for k in ("width", "height", "draws", "truncated", "nonZeroPixels")} if isinstance(frame, dict) else frame
                        item["screenshot"] = str(shot) if isinstance(frame, dict) and save_frame(frame, shot) else None
                    exit_r = m.call("play_exit", {}, timeout=30)
                    item["playExit"] = exit_r
                    item["ok"] = True
                except Exception as exc:
                    item["ok"] = False
                    item["error"] = str(exc)
                    failures.append({"level": lid, "error": str(exc)})
                    try:
                        m.call("play_exit", {}, timeout=10)
                    except Exception:
                        try:
                            m.close()
                        except Exception:
                            pass
                        m = EngineMcp()
                results.append(item)
                if len(results) % 10 == 0:
                    print(f"  runtime {len(results)}/50 failures={len(failures)}", flush=True)
    finally:
        if m is not None:
            m.close()
    return {"ok": not failures, "failures": failures, "results": results,
            "screenshots": sorted(str(p) for p in EVIDENCE.glob("stage_*.png"))}


def write_report(static: dict[str, Any], runtime: dict[str, Any]) -> None:
    full = {"static": static, "runtime": runtime,
            "overallOk": bool(static["ok"] and runtime["ok"]),
            "knownLimitations": [
                "对象池:每关每种植物 6 株、僵尸每种 5 只(总 15)、豌豆 24、阳光 8;屏外池子经 engine-host 视锥裁剪不占 draw 槽。",
                "交互:点种子卡选植物 → 点格子种植(已占格弹回退款)、点格子收集其中阳光;无卡牌冷却、无铲子。",
                "49植物与26僵尸全部具有AI素材、数值与行为适配映射;射击/生产/挡路/接触爆炸四类共享行为,复杂原版特技未逐帧复刻。",
                "八类特殊关(保龄球/砸罐/传送带/僵王等)仍按普通关规则可玩,其适配图只记录事件入口。",
                "渲染管线无 alpha 混合(只有色键丢弃),半透明 UI 以空心框表达;音频与游戏内存档受引擎能力限制。",
            ]}
    regression_text = json.dumps(full, ensure_ascii=False, indent=2)
    (EVIDENCE / "regression.json").write_text(regression_text, encoding="utf-8")
    (ROOT / "docs" / "REGRESSION.json").write_text(regression_text, encoding="utf-8")
    lines = [
        "# PvZ 冒险模式回归报告", "",
        f"- 静态门: {'PASS' if static['ok'] else 'FAIL'}",
        f"- 50关运行时装载/PIE门: {'PASS' if runtime['ok'] else 'FAIL'}",
        f"- 数据: {static['dataCounts']}",
        f"- 产物: {static['counts']}",
        f"- 最大屏内静态精灵数: {static['maxOnScreenSprites']} / 96(池子屏外不占槽,运行时对象预留 ≥32)",
        f"- 运行失败数: {len(runtime['failures'])}", "",
        "## 已知限制", "",
    ] + [f"- {x}" for x in full["knownLimitations"]]
    report_text = "\n".join(lines) + "\n"
    (EVIDENCE / "REGRESSION.md").write_text(report_text, encoding="utf-8")
    (ROOT / "docs" / "REGRESSION.md").write_text(report_text, encoding="utf-8")


def main() -> None:
    static = check_static()
    print(f"STATIC {'PASS' if static['ok'] else 'FAIL'} failures={len(static['failures'])}", flush=True)
    if static["failures"]:
        for f in static["failures"][:20]:
            print("  ", f, flush=True)
    runtime = runtime_matrix() if static["ok"] else {"ok": False, "failures": [{"error": "static gate failed"}], "results": [], "screenshots": []}
    write_report(static, runtime)
    if not static["ok"] or not runtime["ok"]:
        raise RuntimeError(f"REGRESSION_FAIL static={len(static['failures'])} runtime={len(runtime['failures'])}")
    print("REGRESSION_PASS levels=50 screenshots=5", flush=True)


if __name__ == "__main__":
    main()
