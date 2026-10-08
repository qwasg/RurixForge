"""V6 actual local H3 production; immutable prompt receipts and actual video frames."""
import argparse, hashlib, json, math, shutil, sys, time, uuid
from pathlib import Path
from datetime import datetime, timezone
HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
ROOT=PROJECT.parents[1]
sys.path.insert(0,str(HERE.parent/'python-libs'))
import requests
import numpy as np
from scipy import ndimage
from PIL import Image, ImageDraw
import imageio_ffmpeg
BASE='http://127.0.0.1:8188'
DIRS=['s','sw','w','nw','n','ne','e','se']
DIRECTION_MAP=[{'index':i,'label':d,'worldDelta':v,'screenDelta':[(v[0]-v[1])*.5,(v[0]+v[1])*.25],'blenderYawDegrees':-135-i*45} for i,(d,v) in enumerate(zip(DIRS,[[1,1],[0,1],[-1,1],[-1,0],[-1,-1],[0,-1],[1,-1],[1,0]]))]
DIRECTION_CONVENTION={'labels':'screen directions on the isometric map','worldAxes':'map +x,+y; screenX=(x-y)*.5, screenDown=(x+y)*.25','mapping':DIRECTION_MAP,'indexFormula':'positive_mod(round((atan2(dy,dx)-pi/4)/(pi/4)),8)','zeroMovement':'retain previous direction','blenderModelBasis':'mapX=modelX; mapY=-modelY; baked root yaw=-135-index*45 degrees'}
CHARACTERS=['gemini','claude','kimi','minimax','glm','deepseek','gpt']
ACTION_VERSION='actions-v2'
PUBLIC=ROOT/'packages/client/public/games/code-sentinels/characters-v6'
ANIM=PROJECT/'Content/Animations/v6/characters'
# Actual source timeline: 9 seconds. Each segment becomes an independent game clip.
SEGMENTS={'idle':(0,24,6,True),'walk':(24,72,12,True),'attack':(72,104,8,False),
 'cast':(104,152,12,False),'hit':(152,176,6,False),'death':(176,216,10,False)}
def stamp():return datetime.now(timezone.utc).isoformat()
def read(p):return json.loads(p.read_text(encoding='utf-8-sig'))
def save(p,obj):
 p.parent.mkdir(parents=True,exist_ok=True)
 q=p.with_suffix(p.suffix+'.tmp');q.write_text(json.dumps(obj,ensure_ascii=False,indent=2),encoding='utf-8');q.replace(p)
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def emit(obj):print(json.dumps(obj,ensure_ascii=False),flush=True)
def get(path):
 r=requests.get(BASE+path,timeout=45);r.raise_for_status();return r.json()
def key_green(im):
 a=np.asarray(im.convert('RGB'),dtype=np.float32)/255
 excess=a[:,:,1]-np.maximum(a[:,:,0],a[:,:,2])
 green_ratio=excess/np.maximum(a[:,:,1],1/255)
 matte=1-np.clip((green_ratio-.12)/.76,0,1)
 # Preserve enclosed green costume details instead of keying every green pixel globally.
 labels,count=ndimage.label(green_ratio>.15)
 exterior_ids=np.unique(np.concatenate([labels[0],labels[-1],labels[:,0],labels[:,-1]]));lookup=np.zeros(count+1,dtype=bool);lookup[exterior_ids]=True;lookup[0]=False
 exterior=lookup[labels]
 # Compression shifts pure screen green slightly. Near-key holes enclosed by
 # bending hair/limbs remain background while ordinary costume greens survive.
 near_key=(green_ratio>.80)&(a[:,:,1]>.45)
 alpha=np.where(exterior|near_key,matte,1.)
 # Flat green is keyed; neutral/colored foreground remains, with green spill removed.
 rgb=a.copy();rgb[:,:,1]=np.minimum(rgb[:,:,1],np.maximum(rgb[:,:,0],rgb[:,:,2])+.12)
 rgb=np.where((alpha>.98)[:,:,None],a,rgb)
 rgba=np.concatenate([rgb,alpha[:,:,None]],axis=-1);rgba[alpha<.01]=0
 return Image.fromarray(np.uint8(np.clip(rgba*255,0,255)),'RGBA')
def preserve_reviewed_dark_props(original,keyed):
 # Opt-in import matte for visually verified dark physical props that inherit
 # green spill. Only source pixels connected to existing opaque foreground are
 # eligible; an exterior dark-green screen must never become a foreground.
 raw=np.asarray(original.convert('RGB'));rgba=np.asarray(keyed).copy();brightness=raw.max(axis=2).astype(float)/255
 candidates=(brightness<.42)|(rgba[:,:,3]>=192)
 labels,count=ndimage.label(candidates,structure=np.ones((3,3)))
 seeded=np.unique(labels[rgba[:,:,3]>=192]);border=np.unique(np.concatenate([labels[0],labels[-1],labels[:,0],labels[:,-1]]))
 kept=np.setdiff1d(seeded,border);eligible=np.isin(labels,kept)&(brightness<.42)
 recovered=np.uint8(np.clip((.42-brightness)/.07,0,1)*255);raised=eligible&(recovered>rgba[:,:,3])
 rgba[raised,3]=recovered[raised];opaque=raised&(rgba[:,:,3]>=250);rgba[opaque,:3]=raw[opaque]
 return Image.fromarray(rgba,'RGBA')
def prepare(character):
 sheet=PROJECT/'Content/UI/v6/character-sheets'/f'{character}-directions.png'
 if not sheet.exists():return
 im=Image.open(sheet).convert('RGB');w,h=im.size
 for index,direction in enumerate(DIRS):
  cell=im.crop((round(index%4*w/4),round(index//4*h/2),round((index%4+1)*w/4),round((index//4+1)*h/2)))
  rgba=key_green(cell);box=rgba.getbbox()
  if not box:raise RuntimeError('Empty direction '+character+direction)
  subject=rgba.crop(box);scale=min(244/subject.height,244/subject.width)
  subject=subject.resize((round(subject.width*scale),round(subject.height*scale)),Image.Resampling.LANCZOS)
  canvas=Image.new('RGB',(384,384),(0,255,0));canvas.paste(subject,(192-subject.width//2,338-subject.height),subject)
  ref=HERE/'first-frames'/f'{character}-{direction}.png';ref.parent.mkdir(parents=True,exist_ok=True);canvas.save(ref)
  job_id=f'{character}-{direction}-{ACTION_VERSION}'
  prompt=(f'Animate ONLY the exact {character} character from the supplied reference. Single character game sprite, fixed elevated orthographic camera, exact same size and screen position. '
   'Keep her face hairstyle costume ornaments and colors unchanged. Maintain the exact initial body-facing direction for every action; NEVER rotate toward the camera. Stay centered with generous empty margins. The character never grows, moves sideways across the frame or approaches its edges, including when falling. '
   'The background remains perfectly flat solid pure green RGB 0,255,0 throughout with no scene or floor. No camera movement, zoom, cuts, captions or other characters. '
   'Perform this sequence clearly in order over exactly nine seconds: '
   '0-1 second calm breathing idle; 1-3 seconds walk in place taking four distinct alternating steps, no screen translation; '
   '3-4.33 seconds one quick forward basic attack hand thrust then recover; '
   '4.33-6.33 seconds a larger two-handed spell-casting gesture raising both hands then releasing forward, no colored effects obscuring the body; '
   '6.33-7.33 seconds visibly recoil from a hit then recover; 7.33-9 seconds collapse gently to one knee and then lie down unconscious, staying down. '
   'No blood or gore. Keep the entire silhouette including hair, wings, props and fallen body inside the frame. Smooth leg and arm motion. All stages must be visibly different.')
  save(HERE/'jobs'/job_id/'request.json',{'id':job_id,'category':'character','character':character,'direction':direction,
   'firstFrame':str(ref.relative_to(PROJECT)).replace('\\','/'),'sourceSheet':str(sheet.relative_to(PROJECT)).replace('\\','/'),
   'sourceSheetSha256':sha(sheet),'prompt':prompt,'frames':216,'width':384,'height':384,'fps':24,'segments':SEGMENTS})
 emit({'prepared':character,'directions':8})
def history(task):return get('/history/'+task).get(task)
def in_queue(task):
 q=get('/queue');return any(len(r)>1 and r[1]==task for k in ['queue_running','queue_pending'] for r in q.get(k,[]))
def submit(folder):
 spec=read(folder/'request.json');id=spec['id'];receipt=folder/'attempt.json'
 if receipt.exists():return read(receipt)['promptId']
 task=str(uuid.uuid5(uuid.NAMESPACE_URL,'code-sentinels-v6/h3-v1/'+id))
 if history(task) or in_queue(task):raise RuntimeError('Unrecorded existing task requires reconciliation '+task)
 graph=read(ROOT/'crates/gend/src/media/comfyui_h3_v1.json')
 graph['6']['inputs'].update({'prompt':spec['prompt'],'width':spec['width'],'height':spec['height'],'length':spec['frames']})
 graph['10']['inputs']['noise_seed']=int(hashlib.sha256(id.encode()).hexdigest()[:14],16)
 graph['15']['inputs']['filename_prefix']='code-sentinels-v6/'+id+'/'+task
 ref=PROJECT/spec['firstFrame'];raw=ref.read_bytes();name=id+'-'+sha(ref)[:12]+'.png'
 shutil.copy2(ref,folder/'first-frame.png')
 r=requests.post(BASE+'/upload/image',data={'type':'input','subfolder':'code-sentinels-v6','overwrite':'false'},files={'image':(name,raw,'image/png')},timeout=60);r.raise_for_status();uploaded=r.json()
 graph['20']={'class_type':'LoadImage','inputs':{'image':'/'.join(x for x in [uploaded.get('subfolder',''),uploaded['name']] if x)}}
 graph['6']['inputs']['first_frame']=['20',0]
 if spec.get('lastFrame')==spec['firstFrame']:graph['6']['inputs']['last_frame']=['20',0]
 save(folder/'workflow.json',graph)
 with receipt.open('x',encoding='utf-8') as f:json.dump({'id':id,'promptId':task,'provider':'comfyui-minimax-h3','mode':'image2video','cloudPaidCreates':0,'createCount':1,'startedAt':stamp(),'workflowSha256':sha(folder/'workflow.json'),'firstFrameSha256':sha(ref)},f,indent=2)
 r=requests.post(BASE+'/prompt',json={'prompt':graph,'prompt_id':task,'client_id':'code-sentinels-v6-production','front':bool(spec.get('priorityFront'))},timeout=90)
 created=r.json();save(folder/'created.json',created)
 if r.status_code!=200 or created.get('prompt_id')!=task:raise RuntimeError('Submission rejected, receipt retained '+id)
 emit({'submitted':id,'promptId':task});return task
def collect(folder):
 if (folder/'video.json').exists():return True
 if (folder/'cancelled.json').exists():return False
 if (folder/'execution-failure.json').exists() and read(folder/'execution-failure.json').get('replacement'):return False
 if not (folder/'attempt.json').exists():return False
 task=read(folder/'attempt.json')['promptId'];h=history(task)
 if not h:return False
 save(folder/'history.json',h)
 if h.get('status',{}).get('status_str')!='success':
  save(folder/'failure.json',{'at':stamp(),'promptId':task,'status':h.get('status')});raise RuntimeError('Local generation failed '+folder.name)
 videos=[]
 for output in h.get('outputs',{}).values():
  for vals in output.values():
   if isinstance(vals,list):videos.extend(v for v in vals if isinstance(v,dict) and v.get('filename','').endswith('.mp4'))
 if len(videos)!=1:raise RuntimeError('Expected one actual MP4 '+folder.name)
 video=videos[0];r=requests.get(BASE+'/view',params=video,timeout=180);r.raise_for_status();raw=r.content
 if len(raw)<12 or raw[4:8]!=b'ftyp':raise RuntimeError('Invalid MP4')
 target=PROJECT/'SourceMedia/v6'/f'{folder.name}.mp4';target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(raw)
 save(folder/'video.json',{'id':folder.name,'fileRef':str(target.relative_to(PROJECT)).replace('\\','/'),'sha256':sha(target),'bytes':len(raw),'provenance':{'method':'image-to-video-extracted-frames','provider':'comfyui-minimax-h3','promptId':task,'cloudPaidCreates':0,'model':'MiniMax-H3-Base INT8 Turbo8','completedAt':stamp()}})
 emit({'video':folder.name,'bytes':len(raw)});return True
def extract(folder,force=False):
 if (folder/'extracted.json').exists() and not force:return
 spec=read(folder/'request.json');receipt=read(folder/'video.json');video=PROJECT/receipt['fileRef']
 if sha(video)!=receipt['sha256']:raise RuntimeError('Video SHA mismatch')
 reader=imageio_ffmpeg.read_frames(str(video),pix_fmt='rgb24');info=next(reader);w,h=info['size']
 frames=[Image.frombytes('RGB',(w,h),raw) for raw in reader]
 if len(frames)<spec['frames']-1:raise RuntimeError('Video shorter than requested')
 directory=folder/'frames';directory.mkdir(exist_ok=True);clip_info={};raw_unique=[];n=0
 native_directory=folder/'frames-native'
 if spec.get('category')=='character':native_directory.mkdir(exist_ok=True)
 segments=spec.get('segments',SEGMENTS)
 review_path=folder/'source-review.json'
 review=read(review_path) if review_path.exists() else {}
 segments=review.get('segments',segments)
 selected_indices={}
 for action,(start,end,count,loop) in segments.items():
  clip_info[action]={'start':n,'endExclusive':n+count,'fps':count/((end-start)/24),'loop':loop}
  indices=np.linspace(start,min(end-1,len(frames)-1),count).round().astype(int)
  selected_indices[action]=[int(i) for i in indices]
  for local_index,source_index in enumerate(indices):
   if spec.get('chromaKey')=='black':
    rgb=np.asarray(frames[source_index],dtype=np.float32)/255;alpha=rgb.max(axis=-1);alpha=np.where(alpha<3/255,0,alpha)
    color=np.clip(rgb/np.maximum(alpha[:,:,None],1/255),0,1);rgba=np.concatenate([color,alpha[:,:,None]],axis=-1);rgba[alpha==0]=0
    frame=Image.fromarray(np.uint8(rgba*255),'RGBA')
   else:frame=key_green(frames[source_index])
   if review.get('darkForegroundMatte')=='reviewed-connected-low-luminance-props':frame=preserve_reviewed_dark_props(frames[source_index],frame)
   if review.get('garbageMatte')=='primary-component-bounds':
    rgba=np.asarray(frame).copy();labels,component_count=ndimage.label(rgba[:,:,3]>150);sizes=np.bincount(labels.reshape(-1));sizes[0]=0
    if component_count:
     fw,fh=frame.size;margin=math.ceil(6*fw/256);taper=2*fw/256
     ys,xs=np.where(labels==int(sizes.argmax()));left=max(0,int(xs.min())-margin);right=min(fw,int(xs.max())+margin+1);top=max(0,int(ys.min())-margin);bottom=min(fh,int(ys.max())+margin+1)
     yy,xx=np.mgrid[:fh,:fw];distance=np.minimum.reduce([xx-left,yy-top,right-1-xx,bottom-1-yy]).astype(float);mask=np.clip(distance/taper,0,1)
     rgba[:,:,3]=np.uint8(rgba[:,:,3].astype(float)*mask);rgba[rgba[:,:,3]==0]=0;frame=Image.fromarray(rgba,'RGBA')
   if spec.get('category')=='character' and review.get('peripheralEdgeFadePx'):
    fade=float(review['peripheralEdgeFadePx'])
    if not 0<fade<=16 or not review.get('bodyClearOfEdgeFade'):raise RuntimeError('Peripheral fade requires an explicit bounded, body-safe visual review')
    rgba=np.asarray(frame).copy();fw,fh=frame.size;yy,xx=np.mgrid[:fh,:fw];distance=np.minimum.reduce([xx,yy,fw-1-xx,fh-1-yy]).astype(float);a=np.clip(distance/fade,0,1);a=a*a*(3-2*a);rgba[:,:,3]=np.uint8(rgba[:,:,3].astype(float)*a);rgba[rgba[:,:,3]==0]=0;frame=Image.fromarray(rgba,'RGBA')
   if spec.get('category')=='character':frame.save(native_directory/f'{n:03}.png')
   frame=frame.resize((256,256),Image.Resampling.LANCZOS)
   if spec.get('category')=='effect':
    rgba=np.asarray(frame).copy();yy,xx=np.mgrid[:256,:256];distance=np.minimum.reduce([xx,yy,255-xx,255-yy]).astype(float)
    edge=np.clip(distance/8,0,1);edge=edge*edge*(3-2*edge)
    exit_alpha=min(1.,max(0.,(count-1-local_index)/7));entry_alpha=min(1.,local_index/2)
    rgba[:,:,3]=np.uint8(rgba[:,:,3].astype(float)*edge*exit_alpha*entry_alpha);rgba[rgba[:,:,3]==0]=0
    frame=Image.fromarray(rgba,'RGBA')
   frame.save(directory/f'{n:03}.png');raw_unique.append(hashlib.sha256(frame.tobytes()).hexdigest());n+=1
 contact=Image.new('RGB',(8*192,math.ceil(24/8)*212),(30,34,40));draw=ImageDraw.Draw(contact)
 for i,source_index in enumerate(np.linspace(0,len(frames)-1,24).round().astype(int)):
  contact.paste(frames[source_index].resize((192,192)),((i%8)*192,(i//8)*212));draw.text(((i%8)*192+3,(i//8)*212+194),f'{source_index/24:.2f}s',fill='white')
 contact.save(folder/'review-contact.jpg',quality=90)
 save(folder/'extracted.json',{'id':spec['id'],'actualSourceFrames':len(frames),'actualSourceSize':[w,h],'sourceFps':info['fps'],'frames':n,'uniqueFrames':len(set(raw_unique)),'clips':clip_info,'sourceFrameIndices':selected_indices,'review':'approved' if review.get('approved') else 'pending-visual-review','sourceReviewSha256':sha(review_path) if review_path.exists() else None,'videoSha256':sha(video),'nativeFrames':{'directory':'frames-native','frameSize':[w,h],'scaleFromSource':1,'resampledBeforeStorage':False} if spec.get('category')=='character' else None,'previewFrames':{'directory':'frames','frameSize':[256,256],'purpose':'review previews only; final character packing reads nativeFrames'},'matteProcessing':{'edgeFadePx':8,'entryFadeFrames':3,'exitFadeFrames':8,'rgbMotionReplaced':False} if spec.get('category')=='effect' else {'method':'connected chromaticity background matte plus compressed near-key holes; preserves enclosed non-key costume colors','version':4,'garbageMatte':review.get('garbageMatte'),'darkForegroundMatte':review.get('darkForegroundMatte'),'peripheralEdgeFadePx':review.get('peripheralEdgeFadePx',0),'rgbMotionReplaced':False}})
 emit({'extracted':folder.name,'unique':len(set(raw_unique))})
def resolve_source(character,direction,action,overrides):
 folder=HERE/'jobs'/(f'{character}-{direction}-{action}-v1' if action in ['attack','cast'] else f'{character}-{direction}-{ACTION_VERSION}')
 correction=HERE/'jobs'/f'{character}-{direction}-{action}-v1'
 if action not in ['attack','cast'] and (correction/'request.json').exists():folder=correction
 if f'{character}/{direction}/{action}' in overrides:folder=HERE/'jobs'/overrides[f'{character}/{direction}/{action}']
 return folder
def pack_legacy_unused(character):
 overrides=read(HERE/'action-overrides.json') if (HERE/'action-overrides.json').exists() else {}
 anchors=read(HERE/'reviews/reference-anchors.json')[character]
 sources=[]
 for direction in DIRS:
  for action in SEGMENTS:
   folder=resolve_source(character,direction,action,overrides)
   if not (folder/'extracted.json').exists() or not (folder/'source-review.json').exists():return
   if not read(folder/'source-review.json').get('approved'):return
   meta=read(folder/'extracted.json')
   if meta.get('review')!='approved' or meta.get('sourceReviewSha256')!=sha(folder/'source-review.json'):return
   if meta.get('matteProcessing',{}).get('version',0)<4:return
   clip=meta['clips'][action]
   desired={'idle':6,'walk':12,'attack':12,'cast':16,'hit':8,'death':10}[action]
   sources.append((direction,action,folder,clip,desired))
 count=sum(s[4] for s in sources);cols=16;atlas=Image.new('RGBA',(cols*256,math.ceil(count/cols)*256));boxes=[];clips={a:{} for a in SEGMENTS};offset=0;provenance=[]
 for direction,action,folder,clip,count_action in sources:
  if not any(p['id']==folder.name for p in provenance):provenance.append(read(folder/'video.json'))
  selected=np.linspace(clip['start'],clip['endExclusive']-1,count_action).round().astype(int)
  source_meta=read(folder/'extracted.json');source_indices=source_meta['sourceFrameIndices'][action];spec=read(folder/'request.json')
  source_size=source_meta['actualSourceSize'];foot=spec.get('sourceFootAnchor',anchors[direction]['foot'])
  pivot=spec.get('outputPivot',[.5,.88]);target=[v*256 for v in pivot]
  source_foot=[foot[i]*256/source_size[i] for i in range(2)];shift=[target[i]-source_foot[i] for i in range(2)]
  union=[256.,256.,0.,0.]
  for j in selected:
   b=Image.open(folder/'frames'/f'{j:03}.png').getbbox()
   if b:union=[min(union[0],b[0]+shift[0]),min(union[1],b[1]+shift[1]),max(union[2],b[2]+shift[0]),max(union[3],b[3]+shift[1])]
  # Fit real motion around its ground anchor; inverse plane-span compensation keeps
  # actor size fixed in world units even for the larger centered fall capture.
  fit=min(1.,(target[0]-2)/max(1,target[0]-union[0]),(254-target[0])/max(1,union[2]-target[0]),(target[1]-2)/max(1,target[1]-union[1]),(254-target[1])/max(1,union[3]-target[1]))
  span=spec.get('outputPlaneSpan',2.5456)/fit
  for i,j in enumerate(selected):
   im=Image.open(folder/'frames'/f'{j:03}.png').convert('RGBA');im=im.resize((round(256*fit),round(256*fit)),Image.Resampling.LANCZOS)
   tile=Image.new('RGBA',(256,256));tile.paste(im,tuple(round(target[k]-(target[k]-shift[k])*fit) for k in range(2)))
   x=(offset+i)%cols*256;y=(offset+i)//cols*256;atlas.paste(tile,(x,y));boxes.append([x,y,256,256])
  clips[action][direction]={'start':offset,'endExclusive':offset+count_action,'fps':{'idle':6,'walk':12,'attack':16,'cast':12,'hit':20,'death':12}[action],'loop':action in ['idle','walk'],'sourceJob':folder.name,'sourceFrameIndices':[source_indices[int(j)-clip['start']] for j in selected],'pivot':pivot,'nativePlaneSpan':span,'bakeFitScale':fit,'sourceFootAnchor':foot,'sourceSize':source_size,'referenceAnchorShift':shift}
  offset+=count_action
 ANIM.mkdir(parents=True,exist_ok=True);PUBLIC.mkdir(parents=True,exist_ok=True)
 path=ANIM/f'{character}.png';temporary=ANIM/f'{character}.pending.png';atlas.save(temporary);temporary.replace(path)
 metadata={'schemaVersion':2,'id':character,'image':character+'.png','width':atlas.width,'height':atlas.height,'frameCount':count,'frameSize':[256,256],'pivot':[.5,.88],'directions':DIRS,'boxes':boxes,'clips':clips,'provenance':provenance,'visualReview':'approved'}
 metadata['directionConvention']=DIRECTION_CONVENTION
 metadata['nativePlaneSpan']=2.5456;metadata['spatialContract']='per-clip-ground-anchor-v1';metadata['anchorMeaning']='reference foot is visually checked; each clip has one constant pivot/fit with inverse plane-span compensation, preserving world scale and ground position without synthesizing pose pixels'
 save(ANIM/f'{character}.json',metadata);shutil.copy2(path,PUBLIC/path.name);shutil.copy2(ANIM/f'{character}.json',PUBLIC/f'{character}.json')
 manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json');manifest['characters'][character].update({'ready':True,'nativeAtlas':f'Content/Animations/v6/characters/{character}.png','nativeMetadata':f'Content/Animations/v6/characters/{character}.json','frameCount':count,'atlasSha256':sha(path)})
 manifest['characterAnimationState']='ready' if all(c.get('ready') for c in manifest['characters'].values()) else 'production-and-visual-review'
 save(PROJECT/'Content/UI/v6/resource-manifest.json',manifest);save(ROOT/'packages/client/public/games/code-sentinels/ui-v6/resource-manifest.json',manifest)
 emit({'packed':character,'frames':count,'size':atlas.size})
def pack(character):
 import density_pack,finalize_character,importlib
 importlib.reload(density_pack);importlib.reload(finalize_character)
 return finalize_character.finalize(character)
def status():
 jobs=[p for p in sorted((HERE/'jobs').glob('*/request.json')) if p.parent.name.endswith(ACTION_VERSION) or read(p).get('action') or read(p).get('category')=='effect'];q=get('/queue')
 result={'at':stamp(),'jobs':len(jobs),'submitted':sum((p.parent/'attempt.json').exists() for p in jobs),'videos':sum((p.parent/'video.json').exists() for p in jobs),'extracted':sum((p.parent/'extracted.json').exists() for p in jobs),'cancelledBeforeExecution':sum((p.parent/'cancelled.json').exists() for p in jobs),'running':len(q['queue_running']),'pending':len(q['queue_pending']),'cloudPaidCreates':0}
 result['activeSubmitted']=result['submitted']-result['cancelledBeforeExecution'];result['remainingExecutionJobs']=result['running']+result['pending']
 result['failedExecutionAttempts']=sum((p.parent/'execution-failure.json').exists() for p in jobs)
 save(HERE/'status.json',result);emit(result)
def watch():
 while True:
  jobs=sorted((HERE/'jobs').glob('*/request.json'))
  for path in jobs:
   folder=path.parent
   if collect(folder):extract(folder)
  for char in CHARACTERS:
   if not (ANIM/f'{char}.json').exists():pack(char)
  status();time.sleep(20)
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('command',choices=['prepare','submit','collect','watch','status']);p.add_argument('--character');p.add_argument('--limit',type=int,default=0);p.add_argument('--reextract',action='store_true');args=p.parse_args()
 if args.command=='prepare':
  for c in ([args.character] if args.character else CHARACTERS):prepare(c)
 elif args.command=='submit':
  paths=sorted((HERE/'jobs').glob('*/request.json'));paths=[x for x in paths if x.parent.name.endswith(ACTION_VERSION) and (not args.character or read(x).get('character')==args.character)]
  for path in paths[:args.limit or len(paths)]:submit(path.parent)
 elif args.command=='watch':watch()
 elif args.command=='collect':
  for path in (HERE/'jobs').glob('*/request.json'):
   if collect(path.parent):extract(path.parent,force=args.reextract)
  for c in CHARACTERS:pack(c)
 else:status()
