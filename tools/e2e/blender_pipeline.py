"""Exercise real Blender -> agentd -> assetd -> engine templates in isolation.

This is a fixed regression fixture, not evidence of native computer-use authoring.
Requires freshly built forge-agentd, engine-host and both MCP bridge binaries.
Logs, sources, exports and results remain in --output for reproducibility.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import queue
import socket
import struct
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import zlib

REPOSITORY = Path(__file__).resolve().parents[2]
FLAGS = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


class StdioBridge:
    """Real NDJSON MCP client with bounded reads and an explicit stdin shutdown."""
    def __init__(self, executable, origin, output):
        self.output = Path(output)
        self.stderr = (self.output / "bridge-stderr.log").open("wb")
        self.process = subprocess.Popen([str(executable), "--origin", origin, "--workspace-id", "default"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr, text=True, encoding="utf-8", creationflags=FLAGS)
        self.lines = queue.Queue()
        self.history = []
        self.sequence = 0
        self.closed = False
        def read():
            try:
                for line in self.process.stdout:
                    self.lines.put(line)
            finally:
                self.lines.put(None)
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()

    def rpc(self, method, params=None):
        self.sequence += 1
        message = dict(jsonrpc="2.0", id=self.sequence, method=method, params=params or {})
        self.process.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            line = self.lines.get(timeout=max(0.01, deadline - time.monotonic()))
            if line is None:
                raise RuntimeError("Blender STDIO bridge exited before replying")
            response = json.loads(line)
            if response.get("id") != self.sequence:
                continue
            self.history.append(dict(request=message, response=response))
            if "error" in response:
                raise RuntimeError(json.dumps(response["error"]))
            return response["result"]
        raise TimeoutError("Blender STDIO bridge did not reply")

    def initialize(self):
        result = self.rpc("initialize", dict(protocolVersion="2024-11-05", capabilities={}, clientInfo=dict(name="rurix-blender-regression", version="1")))
        self.process.stdin.write(json.dumps(dict(jsonrpc="2.0", method="notifications/initialized")) + "\n")
        self.process.stdin.flush()
        tools = self.rpc("tools/list")
        names = {tool["name"] for tool in tools["tools"]}
        if not {"blender_status", "blender_job_create", "blender_job_list", "blender_job_get"}.issubset(names):
            raise AssertionError("Blender STDIO bridge tool inventory is incomplete")
        return dict(server=result["serverInfo"], protocolVersion=result["protocolVersion"], toolCount=len(names), pid=self.process.pid)

    def call(self, name, arguments=None):
        value = self.rpc("tools/call", dict(name=name, arguments=arguments or {}))
        if value.get("isError"):
            raise RuntimeError(json.dumps(value, ensure_ascii=False))
        return value.get("structuredContent") or json.loads(value["content"][0]["text"])

    def close(self):
        if self.closed:
            return
        self.closed = True
        self.process.stdin.close()
        try:
            code = self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
            raise RuntimeError("Blender STDIO bridge did not exit when stdin closed")
        finally:
            self.reader.join(timeout=2)
            self.process.stdout.close()
            self.stderr.close()
            (self.output / "bridge.json").write_text(json.dumps(self.history, ensure_ascii=False, indent=2), encoding="utf-8")
        if code != 0:
            raise RuntimeError(f"Blender STDIO bridge exited with code {code}")


def request(origin, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(origin + path, data=data, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError(str(error.code) + " " + error.read().decode()) from error


def wait_job(origin, job_id, previous_revision=0, timeout=360, allow_watching_failure=False):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        job = request(origin, "/api/forge/blender/jobs/" + job_id + "?workspaceId=default")
        if job["state"] == "failed" and not (allow_watching_failure and job["stage"] == "watching"):
            raise RuntimeError(json.dumps(job, ensure_ascii=False))
        if job["state"] == "ready" and job["revision"] > previous_revision:
            return job
        time.sleep(0.5)
    raise TimeoutError("Blender publication did not become ready")


def chunk(name, data):
    return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data) & 0xffffffff)


def png(width, height, pixels):
    raw = b"".join(b"\0" + pixels[y * width * 4:(y + 1) * width * 4] for y in range(height))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def frame_bytes(frame):
    # A real engine frame is mandatory; an icon or Blender render is not accepted.
    if "pixelsB64" not in frame and isinstance(frame.get("frame"), dict):
        frame = frame["frame"]
    pixels = base64.b64decode(frame["pixelsB64"])
    width, height = frame["width"], frame["height"]
    if len(pixels) != width * height * 4 or not pixels:
        raise ValueError("Engine returned an invalid RGBA frame")
    return frame, pixels


def exercise_map(origin, project, output, blender, character):
    project, output = Path(project), Path(output)
    source = project / "Sources/Blender/map-fixture"
    with (output / "map-fixture.log").open("wb") as log:
        subprocess.run([blender, "--background", "--factory-startup", "--disable-autoexec", "--python-exit-code", "1", "--python", str(REPOSITORY / "tools/blender/smoke_fixture.py"), "--", "--output", str(source), "--kind", "map"], stdout=log, stderr=subprocess.STDOUT, timeout=90, check=True, creationflags=FLAGS)
    job = request(origin, "/api/forge/blender/jobs", dict(workspaceId="default", name="Blender map fixture", prompt="Fixed floor and wall collision fixture", kind="map"))
    job_id = job["id"]
    claim = request(origin, f"/api/forge/blender/jobs/{job_id}/claim", dict(workspaceId="default", executorId="fixed-map-regression", capabilities=dict(computerUse=True)))
    lease = dict(workspaceId="default", leaseToken=claim["leaseToken"])
    request(origin, f"/api/forge/blender/jobs/{job_id}/bind", dict(lease, sourcePath="Sources/Blender/map-fixture/fixture.blend"))
    request(origin, f"/api/forge/blender/jobs/{job_id}/publish", lease)
    published = wait_job(origin, job_id)
    frame = request(origin, f"/api/forge/blender/jobs/{job_id}/preview", dict(workspaceId="default", width=256, height=256))
    frame, pixels = frame_bytes(frame)
    (output / "map.png").write_bytes(png(frame["width"], frame["height"], pixels))
    def engine(name, arguments=None):
        value = request(origin, "/api/forge/mcp/call", dict(workspaceId="default", tool="mcp__engine-scene__" + name, arguments=arguments or {}))
        if value.get("isError"):
            raise RuntimeError(json.dumps(value))
        return value.get("structuredContent") or json.loads(value["content"][0]["text"])
    engine("scene_new", dict(name="Blender Bridge Validation"))
    map_instance = engine("prefab_instantiate", dict(prefabRef=published["published"]["prefabPath"]))
    character_instance = engine("prefab_instantiate", dict(prefabRef=character["published"]["prefabPath"], translation=[0, 3, 0]))
    root_id = character_instance["rootId"]
    engine("play_enter")
    try:
        engine("play_pause")
        for _ in range(120):
            engine("play_step")
        grounded = engine("transform_get", dict(id=root_id))
        if not -0.15 < grounded["translation"][1] < 0.3:
            raise AssertionError("Character did not land on imported Blender floor: " + json.dumps(grounded))
        engine("logic_inject_input", dict(action="right", value=1))
        for _ in range(90):
            engine("play_step")
        stopped = engine("transform_get", dict(id=root_id))
        if not 0.3 < stopped["translation"][0] < 2.0:
            raise AssertionError("Character did not stop against imported Blender wall: " + json.dumps(stopped))
    finally:
        engine("play_exit")
    engine("transform_set", dict(id=root_id, translation=[0, 0, 0]))
    scene_path = str(project / "Content/Scenes/BlenderBridge.rxscene")
    engine("scene_save", dict(path=scene_path))
    result = dict(passed=True, job=published, mapInstance=map_instance, characterInstance=character_instance, grounded=grounded, stopped=stopped, scenePath=scene_path)
    (output / "map-result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    return result


def run(args):
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    project = output / "project"
    project.mkdir(exist_ok=True)
    (project / "Content").mkdir(exist_ok=True)
    source = project / "Sources" / "Blender" / "fixture"
    source.mkdir(parents=True, exist_ok=True)
    data = output / "agent-data"
    with socket.socket() as port_socket:
        port_socket.bind(("127.0.0.1", 0))
        port = port_socket.getsockname()[1]
    origin = f"http://127.0.0.1:{port}"
    env = dict(os.environ, FORGE_AGENTD_ADDR=f"127.0.0.1:{port}", FORGE_AGENTD_DATA_DIR=str(data), FORGE_AGENTD_WORKSPACE_ROOT=str(project), FORGE_BLENDER_EXECUTABLE=args.blender)
    binaries = Path(args.bin_dir).resolve()
    engine_binaries = Path(args.engine_bin_dir).resolve()
    suffix = ".exe" if os.name == "nt" else ""
    for name in ("forge-agentd", "blender-bridge-mcp", "asset-pipeline-mcp"):
        if not (binaries / (name + suffix)).is_file():
            raise FileNotFoundError("Build " + name + " before running this test")
    for name in ("engine-host", "engine-scene-mcp"):
        if not (engine_binaries / (name + suffix)).is_file():
            raise FileNotFoundError("Build " + name + " before running this test")
    env.update(FORGE_ENGINE_SCENE_MCP_BIN=str(engine_binaries / ("engine-scene-mcp" + suffix)), FORGE_ENGINE_HOST_BIN=str(engine_binaries / ("engine-host" + suffix)), FORGE_ASSET_PIPELINE_MCP_BIN=str(binaries / ("asset-pipeline-mcp" + suffix)))
    log = (output / "agentd.log").open("wb")
    process = subprocess.Popen([str(binaries / ("forge-agentd" + suffix))], cwd=REPOSITORY, env=env, stdout=log, stderr=subprocess.STDOUT, creationflags=FLAGS)
    (output / "service.json").write_text(json.dumps(dict(pid=process.pid, origin=origin, project=str(project)), indent=2))
    result = {"fixtureOnly": True, "nativeComputerUseVerified": False, "bridgeVerified": False, "origin": origin, "project": str(project)}
    bridge = None
    try:
        for _ in range(100):
            try:
                request(origin, "/health")
                break
            except Exception:
                if process.poll() is not None:
                    raise RuntimeError("agentd exited; read agentd.log")
                time.sleep(0.1)
        def make_source(variant):
            with (output / f"fixture-{variant}.log").open("wb") as fixture_log:
                subprocess.run([args.blender, "--background", "--factory-startup", "--disable-autoexec", "--python-exit-code", "1", "--python", str(REPOSITORY / "tools/blender/smoke_fixture.py"), "--", "--output", str(source), "--variant", str(variant)], stdout=fixture_log, stderr=subprocess.STDOUT, check=True, timeout=90, creationflags=FLAGS)
        make_source(0)
        bridge = StdioBridge(binaries / ("blender-bridge-mcp" + suffix), origin, output)
        bridge_info = bridge.initialize()
        if not bridge.call("blender_status")["blender"]["found"]:
            raise AssertionError("MCP status cannot see the coordinator's Blender installation")
        job = bridge.call("blender_job_create", dict(name="Blender pipeline fixture", prompt="Fixed regression fixture; not a computer-use authoring task", kind="character"))
        job_id = job["id"]
        via_rest = request(origin, f"/api/forge/blender/jobs/{job_id}?workspaceId=default")
        if via_rest["sourceId"] != job["sourceId"] or not Path(job["sourceAbsolutePath"]).resolve().is_relative_to(project):
            raise AssertionError("MCP-created job does not belong to the REST coordinator's project")
        # Claim exercises the capability/lease contract with a test executor. This
        # does not claim the real native computer-use plugin was exercised.
        claim = request(origin, f"/api/forge/blender/jobs/{job_id}/claim", dict(workspaceId="default", executorId="fixed-regression-fixture", capabilities=dict(computerUse=True)))
        lease = dict(workspaceId="default", leaseToken=claim["leaseToken"])
        request(origin, f"/api/forge/blender/jobs/{job_id}/bind", dict(lease, sourcePath="Sources/Blender/fixture/fixture.blend"))
        request(origin, f"/api/forge/blender/jobs/{job_id}/publish", lease)
        first = wait_job(origin, job_id)
        if not {"Idle", "Walk"}.issubset(set(first["published"]["clips"])):
            raise AssertionError("Character is missing Idle/Walk animations")
        def preview(name, **options):
            frame = request(origin, f"/api/forge/blender/jobs/{job_id}/preview", dict(workspaceId="default", width=256, height=256, **options))
            frame, pixels = frame_bytes(frame)
            (output / (name + ".json")).write_text(json.dumps({key:value for key,value in frame.items() if key != "pixelsB64"}, indent=2))
            (output / (name + ".png")).write_bytes(png(frame["width"], frame["height"], pixels))
            return pixels
        initial_frame = preview("first", clip="Idle", time=0)
        animation_frame = preview("walk", clip="Walk", time=0.4)
        if initial_frame == animation_frame:
            raise AssertionError("Animation preview did not deform geometry")
        make_source(1)
        second = wait_job(origin, job_id, first["revision"])
        if second["published"]["modelGuid"] != first["published"]["modelGuid"] or second["published"]["prefabGuid"] != first["published"]["prefabGuid"]:
            raise AssertionError("Reimport changed model/template GUID")
        before_texture = preview("before-texture", clip="Idle", time=0)
        # Rewrite only the dependency image at the same dimensions.
        (source / "checker.png").write_bytes(png(16, 16, bytes([20, 50, 245, 255]) * 256))
        third = wait_job(origin, job_id, second["revision"])
        after_texture = preview("after-texture", clip="Idle", time=0)
        if before_texture == after_texture:
            raise AssertionError("Same-size texture change did not reach engine rendering")
        saved_texture = (source / "checker.png").read_bytes()
        (source / "checker.png").unlink()
        missing = None
        for _ in range(30):
            missing = request(origin, f"/api/forge/blender/jobs/{job_id}?workspaceId=default")
            if missing["state"] == "failed" and missing["stage"] == "watching":
                break
            time.sleep(0.25)
        if missing["state"] != "failed" or missing["stage"] != "watching":
            raise AssertionError("Missing source dependency was reported as synchronized")
        time.sleep(3)
        unchanged_error = request(origin, f"/api/forge/blender/jobs/{job_id}?workspaceId=default")
        if missing["updatedAt"] != unchanged_error["updatedAt"]:
            raise AssertionError("Unchanged watcher failure is repeatedly persisted")
        (source / "checker.png").write_bytes(saved_texture)
        restored = wait_job(origin, job_id, third["revision"] - 1, allow_watching_failure=True)
        if restored["revision"] != third["revision"]:
            raise AssertionError("Restoring identical dependency created a new revision")
        cancelled = request(origin, f"/api/forge/blender/jobs/{job_id}/cancel", dict(workspaceId="default"))
        if cancelled["state"] != "cancelled" or not (project / "Content" / third["published"]["modelPath"]).is_file():
            raise AssertionError("Cancellation deleted or hid the committed model")
        request(origin, f"/api/forge/blender/jobs/{job_id}/retry", dict(workspaceId="default"))
        final_ready = wait_job(origin, job_id, third["revision"] - 1)
        map_result = exercise_map(origin, project, output, args.blender, final_ready)
        def engine_pid():
            response = request(origin, "/api/forge/mcp/call", dict(workspaceId="default", tool="mcp__engine-scene__host_ping", arguments={}))
            if response.get("isError"):
                raise RuntimeError(json.dumps(response))
            value = response.get("structuredContent") or json.loads(response["content"][0]["text"])
            return value["pid"]
        engine_before = engine_pid()
        through_bridge = bridge.call("blender_job_get", dict(jobId=job_id))
        listed = bridge.call("blender_job_list")["jobs"]
        bridge_status = bridge.call("blender_status")
        if through_bridge["published"] != final_ready["published"] or through_bridge["workspaceId"] != "default":
            raise AssertionError("MCP job read disagrees with the published coordinator job")
        if not {job_id, map_result["job"]["id"]}.issubset({entry["id"] for entry in listed}):
            raise AssertionError("MCP job list is not scoped to the same project")
        engine_after = engine_pid()
        if engine_before != engine_after:
            raise AssertionError("MCP bridge reads replaced the active scoped engine")
        bridge_info.update(jobId=job_id, workspaceId="default", listedJobIds=[entry["id"] for entry in listed], enginePidBefore=engine_before, enginePidAfter=engine_after, status=bridge_status)
        bridge.close()
        bridge = None
        result.update(passed=True, bridgeVerified=True, bridge=bridge_info, jobId=job_id, first=first, second=second, third=third, missingDependency=missing, restored=restored, cancelled=cancelled, finalReady=final_ready, map=map_result)
        (output / "result.json").write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
        print("BLENDER_PIPELINE_PASS " + str(output / "result.json"))
    except Exception as error:
        result.update(passed=False, error=str(error))
        (output / "result.json").write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
        raise
    finally:
        try:
            if bridge is not None:
                bridge.close()
        finally:
            if not args.keep_running:
                if os.name == "nt" and process.poll() is None:
                    subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=FLAGS)
                elif process.poll() is None:
                    process.terminate()
                process.wait(timeout=15)
            log.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--blender", default=r"E:\SteamLibrary\steamapps\common\Blender\blender.exe")
    parser.add_argument("--output", default=str(Path(tempfile.gettempdir()) / ("rurix-blender-e2e-" + time.strftime("%Y%m%d-%H%M%S"))))
    parser.add_argument("--keep-running", action="store_true")
    parser.add_argument("--bin-dir", default=str(REPOSITORY / "target/blender-validation/debug"))
    parser.add_argument("--engine-bin-dir", default=str(REPOSITORY / "target/debug"))
    run(parser.parse_args())
