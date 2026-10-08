"""Complete F3 strategy comparison. Keep any-branch versus primary T5 distinct."""
from pathlib import Path
import json,hashlib,statistics,collections
ROOT=Path(__file__).resolve().parents[2];BASE=ROOT/'game/v6/strategy-wire-air126-f3f576-20260912'
def load(p):return json.loads(p.read_text(encoding='utf-8'))
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
plan=load(ROOT/'game/v6/strategy-plan.json');build=load(BASE/'runner-build-receipt.json');expected={c['index']:c for c in plan['cases']}
rows=[];inputs=[]
for f in sorted(BASE.glob('results-*.jsonl')):
 rows.extend(json.loads(l)for l in f.read_text(encoding='utf-8').splitlines());inputs.append(dict(path=f.relative_to(ROOT).as_posix(),sha256=sha(f)))
assert len(rows)==126 and {r['index']for r in rows}==set(expected)
old={};old_files=[]
for f in sorted((ROOT/'game/v6').glob('strategy-749391-*.jsonl')):
 for line in f.read_text(encoding='utf-8').splitlines():
  r=json.loads(line);assert r['index']not in old;old[r['index']]=r
 old_files.append(dict(path=f.relative_to(ROOT).as_posix(),sha256=sha(f)))
assert len(old)==126
def dist(v):
 p=[x for x in v if x is not None];return dict(observations=len(v),available=len(p),null=len(v)-len(p),median=statistics.median(p)if p else None,min=min(p)if p else None,max=max(p)if p else None,mean=statistics.mean(p)if p else None)
def obs(r,side):
 a=r['orderMetrics'];return dict(result='unresolved'if r['winner']is None else'win'if r['winner']==side+1 else'loss',seconds=r['seconds'],firstAnyBranchT5=r['firstT5Seconds'][side],secondAnyBranchT5=r['secondT5Seconds'][side],ore=r['oreDelivered'][side],nodeControlSeconds=r['nodeControlSeconds'][side],ownAssetLoss=r['lostValue'][side],enemyAssetLoss=r['lostValue'][1-side],peakAi=a['peakAliveAi'][side],aiUpgrades=sum(x['upgrades']for x in a['aiActors'][side]),aiPlugins=sum(x['plugins']for x in a['aiActors'][side]),airPurchases=a['paidDeploymentsByClass'][side].get('air',0),orbitalPurchases=a['paidDeploymentsByClass'][side].get('orbital',0))
def summary(values):
 own=sum(x['ownAssetLoss']for x in values);return dict(observations=len(values),wins=sum(x['result']=='win'for x in values),losses=sum(x['result']=='loss'for x in values),unresolved=sum(x['result']=='unresolved'for x in values),assetExchangeRatioOfSums=sum(x['enemyAssetLoss']for x in values)/own if own else None,metrics={k:dist([x[k]for x in values])for k in values[0]if k!='result'})
groups={};primary=[];pairs=[]
for r in sorted(rows,key=lambda r:r['index']):
 case=expected[r['index']];reference=old[r['index']]
 assert all(r[k]==case[k]for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder'])
 assert all(r[k]==reference[k]for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder'])
 assert r['planSha256']==build['planSha256'] and r['simulationSourceSha256']==build['rulesFingerprint']
 saved=load(next(BASE.glob(f'diagnostics-*/match-{r["index"]}.save.json')))
 assert saved['rulesFingerprint']==r['simulationSourceSha256']
 for side,style in enumerate(r['strategies']):
  before=obs(reference,side);after=obs(r,side);other=r['strategies'][1-side]
  for key in [f'{style}/including-self',f'{style}/vs/{other}']+([f'{style}/excluding-self']if style!=other else[f'{style}/self']):
   g=groups.setdefault(key,dict(baseline=[],current=[]));g['baseline'].append(before);g['current'].append(after)
  preferred=r['branches'][side];player=next(p for p in saved['snapshot']['players']if p['owner']==side+1);tier=player['branches'].get(preferred,0);first=r['firstT5Seconds'][side];second=r['secondT5Seconds'][side]
  # A branch needs at least four accepted Research requests to reach T5 from
  # the ordinary initialT1. Cancellations can only increase that count, so a
  # unique eligible branch identifies the first T5 without guessing pauses.
  counts=collections.Counter(o['order']['command']['branch']for o in saved['orders']if o['receipt']['accepted']and o['order']['owner']==side+1 and o['order']['command']['op']=='research'and first is not None and o['tick']<=first*60)
  eligible=[b for b,n in counts.items()if n>=4];first_branch=eligible[0]if len(eligible)==1 else None
  time=None;reason='not-reached'if tier<5 else'branch-time-not-directly-instrumented'
  if tier>=5 and first_branch==preferred:time=first;reason='unique-possible-branch-at-first-recorded-T5'
  elif tier>=5 and first_branch is not None and first_branch!=preferred and second is not None and sum(v>=5 for v in player['branches'].values())==2:time=second;reason='other-first-and-exactly-two-final-T5-branches'
  primary.append(dict(index=r['index'],side=side+1,strategy=style,opponent=other,primaryBranch=preferred,finalPrimaryTier=tier,primaryT5Seconds=time,primaryTimeStatus=reason,firstAnyBranchT5=first,secondAnyBranchT5=second))
 pairs.append(dict(index=r['index'],strategies=r['strategies'],branches=r['branches'],seed=r['seed'],oldWinner=reference['winner'],winner=r['winner'],seconds=r['seconds'],winReason=r['winReason']))
primary_groups={}
for style in ['expansion','maintech','multitech','mech','mixed-ai','turtle']:
 for self_mode in [True,False]:
  values=[r for r in primary if r['strategy']==style and(self_mode or r['strategy']!=r['opponent'])]
  primary_groups[f'{style}/{"including-self"if self_mode else"excluding-self"}']=dict(observations=len(values),primaryReached=sum(v['finalPrimaryTier']>=5 for v in values),primaryNotReached=sum(v['finalPrimaryTier']<5 for v in values),primaryTimeUnavailableDespiteReaching=sum(v['finalPrimaryTier']>=5 and v['primaryT5Seconds']is None for v in values),knownPrimaryTimes=dist([v['primaryT5Seconds']for v in values]),firstAnyBranchTimes=dist([v['firstAnyBranchT5']for v in values]),secondAnyBranchTimes=dist([v['secondAnyBranchT5']for v in values]))
report=dict(scope='All126 approved scenarios at F3. No full1000 or GPU/LAN acceptance implied.',integrityPassed=True,planSha256=build['planSha256'],rulesFingerprint=build['rulesFingerprint'],runnerSha256=build['artifact']['sha256'],inputFiles=inputs,baselineFiles=old_files,groups={k:{generation:summary(v)for generation,v in values.items()}for k,values in groups.items()},primaryTechnology=primary_groups,primaryObservations=primary,cases=pairs,duration=dist([r['seconds']for r in rows]),in30To45Minutes=sum(1800<=r['seconds']<=2700 for r in rows),warnings=[dict(index=r['index'],warnings=r['playabilityWarnings'])for r in rows if any(r['playabilityWarnings'].values())],finalBalanceAcceptance=False,limitations=['The existing native runner directly samples first and second ANY-branch T5, not the preferred branch. Primary times are attributed only where ordinary-order necessary conditions make the identity unambiguous; otherwise unavailable is explicit.','Null/unresolved cases remain in denominators.','Old749 baseline is retained verbatim and no missing telemetry is invented.','These126 scenarios cover six defined policies on three seeds/themes, not every possible player strategy.'])
with(BASE/'paired-strategy-analysis.json').open('x',encoding='utf-8')as f:json.dump(report,f,ensure_ascii=False,indent=2)
print(json.dumps(dict(games=126,strategies={k:{g:{x:v[x]for x in ['observations','wins','losses','unresolved']}for g,v in values.items()}for k,values in report['groups'].items()if k.endswith('/excluding-self')},primary=primary_groups,duration=report['duration'],warnings=report['warnings']),ensure_ascii=True))
