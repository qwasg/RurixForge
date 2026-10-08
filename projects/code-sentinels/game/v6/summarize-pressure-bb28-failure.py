from pathlib import Path
from datetime import datetime, timezone, timedelta
from collections import Counter
from statistics import median
import csv,json,hashlib
from PIL import Image
project=Path(__file__).resolve().parents[2]
raw=project/'Logs/v6/pressure-bb28dae8-30e7353a-failed-20260912'
read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
identity=read(project/'Logs/v6/decoded-lru-host-identity-20260911/host-identity.json')
sim=read(raw/'simulation-report.json');gpu=read(raw/'gpu-report.json');telemetry=read(raw/'gpu-telemetry.json')
assert gpu['nativeHash']==identity['engineSha256'] and sim['simulationPassed'] and not gpu['passed']
start=datetime.fromisoformat(telemetry['measurementStartUtc'].replace('Z','+00:00'));end=datetime.fromisoformat(telemetry['measurementEndUtc'].replace('Z','+00:00'))
offset=timezone(timedelta(minutes=telemetry['localUtcOffsetMinutes']))
window=[]
with (raw/'gpu-telemetry.csv').open(encoding='utf-8-sig',newline='') as f:
    for row in csv.DictReader(f):
        row={k.strip():v.strip() for k,v in row.items()}
        at=datetime.strptime(row['timestamp'],'%Y/%m/%d %H:%M:%S.%f').replace(tzinfo=offset).astimezone(timezone.utc)
        if start<=at<=end:row['utc']=at.isoformat();window.append(row)
def summary(column):
    values=[float(row[column].split()[0]) for row in window if row[column] not in ('[N/A]','N/A','[Not Supported]')]
    return {'observedSamples':len(values),'missingSamples':len(window)-len(values),'min':min(values) if values else None,'median':median(values) if values else None,'max':max(values) if values else None}
stats={'scope':'Only raw1Hz samples inside recorded UTC GPU measurement window. No power-mode changes; unavailable readings remain missing. These alone do not establish causality versus older runs without telemetry.',
       'measurementStartUtc':telemetry['measurementStartUtc'],'measurementEndUtc':telemetry['measurementEndUtc'],'sampleCount':len(window),'pstates':dict(Counter(r['pstate'] for r in window)),
       'columns':{key:summary(key) for key in ['temperature.gpu','clocks.current.graphics [MHz]','clocks.current.memory [MHz]','power.draw [W]','power.limit [W]','utilization.gpu [%]','utilization.memory [%]','memory.used [MiB]','memory.total [MiB]']},'windowRows':window}
(raw/'gpu-window-summary.json').write_text(json.dumps(stats,indent=2)+'\n',encoding='utf-8')
pngdir=raw/'current-readback-png';pngdir.mkdir(exist_ok=False)
for ppm in raw.glob('layer-*.ppm'):
    image=Image.open(ppm).convert('RGB');target=pngdir/(ppm.stem+'.png');image.save(target);assert image.tobytes()==Image.open(target).convert('RGB').tobytes()
files=[raw/n for n in ['simulation-report.json','gpu-report.json','phase-profile.json','pressure-save.json','render-frames.json','process-exit.json','gpu-telemetry.json','gpu-telemetry.csv','gpu-window-summary.json']]
files+=list(raw.glob('layer-*.ppm'))+list(pngdir.glob('*.png'))
cache=gpu['privateMetrics']['runtimePages']
report={'schemaVersion':2,'kind':'native-performance-acceptance','passed':False,'finalEligible':False,
    'engineSha256':identity['engineSha256'],'rulesVersion':identity['rulesVersion'],'rulesFingerprint':identity['rulesFingerprint'],
    'scope':'Exclusive unchanged128/512/200moving/min600/eight-layer fixture with optional read-only1Hz GPU telemetry. Simulation passed; three layer GPU thresholds failed.',
    'simulation':sim,'gpu':gpu,'failedLayers':{k:v['fps'] for k,v in gpu['perLayer'].items() if v['fps']<30},
    'decodedPageCache':cache,'decodedPageHitRatio':cache['hits']/cache['requests'] if cache['requests'] else None,
    'gpuWindow':{k:v for k,v in stats.items() if k!='windowRows'},
    'evidence':[{'path':p.relative_to(project).as_posix(),'sha256':sha(p)} for p in files],
    'notes':['No decoded-page repetition or eviction was observed in this fixture:251 requests,251 decodes,0hits,0evictions. This disproves repeated registered-page decoding as this workload\'s bottleneck; noFPS repair claim is made.',
             'OriginalPPM and current-readback-png are the current images. Inherited old top-level PNG/baseline/round files in the working archive are excluded.',
             'The shader/render source is unchanged from E0; an old-engine same-environment control remains necessary before assigning the execution/readback regression to source changes or device power state.']}
target=project/'game/v6/performance-failure-bb28dae8-20260912.json';assert not target.exists();target.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
print(json.dumps({'report':str(target),'sha256':sha(target),'gpuWindow':report['gpuWindow'],'cache':cache}))
