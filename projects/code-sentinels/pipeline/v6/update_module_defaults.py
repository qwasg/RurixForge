"""Update metadata placement only; keep every original pixel, bbox, pivot and world span."""
from media import *
updated=[]
for path in (PROJECT/'Content/UI/v6/model-bakes').glob('*/bake.json'):
 source=read(path)
 if source['category']!='module':continue
 id=source['id'];public=ROOT/'packages/client/public/games/code-sentinels/buildings-v6'/f'{id}.json'
 if not public.exists():continue
 meta=read(public);meta['defaultDirection']='ne';meta['defaultPlacement']='unrotated model geometry aligned to map x/y; direction index5 has root yaw-360degrees'
 save(public,meta);shutil.copy2(public,PROJECT/'Content/Animations/v6/buildings'/f'{id}.json');updated.append(id)
save(HERE/'static-direction-update.json',{'at':stamp(),'updated':updated,'defaultDirection':'ne','defaultIndex':5,'sourceGeometryChanged':False,'pixelsChanged':False,'pivotOrSpanChanged':False,'movingDirectionMapChanged':False});emit({'staticMetadataUpdated':len(updated),'pixelChanges':0})
