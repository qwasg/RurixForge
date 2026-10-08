"""Disambiguate paid production and firing evidence at match-level denominators."""
from pathlib import Path
import datetime,hashlib,json
ROOT=Path(__file__).resolve().parents[2]
BASE=ROOT/'game/v6/final-balance-6828c5b7-20260914/strategy/early-review'
source=BASE/'participation-and-supply.json'
data=json.loads(source.read_text(encoding='utf-8'))
assert data['integrityPassed']and data['caseCount']==126 and len(data['pairedObservations'])==252
report=dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),scope='Derived from the verified full126 paired participation report. Counts are distinct games containing a policy; self-play is one game with two policy owners, not two games.',source={'path':source.relative_to(ROOT).as_posix(),'sha256':hashlib.sha256(source.read_bytes()).hexdigest()},policies={},notes=['Full126 gives36 distinct games perpolicy and42 player observations; excluding self gives30games/30player observations.','paidButNoFiringConfirmedGames means a paid Deploy occurred but available surviving counters/retained events do not confirm any firing. It is unknown actual firing, not zero.','gamesWithAnyPurchasedActorUnconfirmed may overlap confirmedFiringGames when one actor fired and another actor lacks positive evidence.','AI attack-cycle counts are not actual HP damage; the changed hit realization prevents inferring HP-output decline from fewer shots.'],finalBalanceAcceptance=False)
for policy in data['policyGroups']:
    policy_out={}
    for scope,include_self in [('includingSelfDistinctGames',True),('excludingSelf',False)]:
        pairs=[p for p in data['pairedObservations']if p['policy']==policy and(include_self or p['policy']!=p['opponentPolicy'])]
        indices=sorted({p['index'] for p in pairs});assert len(indices)==(36 if include_self else 30)
        groups={generation:dict(games=len(indices),playerObservations=len(pairs),classes={})for generation in ['baseline','current']}
        for generation in groups:
            for category in ['air','orbital']:
                cases=[]
                for index in indices:
                    owners=[p[generation]for p in pairs if p['index']==index]
                    classes=[p['classes'][category]for p in owners]
                    paid=sum(c['paidDeployments']for c in classes)
                    confirmed=any((c['confirmedFiringActorCountLowerBound'] or 0)>0 for c in classes)
                    partial=any(c['purchasedActorsWithoutPositiveFiringEvidence']>0 for c in classes)
                    assert not confirmed or paid>0
                    cases.append(dict(index=index,paidDeployments=paid,confirmedFiring=confirmed,actualFiringUnknownDespitePaid=paid>0 and not confirmed,anyPurchasedActorUnconfirmed=partial))
                groups[generation]['classes'][category]=dict(atLeastOnePaidDeployGames=sum(c['paidDeployments']>0 for c in cases),noPaidDeployGames=sum(c['paidDeployments']==0 for c in cases),confirmedFiringGames=sum(c['confirmedFiring']for c in cases),paidButNoFiringConfirmedGames=sum(c['actualFiringUnknownDespitePaid']for c in cases),gamesWithAnyPurchasedActorUnconfirmed=sum(c['anyPurchasedActorUnconfirmed']for c in cases),paidDeploymentTotal=sum(c['paidDeployments']for c in cases),cases=cases)
        policy_out[scope]=groups
    report['policies'][policy]=policy_out
target=BASE/'air-orbit-production-participation.json'
with target.open('x',encoding='utf-8')as f:json.dump(report,f,ensure_ascii=False,indent=2)
print(json.dumps({p:{g:{c:{k:v for k,v in counts.items()if k!='cases'}for c,counts in group['classes'].items()}for g,group in scopes['includingSelfDistinctGames'].items()}for p,scopes in report['policies'].items()},ensure_ascii=True))
