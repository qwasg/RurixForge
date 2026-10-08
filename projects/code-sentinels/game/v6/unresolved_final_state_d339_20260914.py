"""Describe actual unresolved d339 final Saves and independently recount strata.

No simulation, replay, forced winner or late-activity inference from cumulative
counters. Refuses until the complete1000 branch review and strict audit exist.
"""
from pathlib import Path
from collections import Counter
import json
import sys

sys.dont_write_bytecode = True
import finalize_full_6828_stats_20260914 as io
import release_gate
import summarize_balance as stats

ROOT = io.ROOT
BASE = ROOT / 'game/v6/final-balance-d33999c4-20260914'
OUT = BASE / 'branch'
FP = 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'
read, ref, verify, require = io.read, io.ref, io.verify, io.require


def counts(entries):
    wins = sum(row['winner'] == owner + 1 for row,owner in entries)
    unresolved = sum(row['winner'] is None for row,owner in entries)
    return dict(playerObservations=len(entries), wins=wins, losses=len(entries)-wins-unresolved, unresolved=unresolved)


def windows(events, final_tick):
    earliest = min((event['tick'] for event in events), default=None)
    latest = max((event['tick'] for event in events), default=None)
    result = dict(retainedCount=len(events), earliestRetainedTick=earliest, latestRetainedTick=latest, windows={})
    for seconds in [60,300]:
        start = max(0, final_tick-seconds*60)
        observed = [event for event in events if start < event['tick'] <= final_tick]
        result['windows'][str(seconds)+'seconds'] = dict(startTickExclusive=start, endTickInclusive=final_tick,
            earliestRetainedAtOrBeforeWindowStart=earliest <= start if earliest is not None else None,
            observedEventCount=len(observed), byOwner={str(owner):dict(Counter(event['kind'] for event in observed if event['owner']==owner)) for owner in [0,1,2]})
    return result


def symptoms(row, saved, definitions, save_ref):
    snapshot=saved['snapshot'];final_tick=snapshot['tick'];events=snapshot['events'];orders=saved['orders']
    entities={entity['id']:entity for name in ['units','buildings','rooms','walls','links','shipments','resources'] for entity in snapshot.get(name,[])}
    players=[]
    for owner in [1,2]:
        player=next(p for p in snapshot['players'] if p['owner']==owner)
        own_orders=[o for o in orders if o['order']['owner']==owner]
        accepted=[o for o in own_orders if o['receipt']['accepted']]
        units=[];unmapped=[]
        for unit in snapshot['units']:
            if unit['owner']!=owner or unit['hp']<=0:continue
            definition=definitions.get(unit['kind'])
            if definition is None:
                unmapped.append(unit);continue
            if definition['category'] not in ['ai','vehicle','turret','air','orbital'] or definition['damage']<=0:continue
            value={key:unit.get(key) for key in ['id','kind','owner','pos','hp','maxHp','tier','ammo','ammoMax','energy','energyMax','fuel','fuelMax','battery','batteryMax','covered','wired','moving','goal','route','target','attackCount','lastAttackTick','lastCastTick','statuses','plugins']}
            value['category']=definition['category']
            value['baseWeaponStoresPerAttack']={key:definition.get(key) for key in ['ammoPerShot','energyPerAttack','computePerAttack']}
            target=entities.get(unit.get('target'))
            value['referencedTargetPresentInFinalSave']=target is not None if unit.get('target') is not None else None
            value['referencedTarget']={key:target.get(key) for key in ['id','owner','kind','hp','pos','rect']} if target else None
            units.append(value)
        core=next((b for b in snapshot['buildings'] if b['owner']==owner and b['kind']=='core'),None)
        supply_rooms=[{key:room.get(key) for key in ['id','kind','rect','hp','progress','powered','connected','online','equipmentShare','inventory','stock']} for room in snapshot['rooms'] if room['owner']==owner and room['kind'] in ['ammunition-workshop','depot']]
        shipments=[shipment for shipment in snapshot['shipments'] if shipment['owner']==owner and shipment['hp']>0]
        players.append(dict(owner=owner,branch=row['branches'][owner-1],credits=player['credits'],branchTiers=player['branches'],dominance=player['dominance'],
                            totalOreDelivered=player.get('totals',{}).get('ore-delivered'),nativeTotals=player.get('totals'),
                            cumulativeRaw=dict(firstCombatSeconds=row['firstCombatSeconds'][owner-1],firstDamageSeconds=row['firstDamageSeconds'][owner-1],
                                               oreDelivered=row['oreDelivered'][owner-1],nodeControlSeconds=row['nodeControlSeconds'][owner-1],lostValue=row['lostValue'][owner-1]),
                            core={key:core.get(key) for key in ['id','hp','maxHp','rect']} if core else None,
                            survivingCombatUnits=units,unmappedSurvivingUnits=unmapped,
                            lastOrderTick=max((o['tick'] for o in own_orders),default=None),lastAcceptedOrderTick=max((o['tick'] for o in accepted),default=None),
                            lastAcceptedOrders=accepted[-12:],lastRejectedOrders=[o for o in own_orders if not o['receipt']['accepted']][-5:],
                            localWorkshopsAndDepots=supply_rooms,activeSupplyShipments=shipments))
    return dict(index=row['index'],seed=row['seed'],theme=row['theme'],branches=row['branches'],plannerOrder=row['plannerOrder'],
                seconds=row['seconds'],winner=row['winner'],winReason=row['winReason'],finalSave=save_ref,
                retainedEventEvidence=windows(events,final_tick),players=players,
                nodes=[node for node in snapshot['resources'] if node['kind']=='node'])


def main():
    review_path=OUT/'branch-review-data.json';destination=OUT/'unresolved-final-state.json'
    require(review_path.is_file() and not destination.exists(),'Complete review required; existing derivative is preserved')
    review=read(review_path);strict_path=OUT/'strict-integrity-check.json';strict=read(strict_path)
    require(review['games']==1000 and review['integrityPassed'] and review['rulesFingerprint']==FP
            and strict['games']==1000 and strict['passed'] and strict['rulesFingerprint']==FP,'Incomplete or wrong matrix identity')
    require(verify(review['analysis'])==verify(strict['analysis']),'Analysis references differ')
    analysis=read(verify(strict['analysis']));rows=[]
    for item in analysis['inputFiles']:
        path=verify(item);rows.extend(json.loads(line) for line in path.read_text(encoding='utf-8-sig').splitlines() if line.strip())
    target=dict(rulesVersion='v6.2',rulesFingerprint=FP)
    release_gate.matrix_check(rows,target)
    release_gate.verify_analysis(analysis,rows,[item['sha256'] for item in analysis['inputFiles']],target,'d339 independent unresolved/strata')
    host_path=ROOT/'Logs/v6/native-room-d33999c4-20260914-a-host-identity/host-identity.json';host=read(host_path)
    require(host['rulesFingerprint']==FP,'Unit catalogue host identity differs')
    catalog_path=verify(host['catalog']);definitions={d['id']:d for d in read(catalog_path)['units']}
    save_refs={(ROOT/item['path']).resolve():item for item in strict['finalSaves']}
    unresolved=[]
    for row in sorted(rows,key=lambda r:r['index']):
        if row['winner'] is not None:continue
        require(row['seconds']==3300,'Unresolved match is not its real55-minute budget')
        path=OUT/f'diagnostics/match-{row["index"]}.save.json';reference=save_refs[path];verify(reference);saved=read(path)
        require(saved['rulesFingerprint']==FP and saved['snapshot']['winner'] is None
                and saved['snapshot']['tick']/60==row['seconds'] and saved['snapshot']['seed']==row['seed']
                and saved['snapshot']['theme']==row['theme'] and saved['initialAi'] is False and saved['administrativeEvents']==[], 'Unresolved Save identity differs')
        unresolved.append(symptoms(row,saved,definitions,reference))
    cross=[(row,owner) for row in rows if row['branches'][0]!=row['branches'][1] for owner in [0,1]]
    branch_facts={}
    for branch in stats.BRANCHES:
        entries=[(row,owner) for row,owner in cross if row['branches'][owner]==branch]
        branch_facts[branch]=dict(**counts(entries),cumulativeMetrics=stats.describe(entries),
            byOpponent={other:counts([(r,o) for r,o in entries if r['branches'][1-o]==other]) for other in stats.BRANCHES if other!=branch},
            byActualOwner={str(owner+1):counts([(r,o) for r,o in entries if o==owner]) for owner in [0,1]},
            byPlannerPosition={position:counts([(r,o) for r,o in entries if (r['plannerOrder'][0]==o+1)==(position=='first')]) for position in ['first','second']},
            bySeed={str(seed):counts([(r,o) for r,o in entries if r['seed']==seed]) for seed in range(1000,1020)})
        require(len(entries)==320 and all(v['playerObservations']==80 for v in branch_facts[branch]['byOpponent'].values())
                and all(v['playerObservations']==160 for v in branch_facts[branch]['byActualOwner'].values())
                and all(v['playerObservations']==160 for v in branch_facts[branch]['byPlannerPosition'].values())
                and all(v['playerObservations']==16 for v in branch_facts[branch]['bySeed'].values()),'Unexpected branch strata denominators')
    report=dict(recordedAtUtc=io.now(),scope='Read-only actual final-Save symptoms for unresolved d339 cases and independent count/statistical strata checks. Not a new simulation or balance decision.',
                integrityPassed=True,finalBalanceAcceptance=False,matrixGames=1000,completed=analysis['completed'],unresolved=len(unresolved),
                unresolvedIndices=[case['index'] for case in unresolved],unresolvedCases=unresolved,
                sourceIdentity=dict(review=ref(review_path),strictAudit=ref(strict_path),analysis=ref(verify(strict['analysis'])),catalogue=ref(catalog_path),helper=ref(Path(__file__))),
                independentStrata=dict(crossGames=len(cross)//2,selfGames=1000-len(cross)//2,branches=branch_facts),
                durationSeconds=analysis['durationSeconds'],completedIn30To45Minutes=analysis['completedIn30To45Minutes'],victoryReasons=analysis['victoryReasons'],
                limitations=['Retained final event rings may omit older events. Window counts describe observed emitted events only; earliestRetainedAtOrBeforeWindowStart reports coverage of that boundary, not proof all physical activity is instrumented.',
                             'Zero retained fire/damage events cannot prove no historical combat when the window start is not covered. Cumulative firstCombat, firstDamage, damage, ore and node-control counters never establish late activity.',
                             'No replay, native query, LOS/range/weapon-payment recomputation or counterfactual outcome is performed. Base weapon store costs omit runtime modifiers and are explanatory catalogue values only.',
                             'Missing fields remain null. Final targets, ammunition, local stocks and shipments are instantaneous observations, not inferred causal failures. Unresolved cases keep winner null and the actual55-minute budget.',
                             'Strata counts/metrics are evidence for manual criteria review. Correlated swaps/seeds are not independent random samples; no universal50-percent target or automatic balance acceptance is created.'])
    io.write_new(destination,report)
    print(json.dumps(dict(report=str(destination),unresolvedIndices=report['unresolvedIndices'],completed=analysis['completed'],finalBalanceAcceptance=False)),flush=True)


if __name__=='__main__':main()
