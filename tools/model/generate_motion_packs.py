#!/usr/bin/env python3
"""
tools/model/generate_motion_packs.py
Generates standard humanoid motion pack GLB assets:
  - assets/motion-packs/humanoid/idle.glb
  - assets/motion-packs/humanoid/walk.glb
  - assets/motion-packs/humanoid/run.glb
  - assets/motion-packs/humanoid/attack.glb
"""

import math
import os
import sys
import numpy as np

try:
    from glb_utils import save_glb, add_accessor
    from auto_rig import HUMANOID_BONES
except ImportError:
    from tools.model.glb_utils import save_glb, add_accessor
    from tools.model.auto_rig import HUMANOID_BONES


def quat_from_axis_angle(axis, angle_rad):
    axis = np.array(axis, dtype=np.float32)
    norm = np.linalg.norm(axis)
    if norm > 0:
        axis /= norm
    half = angle_rad * 0.5
    s = math.sin(half)
    c = math.cos(half)
    q = np.array([axis[0] * s, axis[1] * s, axis[2] * s, c], dtype=np.float32)
    q /= np.linalg.norm(q)
    return q


def quat_mult(q1, q2):
    x1, y1, z1, w1 = q1
    x2, y2, z2, w2 = q2
    return np.array(
        [
            w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2,
            w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2,
            w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2,
            w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2,
        ],
        dtype=np.float32,
    )


def create_base_skeleton_gltf():
    gltf = {
        "asset": {"version": "2.0", "generator": "RurixForge Motion Pack Generator"},
        "scenes": [{"nodes": [0]}],
        "scene": 0,
        "nodes": [],
        "animations": [],
        "bufferViews": [],
        "accessors": [],
        "buffers": [{"byteLength": 0}],
    }
    children_map = {i: [] for i in range(len(HUMANOID_BONES))}
    for idx, (name, parent, rel) in enumerate(HUMANOID_BONES):
        if parent is not None:
            children_map[parent].append(idx)

    for idx, (name, parent, rel) in enumerate(HUMANOID_BONES):
        node = {
            "name": name,
            "translation": rel.tolist(),
            "rotation": [0.0, 0.0, 0.0, 1.0],
            "scale": [1.0, 1.0, 1.0],
        }
        if children_map[idx]:
            node["children"] = children_map[idx]
        gltf["nodes"].append(node)

    return gltf


def build_clip(clip_name, duration, keyframes, bone_tracks):
    """
    keyframes: strictly increasing float timestamps [t0, t1, ...]
    bone_tracks: dict mapping bone_name -> dict(
        "rotation": list of [x, y, z, w],
        "translation": list of [x, y, z] (optional)
    )
    """
    gltf = create_base_skeleton_gltf()
    bin_data = bytearray()

    name_to_node = {node["name"]: i for i, node in enumerate(gltf["nodes"])}

    time_arr = np.array(keyframes, dtype=np.float32)
    time_acc = add_accessor(gltf, bin_data, time_arr, "SCALAR")

    samplers = []
    channels = []

    for bone_name, track_dict in bone_tracks.items():
        if bone_name not in name_to_node:
            continue
        node_idx = name_to_node[bone_name]

        if "rotation" in track_dict:
            rot_vals = np.array(track_dict["rotation"], dtype=np.float32)
            rot_acc = add_accessor(gltf, bin_data, rot_vals, "VEC4")
            s_idx = len(samplers)
            samplers.append({
                "input": time_acc,
                "interpolation": "LINEAR",
                "output": rot_acc,
            })
            channels.append({
                "sampler": s_idx,
                "target": {"node": node_idx, "path": "rotation"},
            })

        if "translation" in track_dict:
            trans_vals = np.array(track_dict["translation"], dtype=np.float32)
            trans_acc = add_accessor(gltf, bin_data, trans_vals, "VEC3")
            s_idx = len(samplers)
            samplers.append({
                "input": time_acc,
                "interpolation": "LINEAR",
                "output": trans_acc,
            })
            channels.append({
                "sampler": s_idx,
                "target": {"node": node_idx, "path": "translation"},
            })

    gltf["animations"] = [{
        "name": clip_name,
        "channels": channels,
        "samplers": samplers,
    }]
    gltf["buffers"][0]["byteLength"] = len(bin_data)
    return gltf, bin_data


def generate_motion_packs(output_dir):
    os.makedirs(output_dir, exist_ok=True)
    identity_q = [0.0, 0.0, 0.0, 1.0]

    # 1. Idle (1.6s, breathing, subtle chest expand and hip sway)
    idle_times = [0.0, 0.4, 0.8, 1.2, 1.6]
    idle_tracks = {
        "Hips": {
            "translation": [
                [0.0, 0.550, 0.0],
                [0.0, 0.554, 0.0],
                [0.0, 0.557, 0.0],
                [0.0, 0.553, 0.0],
                [0.0, 0.550, 0.0],
            ],
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 1, 0], 0.01).tolist(),
                identity_q,
                quat_from_axis_angle([0, 1, 0], -0.01).tolist(),
                identity_q,
            ],
        },
        "Spine1": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([1, 0, 0], 0.015).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.030).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.015).tolist(),
                identity_q,
            ]
        },
        "Neck": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 0, 1], 0.01).tolist(),
                identity_q,
                quat_from_axis_angle([0, 0, 1], -0.01).tolist(),
                identity_q,
            ]
        },
        "LeftArm": {
            "rotation": [
                quat_from_axis_angle([0, 0, 1], -0.10).tolist(),
                quat_from_axis_angle([0, 0, 1], -0.11).tolist(),
                quat_from_axis_angle([0, 0, 1], -0.12).tolist(),
                quat_from_axis_angle([0, 0, 1], -0.11).tolist(),
                quat_from_axis_angle([0, 0, 1], -0.10).tolist(),
            ]
        },
        "RightArm": {
            "rotation": [
                quat_from_axis_angle([0, 0, 1], 0.10).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.11).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.12).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.11).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.10).tolist(),
            ]
        },
    }
    g_idle, b_idle = build_clip("Idle", 1.6, idle_times, idle_tracks)
    save_glb(os.path.join(output_dir, "idle.glb"), g_idle, b_idle)

    # 2. Walk (1.2s, standard alternating gait with noticeable leg and arm swings)
    walk_times = [0.0, 0.3, 0.6, 0.9, 1.2]
    walk_tracks = {
        "Hips": {
            "translation": [
                [0.0, 0.550, 0.0],
                [0.0, 0.535, 0.0],
                [0.0, 0.550, 0.0],
                [0.0, 0.535, 0.0],
                [0.0, 0.550, 0.0],
            ],
            "rotation": [
                quat_from_axis_angle([0, 1, 0], -0.05).tolist(),
                identity_q,
                quat_from_axis_angle([0, 1, 0], 0.05).tolist(),
                identity_q,
                quat_from_axis_angle([0, 1, 0], -0.05).tolist(),
            ],
        },
        "LeftUpLeg": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], 0.38).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.38).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.38).tolist(),
            ]
        },
        "RightUpLeg": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], -0.38).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.38).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.38).tolist(),
            ]
        },
        "LeftLeg": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], 0.05).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.45).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.05).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.15).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.05).tolist(),
            ]
        },
        "RightLeg": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], 0.05).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.15).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.05).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.45).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.05).tolist(),
            ]
        },
        "LeftArm": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], -0.30).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.30).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.30).tolist(),
            ]
        },
        "RightArm": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], 0.30).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.30).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.30).tolist(),
            ]
        },
    }
    g_walk, b_walk = build_clip("Walk", 1.2, walk_times, walk_tracks)
    save_glb(os.path.join(output_dir, "walk.glb"), g_walk, b_walk)

    # 3. Run (0.8s, fast running cycle)
    run_times = [0.0, 0.2, 0.4, 0.6, 0.8]
    run_tracks = {
        "Hips": {
            "translation": [
                [0.0, 0.550, 0.0],
                [0.0, 0.520, 0.0],
                [0.0, 0.550, 0.0],
                [0.0, 0.520, 0.0],
                [0.0, 0.550, 0.0],
            ]
        },
        "LeftUpLeg": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], 0.60).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.60).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.60).tolist(),
            ]
        },
        "RightUpLeg": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], -0.60).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.60).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.60).tolist(),
            ]
        },
        "LeftArm": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], -0.50).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.50).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.50).tolist(),
            ]
        },
        "RightArm": {
            "rotation": [
                quat_from_axis_angle([1, 0, 0], 0.50).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.50).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.0).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.50).tolist(),
            ]
        },
    }
    g_run, b_run = build_clip("Run", 0.8, run_times, run_tracks)
    save_glb(os.path.join(output_dir, "run.glb"), g_run, b_run)

    # 4. Attack (1.0s, melee slash/strike)
    attack_times = [0.0, 0.25, 0.5, 0.75, 1.0]
    attack_tracks = {
        "Hips": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 1, 0], -0.20).tolist(),
                quat_from_axis_angle([0, 1, 0], 0.35).tolist(),
                quat_from_axis_angle([0, 1, 0], 0.10).tolist(),
                identity_q,
            ]
        },
        "Spine1": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 1, 0], -0.25).tolist(),
                quat_from_axis_angle([0, 1, 0], 0.40).tolist(),
                quat_from_axis_angle([0, 1, 0], 0.10).tolist(),
                identity_q,
            ]
        },
        "RightArm": {
            "rotation": [
                quat_from_axis_angle([0, 0, 1], 0.10).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.80).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.90).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.30).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.10).tolist(),
            ]
        },
    }
    g_atk, b_atk = build_clip("Attack", 1.0, attack_times, attack_tracks)
    save_glb(os.path.join(output_dir, "attack.glb"), g_atk, b_atk)

    # 5. HeartGesture (2.4s, cute Paimon floating heart gesture / 比心动作)
    # Arms swing forward (+Z) and curve inward so LeftHand and RightHand meet in front of chest/face
    # while Paimon floats up/down, tilts her head cutely, and kicks her legs playfully.
    heart_times = [0.0, 0.4, 0.8, 1.2, 1.6, 2.0, 2.4]
    q_la_raise = quat_mult(
        quat_from_axis_angle([0, 1, 0], 1.02),
        quat_from_axis_angle([0, 0, 1], -0.22),
    ).tolist()
    q_la_pulse = quat_mult(
        quat_from_axis_angle([0, 1, 0], 1.08),
        quat_from_axis_angle([0, 0, 1], -0.16),
    ).tolist()
    q_lfa_heart = quat_mult(
        quat_from_axis_angle([0, 1, 0], 1.48),
        quat_from_axis_angle([0, 0, 1], 0.35),
    ).tolist()
    q_lfa_pulse = quat_mult(
        quat_from_axis_angle([0, 1, 0], 1.56),
        quat_from_axis_angle([0, 0, 1], 0.42),
    ).tolist()

    q_ra_raise = quat_mult(
        quat_from_axis_angle([0, 1, 0], -1.02),
        quat_from_axis_angle([0, 0, 1], 0.22),
    ).tolist()
    q_ra_pulse = quat_mult(
        quat_from_axis_angle([0, 1, 0], -1.08),
        quat_from_axis_angle([0, 0, 1], 0.16),
    ).tolist()
    q_rfa_heart = quat_mult(
        quat_from_axis_angle([0, 1, 0], -1.48),
        quat_from_axis_angle([0, 0, 1], -0.35),
    ).tolist()
    q_rfa_pulse = quat_mult(
        quat_from_axis_angle([0, 1, 0], -1.56),
        quat_from_axis_angle([0, 0, 1], -0.42),
    ).tolist()

    heart_tracks = {
        "Hips": {
            "translation": [
                [0.0, 0.550, 0.0],
                [0.0, 0.585, 0.0],
                [0.0, 0.625, 0.02],
                [0.0, 0.605, 0.02],
                [0.0, 0.635, 0.02],
                [0.0, 0.580, 0.01],
                [0.0, 0.550, 0.0],
            ],
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 1, 0], 0.08).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.06).tolist(),
                quat_from_axis_angle([0, 0, 1], -0.06).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.06).tolist(),
                quat_from_axis_angle([0, 1, 0], -0.05).tolist(),
                identity_q,
            ],
        },
        "Spine1": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([1, 0, 0], 0.06).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.10).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.07).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.11).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.04).tolist(),
                identity_q,
            ]
        },
        "Head": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 0, 1], 0.10).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.20).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.14).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.22).tolist(),
                quat_from_axis_angle([0, 0, 1], 0.08).tolist(),
                identity_q,
            ]
        },
        "LeftArm": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 1, 0], 0.52).tolist(),
                q_la_raise,
                q_la_pulse,
                q_la_raise,
                quat_from_axis_angle([0, 1, 0], 0.52).tolist(),
                identity_q,
            ]
        },
        "LeftForeArm": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 1, 0], 0.65).tolist(),
                q_lfa_heart,
                q_lfa_pulse,
                q_lfa_heart,
                quat_from_axis_angle([0, 1, 0], 0.65).tolist(),
                identity_q,
            ]
        },
        "RightArm": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 1, 0], -0.52).tolist(),
                q_ra_raise,
                q_ra_pulse,
                q_ra_raise,
                quat_from_axis_angle([0, 1, 0], -0.52).tolist(),
                identity_q,
            ]
        },
        "RightForeArm": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([0, 1, 0], -0.65).tolist(),
                q_rfa_heart,
                q_rfa_pulse,
                q_rfa_heart,
                quat_from_axis_angle([0, 1, 0], -0.65).tolist(),
                identity_q,
            ]
        },
        "LeftUpLeg": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([1, 0, 0], 0.20).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.35).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.15).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.38).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.12).tolist(),
                identity_q,
            ]
        },
        "RightUpLeg": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([1, 0, 0], -0.15).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.32).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.18).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.35).tolist(),
                quat_from_axis_angle([1, 0, 0], -0.10).tolist(),
                identity_q,
            ]
        },
        "LeftLeg": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([1, 0, 0], 0.25).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.50).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.30).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.52).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.20).tolist(),
                identity_q,
            ]
        },
        "RightLeg": {
            "rotation": [
                identity_q,
                quat_from_axis_angle([1, 0, 0], 0.30).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.58).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.35).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.60).tolist(),
                quat_from_axis_angle([1, 0, 0], 0.22).tolist(),
                identity_q,
            ]
        },
    }
    g_heart, b_heart = build_clip("HeartGesture", 2.4, heart_times, heart_tracks)
    save_glb(os.path.join(output_dir, "heart_gesture.glb"), g_heart, b_heart)

    print(
        f"Motion packs generated in {output_dir}: idle.glb, walk.glb, run.glb, attack.glb, heart_gesture.glb"
    )


if __name__ == "__main__":
    out_dir = sys.argv[1] if len(sys.argv) > 1 else "assets/motion-packs/humanoid"
    generate_motion_packs(out_dir)

