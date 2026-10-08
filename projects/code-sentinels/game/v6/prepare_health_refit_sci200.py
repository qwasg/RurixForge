"""Canonical, reusable SCI200 plan and an unchanged-source build manifest."""
from pathlib import Path
import hashlib,json,datetime,struct
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'game/v6/health-refit-sci200-20260911';OUT.mkdir(exist_ok=False)
BRANCHES=['speed','security','algorithm','science','lightweight']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(name,v):
 with (OUT/name).open('x',encoding='utf-8') as f:json.dump(v,f,ensure_ascii=False,indent=2)
rows={}
for file in sorted((ROOT/'game/v6').glob('matrix-20seed-20260911-guard-*.jsonl'))+sorted((ROOT/'game/v6').glob('matrix-749391-remaining-*.jsonl')):
 for line in file.read_text(encoding='utf-8').splitlines():
  r=json.loads(line);assert r['index'] not in rows;rows[r['index']]=(r,file)
assert len(rows)==1000
cases=[];references=[]
for index in range(1000):
 a=BRANCHES[index//200];b=BRANCHES[(index//40)%5];swapped=index%2==1
 if not (a=='science' or b=='science') or (swapped and a!=b):continue
 offset=(index//2)%20;branches=[b,a] if swapped else [a,b];planner=[2,1] if swapped else [1,2]
 case=dict(index=index,seed=1000+offset,theme=['river','mining','highland'][offset%3],branches=branches,strategies=['mixed-ai','mixed-ai'],swapped=swapped,plannerOrder=planner,originalPair=[a,b]);cases.append(case)
 row,file=rows[index];assert all(row[k]==case[k] for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder','originalPair'])
 references.append(dict(index=index,sourceFile=file.relative_to(ROOT).as_posix(),sourceSha256=sha(file),row=row))
assert len(cases)==200;assert len({tuple(c['branches']+c['plannerOrder']+[c['seed'],c['theme']])for c in cases})==200
write('case-plan.json',dict(schemaVersion=1,planId='v6-health-refit-science200-canonical-v1',scope='Representative20-seed SCI slice with original1000 indices. Cross-branch planner[1,2] on both sides; self includes both planner orders. Not acceptance.',cases=cases))
write('baseline-749-rows.json',dict(scope='Unmodified executed baseline200 rows; no newly instrumented metrics fabricated.',cases=references))
native=ROOT/'native-v6';rules=sorted([*native.joinpath('src').rglob('*.rs'),native/'Cargo.toml',native/'Cargo.lock',native/'build.rs'],key=lambda p:p.relative_to(native).as_posix());digest=hashlib.sha256(b'code-sentinels-native-rules-v1\0')
for p in rules:
 name=p.relative_to(native).as_posix().encode();data=p.read_bytes();digest.update(struct.pack('<Q',len(name))+name+struct.pack('<Q',len(data))+data)
sources=set(rules)|set(native.joinpath('tests').rglob('*.rs'))|set(ROOT.joinpath('game/v6/balance-runner/src').glob('*.rs'))|{ROOT/'game/v6/balance-runner/Cargo.toml',ROOT/'game/v6/balance-runner/Cargo.lock'}
files=[dict(path=p.relative_to(ROOT).as_posix(),bytes=p.stat().st_size,sha256=sha(p))for p in sorted(sources)]
write('source-before-build.json',dict(capturedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),rulesVersion='v6.2',rulesFingerprint=digest.hexdigest(),files=files))
jobs=[]
for ordinal in range(0,200,25):jobs.append(dict(first=cases[ordinal]['index'],limit=25,startOrdinal=ordinal))
write('prepared-shards.json',dict(scope='Prepared only. --first is a canonical lower bound, never an array offset.',maxProcesses=8,jobs=jobs))
print(json.dumps(dict(cases=len(cases),rulesFingerprint=digest.hexdigest(),planSha256=sha(OUT/'case-plan.json'),sources=len(files),shards=jobs)))
