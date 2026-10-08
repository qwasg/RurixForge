"""Actual16-frame transport propeller loop, with rotating mesh markers and fixed airframe."""
import bpy,math,json
from pathlib import Path
from mathutils import Vector
HERE=Path(__file__).resolve().parent;PROJECT=HERE.parents[1];DIRS=['s','sw','w','nw','n','ne','e','se'];count=16
bpy.ops.wm.open_mainfile(filepath=str(PROJECT/'Content/Models/v6/cargo-aircraft.blend'));scene=bpy.context.scene;root=bpy.data.objects['cargo-aircraft assembly']
scene.render.engine='CYCLES';scene.cycles.device='CPU';scene.cycles.samples=12;scene.render.threads_mode='FIXED';scene.render.threads=6
gold=bpy.data.materials['Warm brass markings'];groups=[]
for sign in [-1,1]:
 for distance in [.62,1.10]:
  x=sign*distance;group=bpy.data.objects.new('transport propeller rotor '+str(x),None);bpy.context.collection.objects.link(group);group.parent=root;group.location=(x,.75,.79);bpy.context.view_layer.update()
  for ob in list(scene.objects):
   if ob.name.startswith('transport propeller blade') and abs(ob.location.x-x)<.01:
    matrix=ob.matrix_world.copy();ob.parent=group;ob.matrix_world=matrix
  bpy.ops.mesh.primitive_cube_add(size=1,location=(0,0,0));mark=bpy.context.object;mark.name='actual propeller balance marker';mark.parent=group;mark.location=(.20*math.cos(.20),-.018,-.20*math.sin(.20));mark.scale=(.065,.028,.055);mark.rotation_euler.y=.20;mark.data.materials.append(gold)
  groups.append(group)
for group in groups:
 for frame in range(count+1):
  group.rotation_euler.y=frame*math.tau/count;group.keyframe_insert('rotation_euler',frame=frame+1)
scene.frame_start=1;scene.frame_end=count+1;scene.render.fps=16;scene.frame_set(1)
bpy.ops.wm.save_as_mainfile(filepath=str(PROJECT/'Content/Models/v6/cargo-aircraft-work.blend'),check_existing=False)
dest=PROJECT/'Content/UI/v6/model-work/cargo-aircraft';dest.mkdir(parents=True,exist_ok=True)
for direction_index,d in enumerate(DIRS):
 for frame in range(count+1):
  scene.frame_set(frame+1);root.rotation_euler.z=math.radians(-135-direction_index*45);scene.render.filepath=str(dest/(f'{d}-{frame:02}.png' if frame<count else f'{d}-closure.png'));bpy.ops.render.render(write_still=True)
meta={'id':'cargo-aircraft','directions':DIRS,'framesPerDirection':count,'fps':16,'loop':True,'sourceModel':'Content/Models/v6/cargo-aircraft-work.blend','method':'Actual four independently assembled propeller mesh rotors, CPU Blender render; airframe fixed','mechanismAnimated':True,'emissionAnimated':False,'closureRender':'separate actual render at full360-degree phase, excluded from atlas','duplicateFramePadding':False}
(dest/'work.json').write_text(json.dumps(meta,indent=2),encoding='utf-8');print('V6_CARGO_FLIGHT_BAKED',flush=True)
