"""Actual gun recoil, launcher actuation and energized emitter meshes from authored weapons."""
import bpy,math,json
from pathlib import Path
from mathutils import Vector
HERE=Path(__file__).resolve().parent;PROJECT=HERE.parents[1];DIRS=['s','sw','w','nw','n','ne','e','se']
IDS=['vscode','pycharm','autocannon','mortar','scout-buggy','light-tank','aa-turret','breach-tank','artillery','rail-tank','particle-cannon','missile-truck','orbital-lance','aegis-array','fighter','stealth-wing','aerospace-fighter'];count=12
for id in IDS:
 dest=PROJECT/'Content/UI/v6/model-attacks'/id
 if (dest/'attack.json').exists():continue
 bpy.ops.wm.open_mainfile(filepath=str(PROJECT/'Content/Models/v6'/(id+'.blend')));scene=bpy.context.scene;root=bpy.data.objects[id+' assembly']
 scene.render.engine='CYCLES';scene.cycles.device='CPU';scene.cycles.samples=12;scene.render.threads_mode='FIXED';scene.render.threads=6
 recoil_words=['main cannon','muzzle brake','mortar barrel','mortar rim','howitzer','split accelerator rail','magnetic coil','energy core']
 recoil=[o for o in scene.objects if any(o.name.startswith(w) for w in recoil_words)];actuation=[o for o in scene.objects if any(o.name.startswith(w) for w in ['missile cassette','missile nose','underwing missile','phased emitter arm','emitter crystal'])]
 originals={o.name:(o.location.copy(),o.rotation_euler.copy()) for o in recoil+actuation};emissions=[]
 for material in bpy.data.materials:
  if material.use_nodes:
   node=material.node_tree.nodes.get('Principled BSDF')
   if node and node.inputs['Emission Strength'].default_value>0:emissions.append((node,node.inputs['Emission Strength'].default_value))
 for frame in range(count):
  t=frame/(count-1);kick=math.sin(min(1,t/.18)*math.pi/2)*math.exp(-max(0,t-.18)*8) if t<1 else 0
  if frame==count-1:kick=0
  flash=math.exp(-((t-.13)/.10)**2)
  for o in recoil:
   loc,rotation=originals[o.name];o.location=loc+Vector((0,-.10*kick,-.018*kick));o.keyframe_insert('location',frame=frame+1)
  for o in actuation:
   loc,rotation=originals[o.name];o.location=loc+Vector((0,.045*kick,0));o.rotation_euler=rotation.copy();o.rotation_euler.x+=.04*kick;o.keyframe_insert('location',frame=frame+1);o.keyframe_insert('rotation_euler',frame=frame+1)
  for node,base in emissions:
   node.inputs['Emission Strength'].default_value=base*(1+3.5*flash);node.inputs['Emission Strength'].keyframe_insert('default_value',frame=frame+1)
 scene.frame_start=1;scene.frame_end=count;scene.render.fps=18;scene.frame_set(1)
 bpy.ops.wm.save_as_mainfile(filepath=str(PROJECT/'Content/Models/v6'/(id+'-attack.blend')),check_existing=False);dest.mkdir(parents=True,exist_ok=True)
 for direction_index,d in enumerate(DIRS):
  for frame in range(count):
   scene.frame_set(frame+1);root.rotation_euler.z=math.radians(-135-direction_index*45);scene.render.filepath=str(dest/f'{d}-{frame:02}.png');bpy.ops.render.render(write_still=True)
 meta={'id':id,'directions':DIRS,'framesPerDirection':count,'fps':18,'loop':False,'sourceModel':f'Content/Models/v6/{id}-attack.blend','method':'Actual Blender barrel recoil / launcher and emitter mechanism motion / energized physical material, CPU render','recoilingMeshes':[o.name for o in recoil],'actuatedMeshes':[o.name for o in actuation],'emissiveMaterials':len(emissions),'wholeImageRotation':False,'projectileAndImpact':'External separately verified true-video VFX and actual native ballistic entities'}
 (dest/'attack.json').write_text(json.dumps(meta,indent=2),encoding='utf-8');print('V6_ATTACK_BAKED '+id,flush=True)
