"""Reopen every saved Blender file with Blender itself, rather than guessing compressed headers."""
import bpy,json,hashlib,math,sys
from pathlib import Path
HERE=Path(__file__).resolve().parent;PROJECT=HERE.parents[1]
only=set(sys.argv[sys.argv.index('--only')+1].split(',')) if '--only' in sys.argv else None
results=[r for r in json.loads((HERE/'model-open-verification.json').read_text())['results'] if r['id'] not in only] if only and (HERE/'model-open-verification.json').exists() else []
for path in sorted((PROJECT/'Content/Models/v6').glob('*.blend')):
 if only and path.stem not in only:continue
 bpy.ops.wm.open_mainfile(filepath=str(path))
 scene=bpy.context.scene;camera=scene.camera
 forward=camera.matrix_world.to_quaternion() @ __import__('mathutils').Vector((0,0,-1))
 elevation=math.degrees(math.asin(abs(forward.z)))
 meshes=sum(o.type=='MESH' for o in scene.objects)
 results.append({'id':path.stem,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'meshObjects':meshes,'cameraType':camera.data.type,'elevation':elevation,'renderer':scene.render.engine,'pass':meshes>0 and camera.data.type=='ORTHO' and abs(elevation-30)<.02})
(HERE/'model-open-verification.json').write_text(json.dumps({'models':len(results),'results':results,'pass':all(r['pass'] for r in results)},indent=2),encoding='utf-8')
print('V6_MODEL_OPEN_VERIFIED '+str(len(results)),flush=True)
