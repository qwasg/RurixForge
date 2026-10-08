"""Paired executed rows; keep uninstrumented baseline telemetry unavailable."""
from pathlib import Path
import json, hashlib, statistics
ROOT=Path(__file__).resolve().parents[2]
BASE=ROOT/'game/v6/supply-prediction-probe-20260911'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def load(p): return json.loads(p.read_text(encoding='utf-8'))
plan=load(BASE/'case-plan.json');previous=load(BASE/'baseline-rows.json');receipt=load(BASE/'runner-build-receipt.json')
rows=[];inputs=[]
for path in sorted(BASE.glob('results-*.jsonl')):
    rows.extend(json.loads(line) for line in path.read_text(encoding='utf-8').splitlines())
    inputs.append(dict(path=path.relative_to(ROOT).as_posix(),sha256=sha(path)))
assert len(rows)==20 and {r['index'] for r in rows}==set(range(20)), 'incomplete or duplicated bounded probe'
old={x['probeIndex']:x for x in previous['cases']};new={r['index']:r for r in rows}
for case in plan['cases']:
    r=new[case['index']]
    assert all(r[k]==case[k] for k in ['seed','theme','branches','strategies','swapped','plannerOrder','originalPair'])
    assert r['planSha256']==sha(BASE/'case-plan.json') and r['simulationSourceSha256']==receipt['rulesFingerprint']
    reference=old[r['index']]
    assert sha(ROOT/reference['sourceFile'])==reference['sourceSha256']
    assert all(r[k]==reference['row'][k] for k in ['seed','theme','branches','strategies','plannerOrder'])
def dist(values):
    present=[v for v in values if v is not None]
    return dict(observations=len(values),available=len(present),unavailable=len(values)-len(present),sum=sum(present) if present else None,median=statistics.median(present) if present else None,min=min(present) if present else None,max=max(present) if present else None)
def actors(row,owner):return row['orderMetrics']['aiActors'][owner]
def outcome(row,owner):return 'unresolved' if row['winner'] is None else 'win' if row['winner']==owner+1 else 'loss'
def obs(row,owner,new_metrics):
    values=dict(outcome=outcome(row,owner),seconds=row['seconds'],firstT5Seconds=row['firstT5Seconds'][owner],ore=row['oreDelivered'][owner],energySpent=row['actualEnergySpent'][owner],shots=row['shotsFired'][owner],peakAi=row['orderMetrics']['peakAliveAi'][owner],geminiShots=sum(a['shots'] for a in actors(row,owner) if a['kind']=='gemini'),geminiCasts=sum(a['activeSkills'] for a in actors(row,owner) if a['kind']=='gemini'))
    for key in ['energyRecharged','chargingUnitSeconds','ammoDeliveredToUnits','energyStarvedFiringUnitSeconds','ammoStarvedFiringUnitSeconds','computeStarvedFiringUnitSeconds']:
        values[key]=row['supplyMetrics'][key][owner] if new_metrics else None
    for key in ['impacts','hpDamagingImpacts','actualHpDamage']:
        values['delayedArea'+key[0].upper()+key[1:]]=row['delayedAreaMetrics'][key][owner] if new_metrics else None
    return values
def summarize(observations):
    return dict(observations=len(observations),wins=sum(o['outcome']=='win' for o in observations),losses=sum(o['outcome']=='loss' for o in observations),unresolved=sum(o['outcome']=='unresolved' for o in observations),metrics={key:dist([o[key] for o in observations]) for key in observations[0] if key!='outcome'})
details=[];buckets={}
for index in range(20):
    r=new[index];baseline=old[index]['row']
    sciences=[]
    for side,branch in enumerate(r['branches']):
        if branch!='science':continue
        opponent=r['branches'][1-side];before=obs(baseline,side,False);after=obs(r,side,True)
        for bucket in [opponent,'all-including-mirror']+(['excluding-mirror'] if opponent!='science' else []):
            b=buckets.setdefault(bucket,dict(baseline=[],current=[]));b['baseline'].append(before);b['current'].append(after)
        sciences.append(dict(side=side+1,opponent=opponent,before=before,after=after))
    details.append(dict(index=index,baselineCanonicalIndex=old[index]['canonicalIndex'],seed=r['seed'],theme=r['theme'],branches=r['branches'],plannerOrder=r['plannerOrder'],oldWinner=baseline['winner'],newWinner=r['winner'],oldSeconds=baseline['seconds'],newSeconds=r['seconds'],oldWinReason=baseline['winReason'],newWinReason=r['winReason'],science=sciences))
source=load(BASE/'source-before-build.json');unchanged=all(sha(ROOT/f['path'])==f['sha256'] for f in source['files'])
report=dict(scope='All 20 exact paired scenarios, not final balance acceptance. Two seeds/two themes; no hidden exclusions.',integrityPassed=True,sourceUnchanged=unchanged,rulesVersion='v6.2',baselineRulesFingerprint=previous['rulesFingerprint'],rulesFingerprint=receipt['rulesFingerprint'],runnerSha256=receipt['artifact']['sha256'],planSha256=sha(BASE/'case-plan.json'),inputs=inputs,groups={k:{generation:summarize(obs) for generation,obs in groups.items()} for k,groups in buckets.items()},cases=details,criticalWarnings=[dict(index=r['index'],warnings=r['playabilityWarnings']) for r in rows if any(r['playabilityWarnings'].values())],finalBalanceAcceptance=False,limitations=['The baseline has no charge/starvation/delayed-area HP instrumentation; unavailable baseline metrics remain null, never fabricated zero.','The bundled corrections are tested together. Paired outcome changes cannot isolate each contribution. Individual physics/selection contracts test the specific defects.','Starvation records weapon-ready unit-seconds with eligible targets and failed payment; it is not wall time, total idle time, or estimated DPS loss.','Gemini delayed-area actual HP damage excludes overkill. Shield-only hits are not misses; other AI damage is not attributed by this metric.','Twenty scenarios with two seeds are insufficient to establish final five-branch and six-strategy balance.'])
assert unchanged
with (BASE/'paired-analysis.json').open('x',encoding='utf-8') as f:json.dump(report,f,ensure_ascii=False,indent=2)
print(json.dumps(dict(games=len(rows),sourceUnchanged=unchanged,crossBranch=report['groups']['excluding-mirror'],warnings=report['criticalWarnings']),ensure_ascii=True))
