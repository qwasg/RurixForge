#!/usr/bin/env python3
"""Isolated REST acceptance for editor collaboration; never invokes a model.

Build binaries separately, then run with --bin-dir C:/.../debug. Evidence and
temporary projects remain under C: temp. Only the owned Windows Job Object is
terminated, including MCP/engine descendants; user application processes are untouched.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid
import zlib


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


spec = importlib.util.spec_from_file_location("isolated_collaboration_smoke", Path(__file__).with_name("collaboration-codex-smoke.py"))
isolation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(isolation)  # Its guarded CLI/main is not invoked.


class ApiError(RuntimeError):
    pass


class Harness:
    def __init__(self, root, binaries, port=0, generation=False):
        self.root, self.binaries = root, binaries
        self.process = self.tree = None
        self.logs, self.checks = [], []
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.starts = 0
        self.port = port
        self.generation = generation

    def start(self):
        self.starts += 1
        stdout = self.root / f"stdout-{self.starts}.log"
        stderr = self.root / f"stderr-{self.starts}.log"
        self.logs = [stdout.open("w", encoding="utf-8"), stderr.open("w", encoding="utf-8")]
        environment = os.environ.copy()
        environment.update(FORGE_AGENTD_DATA_DIR=str(self.root / "data"),
                           FORGE_AGENTD_WORKSPACE_ROOT=str(self.root / "workspace-a"),
                           FORGE_GEN_DATA_DIR=str(self.root / "gen"),
                           FORGE_AGENTD_ADDR=f"127.0.0.1:{self.port}", FORGE_RENDER_BACKEND="rurix",
                           FORGE_ENGINE_HOST_BIN=str(self.binaries / "engine-host.exe"))
        for name in ("engine-scene", "asset-pipeline", "context", "code-forge", "gen-image", "gen-model", "store"):
            environment[f"FORGE_{name.replace('-', '_').upper()}_MCP_BIN"] = str(self.binaries / f"{name}-mcp.exe")
        # Do not inherit a user's runtime or renderer choice into this isolated test.
        for key in ("FORGE_RENDER_METHOD", "FORGE_RENDER_DRIVER", "FORGE_PROJECT_ROOT"):
            environment.pop(key, None)
        self.process = subprocess.Popen([str(self.binaries / "forge-agentd.exe")], cwd=self.root / "workspace-a",
                                        env=environment, stdin=subprocess.DEVNULL,
                                        stdout=self.logs[0], stderr=self.logs[1],
                                        creationflags=subprocess.CREATE_NO_WINDOW | 0x4)
        self.tree = isolation.ProcessTree(self.process)
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            require(self.process.poll() is None, "Isolated daemon exited; inspect retained logs")
            text = stdout.read_text(encoding="utf-8", errors="replace")
            if "listening at http://" in text:
                self.base = text.split("listening at ", 1)[1].splitlines()[0].strip()
                require(self.base.startswith("http://127.0.0.1:"), "Daemon must bind loopback")
                return
            time.sleep(.1)
        raise RuntimeError("Isolated daemon startup timed out")

    def stop(self):
        if self.tree:
            self.tree.close()
            self.tree = None
        if self.process:
            self.process.wait(timeout=15)
            self.process = None
        for log in self.logs:
            log.close()
        self.logs = []

    def api(self, path, body=None, method=None, binary=False):
        req = urllib.request.Request(self.base + path,
                                     data=None if body is None else json.dumps(body).encode(),
                                     headers={"Content-Type": "application/json"}, method=method)
        try:
            with self.http.open(req, timeout=50) as response:
                return response.read() if binary else json.load(response)
        except urllib.error.HTTPError as error:
            text = error.read().decode("utf-8", errors="replace")
            raise ApiError(f"HTTP {error.code}: {text}") from None

    def mcp(self, workspace, name, args=None):
        value = self.api("/api/forge/mcp/call", {"workspaceId": workspace,
                         "tool": "mcp__engine-scene__" + name, "arguments": args or {}})
        if value.get("isError"):
            raise ApiError(json.dumps(value, ensure_ascii=False))
        for content in value.get("content", []):
            if content.get("type") == "text":
                try:
                    return json.loads(content["text"])
                except json.JSONDecodeError:
                    continue
        raise ApiError(f"Unexpected MCP response: {value}")

    def editor(self, workspace, action, **args):
        return self.api("/api/forge/editor/" + action, dict(args, workspaceId=workspace))

    def document(self, workspace, document=None, revision=None):
        path = f"/api/forge/editor/documents/blueprint/main?workspaceId={workspace}"
        return self.api(path) if document is None else self.api(path, {"document": document, "expectedRevision": revision}, method="PUT")

    def baseline(self, workspace):
        value = self.mcp(workspace, "editor_resolve")
        return {key: value[key] for key in ("sceneGuid", "hostEpoch", "contentRevision", "targetMode")}

    def entities(self, workspace):
        value = self.mcp(workspace, "entity_list")
        return value if isinstance(value, list) else value["entities"]

    def passed(self, name):
        self.checks.append(name)
        print("PASS", name, flush=True)
        save(self.root / "acceptance.json", {"status": "running", "checks": self.checks})

    def fails(self, action, expected):
        try:
            action()
        except ApiError as error:
            require(expected in str(error), f"Expected {expected}, got {error}")
        else:
            raise AssertionError("Mutation unexpectedly succeeded: " + expected)

    def wait_applied(self, workspace, request):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            self.api(f"/api/forge/editor/overview?workspaceId={workspace}")
            path = self.root / "workspace-a" / ".forge/editor/changes" / (request["changeSetId"] + ".json")
            if path.exists():
                record = json.loads(path.read_text(encoding="utf-8"))
                require(record["status"] not in ("conflict", "needs_reconcile"), f"Recovery failed: {record}")
                if record["status"] == "applied":
                    return record
            time.sleep(.1)
        raise AssertionError("Pending command did not automatically resume")

    def simulate_interrupted_journal(self, change_set_id, status, clear_result=False):
        """Fault injection confined to this runner's owned temporary project's journal."""
        folder = (self.root / "workspace-a/.forge/editor/changes").resolve(strict=True)
        require(folder.is_relative_to(self.root), "Fault injection path escaped owned evidence")
        path = folder / (change_set_id + ".json")
        record = json.loads(path.read_text(encoding="utf-8"))
        require(record["id"] == change_set_id and record["version"] == 2, "Unexpected journal format")
        record["status"] = status
        if clear_result:
            record["result"] = None
            for patch in record["patches"]:
                patch["after"] = None
        temporary = path.with_suffix(".smoke.tmp")
        save(temporary, record)
        temporary.replace(path)

    def run(self):
        self.start()
        a = self.api("/api/forge/workspaces", {"name": "Editor smoke A", "root": str(self.root / "workspace-a")})["workspace"]["id"]
        b = self.api("/api/forge/workspaces", {"name": "Editor smoke B", "root": str(self.root / "workspace-b")})["workspace"]["id"]
        save(self.root / "workspaces.json", {"a": a, "b": b})
        self.mcp(a, "scene_new", {"name": "Initiating scene", "mode": "3d"})
        created = self.mcp(a, "entity_create", {"name": "Unsaved cube", "components": [{"type": "MeshRenderer", "props": {"mesh": "cube", "material": ""}}]})
        original = created["entity"]
        base = self.baseline(a)
        ref = dict(base, workspaceId=a, kind="entity", entityGuid=original["entityGuid"])
        resolved = self.editor(a, "resolve", reference=ref)
        require(resolved["detail"]["entity"]["name"] == "Unsaved cube", "Unsaved live entity missing")
        require(not resolved["detail"]["entityIdentityPersisted"], "Unsaved entity claimed persistent disk identity")
        self.passed("unsaved live scene is shared between editor REST and agent MCP")
        self.document(a, {"version": 3, "seq": 4, "name": "Initial board",
                          "kinds": [{"id": "role", "label": "角色", "builtin": True, "tone": "info", "category": "role", "defaultFeatures": []}],
                          "nodes": [{"id": n, "kindId": "role", "name": n, "desc": "", "pos": [80 + 250 * i, 80], "features": [], "assets": [], "collapsed": False}
                                    for i, n in enumerate(("hero", "pending", "pie", "generated"))], "edges": [], "bindings": {}}, 0)
        request = {"changeSetId": "bound-command", "expected": base,
                   "ops": [{"op": "create", "name": "Generated entity", "clientId": "hero", "translation": [2, 0, 0]}],
                   "bindings": [{"boardId": "main", "nodeId": "hero", "clientId": "hero", "assets": []}]}
        applied = self.editor(a, "apply", **request)
        guid = applied["result"]["created"][0]["entityGuid"]
        require(applied["status"] == "applied" and self.document(a)["document"]["bindings"]["hero"]["entityGuid"] == guid, "Scene/board commit missing")
        require(self.editor(a, "apply", **request)["result"] == applied["result"], "Idempotent retry changed result")
        require(sum(e["entityGuid"] == guid for e in self.entities(a)) == 1, "Retry duplicated an entity")
        altered = json.loads(json.dumps(request)); altered["ops"][0]["name"] = "Different request"
        self.fails(lambda: self.editor(a, "apply", **altered), "IDEMPOTENCY_MISMATCH")
        self.passed("atomic scene + blueprint binding and request idempotency")
        self.simulate_interrupted_journal("bound-command", "prepared", clear_result=True)
        require(self.editor(a, "apply", **request)["result"] == applied["result"], "Prepared recovery missed host receipt")
        self.simulate_interrupted_journal("bound-command", "scene_committed")
        require(self.editor(a, "apply", **request)["status"] == "applied", "Scene-committed recovery did not finish")
        require(sum(e["entityGuid"] == guid for e in self.entities(a)) == 1, "Crash recovery duplicated creation")
        self.passed("prepared and scene-committed recovery reuse host receipt without duplicate entities")
        doc = self.document(a); doc["document"]["name"] = "Human title"
        doc["document"]["bindings"]["hero"]["note"] = "Human annotation"
        self.document(a, doc["document"], doc["revision"])
        self.editor(a, "undo")
        require(not any(e["entityGuid"] == guid for e in self.entities(a)), "Undo did not remove command entity")
        doc = self.document(a)["document"]
        require(doc["name"] == "Human title" and doc["bindings"]["hero"] == {"note": "Human annotation"}, "Undo overwrote unrelated human fields")
        self.simulate_interrupted_journal("bound-command", "undo_prepared")
        require(self.editor(a, "undo", changeSetId="bound-command")["status"] == "undone", "Interrupted undo did not recover")
        require(any(e["entityGuid"] == original["entityGuid"] for e in self.entities(a)), "Undo recovery popped another command")
        self.editor(a, "redo")
        require(any(e["entityGuid"] == guid for e in self.entities(a)), "Redo did not retain entity GUID")
        require(self.document(a)["document"]["bindings"]["hero"]["note"] == "Human annotation", "Redo overwrote note")
        self.passed("ordinary Undo/Redo coordinates owned fields and preserves human edits")
        stale = dict(request, changeSetId="stale-command", bindings=[])
        self.fails(lambda: self.editor(a, "apply", **stale), "CONTENT_CONFLICT")
        self.fails(lambda: self.mcp(a, "sprite_create", {"name": "Must not create stale sprite", "texture": "unused-texture", "expected": base}), "CONTENT_CONFLICT")
        self.fails(lambda: self.mcp(a, "text_create", {"name": "Must not create stale text", "text": "text", "font": "unused-font", "expected": base}), "CONTENT_CONFLICT")
        self.passed("stale content version refuses mutation")
        # Bare MCP apply and history must route through the same journal coordinator.
        raw = self.mcp(a, "editor_apply", {"changeSetId": "raw-agent-command", "expected": self.baseline(a), "ops": [{"op": "create", "name": "Raw MCP"}]})
        require(raw["status"] == "applied", "MCP editor_apply bypassed journal")
        self.mcp(a, "edit_undo")
        require(not any(e["name"] == "Raw MCP" for e in self.entities(a)), "MCP undo failed")
        self.passed("raw MCP apply/history use the cross-domain coordinator")
        if self.generation:
            self.check_generation(a, original)
        self.mcp(a, "scene_save", {"path": "Content/Scenes/Initiating.rxscene"})
        base = self.baseline(a)
        self.mcp(a, "scene_new", {"name": "Other scene"})
        pending = {"changeSetId": "pending-scene", "expected": base,
                   "ops": [{"op": "create", "name": "Returned to initiating scene", "clientId": "pending"}],
                   "bindings": [{"boardId": "main", "nodeId": "pending", "clientId": "pending"}]}
        require(self.editor(a, "apply", **pending)["status"] == "pending_target_scene", "Switched scene did not defer")
        require(not self.entities(a), "Pending command touched other scene")
        self.mcp(a, "scene_load", {"path": "Content/Scenes/Initiating.rxscene"})
        self.wait_applied(a, pending)
        self.passed("unchanged initiating scene automatically assembles on return")
        base = self.baseline(a)
        self.mcp(a, "play_enter")
        pending_pie = {"changeSetId": "pending-pie", "expected": base,
                       "ops": [{"op": "create", "name": "After PIE", "clientId": "pie"}],
                       "bindings": [{"boardId": "main", "nodeId": "pie", "clientId": "pie"}]}
        require(self.editor(a, "apply", **pending_pie)["status"] == "pending_edit_mode", "PIE did not defer")
        require(not any(e["name"] == "After PIE" for e in self.entities(a)), "Pending command mutated runtime")
        self.mcp(a, "play_exit")
        self.wait_applied(a, pending_pie)
        self.editor(a, "undo")
        require(not any(e["name"] == "After PIE" for e in self.entities(a)), "PIE discarded edit command history")
        self.passed("PIE preserves edit history and pending commands resume on exit")
        other = self.baseline(b)
        require(other["hostEpoch"] != self.baseline(a)["hostEpoch"] and not self.entities(b), "Workspaces share host state")
        self.mcp(b, "entity_create", {"name": "Only B"})
        require(not any(e["name"] == "Only B" for e in self.entities(a)), "Workspace B leaked into A")
        self.passed("two workspaces isolate host, identity and entities")
        alias = self.api("/api/forge/workspaces", {"name": "Editor smoke A alias", "root": "\\\\?\\" + str(self.root / "workspace-a")})["workspace"]["id"]
        require(self.baseline(alias)["hostEpoch"] == self.baseline(a)["hostEpoch"], "Canonical path alias started a second host")
        self.passed("ordinary and Windows extended paths share one project host epoch")
        self.mcp(a, "viewport_set_camera", {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "dist": 8, "ortho": True, "orthoSize": 4})
        capture = self.editor(a, "capture", width=128, height=128)
        candidate = next((c for c in capture["candidates"] if c["entityGuid"] == original["entityGuid"]), None)
        require(candidate is not None, "Frozen rendered cube has no candidate")
        pixels = self.api(capture["imageUrl"], binary=True)
        require(pixels[:8] == b"\x89PNG\r\n\x1a\n" and struct.unpack(">II", pixels[16:24]) == (128, 128), "Capture is not a real PNG of requested dimensions")
        self.mcp(a, "entity_destroy", {"id": original["id"]})
        self.mcp(a, "viewport_set_camera", {"target": [50, 50, 50]})
        bounds = candidate["bounds"]
        historical = self.editor(a, "capture", observationId=capture["observationId"],
                                 region=bounds, point={"x": bounds["x"] + bounds["width"] / 2, "y": bounds["y"] + bounds["height"] / 2})
        require(historical["staleForEditing"] and historical["hit"]["entityGuid"] == original["entityGuid"], "Historical pick used current scene/camera")
        require(any(c["entityGuid"] == original["entityGuid"] for c in historical["candidates"]), "Frozen region lost original candidate")
        require(self.api(historical["imageUrl"], binary=True) == pixels, "Historical screenshot changed")
        self.passed("capture image, point and region use the same frozen scene/camera")
        unknown_expected = self.baseline(a); unknown_expected["sceneGuid"] = "inactive-fault-injection-scene"
        uncertain = {"changeSetId": "restart-uncertain", "expected": unknown_expected, "ops": [{"op": "create", "name": "Must never duplicate after restart"}]}
        require(self.editor(a, "apply", **uncertain)["status"] == "pending_target_scene", "Fault injection setup did not defer")
        self.simulate_interrupted_journal("restart-uncertain", "prepared")
        self.stop(); self.start()
        after_restart = self.editor(a, "capture", observationId=capture["observationId"], region=bounds)
        require(after_restart["status"] == "historical" and after_restart["staleForEditing"], "Expired host observation did not degrade explicitly")
        require(after_restart["candidates"] == [] and self.api(after_restart["imageUrl"], binary=True) == pixels, "Restart used a live candidate fallback or lost image evidence")
        self.passed("restart retains historical image and explicitly expires live picks")
        deadline = time.monotonic() + 15
        journal = self.root / "workspace-a/.forge/editor/changes/restart-uncertain.json"
        while time.monotonic() < deadline:
            self.api(f"/api/forge/editor/overview?workspaceId={a}")
            uncertain = json.loads(journal.read_text(encoding="utf-8"))
            if uncertain["status"] == "needs_reconcile":
                break
            time.sleep(.1)
        require(uncertain["status"] == "needs_reconcile" and "STALE_HOST" in uncertain["lastError"], "Unknown pre-restart command was silently retried")
        require(not self.entities(a), "Recovery created entities in a new host without proving old outcome")
        self.passed("prepared command after host restart requires explicit reconciliation")
        save(self.root / "acceptance.json", {"status": "passed", "checks": self.checks,
             "screenshotSha256": hashlib.sha256(pixels).hexdigest(), "workspaces": {"a": a, "b": b}})

    def check_generation(self, workspace, original):
        def chunk(kind, data):
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
        png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 2, 8, 6, 0, 0, 0))
        png += chunk(b"IDAT", zlib.compress((b"\x00" + bytes([255, 80, 20, 255]) * 2) * 2)) + chunk(b"IEND", b"")
        candidate = self.root / "workspace-a/.forge/tmp/gen/smoke-candidate.png"
        candidate.parent.mkdir(parents=True, exist_ok=True)
        candidate.write_bytes(png)
        request = {"changeSetId": "generation-assembly", "expected": self.baseline(workspace),
                   "candidates": [{"id": "paint", "kind": "image", "fileRef": ".forge/tmp/gen/smoke-candidate.png", "name": "SmokePaint"}],
                   "ops": [{"op": "create", "name": "Generated textured entity", "clientId": "generated", "translation": [5, 0, 0],
                            "components": [{"type": "Sprite", "props": {"texture": "asset:paint"}}]}],
                   "bindings": [{"boardId": "main", "nodeId": "generated", "clientId": "generated", "assets": [{"candidateId": "paint"}]}]}
        result = self.editor(workspace, "accept_and_assemble", **request)
        require(result.get("ok") and result["status"] == "applied", f"Candidate assembly did not commit: {result}")
        asset = result["assets"]["paint"]
        published = self.root / "workspace-a/Content" / asset["path"]
        require(published.is_file() and hashlib.sha256(published.read_bytes()).hexdigest() == asset["version"], "Accepted asset receipt does not identify actual source bytes")
        generated = result["assembly"]["result"]["created"][0]
        require(self.document(workspace)["document"]["bindings"]["generated"]["assets"] == [asset], "Blueprint lost accepted asset version receipt")
        repeated = self.editor(workspace, "accept_and_assemble", **request)
        require(repeated["assets"] == result["assets"] and repeated["assembly"]["result"] == result["assembly"]["result"], "Generation acceptance retry changed asset/entity identity")
        require(sum(e["entityGuid"] == generated["entityGuid"] for e in self.entities(workspace)) == 1, "Generation retry duplicated an entity")
        bad_asset = dict(asset, version="0" * 64)
        invalid = {"changeSetId": "fake-asset-version", "expected": self.baseline(workspace),
                   "ops": [{"op": "rename", "entityGuid": original["entityGuid"], "name": "Must not rename"}],
                   "bindings": [{"boardId": "main", "nodeId": "generated", "target": {"entityGuid": generated["entityGuid"], "id": generated["id"]}, "assets": [bad_asset]}]}
        self.fails(lambda: self.editor(workspace, "apply", **invalid), "EDITOR_ASSET_VERSION_CONFLICT")
        self.editor(workspace, "undo")
        require(not any(e["entityGuid"] == generated["entityGuid"] for e in self.entities(workspace)), "Generation undo did not remove assembled entity")
        require(published.read_bytes() == png and self.document(workspace)["document"]["bindings"].get("generated") is None, "Generation undo deleted source asset or kept its binding")
        self.editor(workspace, "redo")
        require(any(e["entityGuid"] == generated["entityGuid"] for e in self.entities(workspace)), "Generation redo lost stable identity")
        self.passed("accepted candidate imports once, binds exact asset version, assembles and undoes without deleting source")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--serve", type=int, default=0, metavar="SECONDS", help="After success, retain only this isolated daemon for a bounded browser review (max 1800 seconds)")
    parser.add_argument("--review-root", type=Path, help="Reuse this runner's retained workspace/evidence for browser review only")
    parser.add_argument("--port", type=int, default=0, help="Owned loopback review port; default selects an unused ephemeral port")
    parser.add_argument("--include-generation", action="store_true", help="Also exercise explicit candidate acceptance and automatic scene assembly")
    args = parser.parse_args()
    require(0 <= args.port <= 65535, "Port must be 0..65535")
    require(os.name == "nt", "This acceptance runner requires Windows Job Objects")
    binaries = args.bin_dir.resolve(strict=True)
    for name in ("forge-agentd.exe", "engine-scene-mcp.exe", "engine-host.exe"):
        require((binaries / name).is_file(), "Missing separately built binary: " + name)
    temp = Path(os.environ.get("TEMP", tempfile.gettempdir())).resolve()
    require(temp.drive.lower() == "c:", "Use C: temporary storage for isolated evidence")
    if args.review_root:
        root = args.review_root.resolve(strict=True)
        manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
        require(root.parent == temp and manifest.get("schema") == "rurixforge.editor-smoke.v1" and manifest.get("root") == str(root), "Review root must be this runner's owned C: temporary evidence")
        require(all((root / name).resolve().parent == root and not (root / name).is_symlink() for name in ("data", "gen", "workspace-a", "workspace-b")), "Review directory escaped its owned root")
    else:
        root = Path(tempfile.mkdtemp(prefix="forge-editor-smoke-", dir=temp)).resolve()
        for name in ("data", "gen", "workspace-a", "workspace-b"):
            (root / name).mkdir()
        for name in ("workspace-a", "workspace-b"):
            project = root / name
            (project / "Content/Scenes").mkdir(parents=True)
            (project / "forge.toml").write_text('[project]\nname = "Editor smoke"\nengine-version = "0.1.0"\nrurix-ref = "v1.0.1-dist"\nentry-scene = "Content/Scenes/Main.rxscene"\nmode = "3d"\n\n[dirs]\ncontent = "Content"\nscripts = "Content/Scripts"\n', encoding="utf-8")
        save(root / "manifest.json", {"schema": "rurixforge.editor-smoke.v1", "root": str(root), "binaries": str(binaries)})
    isolated_bin = root / "bin"
    isolated_bin.mkdir(exist_ok=True)
    require(isolated_bin.resolve().parent == root and not isolated_bin.is_symlink(), "Isolated binary path escaped owned root")
    for name in ("forge-agentd", "engine-host", "engine-scene-mcp", "asset-pipeline-mcp", "context-mcp", "code-forge-mcp", "gen-image-mcp", "gen-model-mcp", "store-mcp"):
        source = binaries / (name + ".exe")
        if source.is_file():
            shutil.copy2(source, isolated_bin / source.name)
    print("Evidence:", root, flush=True)
    harness = Harness(root, isolated_bin, args.port, args.include_generation)
    try:
        if args.review_root:
            harness.starts = int(time.time())  # Keep previous acceptance process logs.
            harness.start()
        else:
            harness.run()
        if args.serve or args.review_root:
            workspace = json.loads((root / "workspaces.json").read_text(encoding="utf-8"))
            harness.mcp(workspace["a"], "scene_load", {"path": "Content/Scenes/Initiating.rxscene"})
            duration = max(0, min(args.serve or 1800, 1800))
            deadline = time.time() + duration
            save(root / "review-server.json", {"url": harness.base, "workspaces": workspace, "expiresAt": deadline})
            print("Review server:", harness.base, "workspaces:", json.dumps(workspace), flush=True)
            # The owned evidence file can extend a browser review without replacing the host.
            while time.time() < deadline:
                time.sleep(1)
                saved = json.loads((root / "review-server.json").read_text(encoding="utf-8"))
                deadline = min(float(saved.get("expiresAt", deadline)), time.time() + 1800)
    except BaseException as error:
        save(root / ("review-error.json" if args.review_root else "acceptance.json"), {"status": "failed", "checks": harness.checks, "error": str(error)})
        raise
    finally:
        harness.stop()


if __name__ == "__main__":
    main()
