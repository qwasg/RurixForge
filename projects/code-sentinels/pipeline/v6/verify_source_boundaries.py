"""Inspect actual selected raw RGB source boundaries before any atlas padding or garbage matte."""
from media import *
char=sys.argv[1];directory=HERE/'density-staging' if '--staging' in sys.argv else ANIM;doc=read(directory/f'{char}.json');allowances=read(HERE/'reviewed-source-edge-allowances.json');jobs={};issues=[];candidates=[];rgb_checks=0;max_rgb_error=0
for action,dirs in doc['clips'].items():
 for d,clip in dirs.items():
  for frame in clip['sourceFrameIndices']:jobs.setdefault(clip['sourceJob'],{}).setdefault(frame,[]).append((action,d))
checked=0
for id,indices in jobs.items():
 folder=HERE/'jobs'/id;receipt=read(folder/'video.json');extracted=read(folder/'extracted.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader)
 for index,raw in enumerate(reader):
  if index not in indices:continue
  checked+=1;original=Image.frombytes('RGB',info['size'],raw);alpha=np.asarray(key_green(original))[:,:,3]
  if doc.get('preservePixelDensity'):
   action=indices[index][0][0];native_index=extracted['clips'][action]['start']+extracted['sourceFrameIndices'][action].index(index);native=np.asarray(Image.open(folder/'frames-native'/f'{native_index:03}.png').convert('RGBA'));opaque=native[:,:,3]>=253
   error=int(np.abs(native[:,:,:3].astype(np.int16)-np.asarray(original).astype(np.int16))[opaque].max()) if opaque.any() else 999
   rgb_checks+=1;max_rgb_error=max(max_rgb_error,error)
   if tuple(native.shape[:2][::-1])!=info['size'] or error>1:issues.append(f'{id}: native RGB was resampled or altered at raw frame{index}')
  edge=np.concatenate([alpha[0],alpha[-1],alpha[:,0],alpha[:,-1]]);touch=int((edge>=150).sum())
  if not touch:continue
  for action,d in indices[index]:
   allowance=allowances.get(f'{char}/{d}/{action}',{});source_review=read(HERE/'jobs'/id/'source-review.json');permitted=allowance.get('sourceJob')==id and (allowance.get('classification')=='peripheral-effect-only' or (allowance.get('classification')=='isolated-background-only' and source_review.get('garbageMatte')==allowance.get('requiresGarbageMatte')))
   candidates.append({'sourceJob':id,'sourceFrame':index,'action':action,'direction':d,'opaqueEdgePixels':touch,'reviewedPeripheralEffectOnly':permitted})
   if not permitted:issues.append(f'{action}/{d}: unreviewed raw source edge at{index} ({touch}px)')
result={'at':stamp(),'character':char,'rawSourceFramesChecked':checked,'nativeOpaqueRgbFramesCompared':rgb_checks,'maxNativeOpaqueRgbByteDifference':max_rgb_error,'candidates':candidates,'issues':issues,'pass':not issues,'scope':'actual MP4 RGB with chroma key before resizing, padding or garbage matte; peripheral FX exceptions require explicit source-specific visual review'};save(HERE/f'{char}-source-boundary-verification.json',result);emit({'character':char,'rawSourceFramesChecked':checked,'rgbFramesCompared':rgb_checks,'maxRgbError':max_rgb_error,'candidates':len(candidates),'issues':issues})
