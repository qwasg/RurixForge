"""Freeze a bounded paired experiment and its existing, executed baseline."""
from pathlib import Path
import hashlib, json, datetime, struct
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'game/v6/supply-prediction-probe-20260911'
OUT.mkdir(exist_ok=True)
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def write(name,value):
    with (OUT/name).open('x',encoding='utf-8') as f: json.dump(value,f,ensure_ascii=False,indent=2)
rows=[]
files=sorted((ROOT/'game/v6').glob('matrix-20seed-20260911-guard-*.jsonl'))+sorted((ROOT/'game/v6').glob('matrix-749391-remaining-*.jsonl'))
for path in files:
    for line in path.read_text(encoding='utf-8').splitlines():
        row=json.loads(line);rows.append((row,path))
assert len(rows)==1000 and len({r['index'] for r,p in rows})==1000
assert all(r['simulationSourceSha256']=='749391e627e7601628d853751afab4ecf54833ea59d9a56e25389c8c91cef3d2' for r,p in rows)
cases=[];baseline=[]
for opponent in ['speed','security','algorithm','science','lightweight']:
    for seed,theme in [(1000,'river'),(1001,'mining')]:
        for swapped in [False,True]:
            branches=[opponent,'science'] if swapped else ['science',opponent]
            planner=[2,1] if swapped and opponent=='science' else [1,2]
            matching=[(r,p) for r,p in rows if r['seed']==seed and r['theme']==theme and r['branches']==branches and r['strategies']==['mixed-ai','mixed-ai'] and r['plannerOrder']==planner]
            assert len(matching)==1
            row,path=matching[0];index=len(cases)
            cases.append(dict(index=index,seed=seed,theme=theme,branches=branches,strategies=['mixed-ai','mixed-ai'],swapped=swapped,plannerOrder=planner,originalPair=['science',opponent]))
            baseline.append(dict(probeIndex=index,canonicalIndex=row['index'],sourceFile=str(path.relative_to(ROOT)).replace('\\','/'),sourceSha256=sha(path),row=row))
write('case-plan.json',dict(schemaVersion=1,planId='v6-supply-prediction-paired20-v1',scope='Bounded ordinary-command causal probe. Two seeds/two themes; not final branch balance acceptance.',cases=cases))
write('baseline-rows.json',dict(rulesFingerprint=rows[0][0]['simulationSourceSha256'],scope='Unmodified actual baseline rows. New starvation/charge/skill-hit metrics were not instrumented in this baseline and must not be filled as zero.',cases=baseline))
native=ROOT/'native-v6'
rules=sorted([*native.joinpath('src').rglob('*.rs'),native/'Cargo.toml',native/'Cargo.lock',native/'build.rs'],key=lambda p:p.relative_to(native).as_posix())
h=hashlib.sha256(b'code-sentinels-native-rules-v1\0')
for p in rules:
    name=p.relative_to(native).as_posix().encode();data=p.read_bytes();h.update(struct.pack('<Q',len(name))+name+struct.pack('<Q',len(data))+data)
source=set(rules)|set(native.joinpath('tests').rglob('*.rs'))|set(ROOT.joinpath('game/v6/balance-runner/src').glob('*.rs'))|{ROOT/'game/v6/balance-runner/Cargo.toml',ROOT/'game/v6/balance-runner/Cargo.lock'}
manifest=[dict(path=p.relative_to(ROOT).as_posix(),bytes=p.stat().st_size,sha256=sha(p)) for p in sorted(source)]
write('source-before-build.json',dict(capturedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),rulesVersion='v6.2',rulesFingerprint=h.hexdigest(),files=manifest))
print(json.dumps(dict(output=str(OUT),cases=len(cases),rulesFingerprint=h.hexdigest(),sourceFiles=len(manifest),planSha256=sha(OUT/'case-plan.json'))))
