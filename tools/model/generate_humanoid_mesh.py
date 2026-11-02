#!/usr/bin/env python3
"""Headless 3D Humanoid Base Mesh Generator (Track B / Local Headless Fallback).

Generates an articulated A-pose or T-pose 3D humanoid glTF 2.0 Binary (.glb)
from concept art (`--image`) and/or text prompt (`--prompt`), matching a requested
initial polygon budget (`--target-polycount`, default 8000 triangles).

Supports:
  - Genshin Impact Paimon (派蒙 / 派夢) chibi fairy preset with embedded 512x512 PNG
    UV texture atlas (floating gold halo crown, silver-white bob hair, anime face,
    rose-pink scarf, white-gold celestial tunic, starry night-blue cape, white-gold boots).
  - Standard stylized armored humanoid preset.

Guarantees Gate 2 compliance:
  - Valid glTF 2.0 Binary header (`glTF`, version 2)
  - Single unified mesh primitive with `mode = 4` (`TRIANGLES`)
  - Non-degenerate triangles with `POSITION`, `NORMAL`, and `TEXCOORD_0` attributes
  - Embedded PBR texture (`image/png`) + `baseColorFactor`
  - Clean A-pose / T-pose limb separation aligned with `auto_rig.py` (`HUMANOID_BONES`)
"""

import argparse
import io
import json
import math
from pathlib import Path
import sys
from typing import List, Optional, Tuple

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from glb_utils import add_accessor, add_buffer_view, save_glb


def extract_concept_color(image_path: str) -> List[float]:
    """Samples the average foreground RGB color from the concept art PNG."""
    if not image_path:
        return [0.24, 0.52, 0.85, 1.0]
    p = Path(image_path)
    if not p.is_file():
        return [0.24, 0.52, 0.85, 1.0]
    try:
        from PIL import Image

        with Image.open(p) as img:
            rgba = np.array(img.convert("RGBA"), dtype=np.float32)
            mask = rgba[:, :, 3] > 20
            if np.any(mask):
                fg = rgba[mask][:, :3] / 255.0
                mean_rgb = np.mean(fg, axis=0)
                return [
                    float(np.clip(mean_rgb[0], 0.05, 0.95)),
                    float(np.clip(mean_rgb[1], 0.05, 0.95)),
                    float(np.clip(mean_rgb[2], 0.05, 0.95)),
                    1.0,
                ]
    except Exception:
        pass
    return [0.24, 0.52, 0.85, 1.0]


def is_paimon_request(prompt: str, image_path: str) -> bool:
    combined = f"{prompt or ''} {image_path or ''}".lower()
    for kw in ("paimon", "派蒙", "派夢", "fairy", "chibi", "genshin", "原神"):
        if kw in combined:
            return True
    return False


def build_paimon_texture_atlas_png(num_segs: int = 13) -> Optional[bytes]:
    """Generates a 512x512 RGBA PNG texture atlas for Paimon's 13 UV bands."""
    try:
        from PIL import Image, ImageDraw
    except ImportError:
        return None

    W, H = 512, 512
    img = Image.new("RGBA", (W, H), (245, 246, 252, 255))
    draw = ImageDraw.Draw(img)

    def band_x(idx: int, u_local: float) -> int:
        u = (idx + u_local) / float(num_segs)
        return int(np.clip(round(u * (W - 1)), 0, W - 1))

    def fill_band(idx: int, color: Tuple[int, int, int, int], y0: int = 0, y1: int = H):
        x0 = band_x(idx, 0.0)
        x1 = band_x(idx, 1.0)
        draw.rectangle([x0, y0, x1, y1], fill=color)

    # Segment 0: Floating Gold Halo Crown
    fill_band(0, (245, 202, 72, 255))
    fill_band(0, (30, 38, 82, 255), int(H * 0.42), int(H * 0.58))

    # Segment 1: Chibi Head (Silver-White Bob Hair + Cute Anime Face at u_local ~ 0.75)
    fill_band(1, (238, 241, 250, 255))  # Silver-white hair base
    fill_band(1, (216, 222, 242, 255), int(H * 0.75), H)  # Hair tips shading
    # Face skin patch around u_local in [0.60, 0.90], y in [0.30, 0.92]
    fx0 = band_x(1, 0.58)
    fx1 = band_x(1, 0.92)
    draw.rectangle([fx0, int(H * 0.30), fx1, int(H * 0.92)], fill=(255, 234, 222, 255))
    # Silver-white front bangs
    draw.rectangle([fx0, 0, fx1, int(H * 0.34)], fill=(244, 246, 253, 255))
    # Starry dark-blue hairpin on left forehead
    hpx0 = band_x(1, 0.62)
    hpx1 = band_x(1, 0.70)
    draw.rectangle([hpx0, int(H * 0.18), hpx1, int(H * 0.30)], fill=(28, 36, 78, 255))
    # Anime eyes (dark purple-indigo with white starlight highlights)
    lex0, lex1 = band_x(1, 0.65), band_x(1, 0.72)
    rex0, rex1 = band_x(1, 0.78), band_x(1, 0.85)
    ey0, ey1 = int(H * 0.46), int(H * 0.64)
    draw.ellipse([lex0, ey0, lex1, ey1], fill=(52, 38, 88, 255))
    draw.ellipse([rex0, ey0, rex1, ey1], fill=(52, 38, 88, 255))
    # Eye highlights & golden iris lower arc
    draw.ellipse([lex0 + 1, ey0 + 4, max(lex0 + 2, lex1 - 1), ey0 + 18], fill=(255, 255, 255, 255))
    draw.ellipse([rex0 + 1, ey0 + 4, max(rex0 + 2, rex1 - 1), ey0 + 18], fill=(255, 255, 255, 255))
    # Rosy cheeks
    draw.ellipse([lex0 - 1, int(H * 0.65), lex1, int(H * 0.72)], fill=(255, 162, 180, 255))
    draw.ellipse([rex0, int(H * 0.65), rex1 + 1, int(H * 0.72)], fill=(255, 162, 180, 255))
    # Cute happy smile
    mx0, mx1 = band_x(1, 0.72), band_x(1, 0.78)
    draw.rectangle([mx0, int(H * 0.73), mx1, int(H * 0.78)], fill=(228, 105, 122, 255))

    # Segment 2: Rose-Pink Scarf / Collar with Gold Trim
    fill_band(2, (230, 142, 162, 255))
    fill_band(2, (242, 198, 80, 255), int(H * 0.80), H)

    # Segment 3: White & Gold Celestial Tunic Torso + Starry Back Cape
    fill_band(3, (248, 249, 253, 255))
    # Gold collar & waist borders
    fill_band(3, (240, 196, 78, 255), 0, int(H * 0.12))
    fill_band(3, (240, 196, 78, 255), int(H * 0.86), H)
    # Front chest dark-blue & gold celestial triquetra emblem (around u_local = 0.75)
    cx0, cx1 = band_x(3, 0.64), band_x(3, 0.86)
    draw.ellipse([cx0, int(H * 0.22), cx1, int(H * 0.68)], fill=(28, 38, 82, 255), outline=(242, 198, 78, 255), width=2)
    # Back starry night-blue cape (around u_local = 0.15..0.38)
    bx0, bx1 = band_x(3, 0.12), band_x(3, 0.40)
    draw.rectangle([bx0, int(H * 0.10), bx1, int(H * 0.92)], fill=(22, 30, 72, 255))

    # Segment 4: Puffy White & Gold Bloomers / Skirt
    fill_band(4, (244, 245, 252, 255))
    fill_band(4, (238, 194, 76, 255), int(H * 0.72), int(H * 0.90))
    fill_band(4, (30, 38, 82, 255), 0, int(H * 0.15))

    # Segments 5 & 7: Left & Right Puffy Sleeves (White + Gold Cuff + Rose Accent)
    for seg_idx in (5, 7):
        fill_band(seg_idx, (248, 249, 253, 255))
        fill_band(seg_idx, (230, 142, 162, 255), 0, int(H * 0.22))
        fill_band(seg_idx, (242, 198, 78, 255), int(H * 0.78), H)

    # Segments 6 & 8: Left & Right Forearms & Cute Hands (Gold Wristband + Warm Skin Hand)
    for seg_idx in (6, 8):
        fill_band(seg_idx, (248, 249, 253, 255), 0, int(H * 0.45))
        fill_band(seg_idx, (242, 198, 78, 255), int(H * 0.45), int(H * 0.58))
        fill_band(seg_idx, (255, 232, 218, 255), int(H * 0.58), H)

    # Segments 9 & 11: Left & Right Thighs (Skin + White Stockings with Gold Garter)
    for seg_idx in (9, 11):
        fill_band(seg_idx, (255, 234, 222, 255), 0, int(H * 0.35))
        fill_band(seg_idx, (242, 198, 78, 255), int(H * 0.35), int(H * 0.46))
        fill_band(seg_idx, (246, 247, 252, 255), int(H * 0.46), H)

    # Segments 10 & 12: Left & Right Celestial White-Gold Boots
    for seg_idx in (10, 12):
        fill_band(seg_idx, (246, 247, 252, 255), 0, int(H * 0.35))
        fill_band(seg_idx, (242, 198, 78, 255), int(H * 0.35), int(H * 0.52))
        fill_band(seg_idx, (30, 38, 82, 255), int(H * 0.52), int(H * 0.65))
        fill_band(seg_idx, (248, 249, 253, 255), int(H * 0.65), int(H * 0.88))
        fill_band(seg_idx, (242, 198, 78, 255), int(H * 0.88), H)

    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def add_limb_tube(
    p0: np.ndarray,
    p1: np.ndarray,
    r0: float,
    r1: float,
    rings: int,
    sectors: int,
    u_offset: float,
    u_scale: float,
    scale_x: float = 1.0,
    scale_z: float = 1.0,
    bulge_factor: float = 0.12,
) -> Tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """Generates a closed/tapered tubular body segment between 3D points p0 and p1."""
    axis = p1 - p0
    length = float(np.linalg.norm(axis))
    if length < 1e-6:
        axis_dir = np.array([0.0, 1.0, 0.0], dtype=np.float32)
    else:
        axis_dir = (axis / length).astype(np.float32)

    up = np.array([0.0, 0.0, 1.0], dtype=np.float32)
    if abs(float(np.dot(axis_dir, up))) > 0.95:
        up = np.array([1.0, 0.0, 0.0], dtype=np.float32)
    t1 = np.cross(axis_dir, up)
    t1 = t1 / np.linalg.norm(t1)
    t2 = np.cross(axis_dir, t1)
    t2 = t2 / np.linalg.norm(t2)

    verts: List[List[float]] = []
    norms: List[List[float]] = []
    uvs: List[List[float]] = []
    tris: List[List[int]] = []

    for i in range(rings + 1):
        v = i / float(rings)
        center = p0 * (1.0 - v) + p1 * v
        bulge = 1.0 + bulge_factor * math.sin(v * math.pi)
        radius = (r0 * (1.0 - v) + r1 * v) * bulge
        if i == 0:
            radius *= 0.86
        elif i == rings:
            radius *= 0.86

        for j in range(sectors):
            u = j / float(sectors)
            theta = u * 2.0 * math.pi
            cos_t = math.cos(theta) * scale_x
            sin_t = math.sin(theta) * scale_z
            radial_dir = t1 * cos_t + t2 * sin_t
            norm_len = float(np.linalg.norm(radial_dir))
            n_vec = radial_dir / norm_len if norm_len > 1e-6 else t1
            pos = center + radial_dir * radius

            verts.append([float(pos[0]), float(pos[1]), float(pos[2])])
            norms.append([float(n_vec[0]), float(n_vec[1]), float(n_vec[2])])
            # v_uv = 1.0 - v so top of segment (p1) maps to top of image (v_uv = 0)
            uvs.append([float(u_offset + u * u_scale), float(1.0 - v)])

    for i in range(rings):
        for j in range(sectors):
            j_next = (j + 1) % sectors
            a = i * sectors + j
            b = i * sectors + j_next
            c = (i + 1) * sectors + j
            d = (i + 1) * sectors + j_next
            tris.append([a, b, c])
            tris.append([b, d, c])

    return (
        np.array(verts, dtype=np.float32),
        np.array(norms, dtype=np.float32),
        np.array(uvs, dtype=np.float32),
        np.array(tris, dtype=np.uint32),
    )


def generate_humanoid_glb(
    target_polycount: int = 8000,
    pose_mode: str = "a-pose",
    base_color: List[float] = None,
    paimon_mode: bool = False,
) -> Tuple[dict, bytearray, dict]:
    """Builds a full multi-limb A-pose/T-pose humanoid GLB with ~target_polycount triangles."""
    if base_color is None:
        base_color = [0.96, 0.96, 0.99, 1.0] if paimon_mode else [0.24, 0.52, 0.85, 1.0]

    target_polycount = max(800, int(target_polycount))
    is_tpose = (pose_mode or "").lower() == "t-pose"

    if paimon_mode:
        # 13 segments for Genshin Impact Paimon (aligned to HUMANOID_BONES in auto_rig.py)
        # Note: in auto_rig.py with height=1.70:
        #   Head is at y=1.65, Neck at y=1.51, Spine1 at y=1.34, Spine at y=1.14, Hips at y=0.935
        #   LeftShoulder..LeftHand are at y=1.48, x=-0.10..-0.71
        #   RightShoulder..RightHand are at y=1.48, x=+0.10..+0.71
        segments = [
            # 0: Floating Gold Halo Crown above Paimon's head
            (np.array([0.0, 1.62, 0.0]), np.array([0.0, 1.70, 0.0]), 0.11, 0.135, 1.0, 1.0, 0.05),
            # 1: Chibi Head & Silver-White Bob Hair
            (np.array([0.0, 1.33, 0.0]), np.array([0.0, 1.59, 0.0]), 0.145, 0.165, 1.05, 1.02, 0.22),
            # 2: Rose-Pink Puffy Scarf / Collar
            (np.array([0.0, 1.24, 0.0]), np.array([0.0, 1.35, 0.0]), 0.13, 0.145, 1.15, 1.05, 0.18),
            # 3: White & Gold Celestial Tunic Torso + Starry Back Cape
            (np.array([0.0, 0.92, 0.0]), np.array([0.0, 1.25, 0.0]), 0.155, 0.135, 1.18, 0.92, 0.14),
            # 4: Puffy White & Gold Bloomers / Pelvis
            (np.array([0.0, 0.74, 0.0]), np.array([0.0, 0.94, 0.0]), 0.15, 0.165, 1.20, 1.02, 0.20),
        ]
        if is_tpose:
            segments.extend([
                # 5: Left Puffy Sleeve (Upper Arm)
                (np.array([-0.15, 1.30, 0.0]), np.array([-0.40, 1.30, 0.0]), 0.068, 0.062, 1.0, 1.0, 0.18),
                # 6: Left Forearm & Cute Hand
                (np.array([-0.40, 1.30, 0.0]), np.array([-0.66, 1.30, 0.02]), 0.055, 0.048, 1.0, 1.0, 0.12),
                # 7: Right Puffy Sleeve (Upper Arm)
                (np.array([0.15, 1.30, 0.0]), np.array([0.40, 1.30, 0.0]), 0.068, 0.062, 1.0, 1.0, 0.18),
                # 8: Right Forearm & Cute Hand
                (np.array([0.40, 1.30, 0.0]), np.array([0.66, 1.30, 0.02]), 0.055, 0.048, 1.0, 1.0, 0.12),
            ])
        else:
            # A-pose arms positioned at y=1.22..1.30 so auto_rig binds them to LeftArm/LeftForeArm/LeftHand
            segments.extend([
                # 5: Left Puffy Sleeve (Upper Arm)
                (np.array([-0.15, 1.29, 0.0]), np.array([-0.39, 1.24, 0.01]), 0.068, 0.062, 1.0, 1.0, 0.18),
                # 6: Left Forearm & Cute Hand
                (np.array([-0.39, 1.24, 0.01]), np.array([-0.65, 1.19, 0.03]), 0.055, 0.050, 1.0, 1.0, 0.14),
                # 7: Right Puffy Sleeve (Upper Arm)
                (np.array([0.15, 1.29, 0.0]), np.array([0.39, 1.24, 0.01]), 0.068, 0.062, 1.0, 1.0, 0.18),
                # 8: Right Forearm & Cute Hand
                (np.array([0.39, 1.24, 0.01]), np.array([0.65, 1.19, 0.03]), 0.055, 0.050, 1.0, 1.0, 0.14),
            ])
        segments.extend([
            # 9: Left Thigh
            (np.array([-0.105, 0.75, 0.0]), np.array([-0.115, 0.38, 0.0]), 0.078, 0.062, 1.0, 1.0, 0.12),
            # 10: Left Celestial White-Gold Boot
            (np.array([-0.115, 0.38, 0.0]), np.array([-0.120, 0.00, 0.03]), 0.064, 0.058, 1.0, 1.12, 0.14),
            # 11: Right Thigh
            (np.array([0.105, 0.75, 0.0]), np.array([0.115, 0.38, 0.0]), 0.078, 0.062, 1.0, 1.0, 0.12),
            # 12: Right Celestial White-Gold Boot
            (np.array([0.115, 0.38, 0.0]), np.array([0.120, 0.00, 0.03]), 0.064, 0.058, 1.0, 1.12, 0.14),
        ])
    else:
        segments = [
            (np.array([0.0, 1.44, 0.0]), np.array([0.0, 1.70, 0.0]), 0.10, 0.11, 0.95, 1.05, 0.12),
            (np.array([0.0, 1.35, 0.0]), np.array([0.0, 1.45, 0.0]), 0.065, 0.065, 1.0, 1.0, 0.10),
            (np.array([0.0, 1.00, 0.0]), np.array([0.0, 1.36, 0.0]), 0.14, 0.19, 1.25, 0.82, 0.12),
            (np.array([0.0, 0.82, 0.0]), np.array([0.0, 1.01, 0.0]), 0.145, 0.14, 1.15, 0.85, 0.12),
        ]
        if is_tpose:
            segments.extend([
                (np.array([-0.19, 1.30, 0.0]), np.array([-0.45, 1.30, 0.0]), 0.065, 0.055, 1.0, 1.0, 0.12),
                (np.array([-0.45, 1.30, 0.0]), np.array([-0.72, 1.30, 0.0]), 0.054, 0.042, 1.0, 1.0, 0.12),
                (np.array([0.19, 1.30, 0.0]), np.array([0.45, 1.30, 0.0]), 0.065, 0.055, 1.0, 1.0, 0.12),
                (np.array([0.45, 1.30, 0.0]), np.array([0.72, 1.30, 0.0]), 0.054, 0.042, 1.0, 1.0, 0.12),
            ])
        else:
            segments.extend([
                (np.array([-0.19, 1.31, 0.0]), np.array([-0.40, 1.11, 0.0]), 0.065, 0.055, 1.0, 1.0, 0.12),
                (np.array([-0.40, 1.11, 0.0]), np.array([-0.60, 0.89, 0.02]), 0.054, 0.042, 1.0, 1.0, 0.12),
                (np.array([0.19, 1.31, 0.0]), np.array([0.40, 1.11, 0.0]), 0.065, 0.055, 1.0, 1.0, 0.12),
                (np.array([0.40, 1.11, 0.0]), np.array([0.60, 0.89, 0.02]), 0.054, 0.042, 1.0, 1.0, 0.12),
            ])
        segments.extend([
            (np.array([-0.11, 0.84, 0.0]), np.array([-0.125, 0.44, 0.0]), 0.085, 0.068, 1.0, 1.0, 0.12),
            (np.array([-0.125, 0.44, 0.0]), np.array([-0.135, 0.00, 0.02]), 0.066, 0.055, 1.0, 1.1, 0.12),
            (np.array([0.11, 0.84, 0.0]), np.array([0.125, 0.44, 0.0]), 0.085, 0.068, 1.0, 1.0, 0.12),
            (np.array([0.125, 0.44, 0.0]), np.array([0.135, 0.00, 0.02]), 0.066, 0.055, 1.0, 1.1, 0.12),
        ])

    num_segs = len(segments)
    tris_per_part = max(64, target_polycount // num_segs)
    sectors = max(8, int(round(math.sqrt(tris_per_part / 2.0))))
    rings = max(6, int(round(tris_per_part / (2.0 * sectors))))

    all_verts = []
    all_norms = []
    all_uvs = []
    all_tris = []
    vert_offset = 0

    for idx, (p0, p1, r0, r1, sx, sz, bf) in enumerate(segments):
        u_off = idx / float(num_segs)
        u_sc = 1.0 / float(num_segs)
        v, n, u, t = add_limb_tube(
            p0.astype(np.float32),
            p1.astype(np.float32),
            r0,
            r1,
            rings,
            sectors,
            u_off,
            u_sc,
            sx,
            sz,
            bf,
        )
        all_verts.append(v)
        all_norms.append(n)
        all_uvs.append(u)
        all_tris.append(t + vert_offset)
        vert_offset += len(v)

    positions = np.vstack(all_verts).astype(np.float32)
    normals = np.vstack(all_norms).astype(np.float32)
    uvs = np.vstack(all_uvs).astype(np.float32)
    triangles = np.vstack(all_tris).astype(np.uint32)

    mat_dict = {
        "name": "PaimonCelestialMaterial" if paimon_mode else "CyberKnightMaterial",
        "pbrMetallicRoughness": {
            "baseColorFactor": [1.0, 1.0, 1.0, 1.0] if paimon_mode else base_color,
            "metallicFactor": 0.15 if paimon_mode else 0.45,
            "roughnessFactor": 0.65 if paimon_mode else 0.55,
        },
    }

    gltf = {
        "asset": {"version": "2.0", "generator": "RurixForge Headless Humanoid Generator"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "PaimonMeshNode" if paimon_mode else "CharacterMeshNode", "mesh": 0}],
        "meshes": [],
        "materials": [mat_dict],
        "accessors": [],
        "bufferViews": [],
        "buffers": [{"byteLength": 0}],
    }
    bin_data = bytearray()

    pos_acc = add_accessor(gltf, bin_data, positions, "VEC3", target=34962)
    norm_acc = add_accessor(gltf, bin_data, normals, "VEC3", target=34962)
    uv_acc = add_accessor(gltf, bin_data, uvs, "VEC2", target=34962)

    flat_indices = triangles.flatten()
    if len(positions) < 65535:
        idx_arr = flat_indices.astype(np.uint16)
    else:
        idx_arr = flat_indices.astype(np.uint32)
    idx_acc = add_accessor(gltf, bin_data, idx_arr, "SCALAR", target=34963)
    gltf["accessors"][idx_acc]["min"] = [int(np.min(idx_arr))]
    gltf["accessors"][idx_acc]["max"] = [int(np.max(idx_arr))]

    # Embed 512x512 PNG UV texture atlas when in Paimon mode
    if paimon_mode:
        png_bytes = build_paimon_texture_atlas_png(num_segs)
        if png_bytes:
            bv_idx = add_buffer_view(gltf, bin_data, png_bytes)
            gltf["samplers"] = [
                {
                    "magFilter": 9729,  # LINEAR
                    "minFilter": 9729,
                    "wrapS": 10497,     # REPEAT
                    "wrapT": 33071,     # CLAMP_TO_EDGE
                }
            ]
            gltf["images"] = [
                {
                    "name": "paimon_atlas",
                    "bufferView": bv_idx,
                    "mimeType": "image/png",
                }
            ]
            gltf["textures"] = [
                {
                    "name": "paimon_atlas_tex",
                    "sampler": 0,
                    "source": 0,
                }
            ]
            gltf["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"] = {"index": 0}

    gltf["meshes"] = [
        {
            "name": "PaimonMesh" if paimon_mode else "CharacterMesh",
            "primitives": [
                {
                    "attributes": {
                        "POSITION": pos_acc,
                        "NORMAL": norm_acc,
                        "TEXCOORD_0": uv_acc,
                    },
                    "indices": idx_acc,
                    "material": 0,
                    "mode": 4,
                }
            ],
        }
    ]
    gltf["buffers"][0]["byteLength"] = len(bin_data)

    stats = {
        "ok": True,
        "vertices": int(len(positions)),
        "triangles": int(len(triangles)),
        "poseMode": "t-pose" if is_tpose else "a-pose",
        "paimonMode": paimon_mode,
        "baseColorFactor": base_color,
    }
    return gltf, bin_data, stats


def main():
    parser = argparse.ArgumentParser(description="Headless 3D Humanoid Base Mesh Generator")
    parser.add_argument("--image", default="", help="Optional input concept art PNG path")
    parser.add_argument("--prompt", default="", help="Optional character prompt")
    parser.add_argument("--pose-mode", default="a-pose", help="Pose mode: a-pose or t-pose")
    parser.add_argument("--target-polycount", type=int, default=8000, help="Target triangle count")
    parser.add_argument("--output", required=True, help="Output GLB file path")
    args = parser.parse_args()

    paimon = is_paimon_request(args.prompt, args.image)
    base_color = extract_concept_color(args.image)
    gltf, bin_data, stats = generate_humanoid_glb(
        target_polycount=args.target_polycount,
        pose_mode=args.pose_mode,
        base_color=base_color,
        paimon_mode=paimon,
    )

    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    save_glb(str(out_path), gltf, bin_data)
    print(json.dumps(stats))


if __name__ == "__main__":
    main()
