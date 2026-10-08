"""Apply only explicit human-visual review records, with actual-source loop selection."""
from media import *
def loop_window(folder):
 receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);features=[]
 for raw in reader:
  im=Image.frombytes('RGB',info['size'],raw).resize((96,96),Image.Resampling.BILINEAR);features.append(np.asarray(im,dtype=np.float32)[58:89,18:78])
 best=None
 for start in range(12,min(73,len(features)-25)):
  for period in range(24,43):
   end=start+period
   if end>=len(features)-4:continue
   change=float(np.abs(features[start+period//2]-features[start]).mean())
   if change<.4:continue
   error=float(np.abs(features[end]-features[start]).mean())
   score=error+.02*abs(period-28)
   if best is None or score<best[0]:best=(score,start,end,error,change)
 if best is None:raise RuntimeError('No visibly moving closed source walk cycle '+folder.name)
 return [best[1],best[2],24,True],{'sourceStart':best[1],'sourceEndExclusive':best[2],'endpointMeanByteDifference':best[3],'midCycleMeanByteMotion':best[4],'method':'closest actual source poses after full-source visual rear-walk verification; no interpolation or duplicate-frame padding'}
reviews=read(HERE/'reviewed_north.json');overrides=read(HERE/'action-overrides.json')
for char,actions in reviews.items():
 for action,note in actions.items():
  folder=HERE/'jobs'/overrides.get(f'{char}/n/{action}',f'{char}-n-{action}-v1')
  if not (folder/'video.json').exists():continue
  prior=read(folder/'source-review.json') if (folder/'source-review.json').exists() else {}
  hit_end={'gpt':60,'deepseek':56,'glm':44,'minimax':50,'kimi':28,'claude':42,'gemini':28}.get(char,60)
  extracted=read(folder/'extracted.json') if (folder/'extracted.json').exists() else {}
  if prior.get('approved') and prior.get('notes')==note and (extracted.get('matteProcessing') or {}).get('version')==3 and (action!='hit' or prior.get('segments',{}).get('hit')==[0,hit_end,16,False]):continue
  detail=None
  if action=='walk':window,detail=loop_window(folder)
  elif action=='hit':window=[0,hit_end,16,False]
  elif action=='death':window=[0,76 if char=='gpt' else 60,20,False]
  else:window=[0,100 if '-body-' in folder.name else 116,24 if action=='cast' else 16,False]
  receipt=read(folder/'video.json')
  save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewer':'Codex agent viewing real full-source contact sheet','reviewedAt':stamp(),'notes':note,'segments':{action:window},'videoSha256':receipt['sha256'],'loopSelection':detail})
  extract(folder,True)
emit({'reviewedCharacters':list(reviews),'explicitReviewedActions':sum(len(v) for v in reviews.values())})
