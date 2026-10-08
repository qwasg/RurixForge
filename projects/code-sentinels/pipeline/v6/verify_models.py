"""Inspect real output files, alpha bounds, model containers and shared-chassis coverage."""
from media import *
ALIASES=['machinegun','light-mortar','scout','tank','aa-launcher','breacher','artillery','attack-aircraft','rail-accelerator','particle-cannon','missile-truck','bomber','aerospace','orbital-strike','anti-orbital']
BAKES=PROJECT/'Content/UI/v6/model-bakes';PUB=ROOT/'packages/client/public/games/code-sentinels'
results=[];issues=[]
opened={r['id']:r for r in read(HERE/'model-open-verification.json')['results']}
for path in sorted(BAKES.glob('*/bake.json')):
 meta=read(path);id=meta['id'];model=PROJECT/meta['sourceModel'];glb=PROJECT/meta['glb']
 if id not in opened or not opened[id]['pass'] or opened[id]['sha256']!=sha(model):issues.append(id+': Blender reopen verification failed')
 if glb.read_bytes()[:4]!=b'glTF':issues.append(id+': missing real GLB')
 source_clipped=[];runtime_clipped=[];extents=[]
 kind='buildings-v6' if meta['category']=='module' else 'units-v6'
 runtime=Image.open(PUB/kind/(id+'.png')).convert('RGBA')
 runtime_meta=read(PUB/kind/(id+'.json'))
 for i,d in enumerate(DIRS):
  im=Image.open(path.parent/(d+'.png')).convert('RGBA');alpha=np.asarray(im)[:,:,3]
  if alpha.max()==0:issues.append(id+': blank render '+d)
  edges=np.concatenate([alpha[0],alpha[-1],alpha[:,0],alpha[:,-1]])
  if edges.max()>8:source_clipped.append(d)
  x,y,w,h=runtime_meta['boxes'][i];tile=runtime.crop((x,y,x+w,y+h));a=np.asarray(tile)[:,:,3]
  if max(a[0].max(),a[-1].max(),a[:,0].max(),a[:,-1].max())>8:runtime_clipped.append(d)
  extents.append(tile.getbbox())
 if source_clipped:issues.append(id+': source boundary clipping '+','.join(source_clipped))
 if runtime_clipped:issues.append(id+': runtime boundary clipping '+','.join(runtime_clipped))
 for sequence in ['work','attack']:
  if sequence not in runtime_meta['clips']:continue
  source=runtime_meta[sequence+'Provenance'];source_model=PROJECT/source['sourceModel'];source_id=source_model.stem
  if source_id not in opened or not opened[source_id]['pass'] or opened[source_id]['sha256']!=sha(source_model) or source['sourceModelSha256']!=sha(source_model):issues.append(id+': '+sequence+' source verification mismatch')
  for d,clip in runtime_meta['clips'][sequence].items():
   hashes=[]
   for i in range(clip['start'],clip['endExclusive']):
    x,y,w,h=runtime_meta['boxes'][i];tile=runtime.crop((x,y,x+w,y+h));a=np.asarray(tile)[:,:,3];hashes.append(hashlib.sha256(tile.tobytes()).hexdigest())
    if max(a[0].max(),a[-1].max(),a[:,0].max(),a[:,-1].max())>8:issues.append(id+': '+sequence+' boundary clipping '+d)
   if len(set(hashes))<3:issues.append(id+': '+sequence+' is not animated '+d)
 for state in runtime_meta.get('stateProvenance',[]):
  state_model=PROJECT/state['sourceModel'];state_id=state_model.stem
  if state_id not in opened or not opened[state_id]['pass'] or opened[state_id]['sha256']!=sha(state_model) or state['sourceModelSha256']!=sha(state_model):issues.append(id+': state source mismatch '+state['state'])
  for d,clip in runtime_meta['clips'][state['state']].items():
   x,y,w,h=runtime_meta['boxes'][clip['start']];tile=runtime.crop((x,y,x+w,y+h));alpha=np.asarray(tile)[:,:,3]
   if alpha.max()<100 or max(alpha[0].max(),alpha[-1].max(),alpha[:,0].max(),alpha[:,-1].max())>8:issues.append(id+': invalid state boundary '+state['state']+'/'+d)
   ix,iy,iw,ih=runtime_meta['boxes'][runtime_meta['clips']['idle'][d]['start']]
   if tile.tobytes()==runtime.crop((ix,iy,ix+iw,iy+ih)).tobytes():issues.append(id+': state is indistinguishable from idle '+state['state']+'/'+d)
 results.append({'id':id,'directions':8,'sourceClipped':source_clipped,'runtimeClipped':runtime_clipped,'runtimeBounds':extents,'modelBytes':model.stat().st_size,'glbBytes':glb.stat().st_size})
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json')
for alias in ALIASES:
 if alias not in manifest['models']:issues.append('Missing shared chassis '+alias)
save(HERE/'model-verification.json',{'at':stamp(),'models':len(results),'renders':len(results)*8,'sharedChassis':len(ALIASES),'results':results,'issues':issues,'pass':not issues})
emit({'models':len(results),'renders':len(results)*8,'issues':issues})
