"""Read frozen real receipts/Saves for causal diagnosis; no simulation or edits."""
from pathlib import Path
import collections
import datetime
import hashlib
import json
import statistics
import summarize_balance

ROOT=Path(__file__).resolve().parents[2]
BASE=ROOT/'game/v6/final-balance-0acffa83-20260913'
DATA=BASE/'branch'
OUT=ROOT/'game/v6/causal-review-0acffa83-20260913'
FP='0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a'
read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
ref=lambda p:dict(path=p.relative_to(ROOT).as_posix(),sha256=sha(p))


def write(path,value):
    with path.open('x',encoding='utf-8',newline='\n') as f:json.dump(value,f,ensure_ascii=False,indent=2);f.write('\n')


def main():
    OUT.mkdir(exist_ok=True)
    analysis=read(DATA/'analysis.json'); assert analysis['games']==1000 and analysis['integrityPassed']
    rows={}
    for item in analysis['inputFiles']:
        p=Path(item['path']); assert sha(p)==item['sha256']
        for line in p.read_text(encoding='utf-8').splitlines():
            if line.strip():
                r=json.loads(line);assert r['simulationSourceSha256']==FP and r['index'] not in rows;rows[r['index']]=r
    catalog_path=ROOT/'game/v6/lan-runs/final-fa31b360-concurrent-20260913-a/blue-native-catalog.json'
    catalog=read(catalog_path);assert catalog['rulesFingerprint']==FP
    defs={u['id']:u for u in catalog['units']}
    selected=[760,921,780,941]
    observations=[];selected_details=[];unresolved_tails=[]
    for i,r in sorted(rows.items()):
        p=DATA/f'diagnostics/match-{i}.save.json';s=read(p);state=s['snapshot']
        assert s['rulesFingerprint']==FP and state['tick']/60==r['seconds'] and state['winner']==r['winner']
        for owner in (1,2):
            slot=owner-1; units=[u for u in state['units'] if u['owner']==owner and u['hp']>0]
            vehicles=[u for u in units if defs[u['kind']]['category']=='vehicle']
            shops=[w for w in state['rooms'] if w['owner']==owner and w['kind']=='ammunition-workshop' and w['hp']>0]
            owned=next(p for p in state['players'] if p['owner']==owner)
            obs=dict(index=i,owner=owner,branch=r['branches'][slot],opponent=r['branches'][1-slot],seed=r['seed'],theme=r['theme'],plannerFirst=r['plannerOrder'][0]==owner,seconds=r['seconds'],won=r['winner']==owner,unresolved=r['winner'] is None,firstTierSeconds=r['firstTierSeconds'][slot],firstT5Seconds=r['firstT5Seconds'][slot],paidByKind=r['orderMetrics']['paidDeploymentsByKind'][slot],paidByClass=r['orderMetrics']['paidDeploymentsByClass'][slot],appliedDamageEventMagnitude=r['nativeTotals'][slot].get('damage-dealt',0),shotsFired=r['shotsFired'][slot],nativeTotals=r['nativeTotals'][slot],supply={k:v[slot] for k,v in r['supplyMetrics'].items() if isinstance(v,list)},geminiDelayedActualHpDamage=r['delayedAreaMetrics']['actualHpDamage'][slot],nativeLostValue=r['lostValue'][slot],enemyNativeLostValue=r['lostValue'][1-slot],oreDelivered=r['oreDelivered'][slot],nodeControlSeconds=r['nodeControlSeconds'][slot],finalCredits=owned['credits'],finalLivingVehicles=len(vehicles),finalAtOrAboveMixedVehicleThreshold=len(vehicles)>=10,finalLivingVehicleAmmoEmpty=sum(defs[u['kind']]['ammoPerShot']>0 and u['ammo']<defs[u['kind']]['ammoPerShot'] for u in vehicles),finalLivingVehicleEnergyEmpty=sum(defs[u['kind']]['energyPerAttack']>0 and u['energy']<defs[u['kind']]['energyPerAttack'] for u in vehicles),finalWorkshops=[{k:w[k] for k in ['id','rect','equipmentShare','powered','online','capacity','inventory','stock']} for w in shops],finalWorkshopsFull=sum(w['inventory']>=w['capacity']-1e-6 for w in shops),finalFullWorkshopWithoutAmmo=sum(w['inventory']>=w['capacity']-1e-6 and w['stock'].get('ammo',0)<1 for w in shops),finalSave=ref(p))
            observations.append(obs)
        if i in selected:
            accepted=[o for o in s['orders'] if o['receipt']['accepted']]
            detail=dict(index=i,row=r,finalSave=ref(p),owners=observations[-2:],earlyPaidDeployments=[dict(tick=o['tick'],seconds=o['tick']/60,owner=o['order']['owner'],kind=o['order']['command']['kind']) for o in accepted if o['tick']<=720*60 and o['order']['command']['op']=='deploy'],workshopOrders=[o for o in accepted if o['order']['command']['op']=='room' and o['order']['command'].get('kind')=='ammunition-workshop'],researchOrders=[o for o in accepted if o['order']['command']['op']=='research'],finalLivingUnits=units_for_all(state,defs),finalWorkshopRooms=[w for w in state['rooms'] if w['kind']=='ammunition-workshop'],eventTail=event_tail(state),lastAcceptedOrders=accepted[-20:])
            selected_details.append(detail)
        if r['winner'] is None:unresolved_tails.append(dict(index=i,finalSave=ref(p),tail=event_tail(state),lastAcceptedOrders=[o for o in s['orders'] if o['receipt']['accepted']][-15:]))
        if (i+1)%200==0:print(json.dumps(dict(stage='read-only-causal-save-scan',cases=i+1)),flush=True)
    cross=[o for o in observations if o['branch']!=o['opponent']]
    groups={b:describe([o for o in cross if o['branch']==b]) for b in summarize_balance.BRANCHES}
    paired={str(seed):dict(science=describe([o for o in cross if o['seed']==seed and o['branch']=='science' and o['opponent']=='lightweight']),lightweight=describe([o for o in cross if o['seed']==seed and o['branch']=='lightweight' and o['opponent']=='science']),cases=[dict(index=r['index'],branches=r['branches'],plannerOrder=r['plannerOrder'],winner=r['winner'],seconds=r['seconds']) for r in rows.values() if r['seed']==seed and set(r['branches'])=={'science','lightweight'}]) for seed in range(1000,1020)}
    result=dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),scope='Read-only analysis of exact frozen1000 receipts and final Saves. No new native run or rule modification; retrospective final state is not a full time series.',rulesFingerprint=FP,sourceAnalysis=ref(DATA/'analysis.json'),catalog=ref(catalog_path),branchesExcludingSelf=groups,scienceVsLightweight={b:describe([o for o in cross if o['branch']==b and o['opponent'] in {'science','lightweight'}]) for b in ['science','lightweight']},twentySeedScienceVsLight=paired,selectedCases=selected,selectionRule='Earliest seed where science wins all4 canonical configurations is1000; earliest where lightweight wins all4 is1010. For each, inspect science-owner1/lightweight-owner2 at both planner orders:760/921 and780/941. Overall80 cases retain both spawn assignments.',observations=observations,limitations=['damage-dealt is a sum of post-defense applied event magnitudes and can include overkill; it is not a strict HP delta or per-weapon damage attribution.','Raw receipts have no per-weapon shot/hit denominator, so a beam hit-rate advantage cannot be quantified from these rows alone.','Final living vehicle count>=10 observes the bot threshold at the ending instant only. It does not measure time spent capped, and available credits alone do not prove the bot research/AI reservation permits another purchase.','Final full workshop stock without ammo is an ending-state congestion observation, not proof of how long production was blocked.','Nominal catalogue costs/HP/DPS are rule inputs, not equal-budget realized battle output. Actual matched battle observations combine army, AI skills, defenses, routes, research and objectives.'],finalBalanceAcceptance=False)
    write(OUT/'fullmatrix-causal-observations.json',result);write(OUT/'selected-four-save-details.json',dict(selectionRule=result['selectionRule'],cases=selected_details));write(OUT/'unresolved-late-events.json',dict(scope='Retained final event-ring observations only; no complete-period or exact-replay claim.',cases=unresolved_tails))
    print(json.dumps(dict(report=str(OUT/'fullmatrix-causal-observations.json'),groups=groups,scienceVsLightweight=result['scienceVsLightweight'],seedScienceWins={k:v['science']['wins'] for k,v in paired.items()},selectedCases=selected),ensure_ascii=True),flush=True)


def units_for_all(state,defs):
    return [dict(id=u['id'],owner=u['owner'],kind=u['kind'],category=defs[u['kind']]['category'],tier=u['tier'],hp=u['hp'],maxHp=u['maxHp'],ammo=u['ammo'],ammoMax=u['ammoMax'],energy=u['energy'],energyMax=u['energyMax'],pos=u['pos'],moving=u['moving'],lastAttackTick=u['lastAttackTick']) for u in state['units'] if u['hp']>0]


def event_tail(state):
    events=state['events'];cutoff=state['tick']-60*60;recent=[e for e in events if e['tick']>=cutoff]
    return dict(finalTick=state['tick'],earliestRetainedTick=min((e['tick'] for e in events),default=None),latestRetainedTick=max((e['tick'] for e in events),default=None),lastMinuteRetainedEventCounts=dict(collections.Counter(e['kind'] for e in recent)),lastMinuteRetainedByOwner={str(owner):dict(collections.Counter(e['kind'] for e in recent if e['owner']==owner)) for owner in (1,2)},last20Events=events[-20:])


def describe(values):
    stat=summarize_balance.stats
    return dict(observations=len(values),wins=sum(o['won'] for o in values),unresolved=sum(o['unresolved'] for o in values),firstTierSeconds=[stat(o['firstTierSeconds'][tier] for o in values) for tier in range(5)],damageEventMagnitude=stat(o['appliedDamageEventMagnitude'] for o in values),shotsFired=stat(o['shotsFired'] for o in values),nativeLostValueExchange=sum(o['enemyNativeLostValue'] for o in values)/sum(o['nativeLostValue'] for o in values),metrics={k:stat(o[k] for o in values) for k in ['seconds','oreDelivered','nodeControlSeconds','finalCredits','finalLivingVehicles','finalLivingVehicleAmmoEmpty','finalLivingVehicleEnergyEmpty','geminiDelayedActualHpDamage']},supply={k:stat(o['supply'][k] for o in values) for k in values[0]['supply']},vehiclePurchases=stat(o['paidByClass'].get('vehicle',0) for o in values),paidDeploymentsByKind=dict(sum((collections.Counter(o['paidByKind']) for o in values),collections.Counter())),finalVehicleThresholdReached=sum(o['finalAtOrAboveMixedVehicleThreshold'] for o in values),finalWorkshopCounts=dict(collections.Counter(len(o['finalWorkshops']) for o in values)),finalWorkshopFullObservations=sum(o['finalWorkshopsFull']>0 for o in values),finalFullWorkshopWithoutAmmoObservations=sum(o['finalFullWorkshopWithoutAmmo']>0 for o in values))


if __name__=='__main__':main()
