"""Verify real model-working frames, per-direction variation and separately rendered closure."""
from media import *
results=[];issues=[]
paths={p.parent.name:p for root in ['model-work','model-work-v2','model-work-v3'] for p in (PROJECT/'Content/UI/v6'/root).glob('*/work.json')}
for id,path in sorted(paths.items()):
 info=read(path);n=info['framesPerDirection'];directions=[]
 for d in DIRS:
  images=[Image.open(path.parent/f'{d}-{i:02}.png').convert('RGBA') for i in range(n)]
  unique=len({hashlib.sha256(im.tobytes()).hexdigest() for im in images});edge=[]
  for i,im in enumerate(images):
   a=np.asarray(im)[:,:,3]
   if max(a[0].max(),a[-1].max(),a[:,0].max(),a[:,-1].max())>8:edge.append(i)
  closure=path.parent/f'{d}-closure.png';error=None
  if closure.exists():error=float(np.abs(np.asarray(images[0],dtype=np.float32)-np.asarray(Image.open(closure).convert('RGBA'),dtype=np.float32)).mean())
  if id in ['wind-power','hydro-power','cargo-aircraft'] and (n<16 or unique<16):issues.append(f'{id}/{d}: requires16 actual distinct rendered frames, got{unique}/{n}')
  if unique<3:issues.append(f'{id}/{d}: motion not visible')
  if edge:issues.append(f'{id}/{d}: clipped source working frames {edge}')
  if error is not None and error>1.:issues.append(f'{id}/{d}: closure seam {error}')
  directions.append({'direction':d,'frames':n,'uniqueFrames':unique,'sourceBoundaryFrames':edge,'closureMeanAbsoluteByteError':error})
 results.append({'id':id,'source':str(path.relative_to(PROJECT)).replace('\\','/'),'fps':info['fps'],'directions':directions})
save(HERE/'work-verification.json',{'at':stamp(),'workingModules':len(results),'renderedWorkFrames':sum(d['frames'] for r in results for d in r['directions']),'results':results,'issues':issues,'pass':not issues})
emit({'workingModules':len(results),'issues':issues})
