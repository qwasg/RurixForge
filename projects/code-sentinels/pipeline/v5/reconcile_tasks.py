"""Read-only provider task reconciliation; never creates paid tasks."""
import json
import sys
from pathlib import Path
from datetime import datetime,timezone

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
ROOT=PROJECT.parents[1]
sys.path.insert(0,str(HERE.parent))
from aliyun_upload import credentials
import requests
cfg=json.loads((ROOT/'data/gen-backends.json').read_text(encoding='utf-8'))
endpoint=next(b['endpoint'] for b in cfg['backends'] if b['id']=='aliyun-minimax-video').rstrip('/')
key=credentials()
rows=[]
for path in sorted((HERE/'jobs').glob('*/created.json')):
    created=json.loads(path.read_text(encoding='utf-8'))
    task=created.get('output',{}).get('task_id')
    if not task:
        rows.append({'id':path.parent.name,'status':'CREATE_REJECTED','code':created.get('code')})
        continue
    response=requests.get(endpoint+'/api/v1/tasks/'+task,headers={'Authorization':'Bearer '+key},timeout=45)
    doc=response.json()
    out=doc.get('output',{})
    rows.append({'id':path.parent.name,'taskId':task,'http':response.status_code,'status':out.get('task_status'),
                 'code':out.get('code',doc.get('code')),'message':str(out.get('message',doc.get('message',''))).replace(key,'[redacted]')[:400],
                 'downloaded':(path.parent/'video.json').exists()})
result={'at':datetime.now(timezone.utc).isoformat(),'operation':'read-only task GET after user recharge notice','tasks':rows,'paidSubmissions':0}
target=HERE/f'recharge-reconciliation-{datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")}.json'
target.write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(result,ensure_ascii=True))
