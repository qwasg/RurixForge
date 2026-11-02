#!/usr/bin/env python3
"""
tools/model/assemble_head_and_body.py
Splices a separately modeled high-detail 3D Anime Head GLB onto a rigged Full-Body GLB.
Preserves full 2K texture resolution for both the body and the dedicated head via a side-by-side
dual texture atlas, and binds the high-detail head to the Head/Neck joints with normalized weights.
"""

import argparse
import io
import json
import os
import numpy as np
from PIL import Image

try:
    from glb_utils import load_glb, save_glb, get_accessor_data, add_accessor
except ImportError:
    from tools.model.glb_utils import load_glb, save_glb, get_accessor_data, add_accessor


def extract_base_color_image(gltf, bin_data):
    if not gltf.get("images"):
        return Image.new("RGB", (1024, 1024), (240, 240, 240)), 0
    base_img_idx = 0
    mats = gltf.get("materials", [])
    if mats:
        bct = mats[0].get("pbrMetallicRoughness", {}).get("baseColorTexture", {})
        tex_idx = bct.get("index")
        if tex_idx is not None and tex_idx < len(gltf.get("textures", [])):
            src_idx = gltf["textures"][tex_idx].get("source")
            if src_idx is not None and src_idx < len(gltf["images"]):
                base_img_idx = src_idx
    img_info = gltf["images"][base_img_idx]
    bv = gltf["bufferViews"][img_info["bufferView"]]
    start = bv.get("byteOffset", 0)
    raw = bytes(bin_data[start : start + bv["byteLength"]])
    return Image.open(io.BytesIO(raw)).convert("RGB"), base_img_idx


def assemble_head_and_body(
    body_glb_path,
    head_glb_path,
    output_glb_path,
    highres_body_glb_path=None,
    collar_trim_ratio=0.34,
):
    rig_gltf, rig_bin = load_glb(body_glb_path)
    head_gltf, head_bin = load_glb(head_glb_path)

    r_prim = rig_gltf["meshes"][0]["primitives"][0]
    r_pos = get_accessor_data(rig_gltf, rig_bin, r_prim["attributes"]["POSITION"]).astype(np.float32)
    r_joints = get_accessor_data(rig_gltf, rig_bin, r_prim["attributes"]["JOINTS_0"]).astype(np.uint16)
    r_weights = get_accessor_data(rig_gltf, rig_bin, r_prim["attributes"]["WEIGHTS_0"]).astype(np.float32)

    if highres_body_glb_path and os.path.isfile(highres_body_glb_path):
        hr_gltf, hr_bin = load_glb(highres_body_glb_path)
        hr_prim = hr_gltf["meshes"][0]["primitives"][0]
        b_pos_raw = get_accessor_data(hr_gltf, hr_bin, hr_prim["attributes"]["POSITION"]).astype(np.float32)
        b_norm = get_accessor_data(hr_gltf, hr_bin, hr_prim["attributes"]["NORMAL"]).astype(np.float32)
        b_uv = get_accessor_data(hr_gltf, hr_bin, hr_prim["attributes"]["TEXCOORD_0"]).astype(np.float32)
        b_tris = get_accessor_data(hr_gltf, hr_bin, hr_prim["indices"]).astype(np.int32).reshape(-1, 3)

        # Align high-res body mesh into the exact metric coordinate box of the rigged body
        r_min, r_max = r_pos.min(axis=0), r_pos.max(axis=0)
        b_min, b_max = b_pos_raw.min(axis=0), b_pos_raw.max(axis=0)
        b_pos = (b_pos_raw - b_min) / np.maximum(b_max - b_min, 1e-6) * (r_max - r_min) + r_min

        # Transfer exact 4-bone normalized skinning weights via nearest-neighbor lookup on rigged mesh
        dists_sq = np.sum((b_pos[:, None, :] - r_pos[None, :, :]) ** 2, axis=2)
        nn_idx = np.argmin(dists_sq, axis=1)
        b_joints = r_joints[nn_idx].copy()
        b_weights = r_weights[nn_idx].copy()
        body_img, _ = extract_base_color_image(hr_gltf, hr_bin)
        _, body_img_idx = extract_base_color_image(rig_gltf, rig_bin)
    else:
        b_pos = r_pos
        b_norm = get_accessor_data(rig_gltf, rig_bin, r_prim["attributes"]["NORMAL"]).astype(np.float32)
        b_uv = get_accessor_data(rig_gltf, rig_bin, r_prim["attributes"]["TEXCOORD_0"]).astype(np.float32)
        b_joints = r_joints
        b_weights = r_weights
        b_tris = get_accessor_data(rig_gltf, rig_bin, r_prim["indices"]).astype(np.int32).reshape(-1, 3)
        body_img, body_img_idx = extract_base_color_image(rig_gltf, rig_bin)

    # Identify Head and Neck joint indices in skin['joints']
    skin = rig_gltf["skins"][0]
    joint_nodes = skin["joints"]
    head_j_idx = 0
    neck_j_idx = 0
    head_family_indices = []
    for j_i, n_idx in enumerate(joint_nodes):
        j_name = rig_gltf["nodes"][n_idx].get("name", "").lower()
        if j_name == "head":
            head_j_idx = j_i
            head_family_indices.append(j_i)
        elif "head" in j_name:
            head_family_indices.append(j_i)
        elif "neck" in j_name:
            neck_j_idx = j_i
            head_family_indices.append(j_i)
    if neck_j_idx == 0 and head_j_idx > 0:
        neck_j_idx = head_j_idx

    # Compute per-vertex weight on Head + Neck bones
    head_neck_w = np.sum(np.where(np.isin(b_joints, head_family_indices), b_weights, 0.0), axis=1)
    b_ymin, b_ymax = float(b_pos[:, 1].min()), float(b_pos[:, 1].max())
    b_height = b_ymax - b_ymin

    # On the body mesh (1.78m), y=1.43..1.54m is the cylindrical neck inside the collar, and y=1.53m is the chin tip.
    # Keep the body's entire shirt collar and neck cylinder up to y=1.545m (trimming only the old face/hair above y=1.545m,
    # and trimming the old chin tip at z > 0.042m above y=1.520m).
    neck_keep_top_y = b_ymin + 0.868 * b_height  # ~1.545m
    chin_cut_y = b_ymin + 0.854 * b_height       # ~1.520m
    tri_y_mean = np.mean(b_pos[b_tris, 1], axis=1)
    tri_z_mean = np.mean(b_pos[b_tris, 2], axis=1)
    keep_tri_mask = ~(
        (tri_y_mean >= neck_keep_top_y)
        | ((tri_y_mean >= chin_cut_y) & (tri_z_mean >= 0.042))
    )
    kept_b_tris = b_tris[keep_tri_mask]

    # Reference center of the body's upper neck / head region
    neck_ref_mask = (b_pos[:, 1] >= b_ymin + 0.82 * b_height) & (b_pos[:, 1] <= b_ymin + 0.96 * b_height)
    neck_ref_pts = b_pos[neck_ref_mask]
    target_cx = float(0.5 * (neck_ref_pts[:, 0].min() + neck_ref_pts[:, 0].max()))
    target_cz = float(0.5 * (neck_ref_pts[:, 2].min() + neck_ref_pts[:, 2].max())) + 0.006

    # Load high-detail separate Head mesh
    h_prim = head_gltf["meshes"][0]["primitives"][0]
    h_pos = get_accessor_data(head_gltf, head_bin, h_prim["attributes"]["POSITION"]).astype(np.float32)
    h_norm = get_accessor_data(head_gltf, head_bin, h_prim["attributes"]["NORMAL"]).astype(np.float32)
    h_uv = get_accessor_data(head_gltf, head_bin, h_prim["attributes"]["TEXCOORD_0"]).astype(np.float32)
    h_tris = get_accessor_data(head_gltf, head_bin, h_prim["indices"]).astype(np.int32).reshape(-1, 3)

    # In coder_head_only_raw.glb:
    #   r = 1.00 (y = +0.946) is the top of the hair crown
    #   r = 0.30 (y = -0.381) is the tip of the chin
    #   r = 0.16..0.30 (y = -0.646..-0.381) with |x| <= 0.215 is the central skin neck cylinder (while |x| > 0.215 is the bust's shirt collar)
    h_ymin_all, h_ymax_all = float(h_pos[:, 1].min()), float(h_pos[:, 1].max())
    h_span_all = h_ymax_all - h_ymin_all
    chin_r = 0.30
    neck_base_r = 0.16
    y_chin_raw = h_ymin_all + chin_r * h_span_all
    y_neck_base_raw = h_ymin_all + neck_base_r * h_span_all

    h_tri_pts = h_pos[h_tris]
    h_tri_y = np.mean(h_tri_pts[:, :, 1], axis=1)
    h_tri_max_abs_x = np.max(np.abs(h_tri_pts[:, :, 0]), axis=1)
    h_tri_min_z = np.min(h_tri_pts[:, :, 2], axis=1)

    # Keep full head above chin (y >= y_chin_raw), plus central skin neck cylinder down to y_neck_base_raw (excluding outer shirt collar wings)
    keep_head_tri_mask = (h_tri_y >= y_chin_raw) | (
        (h_tri_y >= y_neck_base_raw) & (h_tri_max_abs_x <= 0.215) & (h_tri_min_z >= -0.38)
    )
    kept_h_tris = h_tris[keep_head_tri_mask]

    # Compact kept head vertices
    used_h_verts = np.unique(kept_h_tris)
    remap_h = np.full(len(h_pos), -1, dtype=np.int32)
    remap_h[used_h_verts] = np.arange(len(used_h_verts), dtype=np.int32)
    h_pos = h_pos[used_h_verts]
    h_norm = h_norm[used_h_verts]
    h_uv = h_uv[used_h_verts]
    kept_h_tris = remap_h[kept_h_tris]

    # Align chin (y_chin_raw) to 1.522m on the body and crown (h_ymax_all) to 1.792m,
    # so the head's neck cylinder (y_neck_base_raw..y_chin_raw) extends down to ~1.468m inside the body's collar!
    target_chin_y = b_ymin + 0.855 * b_height   # ~1.522m
    target_crown_y = b_ymax + 0.007 * b_height  # ~1.792m
    scale = (target_crown_y - target_chin_y) / max(h_ymax_all - y_chin_y if False else (h_ymax_all - y_chin_raw), 1e-5)

    # Center X/Z using the head portion above y_chin_raw
    upper_h_mask = h_pos[:, 1] >= y_chin_raw
    h_cx = float(0.5 * (h_pos[upper_h_mask, 0].min() + h_pos[upper_h_mask, 0].max()))
    h_cz = float(0.5 * (h_pos[upper_h_mask, 2].min() + h_pos[upper_h_mask, 2].max()))

    h_pos_aligned = np.zeros_like(h_pos)
    h_pos_aligned[:, 0] = (h_pos[:, 0] - h_cx) * scale + target_cx
    h_pos_aligned[:, 1] = (h_pos[:, 1] - y_chin_raw) * scale + target_chin_y
    h_pos_aligned[:, 2] = (h_pos[:, 2] - h_cz) * scale + target_cz

    # Transfer nearest-neighbor skinning weights from the rigged body for the lower neck (so neck & collar move in 100% lockstep),
    # and blend smoothly into 100% Head bone (head_j_idx) above the chin (y >= 1.545m) so the face/glasses never distort!
    h_dists_sq = np.sum((h_pos_aligned[:, None, :] - r_pos[None, :, :]) ** 2, axis=2)
    h_nn_idx = np.argmin(h_dists_sq, axis=1)
    h_joints = r_joints[h_nn_idx].copy()
    h_weights = r_weights[h_nn_idx].copy()

    rigid_head_mask = h_pos_aligned[:, 1] >= neck_keep_top_y
    h_joints[rigid_head_mask] = 0
    h_weights[rigid_head_mask] = 0.0
    h_joints[rigid_head_mask, 0] = head_j_idx
    h_weights[rigid_head_mask, 0] = 1.0

    # Combine textures into a high-resolution 4096x2048 side-by-side atlas (Left: 2K Body, Right: 2K High-Detail Head)
    head_img, _ = extract_base_color_image(head_gltf, head_bin)
    tile_w, tile_h = 2048, 2048
    body_tile = body_img.resize((tile_w, tile_h), Image.Resampling.LANCZOS)
    head_tile = head_img.resize((tile_w, tile_h), Image.Resampling.LANCZOS)
    combined_atlas = Image.new("RGB", (tile_w * 2, tile_h))
    combined_atlas.paste(body_tile, (0, 0))
    combined_atlas.paste(head_tile, (tile_w, 0))

    # Remap UVs to left half [0.0, 0.5] and right half [0.5, 1.0]
    b_uv_remapped = b_uv.copy()
    b_uv_remapped[:, 0] = np.clip(b_uv[:, 0] % 1.0, 0.001, 0.999) * 0.5
    b_uv_remapped[:, 1] = np.clip(b_uv[:, 1] % 1.0, 0.001, 0.999)

    h_uv_remapped = h_uv.copy()
    h_uv_remapped[:, 0] = 0.5 + np.clip(h_uv[:, 0] % 1.0, 0.001, 0.999) * 0.5
    h_uv_remapped[:, 1] = np.clip(h_uv[:, 1] % 1.0, 0.001, 0.999)

    # Concatenate vertices and triangles
    v_offset = len(b_pos)
    final_pos = np.concatenate([b_pos, h_pos_aligned], axis=0).astype(np.float32)
    final_norm = np.concatenate([b_norm, h_norm], axis=0).astype(np.float32)
    final_uv = np.concatenate([b_uv_remapped, h_uv_remapped], axis=0).astype(np.float32)
    final_joints = np.concatenate([b_joints, h_joints], axis=0).astype(np.uint16)
    final_weights = np.concatenate([b_weights, h_weights], axis=0).astype(np.float32)
    final_tris = np.concatenate([kept_b_tris, kept_h_tris + v_offset], axis=0).astype(np.uint32).flatten()

    # Add new accessors for the combined mesh
    r_prim["attributes"]["POSITION"] = add_accessor(rig_gltf, rig_bin, final_pos, "VEC3", 34962)
    r_prim["attributes"]["NORMAL"] = add_accessor(rig_gltf, rig_bin, final_norm, "VEC3", 34962)
    r_prim["attributes"]["TEXCOORD_0"] = add_accessor(rig_gltf, rig_bin, final_uv, "VEC2", 34962)
    r_prim["attributes"]["JOINTS_0"] = add_accessor(rig_gltf, rig_bin, final_joints, "VEC4", 34962)
    r_prim["attributes"]["WEIGHTS_0"] = add_accessor(rig_gltf, rig_bin, final_weights, "VEC4", 34962)
    r_prim["indices"] = add_accessor(rig_gltf, rig_bin, final_tris, "SCALAR", 34963)

    # Write combined dual-atlas into binary buffer and point baseColorTexture image to it
    buf_io = io.BytesIO()
    combined_atlas.save(buf_io, format="JPEG", quality=98)
    jpg_bytes = buf_io.getvalue()
    while len(rig_bin) % 4 != 0:
        rig_bin.append(0)
    bv_offset = len(rig_bin)
    rig_bin.extend(jpg_bytes)
    while len(rig_bin) % 4 != 0:
        rig_bin.append(0)

    new_bv_idx = len(rig_gltf["bufferViews"])
    rig_gltf["bufferViews"].append({
        "buffer": 0,
        "byteOffset": bv_offset,
        "byteLength": len(jpg_bytes),
    })
    rig_gltf["images"][body_img_idx]["bufferView"] = new_bv_idx
    rig_gltf["images"][body_img_idx]["mimeType"] = "image/jpeg"
    rig_gltf["buffers"][0]["byteLength"] = len(rig_bin)

    os.makedirs(os.path.dirname(os.path.abspath(output_glb_path)), exist_ok=True)
    save_glb(output_glb_path, rig_gltf, rig_bin)

    stats = {
        "bodyTrianglesKept": int(len(kept_b_tris)),
        "headTrianglesSpliced": int(len(kept_h_tris)),
        "totalTriangles": int(len(final_tris) // 3),
        "totalVertices": int(len(final_pos)),
    }
    print(json.dumps(stats))
    return stats


def main():
    parser = argparse.ArgumentParser(description="Splice high-detail 3D anime head onto rigged body GLB")
    parser.add_argument("--body", required=True, help="Path to rigged/animated body GLB")
    parser.add_argument("--head", required=True, help="Path to separately modeled high-detail head GLB")
    parser.add_argument("--output", required=True, help="Path to output assembled GLB")
    parser.add_argument("--highres-body", default=None, help="Optional un-decimated raw body GLB for crisp UV transfer")
    args = parser.parse_args()
    assemble_head_and_body(args.body, args.head, args.output, highres_body_glb_path=args.highres_body)


if __name__ == "__main__":
    main()

