"""StarFrame's documented real I2V API, exclusive paid attempts and durable recovery.

Primary protocol: https://docs.xzapi.vip/index.html
Current H3 constraints and upload protocol: https://web.xzapi.vip (public client).
The configured workbench URL resolves to its documented API, never to an arbitrary host.
"""
import hashlib
import json
import re
import shutil
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[1]
ROOT = PROJECT.parents[1]
sys.path.insert(0, str(HERE.parent))
from aliyun_upload import credentials
import requests
from PIL import Image

API = 'https://api.xzapi.vip'
WEB = 'https://web.xzapi.vip'
MODEL = 'ch1007-minimax-h3-2k'
RECOVERY = '20260909-starframe-v1'
PAUSE = HERE / 'starframe-provider-paused.json'

def stamp(): return datetime.now(timezone.utc).isoformat()
def read(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix+'.tmp')
    temp.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf-8')
    temp.replace(path)
def emit(value): print(json.dumps(value, ensure_ascii=True), flush=True)
def clean(value, key=''):
    if isinstance(value, dict):
        return {k: clean(v, key) for k,v in value.items() if k.lower() not in {'url','video_url','upload_url','file_url','authorization','token','api_key','key'}}
    if isinstance(value, list): return [clean(v,key) for v in value]
    if isinstance(value, str):
        value = value.replace(key,'[redacted]') if key else value
        return re.sub(r'https?://\S+', '[url redacted]', value)
    return value
def pause(clip, phase, result):
    save(PAUSE, {'at':stamp(),'clip':clip,'phase':phase,'result':clean(result),
                 'requires':'Reconcile recorded task IDs; no automatic fresh submission'})
def check_pause():
    if PAUSE.exists(): raise RuntimeError('StarFrame circuit paused; no new create permitted')

def record_folder(folder):
    old = folder/'attempt.json'
    if not old.exists() or read(old).get('provider') == 'starframe': return folder
    legacy_status = read(folder/'status.json')
    task = read(folder/'created.json').get('output',{}).get('task_id')
    if not task or legacy_status.get('output',{}).get('task_status') != 'FAILED':
        raise RuntimeError('Legacy attempt is not explicitly FAILED; reconcile by ID before any create')
    # A successful HTTP task GET already established terminal failure. Preserve
    # that evidence because the user replaced the old channel credential.
    report = read(HERE/'recharge-reconciliation-20260908T115926Z.json')
    row = next((r for r in report['tasks'] if r.get('taskId') == task), None)
    if not row or row.get('http') != 200 or row.get('status') != 'FAILED':
        raise RuntimeError('Legacy task lacks a conclusive read-only provider reconciliation')
    dest = folder/'recoveries'/RECOVERY
    dest.mkdir(parents=True, exist_ok=True)
    if not (dest/'recovery.json').exists():
        archive = dest/'legacy-receipts'
        archive.mkdir(exist_ok=True)
        for name in ['attempt.json','created.json','status.json','failure.json']:
            src = folder/name
            if src.exists() and not (archive/name).exists(): shutil.copy2(src, archive/name)
        save(dest/'recovery.json', {'at':stamp(),'authorization':'User 2026-09-09 configured new video service and explicitly requested continuing the 44 authorized assets',
            'priorTaskId':task,'priorProvider':'aliyun-minimax-video','priorFinalStatus':'FAILED',
            'readOnlyReconciliation':row,'readOnlyReconciliationAt':report['at'],
            'priorCredentialReplaced':True,'newProvider':'starframe','newModel':MODEL,
            'originalReceiptsPreserved':True,'freshCreatesAuthorized':1})
    return dest

def upload_reference(path, dest):
    raw = path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    recorded = dest/(path.stem+'.upload.private.json')
    if recorded.exists():
        saved = read(recorded)
        if saved.get('sha256') == digest and saved.get('file_url'): return saved['file_url']
    # Same signing mechanism used by the service's publicly delivered client.
    page = requests.get(WEB,timeout=30)
    assets = re.findall(r'src="(/assets/[^" ]+\.js)"',page.text)
    if len(assets) != 1: raise RuntimeError('Cannot identify current StarFrame public upload client')
    app = requests.get(WEB+assets[0],timeout=45).text
    match = re.search(r'\w+="X-Upload-Sign-Token",\w+="([^"\r\n]+)"',app)
    if not match: raise RuntimeError('Current StarFrame upload signing protocol changed')
    response = requests.post(WEB+'/api/upload-sign',headers={'X-Upload-Sign-Token':match.group(1)},
        json={'file_name':'code-sentinels-v5-'+digest[:24]+'.png','content_type':'image/png','file_size':len(raw)},timeout=45)
    if response.status_code != 200: raise RuntimeError(f'Reference upload signing HTTP {response.status_code}')
    signed = response.json()
    if not all(str(signed.get(k,'')).startswith('https://') for k in ['upload_url','file_url']):
        raise RuntimeError('Reference signing response lacks HTTPS URLs')
    try:
        result = requests.put(signed['upload_url'],data=raw,headers={'Content-Type':'image/png'},timeout=120)
    except requests.RequestException as exc:
        save(dest/('upload-failure-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')+'.json'),
             {'at':stamp(),'phase':'reference-upload-before-paid-create','errorType':type(exc).__name__,
              'uploadHost':urlparse(signed['upload_url']).hostname,'referenceSha256':digest,'newVideoCreates':0})
        raise RuntimeError('Reference upload transport failed: '+type(exc).__name__) from None
    if result.status_code not in {200,201,204}: raise RuntimeError(f'Reference upload HTTP {result.status_code}')
    save(recorded, {'file_url':signed['file_url'],'sha256':digest,'uploadedAt':stamp(),'source':str(path.relative_to(PROJECT))})
    return signed['file_url']

def prepare_body(clip, spec, dest):
    images=[]
    refs=[]
    for ref in spec['references']:
        original=(PROJECT/ref['path']).resolve(strict=True)
        if not original.is_relative_to(PROJECT): raise RuntimeError('Reference outside project')
        img=Image.open(original).convert('RGB')
        # 16:9 canvas; all normalization remains relative to the central square.
        canvas=Image.new('RGB',(1792,1008),(255,0,255) if spec['key']=='magenta' else (0,0,0))
        canvas.paste(img.resize((1008,1008),Image.Resampling.LANCZOS),(392,0))
        expanded=dest/(ref['type']+'-wide.png')
        canvas.save(expanded)
        check_pause()
        images.append(upload_reference(expanded,dest))
        refs.append({'type':ref['type'],'original':ref['path'],'wide':str(expanded.relative_to(PROJECT)),
                     'sha256':hashlib.sha256(expanded.read_bytes()).hexdigest()})
    prompt=spec['prompt']+' The supplied image is the exact first frame. Preserve its scale, position and background. All action described above occurs during seconds 0 through 4. From seconds 4 through 15 hold the final result without new action, scene changes or camera motion. Use the whole 16:9 reference canvas exactly, retaining the empty side padding; all visible subject matter stays in its central square.'
    if len(images)>1: prompt+=' Reference image 1 is the exact starting pose and reference image 2 is the final pose, at the same camera, scale and horizontal position.'
    body={'model':MODEL,'prompt':prompt,'client_task_id':'code-sentinels-v5-'+RECOVERY+'-'+clip,
          'mode':'references','aspect_ratio':'16:9','duration':15,'resolution':'2k',
          'references':{'image':images[0]} if len(images)==1 else {'images':images}}
    save(dest/'effective-request.json', {**body,'references':refs,'extraction':{'centralSquare':True,'startSec':0,'durationSec':4},'modeMeaning':'real image2video using uploaded reference images'})
    return body

def produce(clip):
    if not re.fullmatch('[a-z0-9-]+',clip): raise RuntimeError('Invalid clip identifier')
    folder=HERE/'jobs'/clip
    spec=read(folder/'request.json')
    if spec['id']!=clip: raise RuntimeError('Request identifier mismatch')
    receipt=folder/'video.json'
    if receipt.exists() and (PROJECT/read(receipt)['fileRef']).is_file():
        emit({'id':clip,'status':'already-downloaded'}); return
    cfg=read(ROOT/'data/gen-backends.json')
    backend=next(b for b in cfg['backends'] if b['id']=='aliyun-minimax-video')
    if not backend.get('enabled') or urlparse(backend['endpoint']).hostname not in {'web.xzapi.vip','api.xzapi.vip','xzapi.vip'}:
        raise RuntimeError('Configured channel is not the user-authorized StarFrame service')
    dest=record_folder(folder)
    key=credentials()
    headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'}
    attempt=dest/'attempt.json'
    created_path=dest/'created.json'
    if created_path.exists():
        created=read(created_path)
        task=created.get('id')
        if not task: raise RuntimeError('Existing StarFrame create has no task ID; no blind retry')
    else:
        if attempt.exists(): raise RuntimeError('Existing StarFrame attempt outcome unknown; no blind retry')
        check_pause()
        body=prepare_body(clip,spec,dest)
        check_pause()
        with attempt.open('x',encoding='utf-8') as f:
            json.dump({'id':clip,'startedAt':stamp(),'provider':'starframe','backend':backend['id'],'model':MODEL,
                'mode':'image2video','executor':'project Codex CLI agent','createCount':1,
                'clientTaskId':body['client_task_id'],'durationSec':15,'sourceActionSec':4,
                'requestSha256':hashlib.sha256(json.dumps(body,sort_keys=True).encode()).hexdigest()},f,indent=2)
        emit({'id':clip,'status':'submitting-single-paid-task','provider':'starframe','model':MODEL,'durationSec':15})
        try:
            response=requests.post(API+'/v1/videos',json=body,headers=headers,timeout=120,allow_redirects=False)
            created=response.json()
        except (requests.RequestException,ValueError):
            error={'at':stamp(),'phase':'create','outcome':'unknown','clientTaskId':body['client_task_id']}
            save(dest/'failure.json',error);pause(clip,'create-unknown',error)
            raise RuntimeError('Create outcome unknown; attempt preserved and circuit paused')
        save(created_path,clean(created,key))
        task=created.get('id')
        if response.status_code not in {200,201,202} or not task:
            error={'at':stamp(),'http':response.status_code,'phase':'create','response':clean(created,key)}
            save(dest/'failure.json',error);pause(clip,'create-rejected',error)
            raise RuntimeError(f'Create rejected HTTP {response.status_code}; preserved and paused')
    if not re.fullmatch('[a-zA-Z0-9_.-]+',task): raise RuntimeError('Invalid recorded task ID')
    emit({'id':clip,'status':'polling-existing-task','taskId':task})
    deadline=time.monotonic()+5400
    previous=None
    while time.monotonic()<deadline:
        try:
            response=requests.get(API+'/v1/videos/'+task,headers=headers,timeout=60,allow_redirects=False)
            result=response.json()
        except (requests.RequestException,ValueError): time.sleep(15);continue
        record={'at':stamp(),'http':response.status_code,'taskId':task,'provider':'starframe','response':clean(result,key)}
        save(dest/'status.json',record)
        status=result.get('status')
        if status!=previous:
            emit({'id':clip,'status':status,'taskId':task});previous=status
        if status=='completed':
            save(dest/'completed.private.json',result)
            download=requests.get(API+'/v1/videos/'+task+'/content',headers=headers,timeout=240)
            raw=download.content
            if download.status_code!=200 or len(raw)<12 or raw[4:8]!=b'ftyp':
                raise RuntimeError('Completed task download incomplete; resume same task ID')
            category='effects-v5' if spec['category']!='building' else 'buildings-v5'
            target=PROJECT/'SourceMedia'/category/(clip+'.mp4')
            target.parent.mkdir(parents=True,exist_ok=True)
            temporary=target.with_suffix('.mp4.download');temporary.write_bytes(raw);temporary.replace(target)
            save(receipt,{'id':clip,'backendId':'starframe','fileRef':str(target.relative_to(PROJECT)).replace('\\','/'),
                'ext':'mp4','mime':'video/mp4','bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),
                'meta':{'provider':'starframe','model':MODEL,'taskId':task,'clientTaskId':read(attempt)['clientTaskId'],
                    'mode':'image2video','resolution':'2k','durationSec':15,'completedAt':stamp(),
                    'extraction':{'centralSquare':True,'startSec':0,'durationSec':4},
                    'receipts':str(dest.relative_to(PROJECT)).replace('\\','/')}})
            emit({'id':clip,'status':'downloaded','taskId':task,'bytes':len(raw)});return
        if status in {'failed','unknown'} or response.status_code in {401,402,403,404}:
            save(dest/'failure.json',record);pause(clip,'task-ended',record)
            raise RuntimeError(f'Existing StarFrame task {task} ended {status}; circuit paused')
        time.sleep(15)
    raise RuntimeError(f'Task {task} polling budget ended; resume by ID without new create')
