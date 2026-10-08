"""Freeze approved80 paired cases and auditable0ac facility observations only."""
from pathlib import Path
import collections
import datetime
import hashlib
import json
import shutil

ROOT=Path(__file__).resolve().parents[2]
FINAL=ROOT/'game/v6/final-balance-0acffa83-20260913'
CAUSAL=ROOT/'game/v6/causal-review-0acffa83-20260913'
OUT=ROOT/'game/v6/direct-range-probe80-20260913'
read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
ref=lambda p:dict(path=p.relative_to(ROOT).as_posix(),sha256=sha(p),bytes=p.stat().st_size)


def write(p,v):
    with p.open('x',encoding='utf-8',newline='\n')as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')


def main():
    assert not OUT.exists(),'Pre-registration is immutable; preserve an existing plan.'
    manifest=read(FINAL/'measurement-manifest.json')
    plan_path=FINAL/'branch/branch-plan.json'; plan=read(plan_path)
    assert sha(plan_path)==manifest['matrices']['branch']['inputPlan']['sha256']
    offsets=[int(i*19/4+0.5)for i in range(5)]; seeds=[1000+n for n in offsets]
    assert offsets==[0,5,10,14,19]
    cases=[c for c in plan['cases'] if c['seed'] in seeds and 'science'in c['branches'] and c['branches'][0]!=c['branches'][1]]
    assert len(cases)==80
    for seed in seeds:
        for opponent in ['speed','security','algorithm','lightweight']:
            selected=[c for c in cases if c['seed']==seed and opponent in c['branches']]
            assert len(selected)==4 and len({(tuple(c['branches']),tuple(c['plannerOrder']))for c in selected})==4
    OUT.mkdir();(OUT/'baseline').mkdir();(OUT/'candidate').mkdir();(OUT/'artifact').mkdir()
    shutil.copy2(plan_path,OUT/'canonical-branch-plan.json')
    bindings=[]
    for c in cases:
        i=c['index'];raw=FINAL/f'branch/results-{i:03}.jsonl';saved=FINAL/f'branch/diagnostics/match-{i}.save.json'
        row=read(raw);assert row['simulationSourceSha256']==manifest['rulesFingerprint']
        assert all(row[k]==c[k]for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder','originalPair'])
        copy=OUT/f'baseline/results-{i:03}.jsonl';shutil.copy2(raw,copy);assert sha(copy)==sha(raw)
        bindings.append(dict(index=i,case=c,originalRaw=ref(raw),preservedRaw=ref(copy),originalFinalSave=ref(saved)))
    prereg=dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),status='root-approved-pre-registered-not-yet-executed',scope='Exactly80 original canonical cases before the authorized direct-projectile lifetime candidate is measured. Not full1000 or final balance approval.',selectionAlgorithm='Choose nearest integer at5 equally spaced positions including endpoints of seed offsets0..19: floor(i*19/4+0.5), i=0..4. Then science vs each other branch, each actual spawn assignment and each planner order.',seedOffsets=offsets,seeds=seeds,caseCount=80,indices=[c['index']for c in cases],baselineRulesFingerprint=manifest['rulesFingerprint'],baselineRunner=manifest['runner'],baselineSourceFreeze=manifest['originalCandidate'],canonicalPlan=ref(OUT/'canonical-branch-plan.json'),cases=bindings,candidateChangeScope='Root authorizes only the verified generic direct-projectile lifetime/range-endpoint correction plus regression tests. No HP/cost/supply/AI-policy changes.',candidateRulesFingerprint=None,candidateExecutionStatus='Waiting for native tests, new rules fingerprint freeze and CPU release before building runner or running cases.',measurement=dict(simulatedMinutesBudget=55,simulationHz=60,maxConcurrentProcesses=16,injectedResources=False,tickSkipping=False,forcedWinner=False),comparisonRules=['Candidate keeps original canonical index, exact full-plan bytes/SHA and metadata; baseline rows remain byte-identical and retain0ac fingerprint.','Every selected case is run once; no dropping short or unfavorable results. Natural unresolved55-minute cases stay unresolved.','Compare all80 paired observations plus opponent, seed, actual side and planner strata. Self matches are absent.','This registered sample tests correction direction; it cannot replace final1000 or strategy126 acceptance.'],finalBalanceAcceptance=False)
    write(OUT/'pre-registration.json',prereg)
    energy_path=CAUSAL/'energy-defense-observations.json';energy=read(energy_path);review_path=FINAL/'branch/branch-review-data.json';review=read(review_path)
    assert len(energy['observations'])==2000 and len(review['finalSaves'])==1000
    examples=[]
    for index in [760,921]:
        saved_path=FINAL/f'branch/diagnostics/match-{index}.save.json';saved=read(saved_path);state=saved['snapshot'];facilities=[]
        for room in state['rooms']:
            if room['kind']!='energy-defense':continue
            orders=[o for o in saved['orders']if o['receipt']['accepted'] and o['order']['owner']==room['owner'] and o['order']['command']['op']=='room' and o['order']['command'].get('kind')=='energy-defense' and o['order']['command'].get('rect')==room['rect']]
            events=[e for e in state['events']if e['kind']=='construction-complete' and e['subject']==room['id']]
            facilities.append(dict(roomId=room['id'],owner=room['owner'],rect=room['rect'],matchingAcceptedBuildOrders=orders,acceptedOrderSeconds=orders[0]['tick']/60 if len(orders)==1 else None,constructionCompleteAtSeconds=min(e['tick']for e in events)/60 if events else None,constructionCompletionTimeStatus='observed-retained-construction-complete-event'if events else'not-available-in-retained-event-ring',constructionCompleteByFinalSnapshot=room['progress']>=1,firstOnlineAtSeconds=None,firstOnlineTimeStatus='not-instrumented-in-saved-history',onlineAtFinalSnapshot=room['online'],poweredAtFinalSnapshot=room['powered'],finalTick=state['tick'],completeFinalRoom=room,retainedCompletionEvents=events))
        examples.append(dict(index=index,raw=ref(FINAL/f'branch/results-{index:03}.jsonl'),fullNativeSave=ref(saved_path),execution=ref(FINAL/f'branch/execution-{index:03}.json'),retainedEventBounds=dict(earliestTick=min(e['tick']for e in state['events']),latestTick=max(e['tick']for e in state['events'])),facilities=facilities))
    facility_audit=dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),scope='Audit index for read-only facility observations; full actual Save references are retained. Order, completion and online facts are distinct.',rulesFingerprint=manifest['rulesFingerprint'],facilityAnalysis=ref(energy_path),analysisSource=ref(ROOT/'game/v6/analyze_0ac_forward_energy_20260913.py'),fullCausalAnalysis=ref(CAUSAL/'fullmatrix-causal-observations.json'),rawSummary=ref(FINAL/'branch/analysis.json'),strictCanonicalStatisticalAudit=ref(FINAL/'branch/strict-integrity-check.json'),sourceSaveIndex=ref(review_path),fullOriginalSaveReferences=review['finalSaves'],games=1000,playerObservations=2000,completeSelectedExamples=examples,limits=['An accepted Room order is a paid construction request, not proof that construction has finished or wiring is online at that tick.','Construction-complete exact time is populated only from a retained matching event, otherwise null. Final progress1 proves completion by the ending snapshot only.','First online time was not sampled by this runner and remains null. Final powered/online state is an actual ending observation.','The full source Save contains all recorded commands, final room/network inventory and retained events; no missing intermediate snapshot has been synthesized.'])
    write(CAUSAL/'facility-evidence-audit.json',facility_audit)
    print(json.dumps(dict(preregistered=str(OUT/'pre-registration.json'),caseCount=80,seeds=seeds,indices=prereg['indices'],candidateExecuted=False,facilityAudit=str(CAUSAL/'facility-evidence-audit.json')),ensure_ascii=True))


if __name__=='__main__':main()
