"""Verify a completed character against the exact reviewed source indices and native tile contract."""
from media import *
char=sys.argv[1];directory=HERE/'density-staging' if '--staging' in sys.argv else ANIM;meta=read(directory/f'{char}.json');atlas=Image.open(directory/f'{char}.png').convert('RGBA');issues=[];clips=[];native_checked=set();pixel_checks=[];threshold_outliers=[]
def main_extent(alpha):
 labels,n=ndimage.label(alpha>=128,structure=np.ones((3,3)));counts=np.bincount(labels.reshape(-1));counts[0]=0
 if not n:return [0,0]
 y,x=np.where(labels==int(counts.argmax()));return [int(x.max()-x.min()+1),int(y.max()-y.min()+1)]
if meta['frameCount']!=512 or len(meta['boxes'])!=512:issues.append('Expected complete512-frame character atlas')
if atlas.size!=(meta['width'],meta['height']):issues.append('Actual atlas dimensions do not match metadata')
if meta['pivot']!=[.5,.88]:issues.append('Wrong common foot anchor')
for action in SEGMENTS:
 for d in DIRS:
  clip=meta['clips'].get(action,{}).get(d)
  if not clip:issues.append(f'Missing {action}/{d}');continue
  folder=HERE/'jobs'/clip['sourceJob'];review=read(folder/'source-review.json');extracted=read(folder/'extracted.json');receipt=read(folder/'video.json');attempt=read(folder/'attempt.json')
  if action not in extracted.get('sourceFrameIndices',{}) or action not in extracted.get('clips',{}):issues.append(f'{action}/{d}: source action has been withdrawn');continue
  if not review.get('approved') or extracted.get('review')!='approved' or extracted.get('sourceReviewSha256')!=sha(folder/'source-review.json'):issues.append(f'{action}/{d}: stale or missing visual approval')
  if not all(i in extracted['sourceFrameIndices'][action] for i in clip['sourceFrameIndices']):issues.append(f'{action}/{d}: output frame not from approved source indices')
  if receipt['sha256']!=review['videoSha256'] or sha(PROJECT/receipt['fileRef'])!=receipt['sha256']:issues.append(f'{action}/{d}: source video mismatch')
  if sha(folder/'first-frame.png')!=attempt['firstFrameSha256']:issues.append(f'{action}/{d}: first-frame reference mismatch')
  if action in ['attack','cast'] and ACTION_VERSION in folder.name:issues.append(f'{action}/{d}: composite firing gesture reused')
  if d=='n' and action in ['walk','hit','death'] and ACTION_VERSION in folder.name:issues.append(f'{action}/{d}: uncorrected composite rear-facing source')
  if meta.get('spatialContract')=='per-clip-ground-anchor-v1':
   spec=read(folder/'request.json');pivot=clip.get('pivot');fit=clip.get('bakeFitScale',0);span=clip.get('nativePlaneSpan',0)
   if pivot!=spec.get('outputPivot',[.5,.88]):issues.append(f'{action}/{d}: wrong per-clip ground pivot')
   if not 0<fit<=1 or not math.isclose(span*fit,spec.get('outputPlaneSpan',2.5456),rel_tol=1e-8):issues.append(f'{action}/{d}: fit does not preserve world scale')
   if clip.get('sourceSize')!=extracted['actualSourceSize'] or extracted['actualSourceSize']!=[spec['width'],spec['height']]:issues.append(f'{action}/{d}: capture dimensions do not match spatial contract')
   if not math.isclose(spec.get('outputPlaneSpan',2.5456)/spec['width'],2.5456/384,rel_tol=1e-8):issues.append(f'{action}/{d}: source pixel world size changes between actions')
  density=meta.get('samplingContract')=='uniform-source-density-v1'
  if density:
   spec=read(folder/'request.json');crop=clip['sourceCrop'];frame_size=clip['frameSize'];span=clip['nativePlaneSpan'];foot=clip['sourceFootAnchor']
   if not meta.get('preservePixelDensity') or frame_size[0]!=frame_size[1] or not 256<=frame_size[0]<=512:issues.append(f'{action}/{d}: invalid density-preserving frame contract')
   if not math.isclose(frame_size[0]/crop[2],2/3,rel_tol=1e-10) or not math.isclose(frame_size[0]/span,256/2.5456,rel_tol=1e-8):issues.append(f'{action}/{d}: delivery pixel density changed')
   if not math.isclose(span/crop[2],spec.get('outputPlaneSpan',2.5456)/spec['width'],rel_tol=1e-8):issues.append(f'{action}/{d}: crop changed actor world size')
   if any(not math.isclose(clip['pivot'][i],(foot[i]-crop[i])/crop[2],abs_tol=1e-8) for i in range(2)):issues.append(f'{action}/{d}: crop lost original ground anchor')
   if not extracted.get('nativeFrames') or extracted['nativeFrames']['frameSize']!=extracted['actualSourceSize'] or extracted['nativeFrames']['scaleFromSource']!=1:issues.append(f'{action}/{d}: missing full-resolution original frame extraction')
   if folder.name not in native_checked:
    native_checked.add(folder.name);source_index=clip['sourceFrameIndices'][0];native_index=extracted['clips'][action]['start']+extracted['sourceFrameIndices'][action].index(source_index)
    native=np.asarray(Image.open(folder/'frames-native'/f'{native_index:03}.png').convert('RGBA'));reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);raw=None
    for source_number,raw_bytes in enumerate(reader):
     if source_number==source_index:raw=np.asarray(Image.frombytes('RGB',info['size'],raw_bytes));break
    reader.close();opaque=native[:,:,3]>=253
    error=int(np.abs(native[:,:,:3].astype(np.int16)-raw.astype(np.int16))[opaque].max()) if opaque.any() else 999
    if list(native.shape[:2][::-1])!=list(info['size']) or error>1:issues.append(f'{folder.name}: stored native frame was resampled or its opaque RGB changed')
    pixel_checks.append({'sourceJob':folder.name,'sourceFrame':source_index,'nativeSize':list(info['size']),'opaquePixelsCompared':int(opaque.sum()),'maxOpaqueRgbByteDifferenceFromActualMp4':error})
  hashes=[];alphas=[]
  for i in range(clip['start'],clip['endExclusive']):
   if not 0<=i<len(meta['boxes']):issues.append('Atlas index outside real rectangles');continue
   x,y,w,h=meta['boxes'][i];tile=atlas.crop((x,y,x+w,y+h));a=np.asarray(tile)[:,:,3]
   if [w,h]!=(clip['frameSize'] if density else [256,256]):issues.append(f'{action}/{d}: wrong tile extent')
   if density:
    source_index=clip['sourceFrameIndices'][i-clip['start']];native_index=extracted['clips'][action]['start']+extracted['sourceFrameIndices'][action].index(source_index);native_image=Image.open(folder/'frames-native'/f'{native_index:03}.png').convert('RGBA');source_alpha=np.asarray(native_image)[:,:,3]
    cx,cy,cw,ch=clip['sourceCrop'];expected=native_image.crop((cx,cy,cx+cw,cy+ch)).resize((w,h),Image.Resampling.LANCZOS)
    if tile.tobytes()!=expected.tobytes():issues.append(f'{action}/{d}: serialized tile differs from direct native crop at fixed density, frame{i}')
    sy,sx=np.where(source_alpha>=128);dy,dx=np.where(a>=128)
    if len(sx) and len(dx):
     native_extent=[int(sx.max()-sx.min()+1),int(sy.max()-sy.min()+1)];delivered_extent=[int(dx.max()-dx.min()+1),int(dy.max()-dy.min()+1)]
     if any(abs(delivered_extent[k]-native_extent[k]*2/3)>3 for k in range(2)):
      source_main=main_extent(source_alpha);delivered_main=main_extent(a);threshold_outliers.append({'frame':i,'sourceGlobalExtent':native_extent,'deliveredGlobalExtent':delivered_extent,'sourceMainExtent':source_main,'deliveredMainExtent':delivered_main,'reason':'isolated threshold-edge pixels may disappear during the exact required Lanczos sampling'})
      if any(abs(delivered_main[k]-source_main[k]*2/3)>3 for k in range(2)):issues.append(f'{action}/{d}: main body extent indicates lost pixel density at frame{i}')
   if max(a[0].max(),a[-1].max(),a[:,0].max(),a[:,-1].max())>10:issues.append(f'{action}/{d}: cropped opaque silhouette at frame{i}')
   if a.max()<100:issues.append(f'{action}/{d}: empty character frame{i}')
   hashes.append(hashlib.sha256(tile.tobytes()).hexdigest());alphas.append(float((a>10).mean()))
  unique=len(set(hashes))
  if unique<min(3,len(hashes)):issues.append(f'{action}/{d}: static or duplicated action')
  clips.append({'action':action,'direction':d,'frames':len(hashes),'uniqueFrames':unique,'sourceJob':folder.name,'sourceIndices':clip['sourceFrameIndices'],'alphaCoverage':[min(alphas),max(alphas)]})
result={'character':char,'at':stamp(),'frameCount':meta['frameCount'],'directions':8,'actions':6,'sourceVideos':len(meta['provenance']),'atlasSha256':sha(directory/f'{char}.png'),'samplingContract':meta.get('samplingContract'),'frameSizes':meta.get('frameSizes',[[256,256]]),'pixelDensityChecks':pixel_checks,'opacityThresholdOutliers':threshold_outliers,'clips':clips,'issues':issues,'pass':not issues}
save(HERE/f'{char}{"-staged-density" if "--staging" in sys.argv else ""}-verification.json',result);emit({'character':char,'frames':meta['frameCount'],'sourceVideos':len(meta['provenance']),'densitySourceChecks':len(pixel_checks),'issues':issues})
