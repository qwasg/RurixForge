"""Raycast the actual modeled doorway: closed leaves block passage, open leaves clear it."""
import bpy,json,hashlib
from pathlib import Path
from mathutils import Vector
HERE=Path(__file__).resolve().parent;PROJECT=HERE.parents[1];checks=[]
for state,file in [('closed','door.blend'),('open','door-open.blend')]:
 path=PROJECT/'Content/Models/v6'/file;bpy.ops.wm.open_mainfile(filepath=str(path));deps=bpy.context.evaluated_depsgraph_get();hits=[]
 for x in [-.10,.10]:
  hit=bpy.context.scene.ray_cast(deps,Vector((x,-1,.65)),Vector((0,1,0)),distance=2.)
  hits.append({'x':x,'blocked':bool(hit[0]),'object':hit[4].name if hit[0] else None})
 checks.append({'state':state,'source':str(path.relative_to(PROJECT)).replace('\\','/'),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'rays':hits,'pass':all(r['blocked']==(state=='closed') for r in hits)})
(HERE/'door-geometry-verification.json').write_text(json.dumps({'checks':checks,'pass':all(c['pass'] for c in checks)},indent=2),encoding='utf-8');print(json.dumps(checks),flush=True)
