"""Open a genuine modeled doorway by rotating only the two hinged door leaves."""
import bpy,math,json
from pathlib import Path
HERE=Path(__file__).resolve().parent;PROJECT=HERE.parents[1];DIRS=['s','sw','w','nw','n','ne','e','se']
bpy.ops.wm.open_mainfile(filepath=str(PROJECT/'Content/Models/v6/door.blend'));scene=bpy.context.scene;root=bpy.data.objects['door assembly']
scene.render.engine='CYCLES';scene.cycles.device='CPU';scene.cycles.samples=12;scene.render.threads_mode='FIXED';scene.render.threads=6
for side,sign in [('left',-1),('right',1)]:
 hinge=bpy.data.objects.new('door '+side+' hinge pivot',None);bpy.context.collection.objects.link(hinge);hinge.parent=root;hinge.location=(sign*.30,-.08,0);bpy.context.view_layer.update()
 for ob in list(scene.objects):
  if ob.name in ['hinged door leaf '+side,'door leaf reinforcement '+side]:
   matrix=ob.matrix_world.copy();ob.parent=hinge;ob.matrix_world=matrix
 hinge.rotation_euler.z=-sign*math.pi/2
bpy.context.view_layer.update();dest=PROJECT/'Content/UI/v6/model-states/door/open';dest.mkdir(parents=True,exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=str(PROJECT/'Content/Models/v6/door-open.blend'),check_existing=False)
for i,d in enumerate(DIRS):
 root.rotation_euler.z=math.radians(-135-i*45);scene.render.filepath=str(dest/(d+'.png'));bpy.ops.render.render(write_still=True)
meta={'id':'door','state':'open','directions':DIRS,'framesPerDirection':1,'sourceModel':'Content/Models/v6/door-open.blend','method':'Real door-leaf meshes pivot90 degrees around two individual hinges; central doorway is empty geometry','imageRotationUsed':False}
(dest/'state.json').write_text(json.dumps(meta,indent=2),encoding='utf-8');print('V6_DOOR_OPEN_BAKED',flush=True)
