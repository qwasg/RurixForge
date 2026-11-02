#!/usr/bin/env python3
r"""Empirical Challenger Stress Harness for REST /api/forge/gen/mesh/publish.

Tests the following specific requirements:
1. POST /api/forge/gen/mesh/publish with invalid names:
   - "../bad"
   - "hero/char"
   - "hero:char"
   - "hero.glb"
   - " hero"
   - "hero "
   - r"hero\char"
   Returns HTTP 400 with code "GEN_BAD_PARAMS" and clean unescaped string message.
2. POST /api/forge/gen/mesh/publish with unskinned GLB:
   Returns HTTP 400 with code "MODEL_CHARACTER_INVALID" or "GEN_BAD_PARAMS",
   and clean unescaped string message (no raw/stringified JSON).
3. Nonexistent files:
   Returns HTTP 404 with code "GEN_FILE_NOT_FOUND" and clean unescaped string message.
4. Valid rigged character:
   Returns HTTP 200 with modelGuid, prefabGuid, and created files on disk.
5. Adversarial inputs:
   - Special control characters, malformed bodies, whitespace refs.
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
from typing import Any, Dict, List, Tuple
import urllib.error
import urllib.request

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent

def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]

def create_minimal_unskinned_glb() -> bytes:
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

def is_clean_string(s: Any) -> bool:
    """Check that message is a clean, non-empty string, not raw/escaped JSON."""
    if not isinstance(s, str) or not s.strip():
        return False
    trimmed = s.strip()
    if trimmed.startswith("{") and trimmed.endswith("}"):
        return False
    if r'\"error\"' in s or r'\"code\"' in s or r'\"message\"' in s:
        return False
    return True

def run_challenger_tests() -> Tuple[int, int, List[str]]:
    passed = 0
    failed = 0
    failures: List[str] = []

    def check(name: str, cond: bool, detail: str = ""):
        nonlocal passed, failed
        if cond:
            passed += 1
            print(f"  [PASS] {name}")
        else:
            failed += 1
            print(f"  [FAIL] {name}: {detail}")
            failures.append(f"{name}: {detail}")

    temp_dir = Path(tempfile.mkdtemp(prefix="challenger_publish_"))
    agentd_proc = None
    try:
        # 1. Project setup
        forge_toml = temp_dir / "forge.toml"
        forge_toml.write_text('[project]\nname = "challenger_test"\n', encoding="utf-8")
        gen_dir = temp_dir / ".forge" / "tmp" / "gen"
        gen_dir.mkdir(parents=True, exist_ok=True)
        content_models = temp_dir / "Content" / "Models"
        content_models.mkdir(parents=True, exist_ok=True)

        # 2. Place unskinned GLB
        unskinned_path = gen_dir / "unskinned_box.glb"
        unskinned_path.write_bytes(create_minimal_unskinned_glb())

        # 3. Place valid rigged GLB
        sys.path.insert(0, str(WORKSPACE_ROOT / "tools" / "e2e"))
        from character_3d_pipeline import create_synthetic_humanoid_glb
        rigged_bytes = create_synthetic_humanoid_glb(
            triangle_count=350,
            include_skin=True,
            include_idle=True,
            include_walk=True,
            motion_amplitude=0.3
        )
        rigged_path = gen_dir / "hero_rigged.glb"
        rigged_path.write_bytes(rigged_bytes)

        # 4. Start forge-agentd
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

        base_url = f"http://{agentd_addr}"
        ready = False
        for _ in range(60):
            time.sleep(0.1)
            try:
                with urllib.request.urlopen(f"{base_url}/health", timeout=1) as resp:
                    if resp.status == 200:
                        ready = True
                        break
            except Exception:
                pass
        assert ready, "forge-agentd failed to start within 6s"

        def post_publish(payload: Any, is_raw: bool = False) -> Tuple[int, Any]:
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

        print("\n=== TEST GROUP 1: Specific Invalid Character Names ===")
        invalid_names = [
            ("../bad", "Path traversal '../bad'"),
            ("hero/char", "Forward slash 'hero/char'"),
            ("hero:char", "Colon 'hero:char'"),
            ("hero.glb", "Extension 'hero.glb'"),
            (" hero", "Leading space ' hero'"),
            ("hero ", "Trailing space 'hero '"),
            (r"hero\char", r"Backslash 'hero\char'"),
        ]

        for bad_name, desc in invalid_names:
            status, body = post_publish({
                "meshFileRef": ".forge/tmp/gen/hero_rigged.glb",
                "name": bad_name
            })
            code = body.get("error", {}).get("code") if isinstance(body, dict) else None
            msg = body.get("error", {}).get("message") if isinstance(body, dict) else None
            clean = is_clean_string(msg)

            check(
                f"Invalid Name ({desc}): HTTP 400",
                status == 400,
                f"Got status {status}, body={body}"
            )
            check(
                f"Invalid Name ({desc}): Code GEN_BAD_PARAMS",
                code == "GEN_BAD_PARAMS",
                f"Got code {code}"
            )
            check(
                f"Invalid Name ({desc}): Clean message string",
                clean,
                f"Got message={msg!r}"
            )

        print("\n=== TEST GROUP 2: Unskinned GLB Character Publish (Redline I-5) ===")
        status, body = post_publish({
            "meshFileRef": ".forge/tmp/gen/unskinned_box.glb",
            "name": "unskinned_hero"
        })
        code = body.get("error", {}).get("code") if isinstance(body, dict) else None
        msg = body.get("error", {}).get("message") if isinstance(body, dict) else None
        clean = is_clean_string(msg)

        check(
            "Unskinned GLB: HTTP 400",
            status == 400,
            f"Got status {status}, body={body}"
        )
        check(
            "Unskinned GLB: Code is MODEL_CHARACTER_INVALID or GEN_BAD_PARAMS",
            code in ("MODEL_CHARACTER_INVALID", "GEN_BAD_PARAMS"),
            f"Got code {code}"
        )
        check(
            "Unskinned GLB: Clean unescaped string message",
            clean and ("角色发布校验失败" in msg or "MODEL_CHARACTER_INVALID" in msg or "skin" in msg.lower()),
            f"Got msg={msg!r}"
        )

        print("\n=== TEST GROUP 3: Nonexistent Mesh Files ===")
        nonexistent_cases = [
            (".forge/tmp/gen/missing_avatar.glb", "Missing GLB inside gen tmpdir"),
            (".forge/tmp/gen/does_not_exist.glb", "Another missing GLB"),
            ("../outside_project.glb", "Path traversal meshFileRef '../outside_project.glb'"),
            ("Content/Models/escape.glb", "Disallowed directory meshFileRef"),
        ]

        for mesh_ref, desc in nonexistent_cases:
            status, body = post_publish({
                "meshFileRef": mesh_ref,
                "name": "valid_avatar"
            })
            code = body.get("error", {}).get("code") if isinstance(body, dict) else None
            msg = body.get("error", {}).get("message") if isinstance(body, dict) else None
            clean = is_clean_string(msg)

            check(
                f"Nonexistent File ({desc}): HTTP 404",
                status == 404,
                f"Got status {status}, body={body}"
            )
            check(
                f"Nonexistent File ({desc}): Code GEN_FILE_NOT_FOUND",
                code == "GEN_FILE_NOT_FOUND",
                f"Got code {code}"
            )
            check(
                f"Nonexistent File ({desc}): Clean message string",
                clean,
                f"Got msg={msg!r}"
            )

        print("\n=== TEST GROUP 4: Valid Character Publish ===")
        status, body = post_publish({
            "meshFileRef": ".forge/tmp/gen/hero_rigged.glb",
            "name": "challenger_paladin"
        })
        check(
            "Valid publish: HTTP 200",
            status == 200,
            f"Got status {status}, body={body}"
        )
        has_guids = isinstance(body, dict) and "modelGuid" in body and "prefabGuid" in body
        check(
            "Valid publish: Returns modelGuid and prefabGuid",
            has_guids,
            f"body={body}"
        )
        if has_guids:
            asset_path = temp_dir / "Content" / body["assetPath"]
            prefab_path = temp_dir / "Content" / body["prefabPath"]
            check(
                "Valid publish: Published model file exists on disk",
                asset_path.is_file(),
                f"Missing {asset_path}"
            )
            check(
                "Valid publish: Published prefab file exists on disk",
                prefab_path.is_file(),
                f"Missing {prefab_path}"
            )

        print("\n=== TEST GROUP 5: Adversarial Boundary Cases ===")
        # Empty body
        status, body = post_publish({})
        check("Adversarial: Empty body returns 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

        # Missing name
        status, body = post_publish({"meshFileRef": ".forge/tmp/gen/hero_rigged.glb"})
        check("Adversarial: Missing name returns 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

        # Empty name
        status, body = post_publish({"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": ""})
        check("Adversarial: Empty name returns 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

        # Whitespace-only name
        status, body = post_publish({"meshFileRef": ".forge/tmp/gen/hero_rigged.glb", "name": "   "})
        check("Adversarial: Whitespace name returns 400 GEN_BAD_PARAMS", status == 400 and body.get("error", {}).get("code") == "GEN_BAD_PARAMS", f"status={status}, body={body}")

        # Malformed JSON payload
        status, body = post_publish(b"###INVALID_JSON###", is_raw=True)
        check("Adversarial: Malformed JSON body returns 400", status == 400, f"status={status}, body={body}")

    finally:
        if agentd_proc:
            agentd_proc.terminate()
            try:
                agentd_proc.wait(timeout=3)
            except Exception:
                agentd_proc.kill()
        shutil.rmtree(temp_dir, ignore_errors=True)

    print("\n=======================================================")
    print(f"CHALLENGER STRESS HARNESS SUMMARY")
    print(f"Total: {passed + failed} | Passed: {passed} | Failed: {failed}")
    if failures:
        print("Failures:")
        for f_str in failures:
            print(f"  - {f_str}")
    print("=======================================================")
    return passed, failed, failures

if __name__ == "__main__":
    p, f, fails = run_challenger_tests()
    if f > 0:
        sys.exit(1)
