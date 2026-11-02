#!/usr/bin/env python3
"""
tools/model/decimate_mesh.py
Headless mesh decimation tool for RurixForge 3D character pipeline.
Reduces face count to <= targetFaces, ensures TRIANGLES mode, preserves TEXCOORD_0,
repairs normals, and prints JSON statistics.
"""

import argparse
import collections
import heapq
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
        add_buffer_view,
    )
except ImportError:
    from tools.model.glb_utils import (
        load_glb,
        save_glb,
        get_accessor_data,
        add_accessor,
        add_buffer_view,
    )


def compute_normals(positions, triangles):
    normals = np.zeros_like(positions, dtype=np.float32)
    if len(triangles) > 0:
        v0 = positions[triangles[:, 0]]
        v1 = positions[triangles[:, 1]]
        v2 = positions[triangles[:, 2]]
        face_normals = np.cross(v1 - v0, v2 - v0)
        for i in range(3):
            np.add.at(normals, triangles[:, i], face_normals)

    norms = np.linalg.norm(normals, axis=1, keepdims=True)
    zero_mask = (norms < 1e-6).flatten()

    if np.any(zero_mask):
        # Calculate outward radial vector from mesh centroid
        center = np.mean(positions, axis=0)
        radial = positions - center
        r_norms = np.linalg.norm(radial, axis=1, keepdims=True)
        r_zero = (r_norms < 1e-6).flatten()

        safe_r_norms = np.where(r_norms < 1e-6, 1.0, r_norms)
        radial_unit = radial / safe_r_norms
        radial_unit[r_zero] = [0.0, 1.0, 0.0]

        normals[zero_mask] = radial_unit[zero_mask]
        norms[zero_mask] = 1.0

    normals = normals / norms
    return normals.astype(np.float32)


def grid_cluster(positions, triangles, uvs, target_faces):
    bbox_min = np.min(positions, axis=0)
    bbox_max = np.max(positions, axis=0)
    bbox_size = np.maximum(bbox_max - bbox_min, 1e-5)

    low = 10
    high = 250
    best_res = high

    for _ in range(8):
        mid = (low + high) // 2
        cell_size = bbox_size / mid
        cell_coords = np.floor((positions - bbox_min) / cell_size).astype(np.int64)
        keys = cell_coords[:, 0] * 1000000 + cell_coords[:, 1] * 1000 + cell_coords[:, 2]
        unique_keys, inverse = np.unique(keys, return_inverse=True)

        t_mapped = inverse[triangles]
        valid = (
            (t_mapped[:, 0] != t_mapped[:, 1])
            & (t_mapped[:, 1] != t_mapped[:, 2])
            & (t_mapped[:, 0] != t_mapped[:, 2])
        )
        count = int(np.sum(valid))
        if count <= target_faces * 1.8 and count >= target_faces:
            best_res = mid
            break
        elif count > target_faces * 1.8:
            high = mid
        else:
            low = mid
            best_res = mid

    cell_size = bbox_size / best_res
    cell_coords = np.floor((positions - bbox_min) / cell_size).astype(np.int64)
    keys = cell_coords[:, 0] * 1000000 + cell_coords[:, 1] * 1000 + cell_coords[:, 2]
    unique_keys, inverse = np.unique(keys, return_inverse=True)

    num_cells = len(unique_keys)
    new_pos = np.zeros((num_cells, 3), dtype=np.float32)
    counts = np.zeros((num_cells, 1), dtype=np.float32)
    np.add.at(new_pos, inverse, positions)
    np.add.at(counts, inverse, 1.0)
    new_pos /= np.maximum(counts, 1.0)

    new_uv = None
    if uvs is not None:
        new_uv = np.zeros((num_cells, 2), dtype=np.float32)
        np.add.at(new_uv, inverse, uvs)
        new_uv /= np.maximum(counts, 1.0)

    t_mapped = inverse[triangles]
    valid = (
        (t_mapped[:, 0] != t_mapped[:, 1])
        & (t_mapped[:, 1] != t_mapped[:, 2])
        & (t_mapped[:, 0] != t_mapped[:, 2])
    )
    new_tris = t_mapped[valid]

    sorted_tris = np.sort(new_tris, axis=1)
    _, unique_idx = np.unique(sorted_tris, axis=0, return_index=True)
    new_tris = new_tris[unique_idx]

    used_verts, new_inverse = np.unique(new_tris, return_inverse=True)
    new_pos = new_pos[used_verts]
    if new_uv is not None:
        new_uv = new_uv[used_verts]
    new_tris = new_inverse.reshape(new_tris.shape)

    return new_pos, new_tris, new_uv


def deduplicate_faces(positions, triangles, uvs=None):
    """
    Removes duplicate and opposing faces sharing the same 3 vertices.
    Selects the triangle orientation that points outward from the mesh centroid.
    Prunes unreferenced vertices and compacts the index buffer.
    """
    if len(triangles) == 0:
        return positions, triangles, uvs

    center = np.mean(positions, axis=0)
    groups = collections.defaultdict(list)
    for tri in triangles:
        key = tuple(sorted(tri))
        groups[key].append(tri)

    chosen_tris = []
    for key, group in groups.items():
        if len(group) == 1:
            chosen_tris.append(group[0])
        else:
            # Pick the face with maximum dot product with outward centroid vector
            best_tri = group[0]
            best_dot = -float("inf")
            for tri in group:
                v0, v1, v2 = positions[tri[0]], positions[tri[1]], positions[tri[2]]
                fn = np.cross(v1 - v0, v2 - v0)
                fc = (v0 + v1 + v2) / 3.0
                dot = float(np.dot(fn, fc - center))
                if dot > best_dot:
                    best_dot = dot
                    best_tri = tri
            chosen_tris.append(best_tri)

    chosen_tris = np.array(chosen_tris, dtype=np.uint32)

    # Compact vertex buffer
    used_verts, new_inv = np.unique(chosen_tris, return_inverse=True)
    clean_pos = positions[used_verts]
    clean_uvs = uvs[used_verts] if uvs is not None else None
    clean_tris = new_inv.reshape(chosen_tris.shape).astype(np.uint32)

    return clean_pos, clean_tris, clean_uvs


def edge_collapse_decimate(positions, triangles, uvs, target_faces):
    if len(triangles) <= target_faces:
        return deduplicate_faces(positions, triangles, uvs)

    pos = positions.copy()
    uv = uvs.copy() if uvs is not None else None
    tris = [list(t) for t in triangles]

    parent = list(range(len(pos)))

    def find(i):
        path = []
        while parent[i] != i:
            path.append(i)
            i = parent[i]
        for node in path:
            parent[node] = i
        return i

    vert_faces = collections.defaultdict(set)
    for f_idx, tri in enumerate(tris):
        for v in tri:
            vert_faces[v].add(f_idx)

    heap = []
    edges_seen = set()
    for tri in tris:
        for i in range(3):
            u, v = tri[i], tri[(i + 1) % 3]
            edge = (min(u, v), max(u, v))
            if edge not in edges_seen:
                edges_seen.add(edge)
                cost = float(np.sum((pos[u] - pos[v]) ** 2))
                heapq.heappush(heap, (cost, u, v))

    alive_faces = set(range(len(tris)))

    while len(alive_faces) > target_faces and heap:
        cost, u_orig, v_orig = heapq.heappop(heap)
        u = find(u_orig)
        v = find(v_orig)
        if u == v:
            continue

        new_pos = 0.5 * (pos[u] + pos[v])
        pos[u] = new_pos
        if uv is not None:
            uv[u] = 0.5 * (uv[u] + uv[v])
        parent[v] = u

        v_inc = list(vert_faces[v])
        for f_idx in v_inc:
            if f_idx not in alive_faces:
                continue
            face = tris[f_idx]
            face = [u if x == v else find(x) for x in face]
            tris[f_idx] = face
            if len(set(face)) < 3:
                alive_faces.remove(f_idx)
            else:
                vert_faces[u].add(f_idx)

    remap = {}
    out_pos = []
    out_uv = [] if uv is not None else None
    out_tris = []
    for f_idx in alive_faces:
        face = tris[f_idx]
        new_tri = []
        for v in face:
            root = find(v)
            if root not in remap:
                remap[root] = len(out_pos)
                out_pos.append(pos[root])
                if uv is not None:
                    out_uv.append(uv[root])
            new_tri.append(remap[root])
        out_tris.append(new_tri)

    out_pos = np.array(out_pos, dtype=np.float32)
    out_tris = np.array(out_tris, dtype=np.uint32)
    out_uv = np.array(out_uv, dtype=np.float32) if uv is not None else None

    return deduplicate_faces(out_pos, out_tris, out_uv)


def decimate_mesh(input_path, output_path, target_faces):
    gltf, bin_data = load_glb(input_path)

    # Gather all mesh primitives so multi-primitive Meshy GLBs are unified cleanly
    pos_list = []
    tri_list = []
    uv_list = []
    has_any_uv = False
    vert_offset = 0
    first_material = None
    mesh_name = "DecimatedMesh"

    for m_idx, mesh in enumerate(gltf.get("meshes", [])):
        if m_idx == 0 and "name" in mesh:
            mesh_name = mesh["name"]
        for prim in mesh.get("primitives", []):
            if "attributes" not in prim or "POSITION" not in prim["attributes"]:
                continue
            if first_material is None and "material" in prim:
                first_material = prim["material"]
            pos_acc = prim["attributes"]["POSITION"]
            p_pos = get_accessor_data(gltf, bin_data, pos_acc).astype(np.float32)
            if "indices" in prim:
                p_idx = get_accessor_data(gltf, bin_data, prim["indices"]).astype(np.uint32)
                p_tri = p_idx.reshape((-1, 3)) + vert_offset
            else:
                p_tri = np.arange(len(p_pos), dtype=np.uint32).reshape((-1, 3)) + vert_offset

            if "TEXCOORD_0" in prim["attributes"]:
                has_any_uv = True
                uv_acc = prim["attributes"]["TEXCOORD_0"]
                p_uv = get_accessor_data(gltf, bin_data, uv_acc).astype(np.float32)
            else:
                p_uv = np.zeros((len(p_pos), 2), dtype=np.float32)

            pos_list.append(p_pos)
            tri_list.append(p_tri)
            uv_list.append(p_uv)
            vert_offset += len(p_pos)

    positions = np.vstack(pos_list) if pos_list else np.zeros((0, 3), dtype=np.float32)
    triangles = np.vstack(tri_list) if tri_list else np.zeros((0, 3), dtype=np.uint32)
    uvs = np.vstack(uv_list) if (uv_list and has_any_uv) else None

    original_triangles = len(triangles)
    original_vertices = len(positions)

    effective_target = target_faces
    if original_triangles > 200 and original_triangles <= target_faces:
        effective_target = max(int(original_triangles * 0.85), 100)

    # Perform decimation if needed
    if original_triangles > effective_target:
        # Coarse pass if significantly larger
        if original_triangles > effective_target * 2:
            positions, triangles, uvs = grid_cluster(
                positions, triangles, uvs, effective_target
            )
        # Fine edge collapse pass
        if len(triangles) > effective_target:
            positions, triangles, uvs = edge_collapse_decimate(
                positions, triangles, uvs, effective_target
            )
        else:
            positions, triangles, uvs = deduplicate_faces(
                positions, triangles, uvs
            )

    # Recompute smooth vertex normals
    normals = compute_normals(positions, triangles)

    # If no UVs originally, create default planar UVs
    if uvs is None:
        uvs = np.zeros((len(positions), 2), dtype=np.float32)
        p_min = np.min(positions, axis=0)
        p_max = np.max(positions, axis=0)
        span = np.maximum(p_max - p_min, 1e-5)
        uvs[:, 0] = (positions[:, 0] - p_min[0]) / span[0]
        uvs[:, 1] = (positions[:, 1] - p_min[1]) / span[1]

    refined_triangles = len(triangles)
    refined_vertices = len(positions)

    # Build new gltf / bin buffer with normalized single-mesh node hierarchy
    out_gltf = {
        "asset": {"version": "2.0", "generator": "RurixForge Decimate Tool"},
        "scenes": [{"nodes": [0]}],
        "scene": 0,
        "nodes": [{"name": mesh_name, "mesh": 0}],
        "meshes": [
            {
                "name": mesh_name,
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

    # Preserve materials and textures if present
    if "materials" in gltf:
        out_gltf["materials"] = gltf["materials"]
        if first_material is not None:
            out_gltf["meshes"][0]["primitives"][0]["material"] = first_material
    if "textures" in gltf:
        out_gltf["textures"] = gltf["textures"]
    if "samplers" in gltf:
        out_gltf["samplers"] = gltf["samplers"]

    out_bin = bytearray()

    # Accessor 0: Indices
    flat_indices = triangles.flatten().astype(np.uint32)
    idx_acc = add_accessor(out_gltf, out_bin, flat_indices, "SCALAR", target=34963)
    out_gltf["meshes"][0]["primitives"][0]["indices"] = idx_acc

    # Accessor 1: Position
    pos_acc = add_accessor(out_gltf, out_bin, positions, "VEC3", target=34962)
    out_gltf["meshes"][0]["primitives"][0]["attributes"]["POSITION"] = pos_acc

    # Accessor 2: Normal
    norm_acc = add_accessor(out_gltf, out_bin, normals, "VEC3", target=34962)
    out_gltf["meshes"][0]["primitives"][0]["attributes"]["NORMAL"] = norm_acc

    # Accessor 3: Texcoord_0
    uv_acc = add_accessor(out_gltf, out_bin, uvs, "VEC2", target=34962)
    out_gltf["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = uv_acc

    # Preserve embedded texture images and update bufferView indices
    if "images" in gltf:
        out_images = []
        bv_remap = {}
        for img_entry in gltf["images"]:
            new_img = dict(img_entry)
            if "bufferView" in img_entry:
                old_bv_idx = img_entry["bufferView"]
                if old_bv_idx not in bv_remap:
                    old_bv = gltf["bufferViews"][old_bv_idx]
                    start = old_bv.get("byteOffset", 0)
                    length = old_bv["byteLength"]
                    raw_bytes = bytes(bin_data[start : start + length])
                    new_bv_idx = add_buffer_view(out_gltf, out_bin, raw_bytes)
                    bv_remap[old_bv_idx] = new_bv_idx
                new_img["bufferView"] = bv_remap[old_bv_idx]
            out_images.append(new_img)
        out_gltf["images"] = out_images

    out_gltf["buffers"][0]["byteLength"] = len(out_bin)

    os.makedirs(os.path.dirname(os.path.abspath(output_path)), exist_ok=True)
    save_glb(output_path, out_gltf, out_bin)

    stats = {
        "originalTriangles": int(original_triangles),
        "refinedTriangles": int(refined_triangles),
        "originalVertices": int(original_vertices),
        "refinedVertices": int(refined_vertices),
    }
    print(json.dumps(stats))
    return stats


def main():
    parser = argparse.ArgumentParser(description="Mesh Decimation Tool")
    parser.add_argument("--input", required=True, help="Path to input GLB")
    parser.add_argument("--output", required=True, help="Path to output GLB")
    parser.add_argument("--target-faces", type=int, required=True, help="Target face count")
    args = parser.parse_args()

    decimate_mesh(args.input, args.output, args.target_faces)


if __name__ == "__main__":
    main()
