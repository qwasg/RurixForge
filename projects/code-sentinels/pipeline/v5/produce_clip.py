"""Single-create paid I2V; restart resumes its recorded task, never resubmits."""
import argparse
import hashlib
import json
import re
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
ROOT=PROJECT.parents[1]
sys.path.insert(0,str(HERE.parent))
from aliyun_upload import credentials, upload
import requests

def stamp(): return datetime.now(timezone.utc).isoformat()
def read(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def save(path,value):
    path.parent.mkdir(parents=True,exist_ok=True)
    temp=path.with_suffix(path.suffix+'.tmp')
    temp.write_text(json.dumps(value,ensure_ascii=False,indent=2),encoding='utf-8')
    temp.replace(path)
def safe_doc(doc):
    return {k:v for k,v in doc.items() if k not in {'video_url','orig_prompt','url'}}
def emit(value): print(json.dumps(value,ensure_ascii=True),flush=True)
def failed(message): raise RuntimeError(message)

def produce(clip):
    if not re.fullmatch('[a-z0-9-]+',clip): failed('Invalid clip identifier')
    folder=HERE/'jobs'/clip
    spec=read(folder/'request.json')
    if spec['id']!=clip: failed('Request identifier mismatch')
    target=PROJECT/'SourceMedia/buildings-v5'/f'{clip}.mp4'
    receipt=folder/'video.json'
    if receipt.exists() and target.is_file():
        saved=read(receipt)
        emit({'id':clip,'status':'already-downloaded','taskId':saved['meta']['taskId'],'video':str(target)})
        return
    cfg=read(ROOT/'data/gen-backends.json')
    if any(b.get('id')=='comfyui-minimax-h3' and b.get('enabled') for b in cfg['backends']):
        from comfyui_provider import produce as produce_local
        return produce_local(clip)
    backend=next(b for b in cfg['backends'] if b['id']=='aliyun-minimax-video')
    from urllib.parse import urlparse
    if urlparse(backend.get('endpoint','')).hostname in {'web.xzapi.vip','api.xzapi.vip','xzapi.vip'}:
        from starframe_provider import produce as produce_starframe
        return produce_starframe(clip)
    if not backend.get('enabled') or backend.get('model')!='MiniMax/MiniMax-H3': failed('Configured media backend is not enabled MiniMax-H3')
    endpoint=backend['endpoint'].rstrip('/')
    if endpoint.endswith('/api/v1'): endpoint=endpoint[:-7]
    if not endpoint.startswith('https://') or '.cn-beijing.maas.aliyuncs.com' not in endpoint: failed('Expected configured official Beijing endpoint')
    key=credentials()
    headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'}
    attempt=folder/'attempt.json'
    created_path=folder/'created.json'
    if created_path.exists():
        created=read(created_path)
        task_id=created.get('output',{}).get('task_id')
        if not task_id: failed('Recorded creation has no task ID; reconcile before any further submission')
    else:
        if attempt.exists(): failed('Existing paid attempt has an unknown outcome; DO NOT resubmit')
        if (HERE/'provider-paused.json').exists(): failed('Provider circuit is paused; no new paid request has been made')
        media=[]
        for ref in spec['references']:
            ref_path=(PROJECT/ref['path']).resolve(strict=True)
            if not ref_path.is_relative_to(PROJECT): failed('Reference outside authorized project')
            ref_hash=hashlib.sha256(ref_path.read_bytes()).hexdigest()
            upload_path=folder/f'{ref["type"]}.upload.private.json'
            uploaded=upload(ref_path)
            save(upload_path,{**uploaded,'sha256':ref_hash,'preparedAt':stamp()})
            media.append({'type':ref['type'],'url':uploaded['uri']})
        body={'model':'MiniMax/MiniMax-H3','input':{'prompt':spec['prompt'],'media':media},
              'parameters':{'resolution':'768P','duration':spec['durationSec'],'ratio':'adaptive','watermark':False}}
        # This file is a transaction guard, created exclusively before network submission.
        with attempt.open('x',encoding='utf-8') as stream:
            json.dump({'id':clip,'startedAt':stamp(),'mode':'image2video','executor':'project Codex CLI agent',
                       'requestSha256':hashlib.sha256((folder/'request.json').read_bytes()).hexdigest(),
                       'backend':'aliyun-minimax-video','durationSec':spec['durationSec'],'createCount':1},stream,indent=2)
        emit({'id':clip,'status':'submitting-single-paid-task','durationSec':spec['durationSec']})
        try:
            response=requests.post(endpoint+'/api/v1/services/aigc/video-generation/video-synthesis',json=body,
                    headers={**headers,'X-DashScope-Async':'enable','X-DashScope-OssResourceResolve':'enable'},timeout=120)
        except requests.RequestException:
            save(folder/'failure.json',{'time':stamp(),'phase':'create','outcome':'unknown','error':'Create connection failed or timed out. Existing attempt must be reconciled; no retry.'})
            failed('Create connection outcome unknown; preserved attempt, do not resubmit')
        try: created=response.json()
        except ValueError:
            save(folder/'failure.json',{'time':stamp(),'phase':'create','http':response.status_code,'outcome':'unknown'})
            failed('Non-JSON creation response; preserved attempt, do not resubmit')
        # Save task ID immediately, before any polling or download.
        save(created_path,safe_doc(created))
        task_id=created.get('output',{}).get('task_id')
        if response.status_code!=200 or not task_id:
            save(folder/'failure.json',{'time':stamp(),'phase':'create','http':response.status_code,'code':created.get('code'),'message':str(created.get('message','')).replace(key,'[redacted]')[:400]})
            failed(f'Provider creation rejected: HTTP {response.status_code}, code {created.get("code")}; receipt preserved')
    if not re.fullmatch('[a-zA-Z0-9_-]+',task_id): failed('Invalid recorded task ID')
    emit({'id':clip,'status':'polling-existing-task','taskId':task_id})
    deadline=time.monotonic()+1800
    old_status=None
    while time.monotonic()<deadline:
        try:
            response=requests.get(endpoint+'/api/v1/tasks/'+task_id,headers=headers,timeout=60)
        except requests.RequestException:
            time.sleep(15)
            continue
        if response.status_code in {429,500,502,503,504}:
            time.sleep(15)
            continue
        try: result=response.json()
        except ValueError:
            time.sleep(15)
            continue
        out=result.get('output',{})
        status=out.get('task_status')
        record={'time':stamp(),'http':response.status_code,'request_id':result.get('request_id'),
                'output':safe_doc(out),'usage':result.get('usage')}
        save(folder/'status.json',record)
        if status!=old_status:
            emit({'id':clip,'status':status,'taskId':task_id})
            old_status=status
        if status=='SUCCEEDED':
            url=out.get('video_url')
            if not url or not url.startswith('https://'): failed('Succeeded task missing HTTPS video URL; resume existing task')
            save(folder/'completed.private.json',result)
            try: download=requests.get(url,timeout=180)
            except requests.RequestException: failed('Video download interrupted; resume existing task without creating another')
            raw=download.content
            if download.status_code!=200 or len(raw)<12 or raw[4:8]!=b'ftyp': failed('Provider download was not a complete MP4; resume existing task')
            target.parent.mkdir(parents=True,exist_ok=True)
            temporary=target.with_suffix('.mp4.download')
            temporary.write_bytes(raw)
            temporary.replace(target)
            metadata={'provider':'aliyun-minimax-video','model':'MiniMax/MiniMax-H3','taskId':task_id,
                      'requestId':result.get('request_id'),'mode':'image2video','resolution':'768P',
                      'durationSec':spec['durationSec'],'usage':result.get('usage'),'completedAt':stamp()}
            save(receipt,{'id':clip,'backendId':'aliyun-minimax-video','fileRef':str(target.relative_to(PROJECT)).replace('\\','/'),
                          'ext':'mp4','mime':'video/mp4','bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'meta':metadata})
            emit({'id':clip,'status':'downloaded','taskId':task_id,'bytes':len(raw),'video':str(target)})
            return
        if status in {'FAILED','CANCELED','UNKNOWN'} or response.status_code!=200:
            save(folder/'failure.json',record)
            if out.get('code') in {'InvalidApiKey','Arrearage','InsufficientBalance','InsufficientQuota'} or 'not activated' in str(out.get('message','')).lower():
                # Stop subsequent queue entries on the first real account/channel
                # failure. Existing provider tasks retain their IDs and can finish.
                save(HERE/'provider-paused.json',{'at':stamp(),'triggerClip':clip,'taskId':task_id,
                     'code':out.get('code'),'message':str(out.get('message','')).replace(key,'[redacted]')[:400],
                     'requires':'Read-only reconciliation and explicit recharge/recovery authorization before any fresh create'})
            failed(f'Existing provider task {task_id} ended {status}; no resubmission')
        time.sleep(15)
    failed(f'Existing task {task_id} still pending after polling window; resume by ID, never resubmit')

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('clip')
    args=parser.parse_args()
    try: produce(args.clip)
    except Exception as exc:
        # No exception trace: request URLs and authentication headers never enter logs.
        message=re.sub(r'https?://\S+|\?X-Amz-[^\s]+', '[url redacted]', str(exc))
        emit({'id':args.clip,'status':'stopped','error':message[:500]})
        sys.exit(1)
