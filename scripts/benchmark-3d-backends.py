#!/usr/bin/env python3
"""Isolated exact-RGBA RPC benchmark; wall latency is NOT a GPU frame time.

Uses existing binaries only. Each backend/object-count/resolution gets a fresh
process and project. Evidence is append-only: --output must not already exist.
Example: py -3 scripts/benchmark-3d-backends.py --backends rurix --sizes 128 
         --resolutions 1920x1080 --samples 20 --rounds 3
"""
from __future__ import annotations

import argparse
import base64
from collections import deque
import ctypes
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import re
import shutil
import socket
import statistics
import struct
import subprocess
import sys
import threading
import time
import traceback
import zlib

BACKENDS = {
    "rurix": None,
    "godot-forward-plus-d3d12": ("forward_plus", "d3d12"),
    "godot-forward-plus-vulkan": ("forward_plus", "vulkan"),
    "godot-mobile-d3d12": ("mobile", "d3d12"),
    "godot-mobile-vulkan": ("mobile", "vulkan"),
    "godot-compat-opengl3": ("gl_compatibility", "opengl3"),
}
STAT_KEYS = ("draws", "triangles", "truncated", "meshFallbacks", "meshClasses",
             "nonZeroPixels", "framePath", "format", "width", "height", "deviceName")


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def sha_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    stat = path.stat()
    return {"path": str(path), "bytes": stat.st_size,
            "modifiedUtc": dt.datetime.fromtimestamp(stat.st_mtime, dt.timezone.utc).isoformat(),
            "sha256": digest.hexdigest()}


def command_snapshot(command, timeout=8):
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout,
                                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        return {"command": command, "exitCode": result.returncode,
                "stdout": result.stdout.strip(), "stderr": result.stderr.strip()}
    except Exception as exc:
        return {"command": command, "unavailable": str(exc)}


def gpu_snapshot():
    return command_snapshot(["nvidia-smi", "--query-gpu=name,uuid,driver_version,pstate,"
                             "temperature.gpu,clocks.gr,clocks.mem,utilization.gpu,"
                             "utilization.memory,memory.used,power.draw,power.limit",
                             "--format=csv,noheader,nounits"])


def storage_snapshot(root):
    drives = [root.anchor] + (["D:\\", "E:\\"] if os.name == "nt" else [])
    result = {}
    for drive in dict.fromkeys(drives):
        try:
            total, used, free = shutil.disk_usage(drive)
            result[drive] = {"totalBytes": total, "freeBytes": free,
                             "freePercent": round(100 * free / total, 3)}
        except OSError as exc:
            result[drive] = {"unavailable": str(exc)}
    return result


class Rpc:
    def __init__(self, port, timeout):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        self.sock.settimeout(timeout)
        self.seq = 0

    def close(self):
        self.sock.close()

    def read_exact(self, size):
        chunks = bytearray(size)
        target = memoryview(chunks)
        while target:
            n = self.sock.recv_into(target)
            if not n:
                raise EOFError("RPC socket closed")
            target = target[n:]
        return chunks

    def call(self, method, params=None):
        self.seq += 1
        body = json.dumps({"jsonrpc": "2.0", "id": self.seq, "method": method,
                           "params": params or {}}, separators=(",", ":")).encode()
        self.sock.sendall(struct.pack("<I", len(body)) + body)
        size = struct.unpack("<I", self.read_exact(4))[0]
        if size > 64 * 1024 * 1024:
            raise ValueError(f"unexpected RPC response size {size}")
        reply = json.loads(self.read_exact(size))
        if reply.get("id") != self.seq:
            raise ValueError(f"RPC response id mismatch: {reply.get('id')} != {self.seq}")
        if "error" in reply:
            raise RuntimeError(f"{method}: {reply['error']}")
        return reply["result"]


class OwnJob:
    """Windows Job owns only our process tree; closing it cannot kill other hosts."""
    def __init__(self, proc):
        self.handle = None
        if os.name != "nt":
            return
        from ctypes import wintypes

        class Basic(ctypes.Structure):
            _fields_ = [("processTime", ctypes.c_int64), ("jobTime", ctypes.c_int64),
                        ("flags", wintypes.DWORD), ("minWs", ctypes.c_size_t),
                        ("maxWs", ctypes.c_size_t), ("activeLimit", wintypes.DWORD),
                        ("affinity", ctypes.c_size_t), ("priority", wintypes.DWORD),
                        ("scheduling", wintypes.DWORD)]

        class Io(ctypes.Structure):
            _fields_ = [(name, ctypes.c_uint64) for name in
                        ("readOps", "writeOps", "otherOps", "readBytes", "writeBytes", "otherBytes")]

        class Extended(ctypes.Structure):
            _fields_ = [("basic", Basic), ("io", Io), ("processMemory", ctypes.c_size_t),
                        ("jobMemory", ctypes.c_size_t), ("peakProcess", ctypes.c_size_t),
                        ("peakJob", ctypes.c_size_t)]

        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.CreateJobObjectW.restype = wintypes.HANDLE
        self.kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        self.kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int,
                                                       ctypes.c_void_p, wintypes.DWORD]
        self.kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        handle = self.kernel.CreateJobObjectW(None, None)
        info = Extended()
        info.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not handle or not self.kernel.SetInformationJobObject(handle, 9, ctypes.byref(info),
                                                                ctypes.sizeof(info)):
            raise ctypes.WinError(ctypes.get_last_error())
        if not self.kernel.AssignProcessToJobObject(handle, wintypes.HANDLE(proc._handle)):
            self.kernel.CloseHandle(handle)
            raise ctypes.WinError(ctypes.get_last_error())
        self.handle = handle

    def close(self):
        if self.handle:
            self.kernel.CloseHandle(self.handle)
            self.handle = None


class Host:
    def __init__(self, repo, project, backend, log_path, timeout, gpu_index, gpu_profile,
                 godot_max_fps=None):
        env = os.environ.copy()
        for name in ("FORGE_HOST_PORT", "FORGE_RENDER_BACKEND", "FORGE_RENDER_METHOD",
                     "FORGE_RENDER_DRIVER", "FORGE_GPU_PARTICLES", "FORGE_GAME_SCENE"):
            env.pop(name, None)
        env["FORGE_PROJECT_ROOT"] = str(project)
        config = BACKENDS[backend]
        if config is None:
            command = [str(repo / "target/debug/engine-host.exe"), "--port", "0"]
            cwd = repo
            env["RURIX_PREFER_DISCRETE_GPU"] = "1"
        else:
            method, driver = config
            cwd = repo / "target/godot-runtime"
            command = [str(cwd / "forge-godot.exe"), "--rendering-method", method,
                       "--rendering-driver", driver, "--audio-driver", "Dummy"]
            if driver != "opengl3":
                command += ["--gpu-index", str(gpu_index)]
            if gpu_profile:
                command += ["--gpu-profile"]
            if godot_max_fps is not None:
                command += ["--max-fps", str(godot_max_fps)]
            env.update(FORGE_HOST_PORT="0", FORGE_RENDER_BACKEND="godot",
                       FORGE_RENDER_METHOD=method, FORGE_RENDER_DRIVER=driver)
        self.command = command
        self.job = None
        self.rpc = None
        self.proc = None
        self.threads = []
        self.log = log_path.open("w", encoding="utf-8")
        self.log_lock = threading.Lock()
        self.log_bytes = 0
        self.tail = deque(maxlen=24)
        self.gpu_profiles = []
        ready = queue.Queue()
        start = time.perf_counter()
        try:
            self.proc = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                         creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            self.job = OwnJob(self.proc)
            for stream, tag in ((self.proc.stdout, "stdout"), (self.proc.stderr, "stderr")):
                thread = threading.Thread(target=self.pump, args=(stream, tag, ready), daemon=True)
                self.threads.append(thread)
                thread.start()
            self.port = ready.get(timeout=timeout)
            self.ready_ms = (time.perf_counter() - start) * 1000
            self.rpc = Rpc(self.port, timeout)
        except Exception:
            self.close()
            raise RuntimeError("host startup failed: " + " | ".join(self.tail)) from None

    def pump(self, stream, tag, ready):
        try:
            for raw in iter(stream.readline, b""):
                received = time.perf_counter()
                line = raw.decode("utf-8", errors="replace").rstrip()
                with self.log_lock:
                    self.tail.append(f"{tag}: {line}")
                    match = re.search(r"GPU PROFILE \(total ([-+0-9.eE]+)ms\)", line)
                    if match:
                        self.gpu_profiles.append({"receivedPerfCounterSeconds": received,
                                                  "totalMs": float(match.group(1)),
                                                  "rawLine": line})
                    if self.log_bytes < 2 * 1024 * 1024:
                        self.log.write(f"{tag}: {line}\n")
                        self.log.flush()
                        self.log_bytes += len(raw)
                prefix = "FORGE_HOST_LISTENING port="
                if line.startswith(prefix):
                    ready.put(int(line[len(prefix):].split()[0]))
        finally:
            stream.close()

    def close(self):
        if self.rpc:
            self.rpc.close()
            self.rpc = None
        if self.proc:
            if self.proc.poll() is None:
                self.proc.terminate()
            if self.job:
                self.job.close()
            try:
                self.proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=8)
        for thread in self.threads:
            thread.join(timeout=2)
        self.log.close()


def entity(index, name, translation, scale, component):
    return {"id": index, "name": name,
            "transform": {"translation": translation, "rotation": [0, 0, 0, 1], "scale": scale},
            "components": [component]}


def scene_fixture(count):
    # XZ plane, generous spacing and 75-degree pitch: every cube projects inside
    # the frustum without overlapping another cube. No instancing or occlusion shortcut.
    cols = math.ceil(math.sqrt(count * 16 / 9))
    rows = math.ceil(count / cols)
    gap = 2.0
    entities = []
    mesh = {"type": "MeshRenderer", "enabled": True, "props": {"mesh": "cube", "material": ""}}
    for index in range(count):
        x = ((index % cols) - (cols - 1) / 2) * gap
        z = ((index // cols) - (rows - 1) / 2) * gap
        entities.append(entity(index + 1, f"cube-{index:05d}", [x, .6, z], [1, 1, 1], mesh))
    entities.append(entity(count + 1, "ground", [0, -.1, 0],
                           [cols * gap + 1, .2, rows * gap + 1], mesh))
    entities.append(entity(count + 2, "sun", [0, 10, 0], [1, 1, 1],
                           {"type": "Light", "enabled": True,
                            "props": {"kind": "directional", "color": [1, 1, 1],
                                      "intensity": 1.0, "castShadow": False}}))
    half_h = max(rows * gap / 2 + 3, (cols * gap / 2 + 3) * 9 / 16)
    camera = {"target": [0, 0, 0], "yaw": 0, "pitch": 75,
              "dist": max(30, rows * gap + 10), "fovY": 50, "ortho": True, "orthoSize": half_h}
    return {"name": f"independent-cubes-{count}", "mode": "3d",
            "next_id": count + 3, "entities": entities}, camera


def png(path, width, height, rgba):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    stride = width * 4
    compressor = zlib.compressobj(6)
    blocks = []
    for row in range(height):
        blocks.append(compressor.compress(b"\0" + rgba[row * stride:(row + 1) * stride]))
    blocks.append(compressor.flush())
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
                     + chunk(b"IDAT", b"".join(blocks)) + chunk(b"IEND", b""))


def percentile(values, p):
    values = sorted(values)
    offset = (len(values) - 1) * p / 100
    lo, hi = math.floor(offset), math.ceil(offset)
    return values[lo] + (values[hi] - values[lo]) * (offset - lo)


def summary(values):
    return {"samples": len(values), "meanMs": statistics.mean(values),
            "p50Ms": percentile(values, 50), "p95Ms": percentile(values, 95),
            "p99Ms": percentile(values, 99), "minMs": min(values), "maxMs": max(values),
            "rpcThroughputFps": 1000 / statistics.mean(values)}


def frame_stats(frame):
    return {key: frame[key] for key in STAT_KEYS if key in frame}


def run_case(repo, output, backend, count, width, height, args):
    name = f"{backend}__{count}__{width}x{height}"
    directory = output / "cases" / name
    project = directory / "project"
    (project / "Content/Scenes").mkdir(parents=True)
    (project / "forge.toml").write_text('[project]\nname="backend-benchmark"\nengine-version="0.1.0"\nmode="3d"\n\n[dirs]\ncontent="Content"\n', encoding="utf-8")
    scene, camera = scene_fixture(count)
    scene_path = project / "Content/Scenes/benchmark.rxscene"
    write_json(scene_path, scene)
    empty = project / "Content/Scenes/empty.rxscene"
    write_json(empty, {"name": "empty", "mode": "3d", "next_id": 1, "entities": []})
    result = {"case": name, "backend": backend, "objects": count,
              "expectedMeshRenderers": count + 1, "width": width, "height": height,
              "camera": camera, "scene": sha_file(scene_path), "gpuBefore": gpu_snapshot(),
              "measurement": "TCP JSON-RPC exact RGBA8 request through response JSON parsing; includes render, readback, encoding, transport, client parsing; excludes client base64 decode; NOT pure GPU FPS",
              "valid": False}
    write_json(directory / "result.json", result)
    host = None
    try:
        host = Host(repo, project, backend, directory / "host.log", args.timeout, args.gpu_index,
                    args.gpu_profile, args.godot_max_fps)
        result.update(pid=host.proc.pid, command=host.command, port=host.port, readyMs=host.ready_ms)
        rpc = host.rpc
        result["backendInfoBefore"] = rpc.call("render.backendInfo")
        result["capabilities"] = rpc.call("render.capabilities")
        start = time.perf_counter()
        result["sceneLoad"] = rpc.call("scene.load", {"path": str(scene_path)})
        result["coldSceneLoadMs"] = (time.perf_counter() - start) * 1000
        result["appliedCamera"] = rpc.call("viewport.setCamera", camera)
        request = {"width": width, "height": height, "format": "rgba8", "exact": True}
        start = time.perf_counter()
        frame = rpc.call("viewport.frame", request)
        result["coldFirstFrameMs"] = (time.perf_counter() - start) * 1000
        result["coldStats"] = frame_stats(frame)
        rgba = base64.b64decode(frame.pop("pixelsB64"), validate=True)
        if len(rgba) != width * height * 4:
            raise ValueError(f"wrong RGBA size {len(rgba)}")
        png(directory / "validation.png", width, height, rgba)
        result["pixelSha256"] = hashlib.sha256(rgba).hexdigest()
        result["sampledRgbColors"] = len({rgba[(y * width + x) * 4:(y * width + x) * 4 + 3]
                                          for y in range(0, height, max(1, height // 64))
                                          for x in range(0, width, max(1, width // 64))})
        del rgba
        start = time.perf_counter()
        for _ in range(args.warmup):
            frame = rpc.call("viewport.frame", request)
        result["warmupMs"] = (time.perf_counter() - start) * 1000
        rounds, all_times, all_stats = [], [], []
        for round_index in range(args.rounds):
            times, stats = [], []
            for _ in range(args.samples):
                start = time.perf_counter()
                frame = rpc.call("viewport.frame", request)
                times.append((time.perf_counter() - start) * 1000)
                stats.append(frame_stats(frame))
            rounds.append({"round": round_index + 1, **summary(times), "latenciesMs": times,
                           "firstStats": stats[0], "lastStats": stats[-1]})
            all_times.extend(times)
            all_stats.extend(stats)
            print(f"{name} round {round_index + 1}/{args.rounds}: p50={percentile(times, 50):.2f} ms p95={percentile(times, 95):.2f} ms", flush=True)
        result.update(rounds=rounds, overall=summary(all_times), lastStats=frame_stats(frame),
                      backendInfoAfter=rpc.call("render.backendInfo"))
        submit_stats = []
        if args.submit_samples:
            submit_request = {**request, "format": "none"}
            submit_times = []
            phase_start = time.perf_counter()
            print(f"{name} continuous submit phase: {args.submit_samples} exact format=none requests", flush=True)
            for _ in range(args.submit_samples):
                start = time.perf_counter()
                submitted = rpc.call("viewport.frame", submit_request)
                submit_times.append((time.perf_counter() - start) * 1000)
                submit_stats.append(frame_stats(submitted))
                if "pixelsB64" in submitted:
                    raise ValueError("format=none unexpectedly returned pixelsB64")
            phase_end = time.perf_counter()
            submit_summary = summary(submit_times)
            submit_summary.pop("rpcThroughputFps")
            result["submitPhase"] = {"startPerfCounterSeconds": phase_start,
                                     "endPerfCounterSeconds": phase_end,
                                     "durationMs": (phase_end - phase_start) * 1000,
                                     "request": submit_request, "latency": submit_summary,
                                     "firstStats": submit_stats[0], "lastStats": submit_stats[-1],
                                     "measurement": "continuous exact format=none RPC submission; synchronization differs between backends; request latency is not GPU frame time"}
            with host.log_lock:
                profiles = [dict(value) for value in host.gpu_profiles
                            if phase_start <= value["receivedPerfCounterSeconds"] <= phase_end]
            result["gpuProfileSubmitPhase"] = {
                "enabled": bool(args.gpu_profile and BACKENDS[backend] is not None),
                "startPerfCounterSeconds": phase_start, "endPerfCounterSeconds": phase_end,
                "samples": profiles,
                "measurement": "Godot GPU timestamp total, printed approximately once per second; raw latest-frame timestamp snapshot, not a p95 distribution or complete game FPS; only lines received within the continuous submission phase are included; no GPU FPS is calculated"}
        faults = []
        for key, expected in (("width", width), ("height", height), ("format", "rgba8")):
            if any(stat.get(key) != expected for stat in all_stats):
                faults.append(f"unexpected {key}")
        if any(stat.get("draws", 0) < count + 1 for stat in all_stats):
            faults.append("fewer draws than requested MeshRenderers")
        if any(stat.get("triangles", 0) < (count + 1) * 12 for stat in all_stats):
            faults.append("fewer triangles than complete cube scene")
        if any(stat.get("truncated") for stat in all_stats):
            faults.append("truncated geometry")
        if any(stat.get("meshFallbacks", 0) for stat in all_stats):
            faults.append("unexpected mesh fallback")
        if any(stat.get("nonZeroPixels", 0) <= 0 for stat in all_stats) or result["sampledRgbColors"] < 2:
            faults.append("blank or uniform validation image")
        if args.expected_device and any(args.expected_device.lower() not in stat.get("deviceName", "").lower()
                                        for stat in all_stats):
            faults.append("actual GPU does not match --expected-device")
        if submit_stats:
            if any(stat.get("draws", 0) < count + 1 or stat.get("triangles", 0) < (count + 1) * 12
                   for stat in submit_stats):
                faults.append("continuous submit phase has incomplete geometry")
            if any(stat.get("truncated") or stat.get("meshFallbacks", 0) for stat in submit_stats):
                faults.append("continuous submit phase has truncation or fallback")
            if any(stat.get("width") != width or stat.get("height") != height or stat.get("format") != "none"
                   for stat in submit_stats):
                faults.append("continuous submit phase has unexpected size or format")
            if args.expected_device and any(args.expected_device.lower() not in stat.get("deviceName", "").lower()
                                            for stat in submit_stats):
                faults.append("continuous submit GPU does not match --expected-device")
        result["invalidReasons"] = faults
        result["valid"] = not faults
        # Prove scene switching works without folding it into steady-state frames.
        rpc.call("scene.load", {"path": str(empty)})
        start = time.perf_counter()
        result["reloadScene"] = rpc.call("scene.load", {"path": str(scene_path)})
        result["reloadSceneMs"] = (time.perf_counter() - start) * 1000
    except Exception as exc:
        result["valid"] = False
        result["error"] = str(exc)
        result["traceback"] = traceback.format_exc()
    finally:
        if host:
            host.close()
            result["hostTail"] = list(host.tail)
            result["hostExitCode"] = host.proc.returncode
        result["gpuAfter"] = gpu_snapshot()
        write_json(directory / "result.json", result)
    print(f"{name}: {'VALID' if result['valid'] else 'INVALID'} {result.get('invalidReasons', result.get('error', ''))}", flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backends", default=",".join(BACKENDS))
    parser.add_argument("--sizes", default="128,1024,4096")
    parser.add_argument("--resolutions", default="1280x720,1920x1080")
    parser.add_argument("--samples", type=int, default=50)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--warmup", type=int, default=10)
    parser.add_argument("--timeout", type=float, default=90)
    parser.add_argument("--gpu-index", type=int, default=0, help="Godot Forward+/Mobile device index; ignored by OpenGL")
    parser.add_argument("--expected-device", default="NVIDIA", help="case invalid if returned deviceName does not include this string; empty disables")
    parser.add_argument("--gpu-profile", action="store_true", help="enable Godot GPU timestamp profiler; only continuous submission phase samples are reported")
    parser.add_argument("--godot-max-fps", type=int, default=None, help="optional Godot CLI max-fps override; 0 means unlimited; default retains runtime cap")
    parser.add_argument("--submit-samples", type=int, default=0, help="optional separate continuous format=none phase after RGBA rounds, e.g. 180")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--prepare-only", action="store_true", help="validate configuration and write metadata; start no GPU process")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    backends = args.backends.split(",")
    sizes = [int(value) for value in args.sizes.split(",")]
    resolutions = [tuple(map(int, value.lower().split("x"))) for value in args.resolutions.split(",")]
    if set(backends) - BACKENDS.keys():
        parser.error("unknown backends: " + ",".join(set(backends) - BACKENDS.keys()))
    if min(args.samples, args.rounds) < 1 or args.warmup < 0 or args.submit_samples < 0 or min(sizes) < 1:
        parser.error("samples, rounds and sizes must be positive; warmup must be nonnegative")
    if args.godot_max_fps is not None and args.godot_max_fps < 0:
        parser.error("godot-max-fps must be nonnegative; 0 means unlimited")
    if any(len(value) != 2 or min(value) < 16 or max(value) > 1920 for value in resolutions):
        parser.error("resolution must be WIDTHxHEIGHT, each dimension 16..1920")
    output = (args.output or repo / "evidence" / ("backend-selection-" + dt.datetime.now().strftime("%Y%m%d-%H%M%S"))).resolve()
    output.mkdir(parents=True, exist_ok=False)
    binaries = [repo / "target/debug/engine-host.exe", repo / "target/godot-runtime/forge-godot.exe",
                repo / "target/godot-runtime/bin/godot_host.dll"]
    metadata = {"createdUtc": dt.datetime.now(dt.timezone.utc).isoformat(),
                "script": sha_file(Path(__file__).resolve()), "repo": str(repo),
                "platform": platform.platform(), "machine": platform.machine(),
                "python": sys.version, "logicalCpuCount": os.cpu_count(),
                "arguments": {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()},
                "binaries": [sha_file(path) if path.exists() else {"missing": str(path)} for path in binaries],
                "runtimeProject": sha_file(repo / "target/godot-runtime/project.godot"),
                "buildCaveat": "existing debug artifacts; source freshness is not assumed; no build performed",
                "timingCaveat": "primary comparison is exact RGBA8 RPC wall latency, not pure GPU FPS; "
                                + ("Godot runtime max_fps=60 retained" if args.godot_max_fps is None
                                   else f"Godot CLI max_fps={args.godot_max_fps} overrides runtime cap (0 means unlimited)")
                                + "; optional continuous format=none phase is separate because synchronization semantics differ; GPU PROFILE totals are approximately once-per-second timestamp snapshots, not p95 or full game FPS",
                "effectiveGodotMaxFps": 60 if args.godot_max_fps is None else args.godot_max_fps,
                "rurixEnvironmentOverrides": {"RURIX_PREFER_DISCRETE_GPU": "1"},
                "gpuProfileImplementation": {"source": "D:/godot-src/4.7.2-stable/servers/rendering/rendering_server_default.cpp", "lines": "167-205", "total": "latest frame_profile GPU timestamp; print threshold >1000000 usec; pass timings in following lines may be averages, so only raw total is extracted"},
                "storageBefore": storage_snapshot(output), "gpuBefore": gpu_snapshot()}
    if os.name == "nt":
        metadata["hardware"] = command_snapshot(["powershell", "-NoProfile", "-Command",
            "@{cpu=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors);gpu=@(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion);system=Get-CimInstance Win32_ComputerSystem | Select-Object TotalPhysicalMemory,Manufacturer,Model} | ConvertTo-Json -Depth 4"], timeout=15)
    write_json(output / "metadata.json", metadata)
    print(f"evidence: {output}", flush=True)
    results = []
    if not args.prepare_only:
        for backend in backends:
            for count in sizes:
                for width, height in resolutions:
                    print(f"START {backend} {count} meshes {width}x{height}", flush=True)
                    result = run_case(repo, output, backend, count, width, height, args)
                    results.append(result)
                    write_json(output / "results.json", results)
    metadata.update(finishedUtc=dt.datetime.now(dt.timezone.utc).isoformat(),
                    storageAfter=storage_snapshot(output), gpuAfter=gpu_snapshot())
    write_json(output / "metadata.json", metadata)
    print(f"DONE {len(results)} cases; {sum(bool(value.get('valid')) for value in results)} valid", flush=True)
    return 0 if all("error" not in value for value in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
