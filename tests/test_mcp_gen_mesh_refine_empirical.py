#!/usr/bin/env python3
"""
tests/test_mcp_gen_mesh_refine_empirical.py
Empirical Challenger Test Suite for gen_mesh_refine MCP tool and parse_mesh_geometry.

Validates:
1. stdio JSON-RPC integration with target/debug/gen-model-mcp.exe
2. Real GLB inputs: quad_indexed.glb, multi-triangle meshes, unindexed meshes
3. Exact metric matching: originalTriangles, originalVertices, refinedTriangles, refinedVertices
   against ground-truth GLB binary accessors for both input and refined output.
4. Error handling: corrupt GLB, missing file, cloud track without config.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
import numpy as np

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if PROJECT_ROOT not in sys.path:
    sys.path.insert(0, PROJECT_ROOT)

from tools.model.glb_utils import (
    load_glb,
    save_glb,
    get_accessor_data,
    add_accessor,
)


def get_glb_ground_truth(glb_path_or_bytes):
    """Parses glTF/GLB structure directly and extracts ground truth (triangles, vertices)."""
    if isinstance(glb_path_or_bytes, (bytes, bytearray)):
        bytes_data = glb_path_or_bytes
    else:
        with open(glb_path_or_bytes, "rb") as f:
            bytes_data = f.read()

    if bytes_data.startswith(b"glTF"):
        chunk_len = int.from_bytes(bytes_data[12:16], "little")
        chunk_type = bytes_data[16:20]
        assert chunk_type == b"JSON"
        json_slice = bytes_data[20 : 20 + chunk_len]
        gltf = json.loads(json_slice.decode("utf-8"))
    else:
        gltf = json.loads(bytes_data.decode("utf-8"))

    accessors = gltf.get("accessors", [])
    total_vertices = 0
    total_triangles = 0

    for mesh in gltf.get("meshes", []):
        for prim in mesh.get("primitives", []):
            pos_idx = prim.get("attributes", {}).get("POSITION")
            pos_count = accessors[pos_idx]["count"] if pos_idx is not None else 0
            total_vertices += pos_count

            mode = prim.get("mode", 4)
            if "indices" in prim:
                idx_idx = prim["indices"]
                count = accessors[idx_idx]["count"]
            else:
                count = pos_count

            if mode == 4:
                total_triangles += count // 3
            elif mode in (5, 6):
                total_triangles += max(0, count - 2)

    return total_triangles, total_vertices


class McpProcessClient:
    def __init__(self, project_dir):
        bin_path = os.path.join(PROJECT_ROOT, "target", "debug", "gen-model-mcp.exe")
        if not os.path.exists(bin_path):
            raise FileNotFoundError(f"Binary not found: {bin_path}")
        self.proc = subprocess.Popen(
            [bin_path, "--project", project_dir],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.req_id = 0

    def call(self, method, params=None):
        self.req_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self.req_id,
            "method": method,
            "params": params or {},
        }
        line = json.dumps(payload) + "\n"
        self.proc.stdin.write(line)
        self.proc.stdin.flush()
        resp_line = self.proc.stdout.readline()
        if not resp_line:
            stderr = self.proc.stderr.read()
            raise RuntimeError(f"gen-model-mcp exited prematurely. Stderr: {stderr}")
        return json.loads(resp_line.strip())

    def call_tool(self, name, arguments):
        resp = self.call("tools/call", {"name": name, "arguments": arguments})
        if "error" in resp:
            return resp
        result = resp.get("result", {})
        content = result.get("content", [])
        if content and content[0].get("type") == "text":
            try:
                parsed_text = json.loads(content[0]["text"])
                if result.get("isError"):
                    return {"isError": True, "error": parsed_text}
                return parsed_text
            except json.JSONDecodeError:
                return content[0]["text"]
        return result

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.terminate()
            self.proc.wait(timeout=2)
        except Exception:
            pass


class TestGenMeshRefineEmpirical(unittest.TestCase):
    def setUp(self):
        self.test_dir = tempfile.mkdtemp(prefix="test_mcp_refine_")
        # Set up a minimal ForgeProject structure
        self.gen_dir = os.path.join(self.test_dir, ".forge", "tmp", "gen")
        os.makedirs(self.gen_dir, exist_ok=True)
        # Create minimal forge.toml
        with open(os.path.join(self.test_dir, "forge.toml"), "w") as f:
            f.write('[project]\nname = "test-proj"\nversion = "0.1.0"\n')
        self.client = McpProcessClient(self.test_dir)

    def tearDown(self):
        self.client.close()
        shutil.rmtree(self.test_dir, ignore_errors=True)

    def test_tools_list_declares_refine_correctly(self):
        """MCP tools/list must list gen_mesh_refine with expected schema."""
        resp = self.client.call("tools/list")
        tools = resp.get("result", {}).get("tools", [])
        refine_tool = next((t for t in tools if t["name"] == "gen_mesh_refine"), None)
        self.assertIsNotNone(refine_tool)
        self.assertIn("meshFileRef", refine_tool["inputSchema"]["required"])
        self.assertIn("ops", refine_tool["inputSchema"]["required"])

    def test_refine_real_fixture_quad_indexed(self):
        """Empirically test gen_mesh_refine on real assetd fixture quad_indexed.glb."""
        fixture_path = os.path.join(PROJECT_ROOT, "crates", "assetd", "tests", "fixtures", "quad_indexed.glb")
        self.assertTrue(os.path.exists(fixture_path), "quad_indexed.glb fixture missing")

        # Ground truth of input
        gt_in_tri, gt_in_vert = get_glb_ground_truth(fixture_path)
        self.assertEqual(gt_in_tri, 2)
        self.assertEqual(gt_in_vert, 4)

        target_file = os.path.join(self.gen_dir, "quad.glb")
        shutil.copyfile(fixture_path, target_file)

        # Call gen_mesh_refine via MCP stdio
        resp = self.client.call_tool(
            "gen_mesh_refine",
            {
                "meshFileRef": ".forge/tmp/gen/quad.glb",
                "targetFaces": 2,
                "track": "local",
                "ops": {"retopo": {"targetFaces": 2}},
            },
        )
        self.assertNotIn("isError", resp)
        self.assertEqual(resp["originalTriangles"], 2)
        self.assertEqual(resp["originalVertices"], 4)
        self.assertLessEqual(resp["refinedTriangles"], 2)
        self.assertGreater(resp["refinedVertices"], 0)

        # Verify ground truth of generated output file
        out_rel = resp["refinedMeshFileRef"]
        out_abs = os.path.join(self.test_dir, out_rel.replace("/", os.sep))
        self.assertTrue(os.path.exists(out_abs), f"Output file does not exist: {out_abs}")

        gt_out_tri, gt_out_vert = get_glb_ground_truth(out_abs)
        self.assertEqual(resp["refinedTriangles"], gt_out_tri, "Reported refinedTriangles != actual GLB triangle count")
        self.assertEqual(resp["refinedVertices"], gt_out_vert, "Reported refinedVertices != actual GLB vertex count")

    def test_refine_high_poly_sphere_exact_metrics(self):
        """Empirically test decimation of 500+ triangle sphere down to 100 triangles."""
        # Generate sphere
        from tests.test_model_stress import generate_sphere_mesh, create_test_glb
        verts, tris, uvs = generate_sphere_mesh(20, 20)
        self.assertEqual(len(tris), 722)
        self.assertEqual(len(verts), 400)

        gltf, bin_data = create_test_glb(verts, tris, uvs)
        in_path = os.path.join(self.gen_dir, "sphere_722.glb")
        save_glb(in_path, gltf, bin_data)

        # Call gen_mesh_refine via MCP stdio
        resp = self.client.call_tool(
            "gen_mesh_refine",
            {
                "meshFileRef": ".forge/tmp/gen/sphere_722.glb",
                "targetFaces": 100,
                "track": "local",
                "ops": {"retopo": {"targetFaces": 100}},
            },
        )
        self.assertNotIn("isError", resp)
        self.assertEqual(resp["originalTriangles"], 722)
        self.assertEqual(resp["originalVertices"], 400)
        self.assertLessEqual(resp["refinedTriangles"], 100)
        self.assertGreater(resp["reductionPercent"], 80.0)

        # Verify ground truth of generated output file
        out_rel = resp["refinedMeshFileRef"]
        out_abs = os.path.join(self.test_dir, out_rel.replace("/", os.sep))
        self.assertTrue(os.path.exists(out_abs))

        gt_out_tri, gt_out_vert = get_glb_ground_truth(out_abs)
        self.assertEqual(resp["refinedTriangles"], gt_out_tri, "refinedTriangles mismatch with real output GLB")
        self.assertEqual(resp["refinedVertices"], gt_out_vert, "refinedVertices mismatch with real output GLB")

    def test_refine_unindexed_glb(self):
        """Test gen_mesh_refine handling of unindexed GLB (primitives without 'indices' accessor)."""
        # Create unindexed triangle mesh: 2 triangles = 6 vertices
        verts = np.array([
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
        ], dtype=np.float32)

        gltf = {
            "asset": {"version": "2.0"},
            "meshes": [{
                "primitives": [{
                    "attributes": {"POSITION": 0},
                    "mode": 4,
                }]
            }],
            "accessors": [{
                "bufferView": 0,
                "byteOffset": 0,
                "componentType": 5126,
                "count": 6,
                "type": "VEC3",
                "max": [1.0, 1.0, 0.0],
                "min": [0.0, 0.0, 0.0],
            }],
            "bufferViews": [{
                "buffer": 0,
                "byteOffset": 0,
                "byteLength": 6 * 12,
            }],
            "buffers": [{"byteLength": 6 * 12}],
        }
        bin_data = verts.tobytes()
        in_path = os.path.join(self.gen_dir, "unindexed.glb")
        save_glb(in_path, gltf, bin_data)

        # Ground truth
        gt_in_tri, gt_in_vert = get_glb_ground_truth(in_path)
        self.assertEqual(gt_in_tri, 2)
        self.assertEqual(gt_in_vert, 6)

        resp = self.client.call_tool(
            "gen_mesh_refine",
            {
                "meshFileRef": ".forge/tmp/gen/unindexed.glb",
                "targetFaces": 2,
                "track": "local",
                "ops": {},
            },
        )
        self.assertNotIn("isError", resp)
        self.assertEqual(resp["originalTriangles"], 2)
        self.assertEqual(resp["originalVertices"], 6)

        out_rel = resp["refinedMeshFileRef"]
        out_abs = os.path.join(self.test_dir, out_rel.replace("/", os.sep))
        gt_out_tri, gt_out_vert = get_glb_ground_truth(out_abs)
        self.assertEqual(resp["refinedTriangles"], gt_out_tri)
        self.assertEqual(resp["refinedVertices"], gt_out_vert)

    def test_refine_missing_params_error(self):
        """Calling refine without ops or targetFaces must return GEN_BAD_PARAMS."""
        resp = self.client.call_tool(
            "gen_mesh_refine",
            {
                "meshFileRef": ".forge/tmp/gen/quad.glb",
            },
        )
        self.assertTrue(resp.get("isError") or "error" in resp)
        err_msg = json.dumps(resp)
        self.assertIn("GEN_BAD_PARAMS", err_msg)

    def test_refine_missing_file_error(self):
        """Calling refine with nonexistent file must fail with GEN_FILE_NOT_FOUND."""
        resp = self.client.call_tool(
            "gen_mesh_refine",
            {
                "meshFileRef": ".forge/tmp/gen/does_not_exist_404.glb",
                "targetFaces": 500,
                "track": "local",
                "ops": {},
            },
        )
        self.assertTrue(resp.get("isError") or "error" in resp)
        err_msg = json.dumps(resp)
        self.assertIn("GEN_FILE_NOT_FOUND", err_msg)

    def test_refine_corrupted_glb_error(self):
        """Calling refine with a corrupted/truncated GLB must return GEN_BACKEND_ERROR cleanly."""
        corrupt_path = os.path.join(self.gen_dir, "corrupt.glb")
        with open(corrupt_path, "wb") as f:
            f.write(b"glTF\x02\x00\x00\x00\x10\x00\x00\x00\x00\x00\x00\x00corrupt")

        resp = self.client.call_tool(
            "gen_mesh_refine",
            {
                "meshFileRef": ".forge/tmp/gen/corrupt.glb",
                "targetFaces": 10,
                "track": "local",
                "ops": {},
            },
        )
        self.assertTrue(resp.get("isError") or "error" in resp)
        err_msg = json.dumps(resp)
        self.assertIn("GEN_BACKEND_ERROR", err_msg)


if __name__ == "__main__":
    unittest.main()
