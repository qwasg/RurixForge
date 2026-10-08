"""All canonicalSCI200 pairs, with explicit self-play and missing T5 values."""
from pathlib import Path
import json,hashlib,statistics,collections,sys
ROOT=Path(__file__).resolve().parents[2];BASE=ROOT/'game/v6/health-refit-sci200-20260911'
def load(p):return json.loads(p.read_text(encoding='utf-8'))
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
plan=load(BASE/'case-plan.json');build=load(BASE/'runner-build-receipt.json');old={x['index']:x for x in load(BASE/'baseline-749-rows.json')['cases']}
rows=[];inputs=[]
for f in sorted(BASE.glob('results-*.jsonl')):
 rows.extend(json.loads(l)for l in f.read_text(encoding='utf-8').splitlines());inputs.append(dict(path=f.relative_to(ROOT).as_posix(),sha256=sha(f)))
expected={c['index']:c for c in plan['cases']};assert len(rows)==200 and {r['index']for r in rows}==set(expected)
for r in rows:
 assert all(r[k]==expected[r['index']][k]for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder','originalPair'])
 assert r['simulationSourceSha256']==build['rulesFingerprint'] and r['planSha256']==sha(BASE/'case-plan.json')
 assert sha(ROOT/old[r['index']]['sourceFile'])==old[r['index']]['sourceSha256']
def distribution(values):
 v=[x for x in values if x is not None];return dict(observations=len(values),available=len(v),missing=len(values)-len(v),median=statistics.median(v)if v else None,sum=sum(v)if v else None,min=min(v)if v else None,max=max(v)if v else None)
def observation(row,side):
 a=row['orderMetrics'];actors=a['aiActors'][side];gemini=[u for u in actors if u['kind']=='gemini'];paid=a['paidDeploymentsByClass'][side]
 return dict(outcome='unresolved' if row['winner'] is None else 'win' if row['winner']==side+1 else 'loss',seconds=row['seconds'],t5=row['firstT5Seconds'][side],ore=row['oreDelivered'][side],ownLoss=row['lostValue'][side],enemyLoss=row['lostValue'][1-side],energySpent=row['actualEnergySpent'][side],shots=row['shotsFired'][side],peakAi=a['peakAliveAi'][side],aiActors=len(actors),aiUpgrades=sum(x['upgrades']for x in actors),aiPlugins=sum(x['plugins']for x in actors),geminiShots=sum(x['shots']for x in gemini),geminiCasts=sum(x['activeSkills']for x in gemini),hardKillPurchases=sum(paid.get(k,0)for k in ['turret','vehicle','air','orbital']),energyRecharged=row.get('supplyMetrics',{}).get('energyRecharged',[None,None])[side],repairMaterialDelivered=row.get('supplyMetrics',{}).get('repairMaterialDeliveredToUnits',[None,None])[side],energyStarvedFiringUnitSeconds=row.get('supplyMetrics',{}).get('energyStarvedFiringUnitSeconds',[None,None])[side],delayedAreaHp=row.get('delayedAreaMetrics',{}).get('actualHpDamage',[None,None])[side])
def summarize(values):
 wins=sum(x['outcome']=='win' for x in values);losses=sum(x['outcome']=='loss'for x in values);own=sum(x['ownLoss']for x in values)
 return dict(observations=len(values),wins=wins,losses=losses,unresolved=len(values)-wins-losses,winRateAll=wins/len(values),assetExchangeRatioOfSums=sum(x['enemyLoss']for x in values)/own if own else None,metrics={k:distribution([x[k]for x in values])for k in values[0]if k!='outcome'})
groups={};cases=[];repair=[]
for r in sorted(rows,key=lambda r:r['index']):
 ref=old[r['index']]['row'];sciences=[]
 for side,b in enumerate(r['branches']):
  if b!='science':continue
  opponent=r['branches'][1-side];before=observation(ref,side);after=observation(r,side)
  names=['all-including-self',f'vs-{opponent}',f'side-{side+1}',f'theme-{r["theme"]}',f'seed-{r["seed"]}']+(['excluding-self',f'excluding-self-side-{side+1}',f'excluding-self-theme-{r["theme"]}'] if opponent!='science'else['self'])
  for name in names:
   g=groups.setdefault(name,{'baseline':[],'current':[]});g['baseline'].append(before);g['current'].append(after)
  sciences.append(dict(side=side+1,before=before,after=after))
 if '--include-repair-orders' in sys.argv:
  # Counts are accepted paid repair orders, not assumed completed healing.
  savefile=next(BASE.glob(f'diagnostics-*/match-{r["index"]}.save.json'));saved=load(savefile);actor_ids=[{a['id']for a in r['orderMetrics']['aiActors'][i]}for i in range(2)]
  counts=[sum(o['receipt']['accepted']and o['order']['owner']==i+1 and o['order']['command']['op']=='repair' and o['order']['command']['id']in actor_ids[i] for o in saved['orders'])for i in range(2)]
  repair.append(dict(index=r['index'],acceptedPaidAiRepairOrders=counts))
 cases.append(dict(index=r['index'],branches=r['branches'],seed=r['seed'],theme=r['theme'],oldWinner=ref['winner'],winner=r['winner'],seconds=r['seconds'],winReason=r['winReason'],science=sciences))
source=load(BASE/'source-before-build.json');live_changes=[f['path']for f in source['files']if sha(ROOT/f['path'])!=f['sha256']]
archive=ROOT/'game/v6/baselines/30e735-20260912';archived=load(archive/'manifest.json');assert all(sha(archive/f['baseline'])==f['sha256']for f in archived['files'])
report=dict(scope='All200 exact representative20-seed SCI scenarios; not full1000 branch/126-strategy, GPU or LAN acceptance.',integrityPassed=True,rulesVersion='v6.2',rulesFingerprint=build['rulesFingerprint'],runnerSha256=build['artifact']['sha256'],planSha256=sha(BASE/'case-plan.json'),liveSourceChangesAfterFreeze=live_changes,archivedCompiledSourceVerified=True,inputFiles=inputs,groups={n:{generation:summarize(v)for generation,v in values.items()}for n,values in groups.items()},repairOrderObservations=repair,cases=cases,warnings=[dict(index=r['index'],warnings=r['playabilityWarnings'])for r in rows if any(r['playabilityWarnings'].values())],finalBalanceAcceptance=False,limitations=['160 cross-branch observations and40 self-play matches. Do not mix self observations into cross-branch win rates.','Missing T5 remains null and its denominator is reported.','Old749 had no new charge/starvation/delayed-areaHP instrumentation; those entries remain null.','Paid repair order counts do not prove completion or quantify healed HP.','Changing outcomes and resources are associations from the entire correction bundle, not isolated treatment effect percentages.'])
with (BASE/'paired-analysis.json').open('x',encoding='utf-8')as f:json.dump(report,f,ensure_ascii=False,indent=2)
print(json.dumps(dict(games=200,crossBranch=report['groups']['excluding-self'],warnings=report['warnings'],sourceChanges=live_changes),ensure_ascii=True))
