"""Fixed regression fixture, never used for user asset authoring.

Builds a textured, hierarchical, skinned object with one animation in an isolated
test directory, so the real Blender exporter can be tested without GUI state.
"""
import argparse
import json
from pathlib import Path
import sys
import uuid
import bpy


def save_fixture(output, kind):
    for obj in bpy.data.objects:
        obj["rurixId"] = str(uuid.uuid5(uuid.NAMESPACE_URL, "rurix-fixture/" + obj.name))
    blend = output / "fixture.blend"
    bpy.ops.wm.save_as_mainfile(filepath=str(blend))
    request = dict(sourceBlend=str(blend), outputDirectory=str(output / "export"), sourceId="blender-smoke-fixture", name="Blender Smoke", kind=kind, revision=1)
    (output / "request.json").write_text(json.dumps(request), encoding="utf-8")
    print("RURIX_FIXTURE " + str(output / "request.json"))


def fixture(output, variant=0, sampler_variant=False, kind="character"):
    output.mkdir(parents=True, exist_ok=True)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    image = bpy.data.images.new("FixtureChecker", width=16, height=16)
    pixels = []
    for y in range(16):
        for x in range(16):
            pixels.extend((0.1, 0.8, 0.3, 1) if (x // 4 + y // 4) % 2 else (0.9, 0.1 + 0.2 * variant, 0.15, 1))
    image.pixels = pixels
    image.filepath_raw = str(output / "checker.png")
    image.file_format = "PNG"
    image.save()
    material = bpy.data.materials.new("CheckerPBR")
    material.use_nodes = True
    material.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.6
    texture = material.node_tree.nodes.new("ShaderNodeTexImage")
    texture.image = image
    material.node_tree.links.new(texture.outputs["Color"], material.node_tree.nodes["Principled BSDF"].inputs["Base Color"])

    parent = bpy.data.objects.new("MapRoot", None)
    bpy.context.collection.objects.link(parent)
    parent.location = (0.25, 0, 0)
    if kind == "map":
        for name, location, scale in (("Floor", (0, 0, -0.25), (10, 10, 0.5)), ("Wall", (2, 0, 1), (0.3, 6, 2))):
            bpy.ops.mesh.primitive_cube_add(size=1, location=location)
            mesh = bpy.context.object
            mesh.name = name
            mesh.scale = scale
            mesh.parent = parent
            mesh["rurixCollision"] = True
            mesh.data.materials.append(material)
        save_fixture(output, kind)
        return
    bpy.ops.mesh.primitive_cube_add(size=1, location=(0, 0, 1))
    mesh = bpy.context.object
    mesh.name = "SkinnedChecker"
    mesh.scale.x = 1.0 + variant * 0.2
    mesh.data.materials.append(material)
    if sampler_variant:
        alternate = material.copy()
        alternate.name = "CheckerClamp"
        alternate.node_tree.nodes.get(texture.name).extension = "EXTEND"
        mesh.data.materials.append(alternate)
        for polygon in mesh.data.polygons:
            polygon.material_index = polygon.index % 2
    bpy.ops.object.armature_add(location=(0, 0, 0))
    rig = bpy.context.object
    rig.name = "FixtureRig"
    rig.parent = parent
    bpy.ops.object.mode_set(mode="EDIT")
    bone = rig.data.edit_bones[0]
    bone.name = "Root"
    bone.head = (0, 0, 0)
    bone.tail = (0, 0, 1)
    tip = rig.data.edit_bones.new("Tip")
    tip.head = (0, 0, 1)
    tip.tail = (0, 0, 2)
    tip.parent = bone
    bpy.ops.object.mode_set(mode="OBJECT")
    mesh.parent = rig
    modifier = mesh.modifiers.new("Skeleton", "ARMATURE")
    modifier.object = rig
    group = mesh.vertex_groups.new(name="Tip")
    group.add(list(range(len(mesh.data.vertices))), 1, "REPLACE")
    pose = rig.pose.bones["Tip"]
    pose.rotation_mode = "XYZ"
    for frame, angle in [(1, 0), (10, 0.5), (20, 0)]:
        pose.rotation_euler[1] = angle
        pose.keyframe_insert("rotation_euler", frame=frame)
    rig.animation_data.action.name = "Idle"
    idle = rig.animation_data.action
    track = rig.animation_data.nla_tracks.new()
    track.name = "Idle"
    track.strips.new("Idle", 1, idle)
    rig.animation_data.action = None
    for frame, angle in [(1, -0.25), (10, 0.7), (20, -0.25)]:
        pose.rotation_euler[1] = angle
        pose.keyframe_insert("rotation_euler", frame=frame)
    rig.animation_data.action.name = "Walk"
    bpy.context.scene.frame_end = 20
    bpy.context.scene.frame_set(1)
    save_fixture(output, kind)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument("--variant", type=int, choices=(0, 1, 2), default=0)
    parser.add_argument("--sampler-variant", action="store_true")
    parser.add_argument("--kind", choices=("map", "character"), default="character")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:])
    fixture(Path(args.output).resolve(), args.variant, args.sampler_variant, args.kind)
