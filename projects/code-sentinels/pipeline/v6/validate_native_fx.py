"""Validate real GPU FX readback against source RGBA and configured Vulkan blend factors."""
from media import *
run=sys.argv[sys.argv.index('--run')+1] if '--run' in sys.argv else 'fx-lifecycle-20260911'
if not all(c in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in run):raise ValueError('Invalid QA label')
out=PROJECT/'Logs/v6'/run;report=read(out/'capture-report.json');base=np.asarray(Image.open(out/'baseline.png').convert('RGBA'));atlases={};docs={};results=[];issues=[];coverage={};expired=[]
for entry in report['captures']:
 expected=entry.get('expected',[])
 if not expected or any(v.get('kind') for v in expected):continue
 actual=np.asarray(Image.open(out/entry['file']).convert('RGBA'))
 if all(v.get('expectedPresent') is False for v in expected):
  same=np.array_equal(actual,base);expired.append({'file':entry['file'],'equalsEmptyNativeBaseline':same,'eventsRetainedBeyondDuration':True})
  if not same:issues.append(entry['file']+': expired FX not empty')
 for v in expected:
  if v.get('expectedPresent') is False:continue
  asset=v['asset'];coverage.setdefault(asset,set()).add(v['frame'])
  if asset not in docs:docs[asset]=read(PROJECT/'Content/Animations/v6/effects'/f'{asset}.json');atlases[asset]=np.asarray(Image.open(PROJECT/'Content/Animations/v6/effects'/f'{asset}.png').convert('RGBA'))
  d=docs[asset];x,y,w,h=d['boxes'][v['frame']];tile=atlases[asset][y:y+h,x:x+w];span=float(np.float32(v['span']))*90;left=v['expectedFoot'][0]-float(np.float32(v['pivot'][0]))*span;top=v['expectedFoot'][1]-float(np.float32(v['pivot'][1]))*span
  px=np.arange(max(0,math.ceil(left-.5)),min(1920,math.ceil(left+span-.5)));py=np.arange(max(0,math.ceil(top-.5)),min(1080,math.ceil(top+span-.5)));qx=(px+.5-left)/span*w;qy=(py+.5-top)/span*h;tx=np.clip(np.floor(qx).astype(int),0,w-1);ty=np.clip(np.floor(qy).astype(int),0,h-1);src=tile[ty[:,None],tx[None,:]].astype(float);bg=base[py[:,None],px[None,:],:3].astype(float);gpu=actual[py[:,None],px[None,:],:3].astype(float);alpha=src[:,:,3:4]/255
  # vendor/rurix/src/rurix-rt/src/render_exec.rs: Alpha uses SRC_ALPHA,
  # ONE_MINUS_SRC_ALPHA; Additive uses SRC_ALPHA, ONE. No synthetic GPU image.
  predicted=np.clip(src[:,:,:3]*alpha+bg*(1 if v['blend']=='additive' else 1-alpha),0,255)
  uncertain=(np.abs(qy-np.round(qy))[:,None]<.02)|(np.abs(qx-np.round(qx))[None,:]<.02)
  meaningful=(np.abs(predicted-bg).max(2)>8)&~uncertain;points=int(meaningful.sum());error=np.abs(predicted-gpu);match=float((error.max(2)[meaningful]<=3).mean()) if points else 1
  results.append({'asset':asset,'frame':v['frame'],'file':entry['file'],'blend':v['blend'],'meaningfulScreenPixelsCompared':points,'matchWithin3Bytes':match,'meanRgbError':float(error[meaningful].mean()) if points else 0})
  if points>30 and match<.985:issues.append(f'{asset} frame{v["frame"]}: blend/source GPU match {match:.5f}')
for asset,frames in coverage.items():
 if frames!=set(range(docs[asset]['frameCount'])):issues.append(asset+': source-frame coverage incomplete')
for loop in report.get('persistentLoops',[]):
 if not loop['sourceStartAndNextLoopStartHaveIdenticalGpuPixels']:issues.append(loop['asset']+': persistent loop did not return to its real source entry frame')
result={'at':stamp(),'engineHash':report['engineHash'],'scope':'Actual Rurix GPU PNG compared to source texels blended over separately captured actual terrain with configured Vulkan factors. Narrow ambiguous raster UV boundaries excluded. Rotation and death-vector cases require separate visual/identity review.','effects':len(coverage),'uniqueSourceFrames':sum(map(len,coverage.values())),'sourceFramesCompared':len(results),'complete':report.get('captured',False),'expired':expired,'persistentLoops':report.get('persistentLoops',[]),'results':results,'issues':issues,'pass':report.get('captured',False) and len(coverage)==15 and not issues}
save(out/'gpu-fx-fidelity.json',result);emit({k:v for k,v in result.items() if k not in ['results','expired','scope']})
