#!/usr/bin/env python3
"""Challenger Stress Test: ASCII .gltf handling in gen-model-mcp.

Empirically tests that invoking `gen_mesh_refine` with an ASCII `.gltf` file:
1. Returns GEN_BACKEND_NOT_CONFIGURED without crashing.
2. Does NOT leak or crash when MESHY_API_KEY is configured in the environment.
3. Does NOT crash or fall through to python decimate_mesh.py with GEN_BACKEND_ERROR.
4. Keeps the stdio JSON-RPC MCP server alive across multiple subsequent calls.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

TRI_MIN_GLTF = (
    '{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],'
    '"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"mode":4}]}],'
    '"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","max":[1,1,0],"min":[0,0,0]}],'
    '"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],'
    '"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,'
    'AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}]}'
)

def run_mcp_query(binary_path: Path, project_root: Path, extra_env: dict, requests: list) -> list:
    env = os.environ.copy()
    env.update(extra_env)
    proc = subprocess.Popen(
        [str(binary_path), "--project", str(project_root)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )

    responses = []
    for req in requests:
        req_line = json.dumps(req) + "\n"
        proc.stdin.write(req_line)
        proc.stdin.flush()
        resp_line = proc.stdout.readline()
        if not resp_line:
            break
        responses.append(json.loads(resp_line.strip()))

    proc.stdin.close()
    proc.wait(timeout=5)
    stderr = proc.stderr.read()
    return responses, proc.returncode, stderr

def main():
    workspace = Path(__file__).resolve().parent.parent.parent
    binary_path = workspace / "target" / "debug" / "gen-model-mcp.exe"
    if not binary_path.exists():
        binary_path = workspace / "target" / "debug" / "gen-model-mcp"
    if not binary_path.exists():
        print(f"Error: binary not found at {binary_path}", file=sys.stderr)
        sys.exit(1)

    print(f"Testing binary: {binary_path}")
    passed = 0
    total = 0

    with tempfile.TemporaryDirectory(prefix="challenger-gltf-") as tmpdir:
        tmp_path = Path(tmpdir)
        forge_gen_dir = tmp_path / ".forge" / "tmp" / "gen"
        forge_gen_dir.mkdir(parents=True, exist_ok=True)
        gltf_file = forge_gen_dir / "ascii_model.gltf"
        gltf_file.write_text(TRI_MIN_GLTF, encoding="utf-8")

        # Test 1: Default track (omitted), MESHY_API_KEY unset
        total += 1
        req1 = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "gen_mesh_refine",
                "arguments": {
                    "meshFileRef": ".forge/tmp/gen/ascii_model.gltf",
                    "ops": {"retopo": {"targetFaces": 1000}},
                },
            },
        }
        resps, retcode, stderr = run_mcp_query(
            binary_path,
            tmp_path,
            {"MESHY_API_KEY": ""},
            [{"jsonrpc": "2.0", "id": 0, "method": "initialize"}, req1],
        )
        assert retcode == 0, f"Process crashed with returncode {retcode}, stderr: {stderr}"
        assert len(resps) == 2, f"Expected 2 responses, got {len(resps)}"
        call_resp = resps[1]
        assert call_resp.get("result", {}).get("isError") is True, f"Expected isError: true, got {call_resp}"
        content_text = call_resp["result"]["content"][0]["text"]
        payload = json.loads(content_text)
        assert payload.get("error") == "GEN_BACKEND_NOT_CONFIGURED", f"Expected GEN_BACKEND_NOT_CONFIGURED, got {payload}"
        print("  [PASS] Test 1: ASCII .gltf with default track returns GEN_BACKEND_NOT_CONFIGURED without crash")
        passed += 1

        # Test 2: Explicit track: 'auto', MESHY_API_KEY unset
        total += 1
        req2 = {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "gen_mesh_refine",
                "arguments": {
                    "meshFileRef": ".forge/tmp/gen/ascii_model.gltf",
                    "track": "auto",
                    "ops": {"retopo": {"targetFaces": 1000}},
                },
            },
        }
        resps, retcode, stderr = run_mcp_query(
            binary_path,
            tmp_path,
            {"MESHY_API_KEY": ""},
            [req2],
        )
        assert retcode == 0, f"Process crashed with returncode {retcode}"
        call_resp = resps[0]
        assert call_resp.get("result", {}).get("isError") is True
        payload = json.loads(call_resp["result"]["content"][0]["text"])
        assert payload.get("error") == "GEN_BACKEND_NOT_CONFIGURED"
        print("  [PASS] Test 2: ASCII .gltf with explicit track='auto' returns GEN_BACKEND_NOT_CONFIGURED without crash")
        passed += 1

        # Test 3: Adversarial test: MESHY_API_KEY IS SET in environment!
        # In iteration 2, this caused it to bypass the guard, try python decimation, and crash with ValueError / GEN_BACKEND_ERROR.
        total += 1
        req3 = {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "gen_mesh_refine",
                "arguments": {
                    "meshFileRef": ".forge/tmp/gen/ascii_model.gltf",
                    "track": "auto",
                    "ops": {"retopo": {"targetFaces": 1000}},
                },
            },
        }
        resps, retcode, stderr = run_mcp_query(
            binary_path,
            tmp_path,
            {"MESHY_API_KEY": "msy_adversarial_stress_test_key_dummy"},
            [req3],
        )
        assert retcode == 0, f"Process crashed with returncode {retcode}, stderr: {stderr}"
        call_resp = resps[0]
        assert call_resp.get("result", {}).get("isError") is True
        payload = json.loads(call_resp["result"]["content"][0]["text"])
        assert payload.get("error") == "GEN_BACKEND_NOT_CONFIGURED", f"Expected GEN_BACKEND_NOT_CONFIGURED, got {payload}"
        print("  [PASS] Test 3: ASCII .gltf with MESHY_API_KEY present still returns GEN_BACKEND_NOT_CONFIGURED (no crash, no GEN_BACKEND_ERROR)")
        passed += 1

        # Test 4: Adversarial test: Multi-call session resilience
        # Perform 5 consecutive refine requests on ASCII .gltf followed by tools/list in the same session
        total += 1
        multi_reqs = [
            {
                "jsonrpc": "2.0",
                "id": i,
                "method": "tools/call",
                "params": {
                    "name": "gen_mesh_refine",
                    "arguments": {
                        "meshFileRef": ".forge/tmp/gen/ascii_model.gltf",
                        "ops": {"retopo": {"targetFaces": 500 * i}},
                    },
                },
            }
            for i in range(1, 6)
        ]
        multi_reqs.append({"jsonrpc": "2.0", "id": 100, "method": "tools/list"})

        resps, retcode, stderr = run_mcp_query(
            binary_path,
            tmp_path,
            {"MESHY_API_KEY": "msy_adversarial_stress_test_key_dummy"},
            multi_reqs,
        )
        assert retcode == 0, f"Process crashed during multi-request sequence: {stderr}"
        assert len(resps) == 6, f"Expected 6 responses, got {len(resps)}"
        for i in range(5):
            p = json.loads(resps[i]["result"]["content"][0]["text"])
            assert p.get("error") == "GEN_BACKEND_NOT_CONFIGURED"
        assert "tools" in resps[5]["result"]
        print("  [PASS] Test 4: Server session remains stable and non-crashing across 5 consecutive ASCII .gltf refine calls + tools/list")
        passed += 1

    print("\n=======================================================")
    print(f"ASCII GLTF EDGE CASE STRESS TEST SUMMARY")
    print(f"Total: {total} | Passed: {passed} | Failed: 0")
    print("=======================================================")

if __name__ == "__main__":
    main()
