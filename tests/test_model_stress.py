#!/usr/bin/env python3
"""
tests/test_model_stress.py
Empirical adversarial stress test harness for Milestone 1:
- tools/model/decimate_mesh.py
- tools/model/auto_rig.py
- tools/model/merge_motion.py
"""

import json
import os
import shutil
import struct
import sys
import tempfile
import unittest
import numpy as np

# Ensure project root is in sys.path
PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if PROJECT_ROOT not in sys.path:
    sys.path.insert(0, PROJECT_ROOT)

from tools.model.glb_utils import (
    load_glb,
    save_glb,
    get_accessor_data,
    add_accessor,
)
from tools.model.decimate_mesh import decimate_mesh
from tools.model.auto_rig import auto_rig, HUMANOID_BONES
from tools.model.merge_motion import merge_motion


def create_test_glb(positions, triangles, uvs=None, normals=None):
    """Creates a basic GLB buffer from numpy arrays."""
    gltf = {
        "asset": {"version": "2.0", "generator": "StressHarness"},
        "scenes": [{"nodes": [0]}],
        "scene": 0,
        "nodes": [{"mesh": 0}],
        "meshes": [
            {
                "name": "TestMesh",
                "primitives": [
                    {
                        "attributes": {},
                        "indices": 0,
                        "mode": 4,  # TRIANGLES
                    }
                ],
            }
        ],
        "bufferViews": [],
        "accessors": [],
        "buffers": [{"byteLength": 0}],
    }
    bin_data = bytearray()

    # Indices
    flat_indices = triangles.flatten().astype(np.uint32)
    idx_acc = add_accessor(gltf, bin_data, flat_indices, "SCALAR", target=34963)
    gltf["meshes"][0]["primitives"][0]["indices"] = idx_acc

    # Position
    pos_acc = add_accessor(gltf, bin_data, positions.astype(np.float32), "VEC3", target=34962)
    gltf["meshes"][0]["primitives"][0]["attributes"]["POSITION"] = pos_acc

    # Normal
    if normals is not None:
        norm_acc = add_accessor(gltf, bin_data, normals.astype(np.float32), "VEC3", target=34962)
        gltf["meshes"][0]["primitives"][0]["attributes"]["NORMAL"] = norm_acc

    # UV
    if uvs is not None:
        uv_acc = add_accessor(gltf, bin_data, uvs.astype(np.float32), "VEC2", target=34962)
        gltf["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = uv_acc

    gltf["buffers"][0]["byteLength"] = len(bin_data)
    return gltf, bin_data


def generate_sphere_mesh(u_res=30, v_res=30, radius=0.8, center=(0.0, 0.9, 0.0)):
    """Generates a UV sphere mesh."""
    u = np.linspace(0, 2 * np.pi, u_res)
    v = np.linspace(0, np.pi, v_res)
    verts = []
    uvs = []
    for vi in range(v_res):
        for ui in range(u_res):
            theta = u[ui]
            phi = v[vi]
            x = center[0] + radius * np.sin(phi) * np.cos(theta)
            y = center[1] + radius * np.cos(phi)
            z = center[2] + radius * np.sin(phi) * np.sin(theta)
            verts.append([x, y, z])
            uvs.append([ui / (u_res - 1), vi / (v_res - 1)])

    triangles = []
    for vi in range(v_res - 1):
        for ui in range(u_res - 1):
            p0 = vi * u_res + ui
            p1 = p0 + 1
            p2 = (vi + 1) * u_res + ui
            p3 = p2 + 1
            triangles.append([p0, p2, p1])
            triangles.append([p1, p2, p3])

    return (
        np.array(verts, dtype=np.float32),
        np.array(triangles, dtype=np.uint32),
        np.array(uvs, dtype=np.float32),
    )


class TestDecimateMeshStress(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp(prefix="test_decimate_")

    def tearDown(self):
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def test_decimate_extreme_50_faces(self):
        """Stress test: 3400+ triangles decimated down to <= 50 target faces."""
        verts, tris, uvs = generate_sphere_mesh(40, 40)
        self.assertGreater(len(tris), 3000)

        input_path = os.path.join(self.tmp_dir, "sphere_3k.glb")
        output_path = os.path.join(self.tmp_dir, "sphere_50.glb")
        gltf, bin_data = create_test_glb(verts, tris, uvs)
        save_glb(input_path, gltf, bin_data)

        stats = decimate_mesh(input_path, output_path, target_faces=50)

        self.assertLessEqual(stats["refinedTriangles"], 50)
        self.assertGreater(stats["refinedTriangles"], 4)

        # Inspect output GLB
        out_gltf, out_bin = load_glb(output_path)
        mesh = out_gltf["meshes"][0]
        prim = mesh["primitives"][0]
        self.assertEqual(prim["mode"], 4)  # TRIANGLES

        pos = get_accessor_data(out_gltf, out_bin, prim["attributes"]["POSITION"])
        normals = get_accessor_data(out_gltf, out_bin, prim["attributes"]["NORMAL"])
        out_uvs = get_accessor_data(out_gltf, out_bin, prim["attributes"]["TEXCOORD_0"])
        idx = get_accessor_data(out_gltf, out_bin, prim["indices"])

        # Check for NaNs and Infs
        self.assertFalse(np.isnan(pos).any(), "Positions contains NaN")
        self.assertFalse(np.isnan(normals).any(), "Normals contains NaN")
        self.assertFalse(np.isnan(out_uvs).any(), "UVs contains NaN")

        # Normals should be unit vectors
        norm_lens = np.linalg.norm(normals, axis=1)
        np.testing.assert_allclose(norm_lens, 1.0, atol=1e-3)

        # Indices in valid range
        self.assertTrue((idx >= 0).all())
        self.assertTrue((idx < len(pos)).all())

    def test_decimate_5000_faces_from_high_poly(self):
        """Stress test: 8000+ triangles decimated down to <= 5000 target faces."""
        verts, tris, uvs = generate_sphere_mesh(70, 60)
        input_path = os.path.join(self.tmp_dir, "sphere_8k.glb")
        output_path = os.path.join(self.tmp_dir, "sphere_5k.glb")
        gltf, bin_data = create_test_glb(verts, tris, uvs)
        save_glb(input_path, gltf, bin_data)

        stats = decimate_mesh(input_path, output_path, target_faces=5000)
        self.assertLessEqual(stats["refinedTriangles"], 5000)
        self.assertGreater(stats["refinedTriangles"], 1000)

    def test_decimate_noop_when_under_budget(self):
        """Target faces exceeds triangle count: face count unchanged."""
        verts, tris, uvs = generate_sphere_mesh(10, 10)
        orig_count = len(tris)
        input_path = os.path.join(self.tmp_dir, "sphere_small.glb")
        output_path = os.path.join(self.tmp_dir, "sphere_small_out.glb")
        gltf, bin_data = create_test_glb(verts, tris, uvs)
        save_glb(input_path, gltf, bin_data)

        stats = decimate_mesh(input_path, output_path, target_faces=5000)
        self.assertEqual(stats["refinedTriangles"], orig_count)

    def test_decimate_degenerate_flat_mesh(self):
        """Completely flat 2D mesh (Z=0) with zero thickness."""
        x = np.linspace(-1, 1, 15)
        y = np.linspace(-1, 1, 15)
        verts = []
        for yi in y:
            for xi in x:
                verts.append([xi, yi, 0.0])  # strictly flat
        triangles = []
        for j in range(14):
            for i in range(14):
                p0 = j * 15 + i
                p1 = p0 + 1
                p2 = (j + 1) * 15 + i
                p3 = p2 + 1
                triangles.append([p0, p2, p1])
                triangles.append([p1, p2, p3])

        input_path = os.path.join(self.tmp_dir, "flat_plane.glb")
        output_path = os.path.join(self.tmp_dir, "flat_decimated.glb")
        gltf, bin_data = create_test_glb(np.array(verts, dtype=np.float32), np.array(triangles, dtype=np.uint32))
        save_glb(input_path, gltf, bin_data)

        stats = decimate_mesh(input_path, output_path, target_faces=60)
        self.assertLessEqual(stats["refinedTriangles"], 60)

        # Normals must still be valid
        out_gltf, out_bin = load_glb(output_path)
        prim = out_gltf["meshes"][0]["primitives"][0]
        normals = get_accessor_data(out_gltf, out_bin, prim["attributes"]["NORMAL"])
        self.assertFalse(np.isnan(normals).any())

    def test_decimate_mesh_without_uvs(self):
        """Mesh without TEXCOORD_0 generates valid planar UVs upon decimation."""
        verts, tris, _ = generate_sphere_mesh(20, 20)
        input_path = os.path.join(self.tmp_dir, "sphere_no_uv.glb")
        output_path = os.path.join(self.tmp_dir, "sphere_with_uv.glb")
        gltf, bin_data = create_test_glb(verts, tris, uvs=None)
        save_glb(input_path, gltf, bin_data)

        stats = decimate_mesh(input_path, output_path, target_faces=100)
        out_gltf, out_bin = load_glb(output_path)
        prim = out_gltf["meshes"][0]["primitives"][0]
        self.assertIn("TEXCOORD_0", prim["attributes"])
        uvs = get_accessor_data(out_gltf, out_bin, prim["attributes"]["TEXCOORD_0"])
        self.assertEqual(len(uvs), stats["refinedVertices"])
        self.assertFalse(np.isnan(uvs).any())


class TestAutoRigStress(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp(prefix="test_rig_")

    def tearDown(self):
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def test_auto_rig_strict_weight_normalization_and_influences(self):
        """Verify sum(w_i) == 1.0, max 4 joints, non-negative, and IBM inversion."""
        verts, tris, uvs = generate_sphere_mesh(35, 35, radius=0.9, center=(0.0, 1.0, 0.0))
        input_path = os.path.join(self.tmp_dir, "character.glb")
        output_path = os.path.join(self.tmp_dir, "character_rigged.glb")
        gltf, bin_data = create_test_glb(verts, tris, uvs)
        save_glb(input_path, gltf, bin_data)

        stats = auto_rig(input_path, output_path, target_height=1.8)
        self.assertEqual(stats["jointsCount"], 19)
        self.assertTrue(stats["normalizedWeights"])

        out_gltf, out_bin = load_glb(output_path)
        prim = out_gltf["meshes"][0]["primitives"][0]

        # Verify attributes exist
        self.assertIn("JOINTS_0", prim["attributes"])
        self.assertIn("WEIGHTS_0", prim["attributes"])

        joints = get_accessor_data(out_gltf, out_bin, prim["attributes"]["JOINTS_0"])
        weights = get_accessor_data(out_gltf, out_bin, prim["attributes"]["WEIGHTS_0"])

        # 1. Exactly 4 joints per vertex
        self.assertEqual(joints.shape[1], 4)
        self.assertEqual(weights.shape[1], 4)

        # 2. Joint indices valid range [0, 18]
        self.assertTrue((joints >= 0).all())
        self.assertTrue((joints < 19).all())

        # 3. Weights strictly non-negative
        self.assertTrue((weights >= 0.0).all(), "Found negative weights")
        self.assertFalse(np.isnan(weights).any(), "Found NaN in weights")
        self.assertFalse(np.isinf(weights).any(), "Found Inf in weights")

        # 4. Weights strictly sum to 1.0 for every single vertex
        sums = np.sum(weights, axis=1)
        np.testing.assert_allclose(sums, 1.0, atol=1e-5, err_msg="Vertex weights do not sum to 1.0")

        # 5. Inverse Bind Matrices strictly invert joint world translations
        skin = out_gltf["skins"][0]
        self.assertEqual(len(skin["joints"]), 19)
        ibm_acc = skin["inverseBindMatrices"]
        ibms = get_accessor_data(out_gltf, out_bin, ibm_acc)
        # MAT4 accessors in glTF are column-major (16 floats per joint)
        self.assertEqual(ibms.shape, (19, 16))

        # Reconstruct world positions
        nodes = out_gltf["nodes"]
        for j_idx, node_idx in enumerate(skin["joints"]):
            ibm_mat = ibms[j_idx].reshape((4, 4), order="F")  # column-major
            det = np.linalg.det(ibm_mat)
            self.assertAlmostEqual(det, 1.0, delta=1e-4, msg=f"IBM determinant is not 1.0: {det}")

            # Check that IBM * [x, y, z, 1]^T == [0, 0, 0, 1]^T for joint's world position
            # Find world position by traversing parents
            curr = node_idx
            world_t = np.array([0.0, 0.0, 0.0], dtype=np.float32)
            while curr is not None:
                n = nodes[curr]
                world_t += np.array(n.get("translation", [0.0, 0.0, 0.0]), dtype=np.float32)
                # Find parent node
                parent = None
                for p_idx, p_node in enumerate(nodes):
                    if curr in p_node.get("children", []):
                        parent = p_idx
                        break
                curr = parent

            joint_vec = np.array([world_t[0], world_t[1], world_t[2], 1.0], dtype=np.float32)
            transformed = ibm_mat @ joint_vec
            np.testing.assert_allclose(
                transformed,
                np.array([0.0, 0.0, 0.0, 1.0]),
                atol=1e-4,
                err_msg=f"IBM did not invert joint {j_idx} world position",
            )

    def test_auto_rig_extreme_scale_and_positions(self):
        """Microscopic character (height 0.02) and giant titan (height 500.0)."""
        verts, tris, _ = generate_sphere_mesh(15, 15, radius=0.01, center=(0.0, 0.01, 0.0))
        input_path = os.path.join(self.tmp_dir, "tiny.glb")
        output_path = os.path.join(self.tmp_dir, "tiny_rigged.glb")
        gltf, bin_data = create_test_glb(verts, tris)
        save_glb(input_path, gltf, bin_data)

        # Tiny scale
        stats = auto_rig(input_path, output_path, target_height=0.02)
        out_gltf, out_bin = load_glb(output_path)
        prim = out_gltf["meshes"][0]["primitives"][0]
        weights = get_accessor_data(out_gltf, out_bin, prim["attributes"]["WEIGHTS_0"])
        np.testing.assert_allclose(np.sum(weights, axis=1), 1.0, atol=1e-5)

        # Giant scale
        giant_path = os.path.join(self.tmp_dir, "giant_rigged.glb")
        stats_giant = auto_rig(input_path, giant_path, target_height=500.0)
        out_gltf_g, out_bin_g = load_glb(giant_path)
        prim_g = out_gltf_g["meshes"][0]["primitives"][0]
        weights_g = get_accessor_data(out_gltf_g, out_bin_g, prim_g["attributes"]["WEIGHTS_0"])
        np.testing.assert_allclose(np.sum(weights_g, axis=1), 1.0, atol=1e-5)


class TestMergeMotionStress(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp(prefix="test_motion_")
        # Generate a base rigged character
        verts, tris, _ = generate_sphere_mesh(15, 15)
        raw_char = os.path.join(self.tmp_dir, "char_raw.glb")
        self.char_rigged = os.path.join(self.tmp_dir, "char_rigged.glb")
        gltf, bin_data = create_test_glb(verts, tris)
        save_glb(raw_char, gltf, bin_data)
        auto_rig(raw_char, self.char_rigged, target_height=1.8)

    def tearDown(self):
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def _create_motion_clip(self, filename, clip_name, timestamps, bone_name="Hips", path="translation"):
        """Helper to create a single animation GLB clip."""
        gltf = {
            "asset": {"version": "2.0"},
            "scenes": [{"nodes": [0]}],
            "scene": 0,
            "nodes": [{"name": bone_name}],
            "animations": [],
            "bufferViews": [],
            "accessors": [],
            "buffers": [{"byteLength": 0}],
        }
        bin_data = bytearray()

        time_arr = np.array(timestamps, dtype=np.float32)
        time_acc = add_accessor(gltf, bin_data, time_arr, "SCALAR")

        # Values
        val_arr = np.zeros((len(timestamps), 3), dtype=np.float32)
        val_acc = add_accessor(gltf, bin_data, val_arr, "VEC3")

        gltf["animations"].append({
            "name": clip_name,
            "samplers": [
                {
                    "input": time_acc,
                    "interpolation": "LINEAR",
                    "output": val_acc,
                }
            ],
            "channels": [
                {
                    "sampler": 0,
                    "target": {
                        "node": 0,
                        "path": path,
                    },
                }
            ],
        })
        gltf["buffers"][0]["byteLength"] = len(bin_data)
        out_file = os.path.join(self.tmp_dir, filename)
        save_glb(out_file, gltf, bin_data)
        return out_file

    def test_merge_motion_monotonicity_repair(self):
        """Clips with non-monotonic or duplicate timestamps are repaired to strictly monotonic."""
        bad_times = [0.0, 0.0, 0.5, 0.4, 0.8, 0.8, 1.0]
        clip_path = self._create_motion_clip("bad_motion.glb", "BadMotion", bad_times)

        output_path = os.path.join(self.tmp_dir, "char_animated.glb")
        stats = merge_motion(self.char_rigged, clip_path, output_path)
        self.assertIn("BadMotion", stats["animations"])

        out_gltf, out_bin = load_glb(output_path)
        self.assertEqual(len(out_gltf["animations"]), 1)
        anim = out_gltf["animations"][0]
        sampler = anim["samplers"][0]
        repaired_times = get_accessor_data(out_gltf, out_bin, sampler["input"])

        diffs = np.diff(repaired_times)
        self.assertTrue((diffs > 0).all(), f"Repaired timestamps not strictly monotonic: {repaired_times}")

    def test_merge_standard_humanoid_motion_pack(self):
        """Verify merging all humanoid motion pack assets (idle, walk, run, attack)."""
        pack_dir = os.path.join(PROJECT_ROOT, "assets", "motion-packs", "humanoid")
        self.assertTrue(os.path.isdir(pack_dir), "Motion pack dir missing")

        output_path = os.path.join(self.tmp_dir, "char_full_pack.glb")
        stats = merge_motion(self.char_rigged, pack_dir, output_path)

        expected = ["Idle", "Walk", "Run", "Attack"]
        for exp in expected:
            self.assertIn(exp, stats["animations"])

        out_gltf, out_bin = load_glb(output_path)
        self.assertEqual(len(out_gltf["animations"]), 4)

        # Verify each merged animation has valid channels and strictly increasing times
        for anim in out_gltf["animations"]:
            self.assertGreater(len(anim["channels"]), 0)
            for sampler in anim["samplers"]:
                times = get_accessor_data(out_gltf, out_bin, sampler["input"])
                self.assertGreater(len(times), 1)
                self.assertTrue((np.diff(times) > 0).all(), f"Animation {anim['name']} has non-monotonic times")

    def test_merge_motion_prefix_and_synonym_normalization(self):
        """Verify bones with 'mixamorig:Hips' or 'pelvis' or 'chest' map to humanoid skeleton."""
        clip1 = self._create_motion_clip("mixamo_hips.glb", "MixamoClip", [0.0, 0.5, 1.0], bone_name="mixamorig:Hips")
        clip2 = self._create_motion_clip("pelvis_clip.glb", "PelvisClip", [0.0, 0.5, 1.0], bone_name="pelvis")
        clip3 = self._create_motion_clip("chest_clip.glb", "ChestClip", [0.0, 0.5, 1.0], bone_name="chest")

        motion_dir = os.path.join(self.tmp_dir, "custom_clips")
        os.makedirs(motion_dir, exist_ok=True)
        shutil.copy(clip1, motion_dir)
        shutil.copy(clip2, motion_dir)
        shutil.copy(clip3, motion_dir)

        output_path = os.path.join(self.tmp_dir, "char_synonyms.glb")
        stats = merge_motion(self.char_rigged, motion_dir, output_path)

        out_gltf, out_bin = load_glb(output_path)
        self.assertEqual(len(out_gltf["animations"]), 3)
        for anim in out_gltf["animations"]:
            for ch in anim["channels"]:
                target_node = ch["target"]["node"]
                target_name = out_gltf["nodes"][target_node]["name"]
                self.assertIn(target_name, ["Hips", "Spine1"])


if __name__ == "__main__":
    unittest.main()
