"""RurixForge's fixed, non-interactive Blender export entry point.

Invocation: blender --background --factory-startup --disable-autoexec
  --python-exit-code 1 --python export_bundle.py -- --request request.json
Only a saved .blend is loaded; this interface does not accept Python source.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import sys
import uuid

import bpy


def fingerprint(paths):
    digest = hashlib.sha256()
    for name in sorted(set(paths)):
        digest.update(name.encode("utf-8") + b"\0")
        with open(name, "rb") as file:
            while chunk := file.read(65536):
                digest.update(chunk)
        digest.update(b"\0")
    return digest.hexdigest()


def export(request):
    source = Path(request["sourceBlend"]).resolve(strict=True)
    output = Path(request["outputDirectory"]).resolve()
    if source.suffix.lower() != ".blend":
        raise ValueError("sourceBlend must be a saved .blend file")
    output.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.open_mainfile(filepath=str(source), use_scripts=False)
    if bpy.context.object and bpy.context.object.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")

    dependencies = {str(source)}
    for library in bpy.data.libraries:
        dependencies.add(str(Path(bpy.path.abspath(library.filepath)).resolve()))
    for image in bpy.data.images:
        if image.source == "FILE" and not image.packed_file and image.filepath:
            path = Path(bpy.path.abspath(image.filepath, library=image.library)).resolve()
            if not path.is_file():
                raise ValueError("Missing texture: " + str(path))
            dependencies.add(str(path))
    before = fingerprint(dependencies)
    # Discovery may have loaded a linked library before its first hash. Reload
    # the saved scene after the snapshot so all geometry and images correspond
    # to bytes covered by the before/after dependency check.
    bpy.ops.wm.open_mainfile(filepath=str(source), use_scripts=False)
    if bpy.context.object and bpy.context.object.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    for image in bpy.data.images:
        if image.source == "FILE" and not image.packed_file:
            image.reload()
        # AUTO preserves WebP in Blender 5.2; force source images to PNG for the
        # engine's supported PNG/JPEG profile, without modifying the .blend.
        if image.source in {"FILE", "GENERATED"} and image.size[0] > 0:
            image.file_format = "PNG"

    procedural = []
    for material in bpy.data.materials:
        if not material.use_nodes:
            continue
        for node in material.node_tree.nodes:
            if node.bl_idname.startswith("ShaderNodeTex") and node.bl_idname not in {
                "ShaderNodeTexImage", "ShaderNodeTexCoord"
            } and any(socket.is_linked for socket in node.outputs):
                procedural.append(material.name + ": " + node.name)
    if procedural:
        raise ValueError("Bake procedural material textures before publishing: " + "; ".join(procedural))

    object_ids = {}
    used_ids = set()
    for collection_name in ("meshes", "materials", "images"):
        for datablock in getattr(bpy.data, collection_name):
            if not datablock.get("rurixId") and (not datablock.library or datablock.override_library):
                datablock["rurixId"] = str(uuid.uuid5(uuid.NAMESPACE_URL, request["sourceId"] + "/" + collection_name + "/" + datablock.name))
    for obj in bpy.context.scene.objects:
        if obj.type == "MESH" and obj.data.shape_keys and len(obj.data.shape_keys.key_blocks) > 1:
            raise ValueError("Morph targets are not supported by this engine profile: " + obj.name)
        if obj.type == "MESH" and any(len([group for group in vertex.groups if group.weight > 0]) > 4 for vertex in obj.data.vertices):
            raise ValueError("Limit vertices to four bone influences before publishing: " + obj.name)
        key = str(obj.get("rurixId", ""))
        if not key or key in used_ids:
            key = str(uuid.uuid5(uuid.NAMESPACE_URL, request["sourceId"] + "/" + obj.name))
            if not obj.library or obj.override_library:
                obj["rurixId"] = key
        used_ids.add(key)
        object_ids[obj.name] = key
    temporary = output / "model.pending.glb"
    options = dict(
        filepath=str(temporary), export_format="GLB", export_extras=True,
        export_yup=True, export_texcoords=True, export_normals=True,
        export_tangents=True, export_materials="EXPORT", export_image_format="AUTO",
        export_image_webp_fallback=True, export_image_add_webp=False,
        export_draco_mesh_compression_enable=False, export_gpu_instances=False,
        export_hierarchy_flatten_objs=False, export_hierarchy_flatten_bones=False,
        export_hierarchy_full_collections=request["kind"] == "map",
        export_skins=True, export_animations=True, export_animation_mode="ACTIONS",
        export_force_sampling=True, export_influence_nb=4,
        export_all_influences=False, export_apply=True, export_morph=False,
        export_lights=False, export_cameras=False,
    )
    # Use only documented parameters exposed by the installed exporter.
    available = bpy.ops.export_scene.gltf.get_rna_type().properties.keys()
    options = {key: value for key, value in options.items() if key in available}
    result = bpy.ops.export_scene.gltf(**options)
    if "FINISHED" not in result or not temporary.is_file():
        raise RuntimeError("Blender glTF exporter did not finish")
    if fingerprint(dependencies) != before:
        raise ValueError("Source or texture changed while exporting; save and retry")
    # Blender does not forward image IDs to every glTF texture object. Preserve
    # source identity in the standard extras fields without touching BIN offsets.
    raw = temporary.read_bytes()
    json_length, json_kind = struct.unpack_from("<II", raw, 12)
    if json_kind != 0x4E4F534A:
        raise ValueError("GLB first chunk is not JSON")
    document = json.loads(raw[20:20 + json_length])
    for node in document.get("nodes", []):
        if node.get("name") in object_ids:
            node.setdefault("extras", {})["rurixId"] = object_ids[node["name"]]
    for gltf_name, blender_name in (("meshes", "meshes"), ("materials", "materials"), ("images", "images")):
        for item in document.get(gltf_name, []):
            source_data = getattr(bpy.data, blender_name).get(item.get("name", ""))
            if source_data and source_data.get("rurixId"):
                item.setdefault("extras", {})["rurixId"] = source_data["rurixId"]
    for texture in document.get("textures", []):
        image_index = texture.get("source")
        if image_index is not None:
            extras = document["images"][image_index].get("extras", {})
            if extras.get("rurixId"):
                sampler_index = texture.get("sampler")
                sampler = document.get("samplers", [])[sampler_index] if sampler_index is not None else {}
                identity = json.dumps({"image": extras["rurixId"], "sampler": sampler, "extensions": texture.get("extensions", {})}, sort_keys=True, separators=(",", ":"))
                texture.setdefault("extras", {})["rurixId"] = str(uuid.uuid5(uuid.NAMESPACE_URL, "rurix-texture/" + identity))
    encoded = json.dumps(document, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    encoded += b" " * ((-len(encoded)) % 4)
    remaining = raw[20 + json_length:]
    temporary.write_bytes(struct.pack("<III", 0x46546C67, 2, 20 + len(encoded) + len(remaining)) + struct.pack("<II", len(encoded), 0x4E4F534A) + encoded + remaining)
    os.replace(temporary, output / "model.glb")
    manifest = {
        "version": 1, "sourceId": request["sourceId"], "name": request["name"],
        "kind": request["kind"], "revision": request["revision"],
        "sourceBlend": str(source), "objectIds": object_ids,
        "idleClip": request.get("idleClip"), "walkClip": request.get("walkClip"),
    }
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False), encoding="utf-8")
    result = {
        "manifest": manifest, "dependencies": sorted(dependencies),
        "blenderVersion": bpy.app.version_string, "objects": len(object_ids),
        "sourceFingerprint": before,
    }
    (output / "export-result.json").write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")
    print("RURIX_EXPORT_COMPLETE " + json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", required=True)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:])
    export(json.loads(Path(args.request).read_text(encoding="utf-8-sig")))
