from pathlib import Path
import json,hashlib
project=Path(__file__).resolve().parents[2]
read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
definitions=[
 ('historical-e0-success','pressure-e0f24d78-749391e6-accepted',False,False),
 ('3f-first-failure','pressure-3f3236df-e235a0d0-failed-20260911',False,False),
 ('bb-lru-failure','pressure-bb28dae8-30e7353a-failed-20260912',True,False),
 ('e0-current-input-control','gpu-control-e0f24d78-same-input-20260912',True,True),
 ('e0-original-input-control','gpu-control-e0f24d78-original-input-20260912',True,True),
]
rows=[];evidence=[]
for label,folder,observer,gpu_only in definitions:
    directory=project/'Logs/v6'/folder;file=directory/'gpu-report.json';gpu=read(file)
    row={'label':label,'engineSha256':gpu['nativeHash'],'wallSeconds':gpu['wallSeconds'],'frames':gpu['frameCount'],'averageFps':gpu['observedFps'],'passed':gpu['passed'],'nvidiaSmi1Hz':observer,'gpuOnlyWrapper':gpu_only,'maxInterframeGapMs':gpu['maxInterframeGapMs'],
         'layers':{layer:{'fps':bucket['fps'],'composeP50Ms':gpu['privateMetrics']['renderStagesByLayer'][layer]['compose']['p50'],'prepareP50Ms':gpu['privateMetrics']['renderStagesByLayer'][layer]['prepareUploads']['p50'],'executeReadbackP50Ms':gpu['privateMetrics']['renderStagesByLayer'][layer]['executeAndReadback']['p50']} for layer,bucket in gpu['perLayer'].items()}}
    for name in ['control-identity.json','gpu-telemetry.json']:
        path=directory/name
        if path.exists():row[name]=read(path);evidence.append({'path':path.relative_to(project).as_posix(),'sha256':sha(path)})
    rows.append(row);evidence.append({'path':file.relative_to(project).as_posix(),'sha256':sha(file)})
audit=project/'pipeline/v6/gpu-original-input-audit-20260912.json';evidence.append({'path':audit.relative_to(project).as_posix(),'sha256':sha(audit)})
report={'kind':'gpu-control-comparison','finalEligible':False,'scope':'Observed stored reports only; no new graphics run or system change. Old E0 fails currently with both current and original inputs. This removes new cached-loader code as a necessary cause, but does not identify an environment cause; observer and GPU-only wrapper differences remain explicit.',
        'assetInputAudit':audit.relative_to(project).as_posix(),'runs':rows,'evidence':evidence,'limitations':['Original successful and current dynamic inputs differ only in added unit velocity fields according to the independent complete120-frame audit; initial player income/rule identity differ. The second E0 control uses the original successful input unchanged.',
        'All204 original native atlas/metadata SHA values match the original candidate; the new nativeRuntimeFrames manifest pointer is ignored by old E0.',
        'Current controls include1Hz nvidia-smi and GPU-only wrapping; original success had no such observer. The first3F failure also had no NVML sampler. No claim of system power/timer causation is made.',
        'Vulkan fence waits are direct bounded vkWaitForFences, not1/2ms polling sleeps. Stream pacing sleep is outside the execute/readback timer. Backend CPU/GPU telemetry is the next bounded observation, not a rendering algorithm or system policy change.']}
target=project/'game/v6/gpu-control-comparison-20260912.json';assert not target.exists();target.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
print(json.dumps({'report':str(target),'sha256':sha(target)}))
