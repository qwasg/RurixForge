"""Actual local MiniMax H3 sampling with durable, query-only resumable prompt IDs."""
import hashlib
import json
import re
import shutil
import sys
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
ROOT=PROJECT.parents[1]
sys.path.insert(0,str(HERE.parent/'python-libs'))
import requests

BACKEND='comfyui-minimax-h3'
VERSION='20260909-local-h3-v1'
SOURCE_SIZE=384
SOURCE_FRAMES=124
PAUSE=HERE/'local-provider-paused.json'
MODEL_FILES={
 'UNETLoader':('unet_name','minimax_h3_fl2va_pruned_int8_convrot.safetensors'),
 'CLIPLoader':('clip_name','qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors'),
 'LoraLoaderModelOnly':('lora_name','minimax_h3_fl2v_turbo_8step_v1.0_comfyui_bf16.safetensors'),
}
def stamp():return datetime.now(timezone.utc).isoformat()
def read(path):return json.loads(path.read_text(encoding='utf-8-sig'))
def save(path,value):
 path.parent.mkdir(parents=True,exist_ok=True)
 tmp=path.with_suffix(path.suffix+'.tmp');tmp.write_text(json.dumps(value,ensure_ascii=False,indent=2),encoding='utf-8');tmp.replace(path)
def emit(value):print(json.dumps(value,ensure_ascii=True),flush=True)
def base_url():
 cfg=read(ROOT/'data/gen-backends.json')
 backend=next((b for b in cfg['backends'] if b['id']==BACKEND and b.get('enabled')),None)
 if not backend:raise RuntimeError('The explicitly selected local H3 backend is not enabled')
 endpoint=backend['endpoint'].rstrip('/')
 parsed=urlparse(endpoint)
 if parsed.scheme not in {'http','https'} or parsed.hostname not in {'localhost','127.0.0.1','::1'} or parsed.username or parsed.password:
  raise RuntimeError('Local H3 only accepts the configured loopback endpoint')
 return endpoint
def get(base,path):
 r=requests.get(base+path,timeout=45);r.raise_for_status();return r.json()
def preflight(base):
 info=get(base,'/object_info')
 for node,(field,name) in MODEL_FILES.items():
  if name not in info[node]['input']['required'][field][0]:raise RuntimeError('Required local H3 model file unavailable: '+name)
 for name in ['minimax_h3_video_vae_fp16.safetensors','minimax_h3_audio_vae_fp32.safetensors']:
  if name not in info['VAELoader']['input']['required']['vae_name'][0]:raise RuntimeError('Required local H3 VAE unavailable: '+name)
 for node in ['MiniMaxH3ImageToVideo','SamplerCustomAdvanced','SaveVideo','LoadImage']:
  if node not in info:raise RuntimeError('Required native local node unavailable: '+node)
 stats=get(base,'/system_stats')
 return {'at':stamp(),'system':stats['system'],'devices':stats['devices'],'modelFiles':MODEL_FILES,
         'sourceResolution':[SOURCE_SIZE,SOURCE_SIZE],'sourceFrames':SOURCE_FRAMES,'sourceFps':24,
         'mode':'native first/last-frame image2video','remoteAPIUsed':False,'concurrency':1}
def upload(base,path,clip,kind,dest):
 path=path.resolve(strict=True)
 if not path.is_relative_to(PROJECT):raise RuntimeError('Reference is outside project')
 raw=path.read_bytes();digest=hashlib.sha256(raw).hexdigest()
 name=clip+'-'+kind+'-'+digest[:16]+'.png'
 r=requests.post(base+'/upload/image',data={'type':'input','subfolder':'code-sentinels-v5','overwrite':'false'},
                 files={'image':(name,raw,'image/png')},timeout=90)
 r.raise_for_status();uploaded=r.json()
 check=requests.get(base+'/view',params={'filename':uploaded['name'],'subfolder':uploaded.get('subfolder',''),'type':uploaded.get('type','input')},timeout=60)
 if check.status_code!=200 or hashlib.sha256(check.content).hexdigest()!=digest:raise RuntimeError('Local uploaded reference bytes do not match source')
 save(dest/(kind+'.local-upload.json'),{'source':str(path.relative_to(PROJECT)),'sha256':digest,'bytes':len(raw),'uploaded':uploaded,'verifiedSameBytes':True})
 return '/'.join(p for p in [uploaded.get('subfolder',''),uploaded['name']] if p)
def build_graph(base,clip,spec,dest,prompt_id):
 graph=read(ROOT/'crates/gend/src/media/comfyui_h3_v1.json')
 graph['6']['inputs'].update({'prompt':spec['prompt']+' Complete the specified action within four seconds; hold the final state for the remaining second. The supplied first frame fixes the exact original camera, geometry, colors and position. Never pan or zoom. No dialogue or music.',
                            'width':SOURCE_SIZE,'height':SOURCE_SIZE,'length':SOURCE_FRAMES})
 graph['10']['inputs']['noise_seed']=int(hashlib.sha256((VERSION+clip).encode()).hexdigest()[:15],16)
 graph['15']['inputs']['filename_prefix']='code-sentinels-v5/'+clip+'/'+prompt_id
 for index,ref in enumerate(spec['references']):
  if ref['type'] not in {'first_frame','last_frame'}:raise RuntimeError('Unsupported reference role')
  uploaded=upload(base,PROJECT/ref['path'],clip,ref['type'],dest)
  node=str(20+index);graph[node]={'class_type':'LoadImage','inputs':{'image':uploaded},'_meta':{'title':clip+' '+ref['type']}}
  graph['6']['inputs'][ref['type']]=[node,0]
 if 'first_frame' not in graph['6']['inputs']:raise RuntimeError('Real I2V requires an actual first-frame connection')
 save(dest/'workflow.json',graph)
 return graph
def recovery_folder(folder):
 dest=folder/'recoveries'/VERSION;dest.mkdir(parents=True,exist_ok=True)
 if (dest/'recovery.json').exists():return dest
 old=folder/'attempt.json'
 prior=None
 if old.exists():
  created=read(folder/'created.json') if (folder/'created.json').exists() else {}
  task=created.get('output',{}).get('task_id')
  old_status=read(folder/'status.json') if (folder/'status.json').exists() else {}
  if not task or old_status.get('output',{}).get('task_status')=='SUCCEEDED':
   raise RuntimeError('Prior attempt is not conclusively failed; reconcile it before local recovery')
  report=read(HERE/'recharge-reconciliation-20260908T115926Z.json')
  prior=next((r for r in report['tasks'] if r.get('taskId')==task and r.get('http')==200 and r.get('status')=='FAILED'),None)
  if not prior:raise RuntimeError('Prior task lacks conclusive task-ID GET evidence')
  # The previous read-only reconciliation covered every task. One legacy
  # worker ended before writing its individual status.json, so the later
  # successful HTTP GET report is the authoritative terminal status for it.
  save(dest/'legacy-reconciliation.json',{'task':prior,'at':report['at'] if 'at' in report else None,
       'source':'pipeline/v5/recharge-reconciliation-20260908T115926Z.json','individualStatusPresent':bool(old_status)})
  archive=dest/'legacy-receipts';archive.mkdir(exist_ok=True)
  for name in ['attempt.json','created.json','status.json','failure.json']:
   src=folder/name
   if src.exists() and not (archive/name).exists():shutil.copy2(src,archive/name)
 # Any attempt by another recovery provider must also have a known terminal
 # outcome. The earlier StarFrame run stopped during upload and has no attempt.
 for attempt in folder.glob('recoveries/*/attempt.json'):
  if attempt.parent==dest:continue
  raise RuntimeError('Another provider recovery attempt exists; reconcile before creating local work')
 save(dest/'recovery.json',{'at':stamp(),'authorization':'User explicitly requested local MiniMax H3 to complete remaining V5 materials and tests',
      'priorTaskReconciliation':prior,'provider':BACKEND,'execution':'local GPU, no cloud create','originalReceiptsPreserved':True})
 return dest
def find_history(base,prompt_id):return get(base,'/history/'+prompt_id).get(prompt_id)
def queued(base,prompt_id):
 q=get(base,'/queue');return any(len(row)>1 and row[1]==prompt_id for key in ['queue_running','queue_pending'] for row in q.get(key,[]))
def produce(clip):
 if not re.fullmatch('[a-z0-9-]+',clip):raise RuntimeError('Invalid clip identifier')
 folder=HERE/'jobs'/clip;spec=read(folder/'request.json');receipt=folder/'video.json'
 if receipt.exists() and (PROJECT/read(receipt)['fileRef']).is_file():
  emit({'id':clip,'status':'already-downloaded'});return
 base=base_url();dest=recovery_folder(folder);attempt=dest/'attempt.json';created_path=dest/'created.json'
 if attempt.exists():
  prompt_id=read(attempt)['promptId']
  history=find_history(base,prompt_id)
  if not history and not queued(base,prompt_id):raise RuntimeError('Recorded local prompt is absent from history/queue; unknown outcome, never resubmit')
 else:
  if PAUSE.exists():raise RuntimeError('Local provider paused after a recorded failure; no new prompt submitted')
  save(dest/'preflight.json',preflight(base))
  prompt_id=str(uuid.uuid5(uuid.NAMESPACE_URL,'code-sentinels-v5/'+VERSION+'/'+clip))
  if find_history(base,prompt_id) or queued(base,prompt_id):raise RuntimeError('Pre-existing local prompt ID requires reconciliation')
  graph=build_graph(base,clip,spec,dest,prompt_id)
  with attempt.open('x',encoding='utf-8') as f:
   json.dump({'id':clip,'startedAt':stamp(),'provider':BACKEND,'model':'MiniMax-H3-Base FL2VA INT8 + Turbo8',
      'mode':'image2video','executor':'project Codex CLI agent','createCount':1,'cloudPaidCreates':0,'promptId':prompt_id,
      'workflowSha256':hashlib.sha256((dest/'workflow.json').read_bytes()).hexdigest(),'sourceFrames':SOURCE_FRAMES,'sourceResolution':[SOURCE_SIZE,SOURCE_SIZE]},f,indent=2)
  emit({'id':clip,'status':'submitting-one-local-h3-prompt','taskId':prompt_id,'sourceFrames':SOURCE_FRAMES})
  try:
   r=requests.post(base+'/prompt',json={'prompt':graph,'prompt_id':prompt_id,'client_id':'code-sentinels-v5-local-production',
       'extra_data':{'production_clip':clip,'receiptVersion':VERSION}},timeout=120)
   created=r.json();save(created_path,created)
  except (requests.RequestException,ValueError):
   save(dest/'failure.json',{'at':stamp(),'phase':'submit','outcome':'unknown','promptId':prompt_id})
   save(PAUSE,{'at':stamp(),'clip':clip,'promptId':prompt_id,'reason':'unknown submission outcome'})
   raise RuntimeError('Local prompt submission outcome unknown; resume only through saved prompt ID')
  if r.status_code!=200 or created.get('prompt_id')!=prompt_id:
   save(dest/'failure.json',{'at':stamp(),'http':r.status_code,'phase':'submit','response':created})
   save(PAUSE,{'at':stamp(),'clip':clip,'promptId':prompt_id,'reason':'graph submission rejected'})
   raise RuntimeError('Local graph rejected; receipt preserved for review')
 emit({'id':clip,'status':'polling-existing-local-prompt','taskId':prompt_id})
 deadline=time.monotonic()+7200
 while time.monotonic()<deadline:
  history=find_history(base,prompt_id)
  if history:
   save(dest/'history.json',history)
   status=history.get('status',{})
   save(dest/'status.json',{'at':stamp(),'taskId':prompt_id,'status':status})
   if status.get('status_str')!='success':
    save(dest/'failure.json',{'at':stamp(),'taskId':prompt_id,'status':status})
    save(PAUSE,{'at':stamp(),'clip':clip,'promptId':prompt_id,'reason':'local execution failed'})
    raise RuntimeError('Local H3 execution failed; recorded prompt retained')
   videos=[]
   for output in history.get('outputs',{}).values():
    for values in output.values():
     if isinstance(values,list):videos.extend(v for v in values if isinstance(v,dict) and v.get('filename','').endswith('.mp4'))
   if len(videos)!=1:raise RuntimeError('Local completed graph does not contain exactly one MP4')
   video=videos[0]
   r=requests.get(base+'/view',params={'filename':video['filename'],'subfolder':video.get('subfolder',''),'type':video.get('type','output')},timeout=180)
   raw=r.content
   if r.status_code!=200 or len(raw)<12 or raw[4:8]!=b'ftyp':raise RuntimeError('Completed local video download was not MP4')
   target=PROJECT/'SourceMedia'/('buildings-v5' if spec['category']=='building' else 'effects-v5')/(clip+'.mp4')
   target.parent.mkdir(parents=True,exist_ok=True);temporary=target.with_suffix('.mp4.download');temporary.write_bytes(raw);temporary.replace(target)
   save(receipt,{'id':clip,'backendId':BACKEND,'fileRef':str(target.relative_to(PROJECT)).replace('\\','/'),'ext':'mp4','mime':'video/mp4',
      'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'meta':{'provider':BACKEND,'model':'MiniMax-H3-Base FL2VA INT8 + Turbo8',
        'taskId':prompt_id,'mode':'image2video','sourceResolution':[SOURCE_SIZE,SOURCE_SIZE],'sourceFrames':SOURCE_FRAMES,'sourceFps':24,
        'durationSec':SOURCE_FRAMES/24,'completedAt':stamp(),'localOutput':video,'receipts':str(dest.relative_to(PROJECT)).replace('\\','/'),
        'workflowSha256':hashlib.sha256((dest/'workflow.json').read_bytes()).hexdigest(),'remoteAPIUsed':False}})
   emit({'id':clip,'status':'downloaded','provider':BACKEND,'taskId':prompt_id,'bytes':len(raw)});return
  time.sleep(10)
 raise RuntimeError('Local prompt still pending after polling window; resume its saved ID, never recreate')
