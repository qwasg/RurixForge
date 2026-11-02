#!/usr/bin/env python3
"""Measure synthetic skinned-model RPC costs using existing backend binaries.

This is a controlled 22,500-vertex / 8-bone skinning workload, not a real asset
or a complete game benchmark. Frame wall time includes RGBA8 readback, encoding,
transport and JSON parsing. Animation step RPC latency is reported separately.
No build is performed. Each backend/count uses a fresh owned process tree.
Example: py -3 scripts/benchmark-3d-models.py --counts 1,16 --samples 5 --rounds 3
"""
from __future__ import annotations

import argparse
import base64
import datetime as dt
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import shutil
import sys
import time
import traceback


SCRIPT = Path(__file__).resolve()
REPO = SCRIPT.parents[1]
_spec = importlib.util.spec_from_file_location("forge_backend_benchmark", SCRIPT.with_name("benchmark-3d-backends.py"))
common = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(common)
EVIDENCE_LIMIT = 30 * 1024 * 1024
CASE_LOG_LIMIT = 256 * 1024
VERTICES = 22500
TRIANGLES = 44402
BONES = 8
DEFAULT_BACKENDS = "rurix,godot-forward-plus-d3d12,godot-forward-plus-vulkan,godot-mobile-d3d12"


class ModelHost(common.Host):
    """Reuse host ownership/RPC; bound logs and observe every MODEL_BUDGET line."""
    def __init__(self, *args, **kwargs):
        self.model_budget_messages = []
        self.model_budget_count = 0
        super().__init__(*args, **kwargs)

    def pump(self, stream, tag, ready):
        try:
            for raw in iter(stream.readline, b""):
                line = raw.decode("utf-8", errors="replace").rstrip()
                with self.log_lock:
                    self.tail.append(f"{tag}: {line}")
                    if "MODEL_BUDGET" in line:
                        self.model_budget_count += 1
                        if len(self.model_budget_messages) < 20:
                            self.model_budget_messages.append(f"{tag}: {line}"[:2048])
                    if self.log_bytes < CASE_LOG_LIMIT:
                        encoded = f"{tag}: {line}\n".encode("utf-8")
                        room = CASE_LOG_LIMIT - self.log_bytes
                        self.log.write(encoded[:room].decode("utf-8", errors="ignore"))
                        self.log.flush()
                        self.log_bytes += min(len(encoded), room)
                prefix = "FORGE_HOST_LISTENING port="
                if line.startswith(prefix):
                    ready.put(int(line[len(prefix):].split()[0]))
        finally:
            stream.close()


def output_bytes(root):
    return sum(path.stat().st_size for path in root.rglob("*") if path.is_file())


def write_json(path, value, output):
    # Evidence remains small even when the caller requests many configurations.
    payload_size = len(json.dumps(value, ensure_ascii=False, indent=2).encode("utf-8"))
    previous = path.stat().st_size if path.exists() else 0
    if output_bytes(output) - previous + payload_size > EVIDENCE_LIMIT:
        raise RuntimeError("30 MiB evidence budget would be exceeded")
    common.write_json(path, value)


def save_png(path, width, height, rgba, output):
    # common.png writes through write_bytes, allowing a size check before disk IO.
    class Buffer:
        def write_bytes(self, value):
            self.value = value
    buffer = Buffer()
    common.png(buffer, width, height, rgba)
    if output_bytes(output) + len(buffer.value) > EVIDENCE_LIMIT:
        raise RuntimeError("30 MiB evidence budget would be exceeded by PNG")
    path.write_bytes(buffer.value)


def validate_model(path):
    with path.open(encoding="utf-8") as stream:
        model = json.load(stream)
    vertices = sum(len(primitive.get("positions", [])) for primitive in model.get("primitives", []))
    triangles = sum(len(primitive.get("indices", [])) // 3 for primitive in model.get("primitives", []))
    bones = sum(len(skin.get("joints", [])) for skin in model.get("skins", []))
    animations = [animation.get("name") for animation in model.get("animations", [])]
    if (vertices, triangles, bones) != (VERTICES, TRIANGLES, BONES) or "walk" not in animations:
        raise ValueError(f"expected grid 22500 vertices / 44402 triangles / 8 bones / walk, got {(vertices, triangles, bones, animations)}")
    return {"vertices": vertices, "triangles": triangles, "bones": bones, "animations": animations}


def animator_props(anim_time, playing=True):
    return {"clip": "walk", "time": anim_time, "loop": True, "playing": playing}


def fixture(count, width, height):
    # Meshes face +Z in the XY plane. Six-unit spacing and a wide orthographic
    # view leave room for the full animated bone chain, not only its rest pose.
    cols = min(count, math.ceil(math.sqrt(count * width / height)))
    rows = math.ceil(count / cols)
    entities = []
    for index in range(count):
        transform = {"translation": [((index % cols) - (cols - 1) / 2) * 6,
                                      ((index // cols) - (rows - 1) / 2) * 6, 0],
                     "rotation": [0, 0, 0, 1], "scale": [1, 1, 1]}
        entities.append({"id": index + 1, "name": f"skinned-grid-{index:03d}",
                         "transform": transform,
                         "components": [{"type": "ModelRenderer", "enabled": True,
                                         "props": {"model": "Models/grid.rxmodel"}},
                                        {"type": "Animator", "enabled": True,
                                         "props": animator_props(.3)}]})
    half_height = max(rows * 3 + 2, (cols * 3 + 2) * height / width)
    camera = {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "dist": 30,
              "fovY": 50, "ortho": True, "orthoSize": half_height}
    return {"name": f"skinned-grids-{count}", "mode": "3d", "next_id": count + 1,
            "entities": entities}, camera


def timed_call(rpc, method, params):
    start = time.perf_counter()
    value = rpc.call(method, params)
    return value, (time.perf_counter() - start) * 1000


def timed_frame(rpc, request, keep_pixels=False):
    frame, elapsed = timed_call(rpc, "viewport.frame", request)
    pixels = frame.pop("pixelsB64")
    stats = common.frame_stats(frame)
    # Hashing and base64 decoding are outside the measured RPC interval.
    pixel_hash = hashlib.sha256(pixels.encode("ascii")).hexdigest()
    rgba = base64.b64decode(pixels, validate=True) if keep_pixels else None
    if rgba is not None and len(rgba) != request["width"] * request["height"] * 4:
        raise ValueError("RGBA8 validation byte count mismatch")
    return {"frameMs": elapsed, "stats": stats, "encodedPixelsSha256": pixel_hash}, rgba


def measure_phase(rpc, request, args, animated):
    phase_rounds, records = [], []
    for round_index in range(args.rounds):
        samples = []
        for sample_index in range(args.samples):
            step, step_ms = (None, None)
            if animated:
                step, step_ms = timed_call(rpc, "play.step", {})
            record, _ = timed_frame(rpc, request)
            record.update(round=round_index + 1, sample=sample_index + 1)
            if animated:
                record.update(stepMs=step_ms, stepResult=step,
                              stepAndFrameMs=step_ms + record["frameMs"])
            samples.append(record)
        entry = {"round": round_index + 1,
                 "frame": common.summary([sample["frameMs"] for sample in samples]),
                 "rawSamples": samples}
        if animated:
            entry["step"] = common.summary([sample["stepMs"] for sample in samples])
            entry["stepAndFrame"] = common.summary([sample["stepAndFrameMs"] for sample in samples])
        phase_rounds.append(entry)
        records.extend(samples)
        print(f"  {'animated' if animated else 'static'} round {round_index + 1}: "
              f"frame p50={entry['frame']['p50Ms']:.2f} ms p95={entry['frame']['p95Ms']:.2f} ms", flush=True)
    phase = {"rounds": phase_rounds,
             "frame": common.summary([sample["frameMs"] for sample in records]),
             "distinctPixelHashes": len({sample["encodedPixelsSha256"] for sample in records})}
    if animated:
        phase["step"] = common.summary([sample["stepMs"] for sample in records])
        phase["stepAndFrame"] = common.summary([sample["stepAndFrameMs"] for sample in records])
    return phase, records


def run_case(output, project, backend, count, scene_path, camera, args):
    case_name = f"{backend}__{count}__{args.width}x{args.height}"
    directory = output / "cases" / case_name
    directory.mkdir(parents=True)
    # Reserve room for our capped live log before starting a process.
    if output_bytes(output) + CASE_LOG_LIMIT + 65536 > EVIDENCE_LIMIT:
        raise RuntimeError("insufficient remaining 30 MiB evidence budget for another case")
    result = {"case": case_name, "backend": backend, "instances": count,
              "width": args.width, "height": args.height,
              "expectedVertices": count * VERTICES, "expectedTriangles": count * TRIANGLES,
              "bonesPerInstance": BONES, "scene": common.sha_file(scene_path), "camera": camera,
              "measurement": "exact RGBA8 JSON-RPC frame wall latency, including readback/encoding/transport/JSON parsing; base64 decode/hash excluded; NOT GPU frame time or full game FPS; play.step reported separately",
              "workload": "synthetic skinned grid control load; not a production asset or complete game",
              "valid": False, "anyMODEL_BUDGET": False, "gpuBefore": common.gpu_snapshot()}
    host = None
    all_records = []
    try:
        host = ModelHost(REPO, project, backend, directory / "host.log", args.timeout, args.gpu_index, False)
        result.update(pid=host.proc.pid, command=host.command, readyMs=host.ready_ms)
        rpc = host.rpc
        result["backendInfoBefore"] = rpc.call("render.backendInfo")
        result["capabilities"] = rpc.call("render.capabilities")
        result["sceneLoad"], result["sceneLoadMs"] = timed_call(rpc, "scene.load", {"path": str(scene_path)})
        result["appliedCamera"] = rpc.call("viewport.setCamera", camera)
        request = {"width": args.width, "height": args.height, "format": "rgba8", "exact": True}
        probes = []
        probe_rgba = []
        for anim_time, label in ((0.0, "time-0"), (.8, "time-0p8")):
            for entity_id in range(1, count + 1):
                rpc.call("component.set", {"id": entity_id, "type": "Animator",
                                           "props": animator_props(anim_time)})
            record, rgba = timed_frame(rpc, request, keep_pixels=True)
            save_png(directory / f"validation-{label}.png", args.width, args.height, rgba, output)
            color_count = len({rgba[(y * args.width + x) * 4:(y * args.width + x) * 4 + 3]
                               for y in range(0, args.height, max(1, args.height // 64))
                               for x in range(0, args.width, max(1, args.width // 64))})
            probes.append({"time": anim_time, **record, "sampledRgbColors": color_count,
                           "rgbaSha256": hashlib.sha256(rgba).hexdigest()})
            probe_rgba.append(rgba)
        result["validation"] = {"probes": probes,
                                "differentTimeChangedPixels": sum(probe_rgba[0][i:i+4] != probe_rgba[1][i:i+4]
                                                                  for i in range(0, len(probe_rgba[0]), 4)),
                                "differentTimeHashesDiffer": probes[0]["rgbaSha256"] != probes[1]["rgbaSha256"]}
        del probe_rgba, rgba
        for entity_id in range(1, count + 1):
            rpc.call("component.set", {"id": entity_id, "type": "Animator", "props": animator_props(.3)})
        for _ in range(args.warmup):
            timed_frame(rpc, request)
        result["static"], records = measure_phase(rpc, request, args, animated=False)
        all_records.extend(records)
        result["playEnter"] = rpc.call("play.enter", {})
        result["playPause"] = rpc.call("play.pause", {})
        for _ in range(args.warmup):
            rpc.call("play.step", {})
            timed_frame(rpc, request)
        result["animated"], records = measure_phase(rpc, request, args, animated=True)
        all_records.extend(records)
        result["playExit"] = rpc.call("play.exit", {})
        result["backendInfoAfter"] = rpc.call("render.backendInfo")
        checked_stats = [record["stats"] for record in all_records] + [probe["stats"] for probe in probes]
        faults = []
        for key, expected in (("width", args.width), ("height", args.height), ("format", "rgba8")):
            if any(stat.get(key) != expected for stat in checked_stats):
                faults.append(f"unexpected {key}")
        if any(stat.get("draws", 0) < count for stat in checked_stats):
            faults.append("fewer draws than requested model instances")
        if any(stat.get("triangles", 0) < count * TRIANGLES for stat in checked_stats):
            faults.append("fewer triangles than complete grid workload")
        if any(stat.get("truncated") for stat in checked_stats):
            faults.append("truncated geometry")
        if any(stat.get("meshFallbacks", 0) for stat in checked_stats):
            faults.append("unexpected mesh fallback")
        if any(stat.get("nonZeroPixels", 0) <= 0 for stat in checked_stats) or any(probe["sampledRgbColors"] < 2 for probe in probes):
            faults.append("blank or uniform validation image")
        if not result["validation"]["differentTimeHashesDiffer"]:
            faults.append("different animation times produced identical RGBA8")
        if args.samples * args.rounds > 1 and result["animated"]["distinctPixelHashes"] < 2:
            faults.append("play.step did not change any sampled animation frame")
        if args.expected_device and any(args.expected_device.lower() not in stat.get("deviceName", "").lower() for stat in checked_stats):
            faults.append("actual GPU does not match --expected-device")
        result.update(invalidReasons=faults, valid=not faults,
                      actualDevices=sorted({stat.get("deviceName", "") for stat in checked_stats}),
                      lastStats=all_records[-1]["stats"])
    except Exception as exc:
        result.update(valid=False, error=str(exc), traceback=traceback.format_exc())
        result["anyMODEL_BUDGET"] = "MODEL_BUDGET" in str(exc)
    finally:
        if host:
            host.close()
            result["hostTail"] = list(host.tail)
            result["hostExitCode"] = host.proc.returncode
            result["modelBudgetMessages"] = host.model_budget_messages
            result["modelBudgetLineCount"] = host.model_budget_count
            result["anyMODEL_BUDGET"] |= bool(host.model_budget_count)
            if result["anyMODEL_BUDGET"]:
                result["valid"] = False
                result.setdefault("invalidReasons", []).append("MODEL_BUDGET observed")
        result["gpuAfter"] = common.gpu_snapshot()
        write_json(directory / "result.json", result, output)
    print(f"{case_name}: {'VALID' if result['valid'] else 'INVALID'} "
          f"{result.get('invalidReasons', result.get('error', ''))}", flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backends", default=DEFAULT_BACKENDS)
    parser.add_argument("--counts", default="1,16", help="comma-separated instance counts, each 1..32")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--warmup", type=int, default=2)
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--gpu-index", type=int, default=0)
    parser.add_argument("--expected-device", default="NVIDIA")
    parser.add_argument("--model", type=Path, default=REPO / "target/g4-fixtures/g4-anim-cost-51880-0/Content/Models/grid.rxmodel")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--prepare-only", action="store_true", help="copy the small fixture and write metadata; start no backend process")
    args = parser.parse_args()
    backends = [value.strip() for value in args.backends.split(",") if value.strip()]
    try:
        counts = [int(value) for value in args.counts.split(",")]
    except ValueError:
        parser.error("--counts must be comma-separated integers")
    if not backends or set(backends) - common.BACKENDS.keys():
        parser.error("unknown or empty --backends")
    if len(set(backends)) != len(backends) or len(set(counts)) != len(counts):
        parser.error("duplicate backends/counts are not allowed")
    if not counts or min(counts) < 1 or max(counts) > 32:
        parser.error("--counts values must be 1..32; default deliberately limited to 1,16")
    if min(args.samples, args.rounds) < 1 or args.warmup < 0 or args.timeout <= 0:
        parser.error("samples/rounds/timeout must be positive, warmup nonnegative")
    if min(args.width, args.height) < 16 or max(args.width, args.height) > 1920:
        parser.error("width/height must be 16..1920")
    source = args.model.resolve(strict=True)
    model_info = validate_model(source)
    output = (args.output or REPO / "evidence" / ("model-backend-selection-" + dt.datetime.now().strftime("%Y%m%d-%H%M%S"))).resolve()
    output.mkdir(parents=True, exist_ok=False)
    project = output / "project"
    (project / "Content/Models").mkdir(parents=True)
    (project / "Content/Scenes").mkdir()
    (project / "forge.toml").write_text('[project]\nname="synthetic-model-benchmark"\nengine-version="0.1.0"\nmode="3d"\n\n[dirs]\ncontent="Content"\n', encoding="utf-8")
    copied_model = project / "Content/Models/grid.rxmodel"
    shutil.copyfile(source, copied_model)  # one small model copy shared by sequential cases
    scenes = {}
    for count in counts:
        scene, camera = fixture(count, args.width, args.height)
        path = project / f"Content/Scenes/grids-{count}.rxscene"
        write_json(path, scene, output)
        scenes[count] = path, camera
    metadata = {"createdUtc": dt.datetime.now(dt.timezone.utc).isoformat(),
                "arguments": {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()},
                "script": common.sha_file(SCRIPT), "helperScript": common.sha_file(SCRIPT.with_name("benchmark-3d-backends.py")),
                "sourceModel": common.sha_file(source), "copiedModel": common.sha_file(copied_model),
                "modelStructure": model_info, "python": sys.version,
                "workload": "synthetic planar skinned grid; 22500 vertices, 44402 triangles, 8 bones per model; no production content or gameplay",
                "timingCaveat": "existing debug backend binaries; no build/source freshness claim; RGBA8 RPC wall latency is not pure GPU frame time, game FPS, or standalone renderer throughput; step and frame costs separate; Godot max_fps=60 remains",
                "samplingCaveat": "default 15 samples per phase; p95 is descriptive for this small sample, not a long-run tail guarantee",
                "gpuSelection": {"rurixEnvironment": {"RURIX_PREFER_DISCRETE_GPU": "1"}, "godotGpuIndex": args.gpu_index,
                                 "requiredDeviceSubstring": args.expected_device},
                "processIsolation": "new owned process tree per backend/count; serial cases; one shared read-mostly model project",
                "evidenceLimitBytes": EVIDENCE_LIMIT, "caseLogLimitBytes": CASE_LOG_LIMIT,
                "storageBefore": common.storage_snapshot(output),
                "binaries": [common.sha_file(path) if path.exists() else {"missing": str(path)} for path in
                             (REPO / "target/debug/engine-host.exe", REPO / "target/godot-runtime/forge-godot.exe", REPO / "target/godot-runtime/bin/godot_host.dll")],
                "runtimeProject": common.sha_file(REPO / "target/godot-runtime/project.godot")}
    write_json(output / "metadata.json", metadata, output)
    print(f"evidence: {output}", flush=True)
    results = []
    if not args.prepare_only:
        for backend in backends:
            for count in counts:
                print(f"START {backend}: {count} skinned models", flush=True)
                path, camera = scenes[count]
                results.append(run_case(output, project, backend, count, path, camera, args))
                write_json(output / "results.json", results, output)
    metadata.update(finishedUtc=dt.datetime.now(dt.timezone.utc).isoformat(),
                    storageAfter=common.storage_snapshot(output), evidenceBytes=output_bytes(output))
    write_json(output / "metadata.json", metadata, output)
    print(f"DONE {len(results)} cases; {sum(bool(result.get('valid')) for result in results)} valid", flush=True)
    return 0 if all(result.get("valid") for result in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
