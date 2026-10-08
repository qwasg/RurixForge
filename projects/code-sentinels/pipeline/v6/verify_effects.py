"""Actual decode/provenance/atlas checks for shipped effects, separate from character completion."""
from media import *
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json');effects=manifest['effects'];results=[];issues=[]
for id,item in effects.items():
 if not item['ready']:issues.append(id+': not ready');continue
 folder=ROOT/'packages/client/public/games/code-sentinels/effects-v6';meta=read(folder/(id+'.json'));im=Image.open(folder/(id+'.png')).convert('RGBA');boxes=meta['boxes'];frames=[]
 for i,b in enumerate(boxes):
  x,y,w,h=b
  if x<0 or y<0 or x+w>im.width or y+h>im.height:issues.append(id+': frame bounds '+str(i));continue
  tile=im.crop((x,y,x+w,y+h));frames.append(tile)
  a=np.asarray(tile)[:,:,3]
  if item['sourceVersion']==6 and max(a[0].max(),a[-1].max(),a[:,0].max(),a[:,-1].max())>8:issues.append(id+': nontransparent tile edge '+str(i))
 unique=len({hashlib.sha256(f.tobytes()).hexdigest() for f in frames})
 if unique<20:issues.append(id+': insufficient actual temporal variation '+str(unique))
 record={'id':id,'frames':len(frames),'uniqueFrames':unique,'sourceVersion':item['sourceVersion'],'atlasSha256':sha(folder/(id+'.png'))}
 if item['sourceVersion']==6:
  job=HERE/'jobs'/f'fx-{id}-v2';receipt=read(job/'video.json');attempt=read(job/'attempt.json');workflow=read(job/'workflow.json');video=PROJECT/receipt['fileRef']
  if sha(video)!=receipt['sha256']:issues.append(id+': original MP4 checksum mismatch')
  if sha(job/'first-frame.png')!=attempt['firstFrameSha256']:issues.append(id+': actual first-frame checksum mismatch')
  if workflow['6']['class_type']!='MiniMaxH3ImageToVideo' or workflow['6']['inputs']['first_frame']!=['20',0]:issues.append(id+': missing native I2V graph')
  if read(job/'history.json')['status']['status_str']!='success':issues.append(id+': provider did not succeed')
  reader=imageio_ffmpeg.read_frames(str(video),pix_fmt='rgb24');info=next(reader);source_count=sum(1 for _ in reader)
  if source_count<124:issues.append(id+': short actual video')
  if np.asarray(frames[0])[:,:,3].max()!=0 or np.asarray(frames[-1])[:,:,3].max()!=0:issues.append(id+': hard entrance or exit')
  record.update({'decodedSourceFrames':source_count,'sourceSize':info['size'],'promptId':attempt['promptId'],'cloudPaidCreates':attempt['cloudPaidCreates'],'videoSha256':sha(video),'nativeI2V':True})
 results.append(record)
save(HERE/'effect-verification.json',{'at':stamp(),'effects':len(results),'frames':sum(r['frames'] for r in results),'newNativeI2V':sum(r['sourceVersion']==6 for r in results),'preservedV5':sum(r['sourceVersion']==5 for r in results),'results':results,'issues':issues,'pass':not issues})
emit({'effects':len(results),'frames':sum(r['frames'] for r in results),'issues':issues})
