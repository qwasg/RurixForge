#!/usr/bin/env python3
"""
tools/model/merge_motion.py
Headless animation track merger for RurixForge 3D character pipeline.
Merges standard humanoid animation tracks (Idle, Walk, Run, Attack, etc.) into
the rigged character GLB with strictly increasing keyframe times and non-zero duration,
printing JSON statistics.
"""

import argparse
import glob
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


def normalize_bone_name(name):
    clean = name
    for prefix in ["mixamorig:", "mixamorig_", "Bip01_", "Bip01 "]:
        if clean.lower().startswith(prefix.lower()):
            clean = clean[len(prefix) :]
            break
    return clean.strip().lower()


def build_node_name_map(nodes):
    mapping = {}
    for idx, node in enumerate(nodes):
        raw_name = node.get("name", "")
        if raw_name:
            mapping[raw_name] = idx
            mapping[normalize_bone_name(raw_name)] = idx
    return mapping


def find_target_node(mapping, clip_node_name):
    if clip_node_name in mapping:
        return mapping[clip_node_name]
    norm = normalize_bone_name(clip_node_name)
    if norm in mapping:
        return mapping[norm]
    # Common synonyms
    synonyms = {
        "chest": "spine1",
        "upperchest": "spine1",
        "spine2": "spine1",
        "pelvis": "hips",
        "root": "hips",
    }
    if norm in synonyms and synonyms[norm] in mapping:
        return mapping[synonyms[norm]]
    return None


def normalize_clip_name(raw_name, fallback_stem=""):
    clean = raw_name.strip()
    if "|" in clean:
        parts = [p for p in clean.split("|") if p and p.lower() != "armature" and p.lower() != "baselayer"]
        if parts:
            clean = parts[0]
    low = clean.lower()
    stem_low = fallback_stem.lower()
    if low in ("idle", "stand", "breathing_idle") or "idle" in stem_low:
        return "Idle"
    if low in ("walk", "walking", "walking_man") or "walk" in stem_low:
        return "Walk"
    if low in ("run", "running", "running_man") or "run" in stem_low:
        return "Run"
    if "heart" in low or "heart" in stem_low:
        return "HeartGesture"
    if "attack" in low or "attack" in stem_low:
        return "Attack"
    return clean


def merge_motion(character_path, motion_pack_dir, output_path):
    char_gltf, char_bin = load_glb(character_path)
    char_nodes = char_gltf.get("nodes", [])
    char_map = build_node_name_map(char_nodes)

    # Filter out Meshy rigging placeholder clip0 animations
    existing_anims = char_gltf.get("animations", [])
    char_gltf["animations"] = [
        a for a in existing_anims if "clip0" not in a.get("name", "").lower()
    ]

    # Find motion clip GLBs (supports comma-separated GLB list, single GLB, or directory)
    if "," in motion_pack_dir:
        clip_paths = [p.strip() for p in motion_pack_dir.split(",") if p.strip() and os.path.isfile(p.strip())]
    elif os.path.isfile(motion_pack_dir) and motion_pack_dir.endswith(".glb"):
        clip_paths = [motion_pack_dir]
    else:
        # Standard ordering priority
        all_files = glob.glob(os.path.join(motion_pack_dir, "*.glb"))
        priority = ["idle.glb", "walk.glb", "run.glb", "attack.glb", "heart_gesture.glb"]
        clip_paths = []
        for p in priority:
            match = [f for f in all_files if os.path.basename(f).lower() == p]
            if match:
                clip_paths.append(match[0])
        for f in all_files:
            if f not in clip_paths:
                clip_paths.append(f)

    if not clip_paths:
        raise ValueError(f"No GLB motion clips found in {motion_pack_dir}")

    merged_animation_names = [a.get("name", "") for a in char_gltf["animations"]]

    for clip_path in clip_paths:
        clip_gltf, clip_bin = load_glb(clip_path)
        clip_nodes = clip_gltf.get("nodes", [])
        stem = os.path.splitext(os.path.basename(clip_path))[0]

        for anim in clip_gltf.get("animations", []):
            raw_clip_name = anim.get("name", stem.capitalize())
            if "clip0" in raw_clip_name.lower():
                continue
            clip_name = normalize_clip_name(raw_clip_name, stem)

            # Map samplers
            sampler_map = {}
            new_samplers = []
            for s_idx, sampler in enumerate(anim.get("samplers", [])):
                time_arr = get_accessor_data(clip_gltf, clip_bin, sampler["input"])
                val_acc_info = clip_gltf["accessors"][sampler["output"]]
                val_type = val_acc_info["type"]
                val_arr = get_accessor_data(clip_gltf, clip_bin, sampler["output"])

                # Enforce strictly increasing timeframes
                if len(time_arr) > 1 and not np.all(np.diff(time_arr) > 0):
                    # Fix any non-monotonic timestamps
                    eps = 1e-4
                    for t_i in range(1, len(time_arr)):
                        if time_arr[t_i] <= time_arr[t_i - 1]:
                            time_arr[t_i] = time_arr[t_i - 1] + eps

                time_acc = add_accessor(char_gltf, char_bin, time_arr, "SCALAR")
                val_acc = add_accessor(char_gltf, char_bin, val_arr, val_type)

                new_s_idx = len(new_samplers)
                new_samplers.append({
                    "input": time_acc,
                    "interpolation": sampler.get("interpolation", "LINEAR"),
                    "output": val_acc,
                })
                sampler_map[s_idx] = new_s_idx

            # Map channels
            new_channels = []
            for ch in anim.get("channels", []):
                old_s_idx = ch["sampler"]
                if old_s_idx not in sampler_map:
                    continue
                target = ch["target"]
                clip_node_idx = target.get("node")
                if clip_node_idx is None or clip_node_idx >= len(clip_nodes):
                    continue
                clip_node_name = clip_nodes[clip_node_idx].get("name", "")
                target_char_node = find_target_node(char_map, clip_node_name)

                if target_char_node is not None:
                    new_channels.append({
                        "sampler": sampler_map[old_s_idx],
                        "target": {
                            "node": target_char_node,
                            "path": target["path"],
                        },
                    })

            if new_channels:
                char_gltf["animations"].append({
                    "name": clip_name,
                    "channels": new_channels,
                    "samplers": new_samplers,
                })
                merged_animation_names.append(clip_name)

    char_gltf["buffers"][0]["byteLength"] = len(char_bin)
    os.makedirs(os.path.dirname(os.path.abspath(output_path)), exist_ok=True)
    save_glb(output_path, char_gltf, char_bin)

    stats = {
        "animations": merged_animation_names,
    }
    print(json.dumps(stats))
    return stats


def main():
    parser = argparse.ArgumentParser(description="Humanoid Motion Pack Merge Tool")
    parser.add_argument("--character", required=True, help="Path to character GLB")
    parser.add_argument("--motion-pack", required=True, help="Path to motion pack directory or GLB")
    parser.add_argument("--output", required=True, help="Path to output animated character GLB")
    args = parser.parse_args()

    merge_motion(args.character, args.motion_pack, args.output)


if __name__ == "__main__":
    main()
