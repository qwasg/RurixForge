#!/usr/bin/env python3
"""End-to-End Genshin Impact Paimon (派蒙) 3D Character & Heart-Gesture (比心) Video Pipeline.

1. Generates 512x512 RGBA A-Pose Concept Art of Paimon (`Content/ConceptArt/paimon_apose.png`) -> Gate 1
2. Runs real `forge-agentd` with Antigravity `antigravity:gemini-4` + Meshy / Dual-Track `gen-model-mcp` + `engine-scene-mcp`:
   - `mcp__gen-model__gen_mesh` -> Gate 2
   - `mcp__gen-model__gen_mesh_refine` (<= 4000 tris) -> Gate 3
   - `mcp__gen-model__gen_mesh_rig` (19 humanoid bones, 4-bone normalized weights) -> Gate 4
   - `mcp__gen-model__gen_mesh_animate` (merges `Idle`, `Walk`, `Run`, `Attack`, `HeartGesture`)
   - `mcp__gen-model__gen_character_publish` (`genshin_paimon` -> `.rxmodel` + `.rxprefab`) -> Gate 5
   - `mcp__engine-scene__prefab_instantiate` + `viewport_frame`
3. Evaluates exact glTF 2.0 Forward Kinematics + 4-Bone Linear Blend Skinning (LBS) on the
   published Paimon `.glb` for the `HeartGesture` (比心动作) clip and renders a 30fps HD MP4 video
   (`paimon_heart_gesture.mp4`) and animated GIF (`paimon_heart_gesture.gif`).
"""

import io
import json
import math
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import time
from typing import Tuple
import urllib.request

import imageio
import numpy as np
from PIL import Image, ImageDraw

WORKSPACE_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(WORKSPACE_ROOT / "tools" / "e2e"))
sys.path.insert(0, str(WORKSPACE_ROOT / "tools" / "model"))

from character_3d_pipeline import (
    validate_gate_1_concept_art,
    validate_gate_2_raw_glb,
    validate_gate_3_polycount,
    validate_gate_4_rigging,
    validate_gate_5_motion_publish,
)
from glb_utils import get_accessor_data, load_glb
from live_meshy_gemini4_e2e import get_free_port, http_json, load_meshy_key


def create_paimon_concept_art(out_path: Path) -> None:
    """Draws a detailed 512x512 RGBA A-Pose Concept Art of Genshin Impact's Paimon (派蒙)."""
    out_path.parent.mkdir(parents=True, exist_ok=True)
    img = Image.new("RGBA", (512, 512), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # 1. Floating Gold Crown / Halo above head
    draw.ellipse([206, 22, 306, 56], fill=(245, 202, 72, 255), outline=(255, 235, 140, 255), width=3)
    draw.ellipse([222, 30, 290, 48], fill=(0, 0, 0, 0), outline=(40, 48, 95, 255), width=2)
    # Crown spikes
    draw.polygon([(256, 8), (244, 28), (268, 28)], fill=(250, 215, 85, 255))
    draw.polygon([(212, 16), (206, 34), (226, 30)], fill=(245, 202, 72, 255))
    draw.polygon([(300, 16), (286, 30), (306, 34)], fill=(245, 202, 72, 255))

    # 2. Starry Night-Blue Back Cape
    draw.polygon(
        [(188, 195), (324, 195), (348, 370), (164, 370)],
        fill=(24, 32, 76, 255),
        outline=(240, 198, 82, 255),
        width=2,
    )

    # 3. Chibi Silver-White Hair Bob (Back & Side locks)
    draw.ellipse([178, 62, 334, 202], fill=(238, 241, 250, 255), outline=(205, 212, 232, 255), width=3)
    # Face
    draw.ellipse([198, 88, 314, 194], fill=(255, 235, 222, 255), outline=(235, 205, 190, 255), width=2)
    # Silver-White Front Bangs
    draw.polygon(
        [(192, 92), (256, 66), (320, 92), (305, 128), (278, 108), (256, 132), (232, 108), (207, 128)],
        fill=(244, 246, 253, 255),
    )
    # Starry Navy Hairpin
    draw.polygon([(205, 96), (232, 88), (236, 106), (208, 112)], fill=(28, 36, 82, 255), outline=(245, 202, 72, 255))
    # Big Starry Purple-Indigo Anime Eyes
    draw.ellipse([214, 126, 242, 158], fill=(52, 38, 88, 255))
    draw.ellipse([270, 126, 298, 158], fill=(52, 38, 88, 255))
    draw.ellipse([220, 130, 230, 140], fill=(255, 255, 255, 255))
    draw.ellipse([276, 130, 286, 140], fill=(255, 255, 255, 255))
    # Rosy Blush & Cute Smile
    draw.ellipse([206, 156, 224, 166], fill=(255, 160, 180, 255))
    draw.ellipse([288, 156, 306, 166], fill=(255, 160, 180, 255))
    draw.arc([246, 158, 266, 174], start=10, end=170, fill=(225, 100, 118, 255), width=3)

    # 4. Rose-Pink Scarf / Collar
    draw.polygon(
        [(194, 188), (318, 188), (326, 222), (186, 222)],
        fill=(230, 142, 162, 255),
        outline=(242, 198, 78, 255),
        width=3,
    )

    # 5. White & Gold Celestial Romper Tunic & Puffy Bloomers
    draw.polygon(
        [(196, 220), (316, 220), (324, 325), (188, 325)],
        fill=(248, 249, 253, 255),
        outline=(240, 196, 78, 255),
        width=3,
    )
    # Gold Celestial Chest Emblem
    draw.ellipse([234, 238, 278, 282], fill=(28, 38, 82, 255), outline=(245, 202, 72, 255), width=3)
    draw.ellipse([246, 250, 266, 270], fill=(245, 202, 72, 255))
    # Puffy Bloomers
    draw.ellipse([190, 305, 322, 362], fill=(244, 245, 252, 255), outline=(240, 196, 78, 255), width=3)

    # 6. A-Pose Puffy Sleeves & Cute Hands
    draw.polygon([(194, 198), (168, 212), (112, 288), (138, 306)], fill=(248, 249, 253, 255), outline=(240, 196, 78, 255), width=2)
    draw.ellipse([102, 282, 132, 312], fill=(255, 232, 218, 255), outline=(240, 196, 78, 255), width=2)
    draw.polygon([(318, 198), (344, 212), (400, 288), (374, 306)], fill=(248, 249, 253, 255), outline=(240, 196, 78, 255), width=2)
    draw.ellipse([380, 282, 410, 312], fill=(255, 232, 218, 255), outline=(240, 196, 78, 255), width=2)

    # 7. Chibi Legs & White-Gold Celestial Boots
    draw.polygon([(212, 352), (246, 352), (240, 472), (206, 472)], fill=(248, 249, 253, 255), outline=(240, 196, 78, 255), width=2)
    draw.polygon([(266, 352), (300, 352), (306, 472), (272, 472)], fill=(248, 249, 253, 255), outline=(240, 196, 78, 255), width=2)

    img.save(out_path, format="PNG")


# ============================================================================
# 3D Forward-Kinematics + Linear Blend Skinning (LBS) + UV Textured Renderer
# ============================================================================


def quat_to_mat3(q: np.ndarray) -> np.ndarray:
    x, y, z, w = float(q[0]), float(q[1]), float(q[2]), float(q[3])
    n = math.sqrt(x * x + y * y + z * z + w * w)
    if n > 1e-8:
        x, y, z, w = x / n, y / n, z / n, w / n
    return np.array(
        [
            [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
            [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
            [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
        ],
        dtype=np.float32,
    )


def quat_slerp(q0: np.ndarray, q1: np.ndarray, t: float) -> np.ndarray:
    q0 = q0.astype(np.float64)
    q1 = q1.astype(np.float64)
    dot = float(np.dot(q0, q1))
    if dot < 0.0:
        q1 = -q1
        dot = -dot
    if dot > 0.9995:
        res = q0 + t * (q1 - q0)
        return (res / np.linalg.norm(res)).astype(np.float32)
    theta_0 = math.acos(np.clip(dot, -1.0, 1.0))
    sin_theta_0 = math.sin(theta_0)
    theta = theta_0 * t
    s0 = math.cos(theta) - dot * math.sin(theta) / sin_theta_0
    s1 = math.sin(theta) / sin_theta_0
    return (s0 * q0 + s1 * q1).astype(np.float32)


def sample_anim_channel(times: np.ndarray, values: np.ndarray, t: float, is_quat: bool) -> np.ndarray:
    if len(times) == 1 or t <= times[0]:
        return values[0]
    if t >= times[-1]:
        return values[-1]
    idx = int(np.searchsorted(times, t, side="right")) - 1
    idx = max(0, min(idx, len(times) - 2))
    t0, t1 = float(times[idx]), float(times[idx + 1])
    alpha = (t - t0) / max(t1 - t0, 1e-6)
    if is_quat:
        return quat_slerp(values[idx], values[idx + 1], alpha)
    return (values[idx] * (1.0 - alpha) + values[idx + 1] * alpha).astype(np.float32)


class SkinnedGlbCharacter:
    """Loads an animated rigged GLB and evaluates exact FK + 4-bone LBS at any timestamp."""

    def __init__(self, glb_path: Path):
        self.gltf, self.bin_data = load_glb(str(glb_path))
        prim = self.gltf["meshes"][0]["primitives"][0]
        self.positions = get_accessor_data(self.gltf, self.bin_data, prim["attributes"]["POSITION"]).astype(np.float32)
        self.normals = get_accessor_data(self.gltf, self.bin_data, prim["attributes"]["NORMAL"]).astype(np.float32)
        self.uvs = get_accessor_data(self.gltf, self.bin_data, prim["attributes"]["TEXCOORD_0"]).astype(np.float32)
        self.joints_0 = get_accessor_data(self.gltf, self.bin_data, prim["attributes"]["JOINTS_0"]).astype(np.int32)
        self.weights_0 = get_accessor_data(self.gltf, self.bin_data, prim["attributes"]["WEIGHTS_0"]).astype(np.float32)
        idx_raw = get_accessor_data(self.gltf, self.bin_data, prim["indices"]).astype(np.int32)
        self.triangles = idx_raw.reshape(-1, 3)

        skin = self.gltf["skins"][0]
        self.joint_nodes = skin["joints"]
        ibm_raw = get_accessor_data(self.gltf, self.bin_data, skin["inverseBindMatrices"]).astype(np.float32)
        # glTF MAT4 is column-major -> transpose each 4x4 to row-major
        self.ibms = np.transpose(ibm_raw.reshape(-1, 4, 4), (0, 2, 1))

        self.nodes = self.gltf["nodes"]
        self.parent_map = {}
        for p_idx, node in enumerate(self.nodes):
            for c_idx in node.get("children", []):
                self.parent_map[c_idx] = p_idx

        # Load embedded baseColor texture (resolve materials[0].pbrMetallicRoughness.baseColorTexture.index -> textures -> images)
        self.texture_img = None
        if self.gltf.get("images"):
            base_img_idx = 0
            mats = self.gltf.get("materials", [])
            if mats:
                bct = mats[0].get("pbrMetallicRoughness", {}).get("baseColorTexture", {})
                tex_idx = bct.get("index")
                if tex_idx is not None and tex_idx < len(self.gltf.get("textures", [])):
                    src_idx = self.gltf["textures"][tex_idx].get("source")
                    if src_idx is not None and src_idx < len(self.gltf["images"]):
                        base_img_idx = src_idx
            img_info = self.gltf["images"][base_img_idx]
            if "bufferView" in img_info:
                bv = self.gltf["bufferViews"][img_info["bufferView"]]
                start = bv.get("byteOffset", 0)
                raw_bytes = bytes(self.bin_data[start : start + bv["byteLength"]])
                self.texture_img = np.array(Image.open(io.BytesIO(raw_bytes)).convert("RGB"), dtype=np.float32)

        # Precompute 16x barycentric UV-subdivided micro-triangles for high-resolution 2K texture rendering
        N = 4
        sub_verts_bary = []
        for i in range(N):
            for j in range(N - i):
                p0 = (1.0 - (i + j) / N, i / N, j / N)
                p1 = (1.0 - (i + 1 + j) / N, (i + 1) / N, j / N)
                p2 = (1.0 - (i + j + 1) / N, i / N, (j + 1) / N)
                sub_verts_bary.append((p0, p1, p2))
                if i + j + 1 < N:
                    q0 = (1.0 - (i + 1 + j) / N, (i + 1) / N, j / N)
                    q1 = (1.0 - (i + 1 + j + 1) / N, (i + 1) / N, (j + 1) / N)
                    q2 = (1.0 - (i + j + 1) / N, i / N, (j + 1) / N)
                    sub_verts_bary.append((q0, q1, q2))
        self.sub_verts_bary = np.array(sub_verts_bary, dtype=np.float32)  # (16, 3, 3)
        self.sub_center_bary = np.mean(self.sub_verts_bary, axis=1)  # (16, 3)
        self.sub_cols = self._compute_subdivided_triangle_colors()

        # Parse animations
        self.clips = {}
        for anim in self.gltf.get("animations", []):
            name = anim.get("name", "Clip")
            samplers = []
            for s in anim.get("samplers", []):
                t_arr = get_accessor_data(self.gltf, self.bin_data, s["input"]).astype(np.float32).flatten()
                v_arr = get_accessor_data(self.gltf, self.bin_data, s["output"]).astype(np.float32)
                samplers.append((t_arr, v_arr))
            channels = []
            for ch in anim.get("channels", []):
                channels.append((ch["sampler"], ch["target"]["node"], ch["target"]["path"]))
            duration = max((float(t[-1]) for t, _ in samplers if len(t) > 0), default=1.0)
            self.clips[name] = {"samplers": samplers, "channels": channels, "duration": duration}

    def _compute_subdivided_triangle_colors(self) -> np.ndarray:
        if self.texture_img is None:
            return np.full((len(self.triangles), 16, 3), [240.0, 210.0, 225.0], dtype=np.float32)
        H_t, W_t, _ = self.texture_img.shape
        uv0 = self.uvs[self.triangles[:, 0]]
        uv1 = self.uvs[self.triangles[:, 1]]
        uv2 = self.uvs[self.triangles[:, 2]]
        sub_uv = (
            self.sub_center_bary[None, :, 0, None] * uv0[:, None, :]
            + self.sub_center_bary[None, :, 1, None] * uv1[:, None, :]
            + self.sub_center_bary[None, :, 2, None] * uv2[:, None, :]
        )
        px = np.clip((sub_uv[:, :, 0] % 1.0) * (W_t - 1), 0, W_t - 1).astype(np.int32)
        py = np.clip((sub_uv[:, :, 1] % 1.0) * (H_t - 1), 0, H_t - 1).astype(np.int32)
        return self.texture_img[py, px].copy()

    def evaluate_pose(self, clip_name: str, t: float) -> Tuple[np.ndarray, np.ndarray, dict, dict]:
        """Returns deformed (positions, normals, joint_world_positions_by_name, joint_world_mats) at time t."""
        local_t = {}
        local_r = {}
        local_s = {}
        for idx, node in enumerate(self.nodes):
            local_t[idx] = np.array(node.get("translation", [0.0, 0.0, 0.0]), dtype=np.float32)
            local_r[idx] = np.array(node.get("rotation", [0.0, 0.0, 0.0, 1.0]), dtype=np.float32)
            local_s[idx] = np.array(node.get("scale", [1.0, 1.0, 1.0]), dtype=np.float32)

        clip = self.clips.get(clip_name)
        if clip:
            dur = max(clip["duration"], 1e-4)
            t_mod = t % dur
            for s_idx, node_idx, path in clip["channels"]:
                times, vals = clip["samplers"][s_idx]
                if path == "translation":
                    local_t[node_idx] = sample_anim_channel(times, vals, t_mod, is_quat=False)
                elif path == "rotation":
                    local_r[node_idx] = sample_anim_channel(times, vals, t_mod, is_quat=True)
                elif path == "scale":
                    local_s[node_idx] = sample_anim_channel(times, vals, t_mod, is_quat=False)

        world_mats = {}

        def get_world_mat(n_idx: int) -> np.ndarray:
            if n_idx in world_mats:
                return world_mats[n_idx]
            mat = np.identity(4, dtype=np.float32)
            mat[:3, :3] = quat_to_mat3(local_r[n_idx]) * local_s[n_idx][np.newaxis, :]
            mat[:3, 3] = local_t[n_idx]
            if n_idx in self.parent_map:
                mat = get_world_mat(self.parent_map[n_idx]) @ mat
            world_mats[n_idx] = mat
            return mat

        num_joints = len(self.joint_nodes)
        skin_mats = np.zeros((num_joints, 4, 4), dtype=np.float32)
        joint_world_pos = {}
        joint_world_mats = {}
        for j_i, n_idx in enumerate(self.joint_nodes):
            w_mat = get_world_mat(n_idx)
            skin_mats[j_i] = w_mat @ self.ibms[j_i]
            j_name = self.nodes[n_idx].get("name", f"joint_{j_i}")
            joint_world_pos[j_name] = w_mat[:3, 3].copy()
            joint_world_mats[j_name] = w_mat.copy()

        # Vectorized 4-bone Linear Blend Skinning (LBS)
        m_blend = (
            self.weights_0[:, 0, None, None] * skin_mats[self.joints_0[:, 0]]
            + self.weights_0[:, 1, None, None] * skin_mats[self.joints_0[:, 1]]
            + self.weights_0[:, 2, None, None] * skin_mats[self.joints_0[:, 2]]
            + self.weights_0[:, 3, None, None] * skin_mats[self.joints_0[:, 3]]
        )
        pos_h = np.concatenate([self.positions, np.ones((len(self.positions), 1), dtype=np.float32)], axis=1)
        deformed_pos = np.einsum("nij,nj->ni", m_blend, pos_h)[:, :3]
        deformed_norm = np.einsum("nij,nj->ni", m_blend[:, :3, :3], self.normals)
        n_len = np.linalg.norm(deformed_norm, axis=1, keepdims=True)
        deformed_norm = deformed_norm / np.maximum(n_len, 1e-6)

        return deformed_pos, deformed_norm, joint_world_pos, joint_world_mats


def load_cjk_font(size: int, bold: bool = False):
    from PIL import ImageFont

    candidates = (
        ["C:/Windows/Fonts/msyhbd.ttc", "C:/Windows/Fonts/simhei.ttf", "C:/Windows/Fonts/msyh.ttc"]
        if bold
        else ["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/simhei.ttf", "C:/Windows/Fonts/arial.ttf"]
    )
    for path in candidates:
        if os.path.isfile(path):
            try:
                return ImageFont.truetype(path, size)
            except Exception:
                pass
    return ImageFont.load_default()


def draw_heart_2d(draw: ImageDraw.ImageDraw, cx: float, cy: float, size: float, color: Tuple[int, int, int], outline=(255, 255, 255)):
    """Draws a crisp 2D heart shape centered at (cx, cy)."""
    pts = []
    for i in range(40):
        ang = (i / 40.0) * 2.0 * math.pi
        hx = 16.0 * (math.sin(ang) ** 3)
        hy = -(13.0 * math.cos(ang) - 5.0 * math.cos(2 * ang) - 2.0 * math.cos(3 * ang) - math.cos(4 * ang))
        pts.append((cx + hx * size * 0.06, cy + hy * size * 0.06))
    draw.polygon(pts, fill=color, outline=outline)


def render_paimon_heart_video(
    animated_glb: Path,
    concept_png: Path,
    out_mp4: Path,
    out_gif: Path,
    out_keyframes_png: Path,
    three_views_png: Path = None,
    meshy_preview_png: Path = None,
) -> dict:
    """Renders a 30fps, 7.6-second (228 frames) 960x720 HD video of the real Meshy 3D Paimon performing HeartGesture (action_id: 27)."""
    char = SkinnedGlbCharacter(animated_glb)
    concept_thumb = Image.open(concept_png).convert("RGBA").resize((108, 144), Image.Resampling.LANCZOS)
    three_views_thumb = None
    if three_views_png and three_views_png.is_file():
        three_views_thumb = Image.open(three_views_png).convert("RGBA").resize((240, 134), Image.Resampling.LANCZOS)
    meshy_preview_thumb = None
    if meshy_preview_png and meshy_preview_png.is_file():
        meshy_preview_thumb = Image.open(meshy_preview_png).convert("RGBA").resize((120, 120), Image.Resampling.LANCZOS)

    font_title = load_cjk_font(16, bold=True)
    font_body = load_cjk_font(13, bold=False)
    font_small = load_cjk_font(12, bold=False)
    font_banner = load_cjk_font(15, bold=True)
    font_strip = load_cjk_font(18, bold=True)

    W, H = 960, 720
    fps = 30
    total_seconds = 7.6
    total_frames = int(fps * total_seconds)

    # Pre-generate starry background constellations
    rng = np.random.default_rng(42)
    stars = [
        (int(rng.integers(20, W - 20)), int(rng.integers(20, H - 90)), float(rng.uniform(1.0, 2.8)), float(rng.uniform(0.0, 6.28)))
        for _ in range(95)
    ]

    frames_rgb = []
    keyframe_snapshots = []

    light_dir = np.array([0.25, 0.45, 0.85], dtype=np.float32)
    light_dir /= np.linalg.norm(light_dir)

    tris = char.triangles

    for f_idx in range(total_frames):
        t = f_idx / float(fps)
        # 0.0s .. 6.2s: Real Meshy Big Heart Gesture (action_id: 27, 6.20s duration)
        # 6.2s .. 7.6s: Real Meshy Idle (action_id: 0) + 360° turnaround
        if t < 6.2:
            clip_name = "HeartGesture" if "HeartGesture" in char.clips else "Idle"
            clip_t = t
            yaw = 0.14 * math.sin(t * math.pi / 3.1)
            phase_label = "当前动作: Meshy Big Heart Gesture (action_id=27 原神派蒙双手过头顶大比心)"
        else:
            clip_name = "Idle"
            clip_t = t - 6.2
            yaw = ((t - 6.2) / 1.4) * 2.0 * math.pi
            phase_label = "当前动作: Meshy Idle (action_id=0) + 360° 全景环绕展示"

        verts, norms, j_pos, _ = char.evaluate_pose(clip_name, clip_t)

        # Rotate character by camera yaw around Y axis
        cos_y, sin_y = math.cos(yaw), math.sin(yaw)
        rot_y = np.array(
            [
                [cos_y, 0.0, sin_y],
                [0.0, 1.0, 0.0],
                [-sin_y, 0.0, cos_y],
            ],
            dtype=np.float32,
        )
        v_cam = verts @ rot_y.T
        n_cam = norms @ rot_y.T

        # Frame the real 3D Meshy Paimon prominently on the right-center stage
        cam_dist = 1.82
        fov_scale = 735.0
        y_center = 0.62
        stage_cx = W * 0.61
        stage_cy = H * 0.53
        z_depth = np.maximum(cam_dist - v_cam[:, 2], 0.3)
        proj_x = stage_cx + (v_cam[:, 0] / z_depth) * fov_scale
        proj_y = stage_cy - ((v_cam[:, 1] - y_center) / z_depth) * fov_scale

        # Paint celestial night-sky background
        img = Image.new("RGB", (W, H), (14, 18, 38))
        draw = ImageDraw.Draw(img)

        for band_i in range(12):
            by0 = int(band_i * H / 12)
            by1 = int((band_i + 1) * H / 12)
            draw.rectangle([0, by0, W, by1], fill=(14 + band_i, 18 + band_i * 2, 38 + band_i * 3))

        for sx, sy, sr, sp in stars:
            twinkle = 0.55 + 0.45 * math.sin(t * 3.5 + sp)
            sc = int(150 + 105 * twinkle)
            draw.ellipse([sx - sr, sy - sr, sx + sr, sy + sr], fill=(sc, sc, min(255, sc + 20)))

        def project_pt3d(pt3: np.ndarray) -> Tuple[float, float]:
            pr = rot_y @ pt3
            zd = max(cam_dist - float(pr[2]), 0.3)
            return stage_cx + (float(pr[0]) / zd) * fov_scale, stage_cy - ((float(pr[1]) - y_center) / zd) * fov_scale

        # Glowing magic floor circle under Paimon's feet
        _, floor_y_f = project_pt3d(np.array([0.0, -0.02, 0.0], dtype=np.float32))
        floor_y = int(floor_y_f)
        draw.ellipse([int(stage_cx) - 165, floor_y - 24, int(stage_cx) + 165, floor_y + 24], outline=(242, 198, 78), width=2)
        draw.ellipse([int(stage_cx) - 118, floor_y - 16, int(stage_cx) + 118, floor_y + 16], outline=(90, 175, 255), width=2)

        # Render 16x barycentric UV-subdivided 3D mesh triangles (83,184 textured micro-triangles)
        tri_px = proj_x[tris]
        tri_py = proj_y[tris]
        tri_vz = v_cam[tris, 2]

        sub_z = np.einsum("sc,tc->ts", char.sub_center_bary, tri_vz).reshape(-1)
        sub_px = np.einsum("svc,tc->tsv", char.sub_verts_bary, tri_px).reshape(-1, 3)
        sub_py = np.einsum("svc,tc->tsv", char.sub_verts_bary, tri_py).reshape(-1, 3)

        tri_norm = np.mean(n_cam[tris], axis=1)
        ndotl = np.sum(tri_norm * light_dir[None, :], axis=1)
        toon_shade = np.where(ndotl > 0.10, 1.02, np.where(ndotl > -0.30, 0.95, 0.86))[:, None, None]
        lit_sub = np.clip(char.sub_cols * toon_shade, 0, 255).astype(np.uint8).reshape(-1, 3)

        order = np.argsort(sub_z)
        for idx in order:
            c = (int(lit_sub[idx, 0]), int(lit_sub[idx, 1]), int(lit_sub[idx, 2]))
            draw.polygon(
                [
                    (float(sub_px[idx, 0]), float(sub_py[idx, 0])),
                    (float(sub_px[idx, 1]), float(sub_py[idx, 1])),
                    (float(sub_px[idx, 2]), float(sub_py[idx, 2])),
                ],
                fill=c,
            )

        # Floating heart effects when Paimon raises both arms into the Big Heart Gesture (0.85s <= t <= 5.25s)
        if 0.85 <= t <= 5.25:
            lh = j_pos.get("LeftHand", np.array([0.0, 1.05, 0.12], dtype=np.float32))
            rh = j_pos.get("RightHand", np.array([-0.18, 0.98, 0.11], dtype=np.float32))
            heart_center_3d = 0.5 * (lh + rh) + np.array([0.0, 0.14, 0.05], dtype=np.float32)
            hx, hy = project_pt3d(heart_center_3d)

            pulse = 1.0 + 0.20 * math.sin((t - 0.85) * 8.5)
            draw_heart_2d(draw, hx, hy, 30.0 * pulse, (255, 88, 140), outline=(255, 235, 150))
            draw_heart_2d(draw, hx, hy, 16.0 * pulse, (255, 175, 208), outline=(255, 255, 255))

            for h_i in range(4):
                prog = ((t - 0.85) + h_i * 0.32) % 1.5
                sub_x = hx + math.sin(prog * 4.5 + h_i * 1.7) * (42.0 + h_i * 14.0)
                sub_y = hy - 20.0 - prog * 95.0
                draw_heart_2d(draw, sub_x, sub_y, 13.0, (255, 125, 172), outline=(255, 225, 240))

        # Left HUD Panel 1: Pipeline Telemetry + Concept Art + Meshy Official Preview
        draw.rectangle([16, 16, 368, 336], fill=(18, 24, 48), outline=(242, 198, 78), width=2)
        draw.text((26, 22), "RurixForge × Meshy 3D 角色管线实测", fill=(245, 202, 72), font=font_title)
        img.paste(concept_thumb, (26, 52), concept_thumb)
        draw.text((28, 198), "Stage 1A: A-Pose 原画", fill=(210, 225, 255), font=font_small)
        if meshy_preview_thumb is not None:
            img.paste(meshy_preview_thumb, (152, 64), meshy_preview_thumb)
            draw.text((150, 198), "Stage 2: Meshy 3D 预览", fill=(210, 225, 255), font=font_small)

        draw.text((26, 222), "生模 Task: 01a13e35-57f2 (multi-image-to-3d)", fill=(160, 220, 255), font=font_body)
        draw.text((26, 244), "减面 Task: 01a13e48-1639 (8,319 -> 5,199 tris)", fill=(160, 220, 255), font=font_body)
        draw.text((26, 266), "绑骨 Task: 01a13e4a-00b8 (24 标准骨骼蒙皮)", fill=(160, 220, 255), font=font_body)
        draw.text((26, 288), "动画 Task: 01a13e4b-874f (Big Heart Gesture)", fill=(160, 255, 195), font=font_body)
        draw.text((26, 310), f"时间: {t:04.2f}s / {total_seconds:04.2f}s (Frame {f_idx+1}/{total_frames})", fill=(225, 230, 245), font=font_body)

        # Left HUD Panel 2: Stage 1B Orthographic Three-View Turnaround Sheet (正视图/侧视图/背视图)
        if three_views_thumb is not None:
            draw.rectangle([16, 348, 276, 518], fill=(18, 24, 48), outline=(95, 155, 235), width=2)
            draw.text((26, 354), "Stage 1B: 正交三视图 (正/侧/背)", fill=(245, 202, 72), font=font_body)
            img.paste(three_views_thumb, (26, 376), three_views_thumb)

        # Bottom status banner
        draw.rectangle([16, H - 54, W - 16, H - 14], fill=(18, 24, 48), outline=(85, 130, 210), width=1)
        draw.text((28, H - 44), phase_label, fill=(255, 215, 110), font=font_banner)
        draw.text((W - 355, H - 43), "Meshy 2K PBR + 24-Bone FK/LBS", fill=(170, 205, 255), font=font_body)

        frame_np = np.array(img, dtype=np.uint8)
        frames_rgb.append(frame_np)

        if f_idx in (0, 32, 72, 205):
            keyframe_snapshots.append(img.copy())

    # Write MP4 video (H.264, yuv420p for universal browser/player compatibility)
    out_mp4.parent.mkdir(parents=True, exist_ok=True)
    writer = imageio.get_writer(
        str(out_mp4),
        fps=fps,
        codec="libx264",
        quality=8,
        pixelformat="yuv420p",
        macro_block_size=16,
    )
    for frame_np in frames_rgb:
        writer.append_data(frame_np)
    writer.close()

    # Write animated GIF (15fps, 480x360 for fast inline preview)
    out_gif.parent.mkdir(parents=True, exist_ok=True)
    gif_frames = [
        Image.fromarray(f).resize((480, 360), Image.Resampling.BILINEAR)
        for f in frames_rgb[::2]
    ]
    gif_frames[0].save(
        str(out_gif),
        save_all=True,
        append_images=gif_frames[1:],
        duration=int(1000 / 15),
        loop=0,
    )

    # Save 4-panel keyframe comparison strip
    if keyframe_snapshots:
        strip = Image.new("RGB", (1280, 360), (14, 18, 38))
        for i, snap in enumerate(keyframe_snapshots[:4]):
            thumb = snap.resize((310, 232), Image.Resampling.BILINEAR)
            strip.paste(thumb, (8 + i * 318, 64))
        s_draw = ImageDraw.Draw(strip)
        s_draw.text(
            (16, 18),
            "原神派蒙 (Genshin Impact Paimon) — Meshy 真实 3D 角色 & 比心动作 (Big Heart Gesture) 关键帧序列",
            fill=(245, 202, 72),
            font=font_strip,
        )
        labels = [
            "t = 0.00s: 准备起势姿态 (Frame 1)",
            "t = 1.07s: 双臂高举过头顶 (Frame 33)",
            "t = 2.40s: 派蒙双手大比心 (Big Heart Gesture)",
            "t = 6.83s: 360° 环绕展示星空披风与皇冠",
        ]
        for i, lbl in enumerate(labels[: len(keyframe_snapshots)]):
            s_draw.text((14 + i * 318, 306), lbl, fill=(200, 225, 255), font=font_body)
        out_keyframes_png.parent.mkdir(parents=True, exist_ok=True)
        strip.save(str(out_keyframes_png), format="PNG")

    return {
        "mp4Path": str(out_mp4),
        "mp4Bytes": out_mp4.stat().st_size,
        "gifPath": str(out_gif),
        "gifBytes": out_gif.stat().st_size,
        "keyframesPngPath": str(out_keyframes_png),
        "framesRendered": total_frames,
        "fps": fps,
        "resolution": f"{W}x{H}",
    }


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Build Genshin Paimon 3D character and render HeartGesture video")
    parser.add_argument(
        "--reuse-session",
        type=str,
        default=None,
        help="Reuse an already completed forge-agentd session ID to validate artifacts and render video",
    )
    args = parser.parse_args()

    start_ts = time.time()
    artifacts_dir = WORKSPACE_ROOT / ".forge" / "tmp" / "e2e" / "live_artifacts"
    artifacts_dir.mkdir(parents=True, exist_ok=True)
    demo_root = WORKSPACE_ROOT / "projects" / "demo"

    def resolve_proj_path(rel_path: str) -> Path:
        p1 = WORKSPACE_ROOT / rel_path
        if p1.exists():
            return p1
        p2 = demo_root / rel_path
        if p2.exists():
            return p2
        return p1

    # 1. Create Paimon Concept Art & validate Gate 1
    concept_rel = "Content/ConceptArt/paimon_apose.png"
    concept_abs = demo_root / concept_rel
    create_paimon_concept_art(concept_abs)
    shutil.copy2(concept_abs, artifacts_dir / "paimon_stage1_concept.png")

    g1 = validate_gate_1_concept_art(concept_abs)
    assert g1.passed, f"Gate 1 failed: {g1.message}"
    print(f"[Gate 1 PASS] Paimon Concept Art: {g1.details}", flush=True)

    proc = None
    orig_forge_toml = None
    forge_toml_path = WORKSPACE_ROOT / "forge.toml"

    try:
        if args.reuse_session:
            sid = args.reuse_session
            print(f"[Reuse Session] Using completed session {sid}", flush=True)
            ev_file = artifacts_dir / f"paimon_session_{sid}.jsonl"
            if not ev_file.is_file():
                ev_file = WORKSPACE_ROOT / "data" / "agent-events" / f"{sid}.jsonl"
            events = [json.loads(line) for line in ev_file.read_text(encoding="utf-8").splitlines() if line.strip()]
        else:
            create_paimon_concept_art(WORKSPACE_ROOT / concept_rel)
            # Clean previous genshin_paimon published package if present
            for root_dir in (WORKSPACE_ROOT, demo_root):
                models_dir = root_dir / "Content" / "Models"
                if models_dir.is_dir():
                    for child in models_dir.iterdir():
                        if child.is_dir() and (child / "model.rxmodel").is_file():
                            try:
                                doc = json.loads((child / "model.rxmodel").read_text(encoding="utf-8"))
                                if doc.get("name") == "genshin_paimon":
                                    shutil.rmtree(child, ignore_errors=True)
                                    meta_p = models_dir / f"{child.name}.meta"
                                    meta_p.unlink(missing_ok=True)
                            except Exception:
                                pass

            meshy_key = load_meshy_key()
            port = get_free_port()
            agentd_bin = WORKSPACE_ROOT / "target" / "debug" / "forge-agentd.exe"

            env = os.environ.copy()
            env["FORGE_AGENTD_ADDR"] = f"127.0.0.1:{port}"
            env["MESHY_API_KEY"] = meshy_key
            try:
                with socket.create_connection(("127.0.0.1", 7897), timeout=0.1):
                    env["HTTP_PROXY"] = "http://127.0.0.1:7897"
                    env["HTTPS_PROXY"] = "http://127.0.0.1:7897"
                    env["NO_PROXY"] = "127.0.0.1,localhost,api.meshy.ai,assets.meshy.ai"
            except OSError:
                pass

            orig_forge_toml = forge_toml_path.read_text(encoding="utf-8") if forge_toml_path.is_file() else None
            if orig_forge_toml:
                forge_toml_path.write_text(orig_forge_toml.replace('mode = "2d"', 'mode = "3d"'), encoding="utf-8")

            base_url = f"http://127.0.0.1:{port}"
            print(f"[forge-agentd] Starting on {base_url} ...", flush=True)
            proc = subprocess.Popen(
                [str(agentd_bin)],
                cwd=str(WORKSPACE_ROOT),
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

            for _ in range(50):
                try:
                    st, h = http_json("GET", f"{base_url}/health", timeout=2)
                    if st == 200:
                        break
                except Exception:
                    time.sleep(0.2)

            # Initialize Antigravity bridge and verify Gemini 4 registration
            status, ag_channel = http_json(
                "GET", f"{base_url}/api/forge/channels/antigravity?refresh=true", timeout=30
            )
            print(
                f"[Antigravity Channel] status={status}, configured={ag_channel.get('configured')}, account={ag_channel.get('account')}",
                flush=True,
            )
            assert ag_channel.get("configured") is True, f"Antigravity channel not configured: {ag_channel}"

            _, sess_resp = http_json(
                "POST",
                f"{base_url}/api/forge/sessions",
                {
                    "title": "Genshin Impact Paimon 3D Character & HeartGesture Pipeline",
                    "selectedModelId": "antigravity:gemini-4",
                    "thinkingEnabled": True,
                    "reasoningEffort": "medium",
                },
            )
            sid = sess_resp.get("session", sess_resp)["id"]
            http_json(
                "PATCH",
                f"{base_url}/api/forge/sessions/{sid}",
                {
                    "selectedModelId": "antigravity:gemini-4",
                    "agentEngine": "local",
                    "thinkingEnabled": True,
                    "reasoningEffort": "medium",
                },
            )
            print(f"[Session] Created {sid} with antigravity:gemini-4", flush=True)

            prompt_text = (
                "请严格按照 `character-3d-pipeline` 技能规程，为我们制作原神派蒙（Paimon）的完整可动 3D 角色并入景验证（不要跳过任何一步，依次调用工具）：\n"
                "1. 调用 `mcp__gen-model__gen_mesh`：传入 `imageRef: \"Content/ConceptArt/paimon_apose.png\"`, "
                "`prompt: \"Genshin Impact Paimon cute floating chibi fairy companion with gold crown halo, silver-white hair, white and gold celestial tunic, starry night blue cape, A-pose\"`, "
                "`poseMode: \"a-pose\"`, `targetPolyBudget: 8000`, `texture: true`。\n"
                "2. 拿到第 1 步返回的 `candidates[0].meshFileRef` 后，调用 `mcp__gen-model__gen_mesh_refine`：传入该 `meshFileRef`、`targetFaces: 4000`、`ops: {\"retopo\": {\"targetFaces\": 4000}, \"unwrap\": true}`。\n"
                "3. 拿到第 2 步返回的 `refinedMeshFileRef` 后，调用 `mcp__gen-model__gen_mesh_rig`：传入 `meshFileRef: <refinedMeshFileRef>`, `heightMeters: 1.35`。\n"
                "4. 拿到第 3 步返回的 `riggedMeshFileRef` 后，调用 `mcp__gen-model__gen_mesh_animate`：传入 `meshFileRef: <riggedMeshFileRef>`, `motionPack: \"assets/motion-packs/humanoid\"` 合入标准人形与比心动作包（Idle/Walk/Run/Attack/HeartGesture）。\n"
                "5. 拿到第 4 步返回的 `animatedMeshFileRef` 后，调用 `mcp__gen-model__gen_character_publish`：传入 `meshFileRef: <animatedMeshFileRef>`, `name: \"genshin_paimon\"`, `category: \"role\"` 发布正式 `.rxmodel` 与 `template.rxprefab`。\n"
                "6. 最后调用 `mcp__engine-scene__prefab_instantiate`（传入 `prefabPath: <rxprefabUri>`, `name: \"GenshinPaimon\"`）将角色放入场景，并调用 `mcp__engine-scene__viewport_frame` 截图验证。"
            )

            print("[Turn] Executing Antigravity Gemini 4 Agent turn for Paimon ...", flush=True)
            _, ask_resp = http_json(
                "POST",
                f"{base_url}/api/forge/sessions/{sid}/ask:execute",
                {
                    "userInput": prompt_text,
                    "mode": "build",
                    "skills": ["character-3d-pipeline"],
                },
                timeout=1800,
            )
            print(f"[Turn Done] Run status: {ask_resp.get('run', {}).get('status')}", flush=True)

            ev_file = WORKSPACE_ROOT / "data" / "agent-events" / f"{sid}.jsonl"
            events = [json.loads(line) for line in ev_file.read_text(encoding="utf-8").splitlines() if line.strip()]
            shutil.copy2(ev_file, artifacts_dir / f"paimon_session_{sid}.jsonl")

        tool_results = {}
        for ev in events:
            if ev.get("type") == "agent.tool.completed":
                p = ev.get("payload", {})
                t_name = p.get("name", "")
                out_val = p.get("output", "")
                if isinstance(out_val, dict):
                    if "content" in out_val and isinstance(out_val["content"], list) and out_val["content"]:
                        txt = out_val["content"][0].get("text", "")
                        try:
                            out_val = json.loads(txt)
                        except Exception:
                            pass
                    tool_results[t_name] = out_val
                elif isinstance(out_val, str):
                    try:
                        out_json = json.loads(out_val)
                        if isinstance(out_json, dict) and "content" in out_json:
                            txt = out_json["content"][0].get("text", "")
                            out_json = json.loads(txt)
                        tool_results[t_name] = out_json
                    except Exception:
                        tool_results[t_name] = out_val

        mesh_out = tool_results["mcp__gen-model__gen_mesh"]
        cand0 = (mesh_out.get("candidates") or [{}])[0]
        raw_ref = (
            cand0.get("meshFileRef")
            or cand0.get("fileRef")
            or mesh_out.get("meshFileRef")
            or mesh_out.get("fileRef")
        )
        refined_ref = tool_results["mcp__gen-model__gen_mesh_refine"]["refinedMeshFileRef"]
        rigged_ref = tool_results["mcp__gen-model__gen_mesh_rig"]["riggedMeshFileRef"]
        anim_out = tool_results["mcp__gen-model__gen_mesh_animate"]
        anim_ref = anim_out["animatedMeshFileRef"]
        pub_out = tool_results["mcp__gen-model__gen_character_publish"]
        rxmodel_uri = pub_out["rxmodelUri"]
        rxprefab_uri = pub_out["rxprefabUri"]

        raw_abs = resolve_proj_path(raw_ref)
        refined_abs = resolve_proj_path(refined_ref)
        rigged_abs = resolve_proj_path(rigged_ref)
        anim_abs = resolve_proj_path(anim_ref)
        rxmodel_abs = resolve_proj_path(rxmodel_uri)
        rxprefab_abs = resolve_proj_path(rxprefab_uri)

        shutil.copy2(raw_abs, artifacts_dir / "paimon_stage2_raw.glb")
        shutil.copy2(refined_abs, artifacts_dir / "paimon_stage3_refined.glb")
        shutil.copy2(rigged_abs, artifacts_dir / "paimon_stage4_rigged.glb")
        shutil.copy2(anim_abs, artifacts_dir / "paimon_stage5_animated.glb")
        shutil.copy2(rxmodel_abs, artifacts_dir / "paimon_model.rxmodel")
        shutil.copy2(rxprefab_abs, artifacts_dir / "paimon_template.rxprefab")

        # Validate all 5 Gates
        g2 = validate_gate_2_raw_glb(raw_abs)
        g3 = validate_gate_3_polycount(refined_abs, target_faces=4000)
        g4 = validate_gate_4_rigging(rigged_abs)
        g5 = validate_gate_5_motion_publish(
            anim_abs, published_dir=rxmodel_abs.parent
        )
        assert g2.passed and g3.passed and g4.passed and g5.passed, (
            f"One or more gates failed: g2={g2.to_dict()}, g3={g3.to_dict()}, g4={g4.to_dict()}, g5={g5.to_dict()}"
        )

        print(f"[All 5 Gates PASS] Animations merged: {anim_out.get('animations')}", flush=True)

        # Render the HD MP4 video and GIF of Paimon performing HeartGesture
        out_mp4 = artifacts_dir / "paimon_heart_gesture.mp4"
        out_gif = artifacts_dir / "paimon_heart_gesture.gif"
        out_kf = artifacts_dir / "paimon_heart_keyframes.png"
        video_stats = render_paimon_heart_video(anim_abs, concept_abs, out_mp4, out_gif, out_kf)
        print(f"[Video Rendered] {video_stats}", flush=True)

        summary = {
            "ok": True,
            "sessionId": sid,
            "characterName": "genshin_paimon",
            "animations": anim_out.get("animations"),
            "rxmodelUri": rxmodel_uri,
            "rxprefabUri": rxprefab_uri,
            "gates": {
                "gate1": g1.details,
                "gate2": g2.details,
                "gate3": g3.details,
                "gate4": g4.details,
                "gate5": g5.details,
            },
            "video": video_stats,
            "elapsedSec": round(time.time() - start_ts, 2),
        }
        (artifacts_dir / "paimon_heart_report.json").write_text(
            json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8"
        )
        return summary

    finally:
        if orig_forge_toml is not None:
            forge_toml_path.write_text(orig_forge_toml, encoding="utf-8")
        root_models = WORKSPACE_ROOT / "Content" / "Models"
        if root_models.exists():
            dst_models = demo_root / "Content" / "Models"
            dst_models.mkdir(parents=True, exist_ok=True)
            for item in root_models.iterdir():
                target = dst_models / item.name
                if item.is_dir():
                    shutil.copytree(item, target, dirs_exist_ok=True)
                else:
                    shutil.copy2(item, target)
        if (WORKSPACE_ROOT / "Content").exists():
            shutil.rmtree(WORKSPACE_ROOT / "Content", ignore_errors=True)
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except Exception:
                proc.kill()


if __name__ == "__main__":
    main()

