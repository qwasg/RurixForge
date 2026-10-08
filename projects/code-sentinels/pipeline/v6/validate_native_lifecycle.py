"""Compare actual GPU readbacks to expected source texels at the native sprite projection.
No image is synthesized here. PNGs under Logs are genuine native output; this is
an offline numerical check of frame/anchor/density consumption, not gameplay.
"""
from media import *
run=sys.argv[sys.argv.index('--run')+1] if '--run' in sys.argv else 'media-lifecycle-20260911'
if not all(c in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in run):raise ValueError('Invalid QA label')
out=PROJECT/'Logs/v6'/run;report=read(out/'capture-report.json');baseline=np.asarray(Image.open(out/'baseline.png').convert('RGBA'));atlases={};metadata={};results=[];issues=[];coverage={};expired=[]
for entry in report['captures']:
 expected=entry.get('expected',[])
 if not expected:continue
 actual=np.asarray(Image.open(out/entry['file']).convert('RGBA'));height,width=actual.shape[:2];density=height*report['view']['zoom']/48
 if expected[0].get('expectedPresent') is False:
  same=np.array_equal(actual,baseline);expired.append({'file':entry['file'],'equalsEmptyNativeBaseline':same})
  if not same:issues.append(entry['file']+': expired death did not return to baseline')
  continue
 for item in expected:
  char=item['character'];action=item['action'];direction=item['direction'];key=f'{char}/{action}/{direction}';coverage.setdefault(key,set()).add(item['frame'])
  if char not in atlases:atlases[char]=np.asarray(Image.open(ANIM/f'{char}.png').convert('RGBA'));metadata[char]=read(ANIM/f'{char}.json')
  doc=metadata[char];x,y,w,h=doc['boxes'][item['frame']];tile=atlases[char][y:y+h,x:x+w]
  # shader receives f32 metadata values; expectedFoot comes from the exact
  # fixture world coordinates and checked isometric projection.
  span=float(np.float32(item['span']))*density;plane_h=span*h/w;pivot=[float(np.float32(v)) for v in item['pivot']]
  left=item['expectedFoot'][0]-pivot[0]*span;top=item['expectedFoot'][1]-pivot[1]*plane_h
  px=np.arange(max(0,math.ceil(left-.5)),min(width,math.ceil(left+span-.5)));py=np.arange(max(0,math.ceil(top-.5)),min(height,math.ceil(top+plane_h-.5)))
  qx=(px+.5-left)/span*w;qy=(py+.5-top)/plane_h*h
  tx=np.clip(np.floor(qx).astype(int),0,w-1);ty=np.clip(np.floor(qy).astype(int),0,h-1)
  source=tile[ty[:,None],tx[None,:]];all_opaque=source[:,:,3]>=253
  # At near-integral UVs, Vulkan's subpixel vertex quantization/f32 interpolation
  # can validly select either neighboring texel. Exclude only those narrow
  # boundaries from RGB equality, not any broad silhouette/depth area.
  uncertain=(np.abs(qy-np.round(qy))[:,None]<.02)|(np.abs(qx-np.round(qx))[None,:]<.02)
  opaque=all_opaque&~uncertain;points=int(opaque.sum());best=None
  for dx,dy in [(0,0),(-1,0),(1,0),(0,-1),(0,1)]:
   if px[0]+dx<0 or px[-1]+dx>=width or py[0]+dy<0 or py[-1]+dy>=height:continue
   observed=actual[py[:,None]+dy,px[None,:]+dx,:3];error=np.abs(source[:,:,:3].astype(np.int16)-observed.astype(np.int16));max_error=error.max(axis=2)
   match=float((max_error[opaque]<=3).mean()) if points else 0.;mean=float(error[opaque].mean()) if points else 999.
   record={'character':char,'action':action,'direction':direction,'frame':item['frame'],'frameSize':[w,h],'file':entry['file'],'opaqueScreenPixelsChecked':points,'opaqueScreenPixelsAtAmbiguousSubpixelBoundaries':int((all_opaque&uncertain).sum()),'matchWithin3Bytes':match,'meanRgbError':mean,'pixelAlignment':[dx,dy],'projectionPlane':[left,top,span,plane_h],'expectedFoot':item['expectedFoot']}
   if best is None or match>best['matchWithin3Bytes']:best=record
   if match>=.9995:break
  results.append(best)
  if best['matchWithin3Bytes']<.99 or points<100:issues.append(f'{key} frame{item["frame"]}: native/source texel match {best["matchWithin3Bytes"]:.5f}, pixels {points}')
complete=report.get('captured',False) and len(report['captures'])==456
if complete:
 for char in CHARACTERS:
  doc=metadata[char]
  for action,dirs in doc['clips'].items():
   for d,clip in dirs.items():
    if coverage.get(f'{char}/{action}/{d}',set())!=set(range(clip['start'],clip['endExclusive'])):issues.append(f'{char}/{action}/{d}: not every actual atlas frame reached GPU capture')
result={'at':stamp(),'engineHash':report['engineHash'],'scope':'Actual Rurix GPU PNG vs delivered atlas opaque RGB texels at exact isometric sprite projection. A one-pixel alignment search tolerates raster rounding only; no RGB or pose generation. Alpha silhouettes/FX also receive separate visual review.','captureCount':len(report['captures']),'characterFramesCompared':len(results),'complete':complete,'expiredDeaths':expired,'minTexelMatch':min((r['matchWithin3Bytes'] for r in results),default=0),'maxMeanRgbError':max((r['meanRgbError'] for r in results),default=0),'results':results,'issues':issues,'pass':complete and not issues}
save(out/'gpu-frame-fidelity.json',result);emit({k:v for k,v in result.items() if k not in ['results','scope','expiredDeaths']})
