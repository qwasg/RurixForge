#!/usr/bin/env python3
"""
tools/model/auto_rig.py
Headless humanoid auto-rigging tool for RurixForge 3D character pipeline.
Binds standard humanoid skeleton, non-empty skins, matching inverseBindMatrices,
and strictly normalized 4-bone weights (JOINTS_0, WEIGHTS_0), printing JSON statistics.
"""

import argparse
import json
import os
import sys
import numpy as np

try:
    from glb_utils import (
        load_glb,
        save_glb,
        get_accessor_data,
        add_accessor,
    )
except ImportError:
    from tools.model.glb_utils import (
        load_glb,
        save_glb,
        get_accessor_data,
        add_accessor,
    )

# Standard humanoid joint definitions
# (name, parent_index_or_None, rel_pos_scale)
HUMANOID_BONES = [
    ("Hips", None, np.array([0.0, 0.55, 0.0], dtype=np.float32)),
    ("Spine", 0, np.array([0.0, 0.12, 0.0], dtype=np.float32)),
    ("Spine1", 1, np.array([0.0, 0.12, 0.0], dtype=np.float32)),
    ("Neck", 2, np.array([0.0, 0.10, 0.0], dtype=np.float32)),
    ("Head", 3, np.array([0.0, 0.08, 0.0], dtype=np.float32)),
    ("LeftShoulder", 2, np.array([-0.06, 0.08, 0.0], dtype=np.float32)),
    ("LeftArm", 5, np.array([-0.12, 0.0, 0.0], dtype=np.float32)),
    ("LeftForeArm", 6, np.array([-0.14, 0.0, 0.0], dtype=np.float32)),
    ("LeftHand", 7, np.array([-0.10, 0.0, 0.0], dtype=np.float32)),
    ("RightShoulder", 2, np.array([0.06, 0.08, 0.0], dtype=np.float32)),
    ("RightArm", 9, np.array([0.12, 0.0, 0.0], dtype=np.float32)),
    ("RightForeArm", 10, np.array([0.14, 0.0, 0.0], dtype=np.float32)),
    ("RightHand", 11, np.array([0.10, 0.0, 0.0], dtype=np.float32)),
    ("LeftUpLeg", 0, np.array([-0.10, -0.05, 0.0], dtype=np.float32)),
    ("LeftLeg", 13, np.array([0.0, -0.22, 0.0], dtype=np.float32)),
    ("LeftFoot", 14, np.array([0.0, -0.20, 0.06], dtype=np.float32)),
    ("RightUpLeg", 0, np.array([0.10, -0.05, 0.0], dtype=np.float32)),
    ("RightLeg", 16, np.array([0.0, -0.22, 0.0], dtype=np.float32)),
    ("RightFoot", 17, np.array([0.0, -0.20, 0.06], dtype=np.float32)),
]


def compute_world_positions(bones, height):
    world_pos = []
    for idx, (name, parent, rel) in enumerate(bones):
        scaled_rel = rel * height
        if parent is None:
            world_pos.append(scaled_rel)
        else:
            world_pos.append(world_pos[parent] + scaled_rel)
    return np.array(world_pos, dtype=np.float32)


def compute_inverse_bind_matrices(world_positions):
    num_joints = len(world_positions)
    ibms = np.zeros((num_joints, 4, 4), dtype=np.float32)
    for i, w in enumerate(world_positions):
        # Inverse of translation matrix [I | w] is [I | -w]
        ibms[i] = np.identity(4, dtype=np.float32)
        ibms[i, 0, 3] = -w[0]
        ibms[i, 1, 3] = -w[1]
        ibms[i, 2, 3] = -w[2]
    # Return as column-major flattened array for glTF MAT4
    return np.transpose(ibms, (0, 2, 1)).reshape((num_joints, 16))


def calculate_skin_weights(positions, world_positions):
    """
    Computes 4-bone normalized influences for each vertex.
    Returns:
        joints: (N, 4) uint16
        weights: (N, 4) float32, sum to 1.0
    """
    N = len(positions)
    J = len(world_positions)

    # Distances between all vertices and all joints: (N, J)
    diff = positions[:, np.newaxis, :] - world_positions[np.newaxis, :, :]
    dists = np.linalg.norm(diff, axis=2)

    # Weights proportional to 1 / (dist + epsilon)^2
    raw_weights = 1.0 / (dists + 0.05) ** 2

    # Find top 4 joints for each vertex
    top4_indices = np.argsort(-raw_weights, axis=1)[:, :4]
    row_indices = np.arange(N)[:, np.newaxis]
    top4_weights = raw_weights[row_indices, top4_indices]

    # Strictly normalize weights to sum to 1.0
    sums = np.sum(top4_weights, axis=1, keepdims=True)
    sums[sums == 0] = 1.0
    normalized_weights = top4_weights / sums

    return top4_indices.astype(np.uint16), normalized_weights.astype(np.float32)


def auto_rig(input_path, output_path, target_height=None):
    gltf, bin_data = load_glb(input_path)

    mesh = gltf["meshes"][0]
    prim = mesh["primitives"][0]
    pos_acc = prim["attributes"]["POSITION"]
    positions = get_accessor_data(gltf, bin_data, pos_acc).astype(np.float32)

    # Compute mesh height from bounding box
    bbox_min = np.min(positions, axis=0)
    bbox_max = np.max(positions, axis=0)
    mesh_height = float(bbox_max[1] - bbox_min[1])
    if mesh_height <= 0.05:
        mesh_height = 1.7

    height = target_height if target_height and target_height > 0 else mesh_height

    # Skeleton world positions aligned to character base
    world_pos = compute_world_positions(HUMANOID_BONES, height)
    # Offset skeleton Y to match mesh bbox_min[1]
    world_pos[:, 1] += bbox_min[1]

    # Calculate bone weights and inverse bind matrices
    joints_0, weights_0 = calculate_skin_weights(positions, world_pos)
    ibms = compute_inverse_bind_matrices(world_pos)

    # Construct joint nodes in gltf
    if "nodes" not in gltf or not gltf["nodes"]:
        gltf["nodes"] = [{"name": "MeshNode", "mesh": 0}]
        mesh_node_idx = 0
    else:
        found_mesh_node = None
        for idx, n in enumerate(gltf["nodes"]):
            if "mesh" in n:
                found_mesh_node = idx
                break
        if found_mesh_node is not None:
            mesh_node_idx = found_mesh_node
        else:
            gltf["nodes"][0]["mesh"] = 0
            mesh_node_idx = 0

    joint_node_offset = len(gltf["nodes"])

    joint_node_indices = []
    children_map = {i: [] for i in range(len(HUMANOID_BONES))}
    for idx, (name, parent, rel) in enumerate(HUMANOID_BONES):
        if parent is not None:
            children_map[parent].append(joint_node_offset + idx)

    for idx, (name, parent, rel) in enumerate(HUMANOID_BONES):
        node_idx = joint_node_offset + idx
        joint_node_indices.append(node_idx)
        scaled_rel = (rel * height).tolist()
        if parent is None:
            # Add base Y offset to root
            scaled_rel[1] += float(bbox_min[1])

        node = {
            "name": name,
            "translation": scaled_rel,
            "rotation": [0.0, 0.0, 0.0, 1.0],
            "scale": [1.0, 1.0, 1.0],
        }
        if children_map[idx]:
            node["children"] = children_map[idx]
        gltf["nodes"].append(node)

    # Attach root joint node to default scene
    root_joint_node = joint_node_indices[0]
    if "scenes" not in gltf or not gltf["scenes"]:
        gltf["scenes"] = [{"nodes": [mesh_node_idx, root_joint_node]}]
    else:
        scene = gltf["scenes"][gltf.get("scene", 0)]
        if "nodes" not in scene:
            scene["nodes"] = []
        if mesh_node_idx not in scene["nodes"]:
            scene["nodes"].append(mesh_node_idx)
        if root_joint_node not in scene["nodes"]:
            scene["nodes"].append(root_joint_node)

    # Add accessors for IBM, JOINTS_0, WEIGHTS_0
    ibm_acc = add_accessor(gltf, bin_data, ibms, "MAT4")
    joints_acc = add_accessor(gltf, bin_data, joints_0, "VEC4", target=34962)
    weights_acc = add_accessor(gltf, bin_data, weights_0, "VEC4", target=34962)

    # Create skin
    skin_idx = len(gltf.get("skins", []))
    skin = {
        "name": "HumanoidSkin",
        "inverseBindMatrices": ibm_acc,
        "joints": joint_node_indices,
        "skeleton": root_joint_node,
    }
    if "skins" not in gltf:
        gltf["skins"] = []
    gltf["skins"].append(skin)

    # Attach skin and skin attributes to mesh node and primitive
    gltf["nodes"][mesh_node_idx]["skin"] = skin_idx
    prim["attributes"]["JOINTS_0"] = joints_acc
    prim["attributes"]["WEIGHTS_0"] = weights_acc

    gltf["buffers"][0]["byteLength"] = len(bin_data)

    os.makedirs(os.path.dirname(os.path.abspath(output_path)), exist_ok=True)
    save_glb(output_path, gltf, bin_data)

    stats = {
        "jointsCount": int(len(joint_node_indices)),
        "normalizedWeights": True,
    }
    print(json.dumps(stats))
    return stats


def main():
    parser = argparse.ArgumentParser(description="Humanoid Auto-Rigging Tool")
    parser.add_argument("--input", required=True, help="Path to input GLB")
    parser.add_argument("--output", required=True, help="Path to output rigged GLB")
    parser.add_argument("--height", type=float, default=1.7, help="Character height in meters")
    args = parser.parse_args()

    auto_rig(args.input, args.output, args.height)


if __name__ == "__main__":
    main()
