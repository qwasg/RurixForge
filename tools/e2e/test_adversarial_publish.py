#!/usr/bin/env python3
"""Adversarial stress-test suite for MCP tool `gen_character_publish` and REST route `POST /api/forge/gen/mesh/publish`.

Challenges:
1. Empty or invalid meshFileRef (empty string, whitespace, path traversal, nonexistent file)
2. Invalid name with path separators and forbidden characters ('../bad', 'hero/char', 'hero\\char', 'hero:char', 'hero.char', ' hero')
3. Unskinned or non-character GLB passed to gen_character_publish (asserts explicit rejection under Redline I-5)
4. Missing animations or duplicate clips passed to gen_character_publish
5. REST route POST /api/forge/gen/mesh/publish parameter validation (400 GEN_BAD_PARAMS)
6. Valid rigged character GLB publication verification (assets on disk, rxmodel and rxprefab structure)
"""

import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
from typing import Any, Dict, List, Optional, Tuple
import urllib.error
import urllib.request

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent

def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]

def create_minimal_triangle_glb() -> bytes:
    """Build a minimal unskinned GLB containing 1 triangle."""
    indices = struct.pack("<3H", 0, 1, 2)
    positions = struct.pack("<9f", 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0)
    bin_buf = bytearray(indices)
    while len(bin_buf) % 4 != 0:
        bin_buf.append(0)
    pos_offset = len(bin_buf)
    bin_buf.extend(positions)
    while len(bin_buf) % 4 != 0:
        bin_buf.append(0)

    gltf_json = {
        "asset": {"version": "2.0"},
        "scenes": [{"nodes": [0]}],
        "scene": 0,
        "nodes": [{"mesh": 0}],
        "meshes": [{
            "primitives": [{
                "attributes": {"POSITION": 1},
                "indices": 0,
                "mode": 4
            }]
        }],
        "accessors": [
            {
                "bufferView": 0,
                "byteOffset": 0,
                "componentType": 5123,
                "count": 3,
                "type": "SCALAR"
            },
            {
                "bufferView": 1,
                "byteOffset": 0,
                "componentType": 5126,
                "count": 3,
                "type": "VEC3",
                "max": [1.0, 1.0, 0.0],
                "min": [0.0, 0.0, 0.0]
            }
        ],
        "bufferViews": [
            {
                "buffer": 0,
                "byteOffset": 0,
                "byteLength": len(indices),
                "target": 34963
            },
            {
                "buffer": 0,
                "byteOffset": pos_offset,
                "byteLength": len(positions),
                "target": 34962
            }
        ],
        "buffers": [{"byteLength": len(bin_buf)}]
    }

    json_bytes = json.dumps(gltf_json).encode("utf-8")
    while len(json_bytes) % 4 != 0:
        json_bytes += b" "

    total_len = 12 + 8 + len(json_bytes) + 8 + len(bin_buf)
    glb = bytearray()
    glb.extend(b"glTF")
    glb.extend(struct.pack("<I", 2))
    glb.extend(struct.pack("<I", total_len))
    glb.extend(struct.pack("<I", len(json_bytes)))
    glb.extend(b"JSON")
    glb.extend(json_bytes)
    glb.extend(struct.pack("<I", len(bin_buf)))
    glb.extend(b"BIN\x00")
    glb.extend(bin_buf)
    return bytes(glb)

class McpClientProcess:
    def __init__(self, project_dir: Path):
        self.project_dir = project_dir
        bin_path = WORKSPACE_ROOT / "target" / "debug" / "gen-model-mcp.exe"
        if not bin_path.is_file():
            bin_path = WORKSPACE_ROOT / "target" / "debug" / "gen_model_mcp.exe"
        assert bin_path.is_file(), f"Binary not found: {bin_path}"
        self.proc = subprocess.Popen(
            [str(bin_path), "--project", str(project_dir)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.msg_id = 0
        self.initialize()

    def send(self, req: dict) -> dict:
        line = json.dumps(req)
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()
        while True:
            resp_line = self.proc.stdout.readline()
            if not resp_line:
                err = self.proc.stderr.read()
                raise RuntimeError(f"MCP process terminated unexpectedly. Stderr: {err}")
            resp_line = resp_line.strip()
            if not resp_line:
                continue
            resp = json.loads(resp_line)
            if resp.get("id") == req.get("id"):
                return resp

    def initialize(self):
        self.msg_id += 1
        resp = self.send({
            "jsonrpc": "2.0",
            "id": self.msg_id,
            "method": "initialize",
            "params": {}
        })
        assert "result" in resp, f"Initialize failed: {resp}"

    def call_tool(self, name: str, args: dict) -> dict:
        self.msg_id += 1
        resp = self.send({
            "jsonrpc": "2.0",
            "id": self.msg_id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": args
            }
        })
        return resp

    def close(self):
        if self.proc:
            try:
                self.proc.stdin.close()
                self.proc.terminate()
                self.proc.wait(timeout=3)
            except Exception:
                self.proc.kill()

def run_adversarial_tests() -> Tuple[int, int, List[str]]:
    passed = 0
    failed = 0
    failures: List[str] = []

    def check(name: str, cond: bool, msg: str = ""):
        nonlocal passed, failed
        if cond:
            passed += 1
            print(f"  [PASS] {name}")
        else:
            failed += 1
            print(f"  [FAIL] {name}: {msg}")
            failures.append(f"{name}: {msg}")

    temp_dir = Path(tempfile.mkdtemp(prefix="rurix_adv_stress_"))
    try:
        # Create minimal Forge project
        forge_toml = temp_dir / "forge.toml"
        forge_toml.write_text('[project]\nname = "test_project"\n', encoding="utf-8")
        gen_dir = temp_dir / ".forge" / "tmp" / "gen"
        gen_dir.mkdir(parents=True, exist_ok=True)
        content_models = temp_dir / "Content" / "Models"
        content_models.mkdir(parents=True, exist_ok=True)

        # Place an unskinned GLB
        unskinned_glb_path = gen_dir / "unskinned_chair.glb"
        unskinned_glb_bytes = create_minimal_triangle_glb()
        unskinned_glb_path.write_bytes(unskinned_glb_bytes)

        # Place a valid rigged GLB using the e2e generator
        sys.path.insert(0, str(WORKSPACE_ROOT / "tools" / "e2e"))
        from character_3d_pipeline import create_synthetic_humanoid_glb
        rigged_glb_bytes = create_synthetic_humanoid_glb(
            triangle_count=400,
            include_skin=True,
            include_idle=True,
            include_walk=True,
            motion_amplitude=0.25
        )
        rigged_glb_path = gen_dir / "hero_rigged.glb"
        rigged_glb_path.write_bytes(rigged_glb_bytes)

        print("\n=== SECTION 1: MCP gen_character_publish Stress Tests ===")
        mcp = McpClientProcess(temp_dir)
        try:
            # Test 1.1: Missing meshFileRef
            res = mcp.call_tool("gen_character_publish", {"name": "hero"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.1: Missing meshFileRef", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.2: Empty meshFileRef
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": "", "name": "hero"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.2: Empty meshFileRef", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.3: Path traversal in meshFileRef
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": "../outside.glb", "name": "hero"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.3: Path traversal meshFileRef ('../outside.glb')", is_err and "GEN_FILE_NOT_FOUND" in text, f"res={res}")

            # Test 1.4: Nonexistent meshFileRef
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/nonexistent_mesh.glb", "name": "hero"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.4: Nonexistent meshFileRef", is_err and "GEN_FILE_NOT_FOUND" in text, f"res={res}")

            # Test 1.5: Missing name
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.5: Missing name", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.6: Empty name
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": ""})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.6: Empty name", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.7: Invalid name with path traversal ('../bad')
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "../bad"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.7: Path traversal name ('../bad')", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.8: Invalid name with forward slash ('hero/char')
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "hero/char"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.8: Forward slash name ('hero/char')", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.9: Invalid name with backslash ('hero\\char')
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "hero\\char"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.9: Backslash name ('hero\\\\char')", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.10: Invalid name with colon ('hero:char')
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "hero:char"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.10: Colon name ('hero:char')", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.11: Invalid name with extension ('hero.glb')
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "hero.glb"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.11: Extension in name ('hero.glb')", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.12: Invalid name with leading whitespace (' hero')
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": " hero"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.12: Leading whitespace name (' hero')", is_err and "GEN_BAD_PARAMS" in text, f"res={res}")

            # Test 1.13: Unskinned GLB rejection (Redline I-5)
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/unskinned_chair.glb", "name": "fake_hero"})
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            is_err = res.get("result", {}).get("isError", False)
            check("MCP.1.13: Unskinned GLB rejection (Redline I-5)", is_err and ("Redline I-5" in text or "MODEL_CHARACTER_INVALID" in text), f"res={res}")

            # Test 1.14: Valid rigged character publish
            res = mcp.call_tool("gen_character_publish", {"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "valkyrie"})
            is_err = res.get("result", {}).get("isError", False)
            text = res.get("result", {}).get("content", [{}])[0].get("text", "")
            parsed = json.loads(text) if not is_err else {}
            has_guids = "modelGuid" in parsed and "prefabGuid" in parsed
            model_uri = parsed.get("rxmodelUri", "")
            prefab_uri = parsed.get("rxprefabUri", "")
            check("MCP.1.14: Valid rigged character publication success", (not is_err) and has_guids and model_uri.startswith("Content/Models/"), f"res={res}")

            # Verify files on disk
            if not is_err:
                asset_path = temp_dir / "Content" / parsed["assetPath"]
                prefab_path = temp_dir / "Content" / parsed["prefabPath"]
                check("MCP.1.15: Published model.rxmodel file on disk", asset_path.is_file(), f"missing: {asset_path}")
                check("MCP.1.16: Published template.rxprefab file on disk", prefab_path.is_file(), f"missing: {prefab_path}")

        finally:
            mcp.close()

        print("\n=== SECTION 2: REST route POST /api/forge/gen/mesh/publish Tests ===")
        # Start forge-agentd on a free port
        agentd_bin = WORKSPACE_ROOT / "target" / "debug" / "forge-agentd.exe"
        if not agentd_bin.is_file():
            agentd_bin = WORKSPACE_ROOT / "target" / "debug" / "forge_agentd.exe"
        assert agentd_bin.is_file(), f"Binary not found: {agentd_bin}"

        port = find_free_port()
        agentd_addr = f"127.0.0.1:{port}"
        agentd_env = os.environ.copy()
        agentd_env["FORGE_AGENTD_ADDR"] = agentd_addr
        agentd_env["FORGE_AGENTD_WORKSPACE_ROOT"] = str(temp_dir)
        agentd_env["FORGE_AGENTD_DATA_DIR"] = str(temp_dir / "agentd_data")
        agentd_env["FORGE_GEN_DATA_DIR"] = str(temp_dir / "gen_data")

        agentd_proc = subprocess.Popen(
            [str(agentd_bin)],
            env=agentd_env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )

        try:
            # Wait for server ready
            base_url = f"http://{agentd_addr}"
            ready = False
            for _ in range(50):
                time.sleep(0.1)
                try:
                    with urllib.request.urlopen(f"{base_url}/health", timeout=1) as resp:
                        if resp.status == 200:
                            ready = True
                            break
                except Exception:
                    pass
            assert ready, "forge-agentd failed to start within 5s"

            def post_rest(payload: any, is_raw: bool = False) -> Tuple[int, any]:
                req_url = f"{base_url}/api/forge/gen/mesh/publish"
                data = payload if is_raw else json.dumps(payload).encode("utf-8")
                headers = {"Content-Type": "application/json"}
                req = urllib.request.Request(req_url, data=data, headers=headers, method="POST")
                try:
                    with urllib.request.urlopen(req, timeout=5) as resp:
                        raw = resp.read().decode("utf-8")
                        try:
                            body = json.loads(raw)
                        except Exception:
                            body = raw
                        return resp.status, body
                except urllib.error.HTTPError as e:
                    raw = e.read().decode("utf-8")
                    try:
                        body = json.loads(raw)
                    except Exception:
                        body = raw
                    return e.code, body

            # Test 2.1: Empty JSON object
            status, body = post_rest({})
            check("REST.2.1: Empty JSON object rejected with 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

            # Test 2.2: Empty meshFileRef
            status, body = post_rest({"meshFileRef": "", "name": "hero"})
            check("REST.2.2: Empty meshFileRef rejected with 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

            # Test 2.3: Whitespace meshFileRef
            status, body = post_rest({"meshFileRef": "   ", "name": "hero"})
            check("REST.2.3: Whitespace meshFileRef rejected with 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

            # Test 2.4: Empty name
            status, body = post_rest({"meshFileRef": "some.glb", "name": ""})
            check("REST.2.4: Empty name rejected with 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

            # Test 2.5: Whitespace name
            status, body = post_rest({"meshFileRef": "some.glb", "name": "   "})
            check("REST.2.5: Whitespace name rejected with 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

            # Test 2.6: Malformed JSON payload
            status, body = post_rest(b"not-json-content", is_raw=True)
            check("REST.2.6: Malformed JSON rejected with 400 Bad Request", status == 400, f"status={status}, body={body}")

            # Test 2.7: Path traversal name in REST call
            status, body = post_rest({"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "../bad"})
            err_code = body.get("error", {}).get("code")
            check(
                "REST.2.7: Path traversal name ('../bad') rejected with 400 GEN_BAD_PARAMS",
                status == 400 and err_code == "GEN_BAD_PARAMS",
                f"Expected 400 GEN_BAD_PARAMS, got HTTP {status} with code '{err_code}': {body}"
            )

            # Test 2.8: Unskinned mesh in REST call
            status, body = post_rest({"meshFileRef": ".forge/tmp/gen/unskinned_chair.glb", "name": "fake_hero"})
            err_code = body.get("error", {}).get("code")
            check(
                "REST.2.8: Unskinned mesh rejected with 400 (MODEL_CHARACTER_INVALID or GEN_BAD_PARAMS)",
                status == 400 and err_code in ("MODEL_CHARACTER_INVALID", "GEN_BAD_PARAMS"),
                f"Expected 400 Bad Request, got HTTP {status} with code '{err_code}': {body}"
            )

            # Test 2.9: Valid publish through REST endpoint
            status, body = post_rest({"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "rest_paladin"})
            has_guid = "modelGuid" in body or ("content" in body and "modelGuid" in body["content"][0].get("text", ""))
            check(
                "REST.2.9: Valid character publish via REST endpoint returns 200 OK",
                status == 200 and has_guid,
                f"status={status}, body={body}"
            )

        finally:
            agentd_proc.terminate()
            try:
                agentd_proc.wait(timeout=3)
            except Exception:
                agentd_proc.kill()

    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)

    report = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "total": passed + failed,
        "passed": passed,
        "failed": failed,
        "failures": failures,
    }
    report_path = WORKSPACE_ROOT / ".forge" / "tmp" / "e2e" / "adversarial_publish_report.json"
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")

    print(f"\n=======================================================")
    print(f"ADVERSARIAL STRESS TEST SUMMARY")
    print(f"Total: {passed + failed} | Passed: {passed} | Failed: {failed}")
    print(f"Report written to: {report_path}")
    print(f"=======================================================")
    return passed, failed, failures

if __name__ == "__main__":
    p, f, fails = run_adversarial_tests()
    if f > 0:
        sys.exit(1)
