"""Verify actual raw weapon animation geometry renders, boundaries and temporal change."""
from media import *
results=[];issues=[]
for path in sorted((PROJECT/'Content/UI/v6/model-attacks').glob('*/attack.json')):
 meta=read(path);id=meta['id'];directions=[]
 for d in DIRS:
  images=[Image.open(path.parent/f'{d}-{i:02}.png').convert('RGBA') for i in range(meta['framesPerDirection'])]
  unique=len({hashlib.sha256(im.tobytes()).hexdigest() for im in images});bad=[]
  for i,im in enumerate(images):
   a=np.asarray(im)[:,:,3]
   if a.max()<100 or max(a[0].max(),a[-1].max(),a[:,0].max(),a[:,-1].max())>8:bad.append(i)
  if bad:issues.append(f'{id}/{d}: blank or edge-clipped frames {bad}')
  if unique<6:issues.append(f'{id}/{d}: insufficient actual rendered variation {unique}')
  directions.append({'direction':d,'frames':len(images),'uniqueFrames':unique,'badBoundaryFrames':bad})
 results.append({'id':id,'sourceModel':meta['sourceModel'],'directions':directions,'recoilingMeshes':len(meta['recoilingMeshes']),'actuatedMeshes':len(meta['actuatedMeshes']),'emissiveMaterials':meta['emissiveMaterials']})
save(HERE/'weapon-attack-verification.json',{'at':stamp(),'weapons':len(results),'renderedAttackFrames':sum(d['frames'] for r in results for d in r['directions']),'results':results,'issues':issues,'pass':not issues});emit({'weapons':len(results),'issues':issues})
