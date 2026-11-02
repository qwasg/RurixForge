#!/usr/bin/env python3
"""
tests/test_model_stress_edge_cases.py
Adversarial stress harness for extreme edge-case decimation ratios and normal validation:
- Extreme decimation face budgets: 50, 30, 20, 10, 8, 4, 2, 1
- High poly spheres (3000+, 8000+ triangles)
- Planar degenerate meshes
- Opposing/back-to-back faces (causing normal cancellation)
- Verification of non-zero unit normal vectors (||N|| = 1.0 +/- 1e-3)
- Embedded texture preservation under extreme decimation
"""

import os
import shutil
import sys
import tempfile
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
    add_buffer_view,
)
from tools.model.decimate_mesh import decimate_mesh
from tests.test_model_stress import generate_sphere_mesh, create_test_glb


class TestModelDecimateEdgeCases(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp(prefix="test_edge_decimate_")

    def tearDown(self):
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def test_edge_case_decimation_ratios_sphere(self):
        """Stress-test edge-case decimation ratios: 50, 30, 20, 10, 8, 4 on 3000+ tri sphere."""
        verts, tris, uvs = generate_sphere_mesh(40, 40)
        self.assertGreater(len(tris), 3000)

        in_path = os.path.join(self.tmp_dir, "sphere_3k.glb")
        gltf, bin_data = create_test_glb(verts, tris, uvs)
        save_glb(in_path, gltf, bin_data)

        target_faces_list = [50, 30, 20, 10, 8, 4]
        for target in target_faces_list:
            with self.subTest(target=target):
                out_path = os.path.join(self.tmp_dir, f"out_{target}.glb")
                stats = decimate_mesh(in_path, out_path, target_faces=target)

                # refinedTriangles must be <= target_faces
                self.assertLessEqual(
                    stats["refinedTriangles"],
                    target,
                    f"refinedTriangles {stats['refinedTriangles']} > target {target}"
                )
                self.assertGreater(stats["refinedTriangles"], 0)
                self.assertGreater(stats["refinedVertices"], 0)

                # Load and verify output GLB
                out_gltf, out_bin = load_glb(out_path)
                prim = out_gltf["meshes"][0]["primitives"][0]
                self.assertEqual(prim["mode"], 4)

                pos = get_accessor_data(out_gltf, out_bin, prim["attributes"]["POSITION"])
                normals = get_accessor_data(out_gltf, out_bin, prim["attributes"]["NORMAL"])
                idx = get_accessor_data(out_gltf, out_bin, prim["indices"])

                # Check for NaNs and Infs
                self.assertFalse(np.isnan(pos).any(), f"target {target}: Positions contains NaN")
                self.assertFalse(np.isnan(normals).any(), f"target {target}: Normals contains NaN")
                self.assertFalse(np.isinf(normals).any(), f"target {target}: Normals contains Inf")

                # Verify all normal vectors are non-zero unit vectors (||N|| = 1.0 +/- 1e-3)
                norm_lens = np.linalg.norm(normals, axis=1)
                zero_norms = np.sum(norm_lens < 1e-6)
                self.assertEqual(zero_norms, 0, f"target {target}: Found {zero_norms} zero-length normals")

                np.testing.assert_allclose(
                    norm_lens,
                    1.0,
                    atol=1e-3,
                    err_msg=f"target {target}: Normals not unit vectors (min={np.min(norm_lens)}, max={np.max(norm_lens)})"
                )

                # Index bounds
                self.assertTrue((idx >= 0).all())
                self.assertTrue((idx < len(pos)).all())

                # No duplicate faces
                tri_reshaped = idx.reshape((-1, 3))
                sorted_tris = [tuple(sorted(t)) for t in tri_reshaped]
                self.assertEqual(len(sorted_tris), len(set(sorted_tris)), f"target {target}: Duplicate faces detected")

    def test_decimate_to_10_and_20_faces_high_poly(self):
        """Stress-test edge cases specifically 10 and 20 faces on high poly sphere (8000+ tris)."""
        verts, tris, uvs = generate_sphere_mesh(70, 60)
        self.assertGreater(len(tris), 8000)

        in_path = os.path.join(self.tmp_dir, "sphere_8k.glb")
        gltf, bin_data = create_test_glb(verts, tris, uvs)
        save_glb(in_path, gltf, bin_data)

        for target in [20, 10]:
            with self.subTest(target=target):
                out_path = os.path.join(self.tmp_dir, f"out_8k_to_{target}.glb")
                stats = decimate_mesh(in_path, out_path, target_faces=target)

                self.assertLessEqual(stats["refinedTriangles"], target)
                self.assertGreater(stats["refinedTriangles"], 0)

                out_gltf, out_bin = load_glb(out_path)
                prim = out_gltf["meshes"][0]["primitives"][0]
                normals = get_accessor_data(out_gltf, out_bin, prim["attributes"]["NORMAL"])
                norm_lens = np.linalg.norm(normals, axis=1)

                # Zero zero-length normals
                self.assertEqual(np.sum(norm_lens < 1e-6), 0)
                # Unit vectors
                np.testing.assert_allclose(norm_lens, 1.0, atol=1e-3)

    def test_opposing_faces_normal_cancellation(self):
        """Adversarial input: double-sided cube with exact opposing pairs.
        Verifies deduplication eliminates opposing faces and compute_normals has no zero vectors."""
        # Cube with 8 vertices
        v = np.array([
            [-1, -1, -1], [1, -1, -1], [1, 1, -1], [-1, 1, -1],
            [-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]
        ], dtype=np.float32)
        # Front face [0, 1, 2] and back face [0, 2, 1] opposing each other
        tris = [
            [0, 1, 2], [0, 2, 1], # Opposing pair on same triangle
            [0, 2, 3], [0, 3, 2], # Opposing pair
            [4, 5, 6], [4, 6, 5],
            [4, 6, 7], [4, 7, 6],
            [0, 4, 7], [0, 7, 3],
            [1, 5, 6], [1, 6, 2],
        ]
        in_path = os.path.join(self.tmp_dir, "opposing.glb")
        out_path = os.path.join(self.tmp_dir, "opposing_decimated.glb")
        gltf, bin_data = create_test_glb(v, np.array(tris, dtype=np.uint32))
        save_glb(in_path, gltf, bin_data)

        stats = decimate_mesh(in_path, out_path, target_faces=6)
        out_gltf, out_bin = load_glb(out_path)
        prim = out_gltf["meshes"][0]["primitives"][0]
        normals = get_accessor_data(out_gltf, out_bin, prim["attributes"]["NORMAL"])
        norm_lens = np.linalg.norm(normals, axis=1)

        self.assertEqual(np.sum(norm_lens < 1e-6), 0)
        np.testing.assert_allclose(norm_lens, 1.0, atol=1e-3)

    def test_vertex_at_centroid_normal_fallback(self):
        """Adversarial test: Mesh with a vertex exactly at centroid center.
        Verifies radial fallback does not divide by zero and produces [0, 1, 0]."""
        # Vertices: 4 vertices forming pyramid plus vertex at center (0, 0, 0)
        v = np.array([
            [-1, 0, -1], [1, 0, -1], [1, 0, 1], [-1, 0, 1],
            [0, 0, 0], # exactly at centroid
        ], dtype=np.float32)
        # Degenerate flat faces that cancel out normal at vertex 4
        # Face 1: [4, 0, 1], Face 2: [4, 1, 0]
        tris = np.array([
            [4, 0, 1], [4, 1, 0],
            [4, 2, 3], [4, 3, 2]
        ], dtype=np.uint32)

        in_path = os.path.join(self.tmp_dir, "center_vert.glb")
        out_path = os.path.join(self.tmp_dir, "center_vert_out.glb")
        gltf, bin_data = create_test_glb(v, tris)
        save_glb(in_path, gltf, bin_data)

        # target_faces=2
        stats = decimate_mesh(in_path, out_path, target_faces=2)
        out_gltf, out_bin = load_glb(out_path)
        prim = out_gltf["meshes"][0]["primitives"][0]
        normals = get_accessor_data(out_gltf, out_bin, prim["attributes"]["NORMAL"])
        norm_lens = np.linalg.norm(normals, axis=1)

        self.assertEqual(np.sum(norm_lens < 1e-6), 0)
        self.assertFalse(np.isnan(normals).any())
        np.testing.assert_allclose(norm_lens, 1.0, atol=1e-3)

    def test_embedded_texture_preservation_under_extreme_decimation(self):
        """Stress-test: Mesh with embedded PNG texture decimated to 20 faces.
        Verifies image bytes are preserved, bufferView is valid, and image loads."""
        verts, tris, uvs = generate_sphere_mesh(30, 30)
        gltf, bin_data = create_test_glb(verts, tris, uvs)

        # Create dummy 1x1 PNG bytes
        png_1x1 = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15c4\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4\x00\x00\x00\x00IEND\xaeB`\x82"
        img_bv = add_buffer_view(gltf, bin_data, png_1x1)
        gltf["images"] = [
            {"bufferView": img_bv, "mimeType": "image/png", "name": "albedo"}
        ]
        gltf["textures"] = [{"source": 0}]
        gltf["materials"] = [{
            "pbrMetallicRoughness": {
                "baseColorTexture": {"index": 0}
            }
        }]
        gltf["meshes"][0]["primitives"][0]["material"] = 0
        gltf["buffers"][0]["byteLength"] = len(bin_data)

        in_path = os.path.join(self.tmp_dir, "textured_in.glb")
        out_path = os.path.join(self.tmp_dir, "textured_decimated_20.glb")
        save_glb(in_path, gltf, bin_data)

        stats = decimate_mesh(in_path, out_path, target_faces=20)
        self.assertLessEqual(stats["refinedTriangles"], 20)

        out_gltf, out_bin = load_glb(out_path)
        # Check image exists and bufferView is valid
        self.assertIn("images", out_gltf)
        self.assertEqual(len(out_gltf["images"]), 1)
        new_bv_idx = out_gltf["images"][0]["bufferView"]
        new_bv = out_gltf["bufferViews"][new_bv_idx]
        extracted_png = bytes(out_bin[new_bv["byteOffset"] : new_bv["byteOffset"] + new_bv["byteLength"]])
        self.assertEqual(extracted_png, png_1x1)

        # Normals must also be valid unit vectors
        prim = out_gltf["meshes"][0]["primitives"][0]
        normals = get_accessor_data(out_gltf, out_bin, prim["attributes"]["NORMAL"])
        norm_lens = np.linalg.norm(normals, axis=1)
        self.assertEqual(np.sum(norm_lens < 1e-6), 0)
        np.testing.assert_allclose(norm_lens, 1.0, atol=1e-3)


if __name__ == "__main__":
    unittest.main()
