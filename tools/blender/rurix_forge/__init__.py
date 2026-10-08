"""Blender authoring UI for the RurixForge asset bridge."""
bl_info = {
    "name": "RurixForge Asset Bridge", "author": "RurixForge",
    "version": (1, 0, 0), "blender": (4, 5, 0),
    "location": "View3D > Sidebar > RurixForge", "category": "Import-Export",
    "description": "Bind saved Blender projects and publish engine templates",
}

import json
import urllib.request
import urllib.error
import urllib.parse
import uuid
import bpy
from bpy.app.handlers import persistent


@persistent
def stable_ids(_):
    seen = set()
    datablocks = list(bpy.data.objects) + list(bpy.data.meshes) + list(bpy.data.materials) + list(bpy.data.images)
    for obj in datablocks:
        if obj.library and not obj.override_library:
            continue
        identity = str(obj.get("rurixId", ""))
        if not identity or identity in seen:
            obj["rurixId"] = str(uuid.uuid4())
        seen.add(str(obj["rurixId"]))


class RURIX_Settings(bpy.types.PropertyGroup):
    origin: bpy.props.StringProperty(name="Engine address", default="http://127.0.0.1:8103")
    workspace_id: bpy.props.StringProperty(name="Workspace", default="default")
    job_id: bpy.props.StringProperty(name="Job ID")
    lease_token: bpy.props.StringProperty(name="Lease token", subtype="PASSWORD")
    message: bpy.props.StringProperty(name="Status", default="Save this project, then bind it to the engine job.")


def call(settings, action):
    # The addon sends a bounded bridge command; it never hosts a Python socket
    # executor or modifies Blender data from a background thread.
    address = urllib.parse.urlsplit(settings.origin)
    if address.scheme != "http" or address.hostname not in {"127.0.0.1", "localhost"} or address.username or address.password or address.path not in {"", "/"} or address.query or address.fragment:
        raise ValueError("Use the local RurixForge engine address")
    if not settings.job_id or not settings.lease_token:
        raise ValueError("Enter the job ID and current lease token from Codex")
    if not all(c.isascii() and (c.isalnum() or c in "_-") for c in settings.job_id):
        raise ValueError("Job ID is not valid")
    payload = {"workspaceId": settings.workspace_id, "leaseToken": settings.lease_token}
    if action == "bind":
        if not bpy.data.filepath or bpy.data.is_dirty:
            raise ValueError("Save the Blender project before binding")
        payload.update(sourcePath=bpy.data.filepath, autoSync=True)
    url = settings.origin.rstrip("/") + "/api/forge/blender/jobs/" + settings.job_id + "/" + action
    request = urllib.request.Request(url, data=json.dumps(payload).encode(), headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        detail = json.load(error)
        raise ValueError(detail.get("error", {}).get("message", str(error))) from error


class RURIX_OT_bind(bpy.types.Operator):
    bl_idname = "rurix.bind_project"
    bl_label = "Bind saved project"
    def execute(self, context):
        settings = context.window_manager.rurix_forge
        try:
            result = call(settings, "bind")
            settings.message = result.get("message", "Bound; ready to publish")
            return {"FINISHED"}
        except Exception as error:
            settings.message = str(error)
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}


class RURIX_OT_publish(bpy.types.Operator):
    bl_idname = "rurix.publish_project"
    bl_label = "Publish to RurixForge"
    def execute(self, context):
        settings = context.window_manager.rurix_forge
        try:
            if bpy.data.is_dirty:
                raise ValueError("Save changes before publishing")
            result = call(settings, "publish")
            settings.message = result.get("message", "Engine export queued")
            return {"FINISHED"}
        except Exception as error:
            settings.message = str(error)
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}


class RURIX_PT_bridge(bpy.types.Panel):
    bl_label = "RurixForge Asset Bridge"
    bl_idname = "RURIX_PT_bridge"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "RurixForge"
    def draw(self, context):
        layout = self.layout
        settings = context.window_manager.rurix_forge
        for key in ("origin", "workspace_id", "job_id", "lease_token"):
            layout.prop(settings, key)
        layout.operator("rurix.bind_project")
        layout.operator("rurix.publish_project")
        layout.label(text=settings.message[:100])
        layout.label(text="After first publish, saved changes sync automatically.")


CLASSES = (RURIX_Settings, RURIX_OT_bind, RURIX_OT_publish, RURIX_PT_bridge)


def register():
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.WindowManager.rurix_forge = bpy.props.PointerProperty(type=RURIX_Settings)
    if stable_ids not in bpy.app.handlers.save_pre:
        bpy.app.handlers.save_pre.append(stable_ids)


def unregister():
    if stable_ids in bpy.app.handlers.save_pre:
        bpy.app.handlers.save_pre.remove(stable_ids)
    del bpy.types.WindowManager.rurix_forge
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
