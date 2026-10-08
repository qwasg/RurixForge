from pathlib import Path
import json, hashlib
from PIL import Image

project=Path(__file__).resolve().parents[2]
raw=project/'Logs/v6/pressure-3f3236df-e235a0d0-failed-20260911'
read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
identity=read(project/'Logs/v6/performance-repair-host-identity-20260911/host-identity.json')
simulation=read(raw/'simulation-report.json');gpu=read(raw/'gpu-report.json')
assert gpu['nativeHash']==identity['engineSha256']
assert simulation['simulationPassed'] and not gpu['passed']
png_dir=raw/'current-readback-png';png_dir.mkdir(exist_ok=False)
images=[]
for ppm in sorted(raw.glob('layer-*.ppm')):
    image=Image.open(ppm).convert('RGB');target=png_dir/(ppm.stem+'.png');image.save(target)
    assert Image.open(target).convert('RGB').tobytes()==image.tobytes()
    images.append({'nativePpm':ppm.relative_to(project).as_posix(),'losslessPng':target.relative_to(project).as_posix(),'pixelBytesEqual':True})
files=[raw/n for n in ('simulation-report.json','gpu-report.json','phase-profile.json','pressure-save.json','render-frames.json','process-exit.json')]
files+=list(raw.glob('layer-*.ppm'))+list(png_dir.glob('*.png'))
report={
    'schemaVersion':2,'kind':'native-performance-acceptance','passed':False,'finalEligible':False,
    'engineSha256':identity['engineSha256'],'rulesVersion':identity['rulesVersion'],'rulesFingerprint':identity['rulesFingerprint'],
    'scope':'Exclusive unchanged128/512/200moving/min600/eight-layer pressure fixture. Simulation passed, GPU per-layer threshold failed. No45-minute or normal earned-economy claim.',
    'simulation':simulation,'gpu':gpu,'failedLayers':{k:v['fps'] for k,v in gpu['perLayer'].items() if v['fps']<30},
    'images':images,'evidence':[{'path':p.relative_to(project).as_posix(),'sha256':sha(p)} for p in files],
    'notes':['The archive contains older named baseline/round JSON and legacy top-level PNGs inherited from the working directory; those are excluded from this evidence list. Only current-readback-png contains this run\'s PNGs, byte-verified against its actual PPM readbacks.',
             'No GPU-state telemetry was recorded during this run. Post-exit P8/39C does not establish the measurement-time state.'],
}
target=project/'game/v6/performance-failure-3f3236df-20260911.json'
assert not target.exists();target.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
print(json.dumps({'report':str(target),'sha256':sha(target),'failedLayers':report['failedLayers']}))
