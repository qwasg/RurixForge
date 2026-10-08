"""True Blender model work loops: mechanics and emissive equipment, CPU-rendered."""
import bpy,math,json,sys
from pathlib import Path
from mathutils import Vector
HERE=Path(__file__).resolve().parent;PROJECT=HERE.parents[1]
DIRS=['s','sw','w','nw','n','ne','e','se']
IDS=['wind-power','hydro-power','extractor','mobile-relay','factory','repair-bay','particle-foundry','radar-array','logistics-belt','modular-workshop','rack','research-console','network-defense','energy-defense']
for id in IDS:
 fast=id in ['wind-power','hydro-power'];count=16 if fast else 8;version='-v3' if id=='hydro-power' else '-v2' if fast else ''
 dest=PROJECT/('Content/UI/v6/model-work'+version)/id
 if (dest/'work.json').exists():continue
 bpy.ops.wm.open_mainfile(filepath=str(PROJECT/'Content/Models/v6'/(id+'.blend')))
 scene=bpy.context.scene;root=bpy.data.objects[id+' assembly'];scene.render.threads_mode='FIXED';scene.render.threads=6
 scene.render.engine='CYCLES';scene.cycles.device='CPU';scene.cycles.samples=12
 group=None;axis='Z'
 if id in ['wind-power','hydro-power']:
  group=bpy.data.objects.new('working rotor pivot',None);bpy.context.collection.objects.link(group);group.parent=root;group.location=(0,-.25,1.66) if id=='wind-power' else (0,-.54,.45);axis='Y'
  bpy.context.view_layer.update()
  for ob in list(scene.objects):
   if ob.name.startswith('wind rotor' if id=='wind-power' else 'hydro rotor'):
    matrix=ob.matrix_world.copy();ob.parent=group;ob.matrix_world=matrix
 elif id in ['extractor','radar-array','mobile-relay']:
  names={'extractor':['drill'],'radar-array':['phased radar face','radar grid'],'mobile-relay':['relay radome']}[id]
  objects=[o for o in scene.objects if any(o.name.startswith(n) for n in names)]
  if objects:
   group=bpy.data.objects.new('working mechanism pivot',None);bpy.context.collection.objects.link(group);group.parent=root
   group.location=Vector((0,0,0)) if id=='extractor' else sum((o.location for o in objects),Vector())/len(objects);bpy.context.view_layer.update()
   for ob in objects:
    matrix=ob.matrix_world.copy();ob.parent=group;ob.matrix_world=matrix
 elif id in ['factory','repair-bay','modular-workshop']:
  objects=[o for o in scene.objects if any(w in o.name for w in ['robot forearm','tool claw','repair tool','assembly probe'])]
  if objects:
   group=bpy.data.objects.new('working articulated tool',None);bpy.context.collection.objects.link(group);group.parent=root;group.location=(0,0,.95);bpy.context.view_layer.update()
   for ob in objects:
    matrix=ob.matrix_world.copy();ob.parent=group;ob.matrix_world=matrix
 if group:
  for frame in range(1,count+2):
   angle=(frame-1)*math.tau/count if id in ['wind-power','hydro-power','extractor'] else math.sin((frame-1)*math.tau/count)*.22
   group.rotation_euler['XYZ'.index(axis)]=angle;group.keyframe_insert('rotation_euler',frame=frame)
  if group.animation_data and group.animation_data.action:group.animation_data.action.name=id+' work cycle'
 if id=='hydro-power':
  secondary=bpy.data.objects.new('hydro generator rotating coupling',None);bpy.context.collection.objects.link(secondary);secondary.parent=root;secondary.location=(.4,0,.83);bpy.context.view_layer.update()
  for ob in list(scene.objects):
   if ob.name.startswith('hydro generator rotor'):
    matrix=ob.matrix_world.copy();ob.parent=secondary;ob.matrix_world=matrix
  for frame in range(1,count+2):
   secondary.rotation_euler.z=(frame-1)*math.tau/count;secondary.keyframe_insert('rotation_euler',frame=frame)
 animated=[]
 for material in bpy.data.materials:
  if material.use_nodes:
   node=material.node_tree.nodes.get('Principled BSDF')
   if node and node.inputs['Emission Strength'].default_value>0:
    base=node.inputs['Emission Strength'].default_value;animated.append((node,base))
    for frame in range(1,count+2):
     node.inputs['Emission Strength'].default_value=base*(.78+.32*math.sin((frame-1)*math.tau/count));node.inputs['Emission Strength'].keyframe_insert('default_value',frame=frame)
 scene.frame_start=1;scene.frame_end=count+1;scene.render.fps=count;scene.frame_set(1)
 bpy.ops.wm.save_as_mainfile(filepath=str(PROJECT/'Content/Models/v6'/(id+'-work'+version+'.blend')),check_existing=False)
 dest.mkdir(parents=True,exist_ok=True)
 for direction_index,direction in enumerate(DIRS):
  for frame in range(count+1):
   scene.frame_set(frame+1);root.rotation_euler.z=math.radians(-135-direction_index*45)
   if id=='logistics-belt':
    cargo=next((o for o in scene.objects if o.name.startswith('cargo in transit')),None)
    if cargo:cargo.location.y=-.42+frame*.12
   scene.render.filepath=str(dest/(f'{direction}-{frame:02}.png' if frame<count else f'{direction}-closure.png'));bpy.ops.render.render(write_still=True)
 meta={'id':id,'directions':DIRS,'framesPerDirection':count,'fps':count,'loop':True,'sourceModel':f'Content/Models/v6/{id}-work{version}.blend','method':'actual Blender animated geometry and equipment emission; CPU Cycles render','mechanismAnimated':group is not None or id=='logistics-belt','emissionAnimated':bool(animated),'closureRender':'separate actual render at full360-degree phase; not packed as extra frame','duplicateFramePadding':False}
 (dest/'work.json').write_text(json.dumps(meta,indent=2),encoding='utf-8');print('V6_WORK_BAKED '+id,flush=True)
