"""Read-only evidence derivation for 6828/d339 strategy participation and supply.

Creates only a new participation-and-supply.json under strategy/early-review;
no native execution, simulation, compilation, gate change or source mutation.
"""
from pathlib import Path
from collections import Counter
import json
import sys

sys.dont_write_bytecode = True
import finalize_full_6828_stats_20260914 as support
import summarize_balance as stats
from summarize_direct_range_probe80_20260913 import paired_values
from finalize_full_d339_stats_20260914 import verified_dispatch

ROOT = support.ROOT
BASE = ROOT / 'game/v6/final-balance-d33999c4-20260914'
OLD = ROOT / 'game/v6/final-balance-6828c5b7-20260914/strategy'
CURRENT = BASE / 'strategy'
OUT = CURRENT
read, sha, ref, verify, require = support.read, support.sha, support.ref, support.verify, support.require
FPS = {'baseline': '6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781',
       'current': 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'}
CLOCK_BINS = ['under18m', '18toUnder24m', '24toUnder30m', '30to45mInclusive', 'over45m']


def clock_bin(seconds):
    return CLOCK_BINS[0 if seconds < 1080 else 1 if seconds < 1440 else 2 if seconds < 1800 else 3 if seconds <= 2700 else 4]


def load_rows(analysis):
    rows, refs = {}, []
    for item in analysis['inputFiles']:
        path = verify(item); refs.append(ref(path))
        for line in path.read_text(encoding='utf-8-sig').splitlines():
            if not line.strip(): continue
            row = json.loads(line)
            require(row['index'] not in rows, 'Duplicate strategy index')
            rows[row['index']] = row
    require(set(rows) == set(range(126)), 'Exactly126 strategy cases are required')
    return rows, refs


def actor_evidence(row, owner, saved, categories):
    """Union actor IDs; max counts across overlapping lifetime/ring evidence."""
    records, unknown = {}, []

    def actor(actor_id, kind):
        key = (owner + 1, actor_id)
        value = records.setdefault(key, dict(id=actor_id, owner=owner + 1, kind=kind,
                                             category=categories[kind], survivingAttackCount=None,
                                             retainedFireEvents=0, rawAiLifetimeShots=None))
        require(value['kind'] == kind, 'Same actor ID has conflicting catalogue kind')
        return value

    for unit in saved['snapshot']['units']:
        if unit['owner'] == owner + 1 and unit['hp'] > 0 and unit['kind'] in categories:
            if categories[unit['kind']] in ['ai', 'air', 'orbital']:
                actor(unit['id'], unit['kind'])['survivingAttackCount'] = unit['attackCount']
    seen_events = set()
    for event in saved['snapshot']['events']:
        if event['kind'] != 'fire' or event['owner'] != owner + 1:
            continue
        require(event['id'] not in seen_events, 'Duplicate retained fire event ID')
        seen_events.add(event['id'])
        kind = event.get('subjectKind', '')
        if kind not in categories:
            unknown.append(dict(eventId=event['id'], actorId=event['subject'], kind=kind, tick=event['tick']))
            continue
        if categories[kind] in ['ai', 'air', 'orbital']:
            actor(event['subject'], kind)['retainedFireEvents'] += 1
    for raw_actor in row['orderMetrics']['aiActors'][owner]:
        require(categories[raw_actor['kind']] == 'ai', 'Raw AI actor category mismatch')
        actor(raw_actor['id'], raw_actor['kind'])['rawAiLifetimeShots'] = raw_actor['shots']
    classes = {}
    for category in ['ai', 'air', 'orbital']:
        paid = row['orderMetrics']['paidDeploymentsByClass'][owner].get(category, 0)
        values = [a for a in records.values() if a['category'] == category]
        for value in values:
            value['confirmedAttackCyclesLowerBound'] = max(value['survivingAttackCount'] or 0,
                                                          value['retainedFireEvents'],
                                                          value['rawAiLifetimeShots'] or 0)
        firing = [a for a in values if a['confirmedAttackCyclesLowerBound'] > 0]
        require(len(firing) <= paid, 'More confirmed actor IDs than paid deployments')
        status = 'confirmed-firing' if firing else 'purchased-firing-unconfirmed' if paid else 'not-purchased'
        unavailable = paid > 0 and not firing
        classes[category] = dict(paidDeployments=paid, firstPaidSeconds=row['orderMetrics']['firstPaidClassSeconds'][owner].get(category), status=status,
                                 confirmedFiringActorCountLowerBound=None if unavailable else len(firing),
                                 confirmedAttackCyclesLowerBound=None if unavailable else sum(a['confirmedAttackCyclesLowerBound'] for a in firing),
                                 purchasedActorsWithoutPositiveFiringEvidence=paid-len(firing),
                                 confirmedActors=sorted(firing, key=lambda a: a['id']))
    return classes, unknown


def player_observation(row, owner, saved, categories, save_ref):
    order = row['orderMetrics']; actors = order['aiActors'][owner]
    require(len({a['id'] for a in actors}) == len(actors), 'Duplicate lifetime AI actor IDs')
    active = lambda a: a['shots'] > 0 or a['activeSkills'] > 0 or a['passiveUses'] > 0
    require(order['activeAiCount'][owner] == sum(active(a) for a in actors), 'Raw AI participation counter mismatch')
    paid_from_orders = Counter(e['order']['command']['kind'] for e in saved['orders']
                              if e['receipt']['accepted'] and e['order']['owner'] == owner + 1
                              and e['order']['command']['op'] == 'deploy')
    require(dict(paid_from_orders) == order['paidDeploymentsByKind'][owner], 'Paid composition differs from Save orders')
    paid_classes = Counter()
    for kind, count in paid_from_orders.items():
        paid_classes[categories[kind]] += count
    require(dict(paid_classes) == order['paidDeploymentsByClass'][owner], 'Exported category mapping differs from raw paid classes')
    classes, unknown = actor_evidence(row, owner, saved, categories)
    metrics = {name: row[name][owner] for name in ['oreDelivered', 'nodeControlSeconds', 'lostValue',
               'actualAmmoSpent', 'actualComputeSpent', 'actualEnergySpent', 'recoverableCargoWreckAmount', 'credits']}
    for name, value in row['supplyMetrics'].items():
        if isinstance(value, list): metrics['supply.' + name] = value[owner]
    for name in ['damage-dealt', 'transport-spent', 'compute-maintenance']:
        metrics['nativeTotals.' + name] = row['nativeTotals'][owner].get(name, 0.)
    metrics['geminiActiveActualHpDamage'] = row['delayedAreaMetrics']['actualHpDamage'][owner]
    metrics['shieldInterceptedPayload'] = row['defenseMetrics']['interceptedPayload'][owner]
    ai = dict(recordedLifetimeActors=len(actors), firingActors=sum(a['shots'] > 0 for a in actors),
              skillActors=sum(a['activeSkills'] > 0 for a in actors), passiveActors=sum(a['passiveUses'] > 0 for a in actors),
              activeActors=sum(active(a) for a in actors), peakAliveAi=order['peakAliveAi'][owner],
              **{key: sum(a[key] for a in actors) for key in ['shots', 'activeSkills', 'passiveUses', 'computeSpent', 'upgrades', 'plugins']})
    winner_player = next((p for p in saved['snapshot']['players'] if p['owner'] == row['winner']), None)
    own_player = next(p for p in saved['snapshot']['players'] if p['owner'] == owner + 1)
    events = saved['snapshot']['events']
    return dict(index=row['index'], owner=owner + 1, policy=row['strategies'][owner],
                opponentPolicy=row['strategies'][1-owner], branch=row['branches'][owner],
                seconds=row['seconds'], clockBin=clock_bin(row['seconds']), winReason=row['winReason'],
                result='unresolved' if row['winner'] is None else 'win' if row['winner'] == owner + 1 else 'loss',
                winnerFinalMaximumResearchedBranchTier=max(winner_player['branches'].values(), default=0) if winner_player else None,
                ownFinalMaximumResearchedBranchTier=max(own_player['branches'].values(), default=0),
                firstAnyBranchT5Seconds=row['firstT5Seconds'][owner], secondAnyBranchT5Seconds=row['secondT5Seconds'][owner],
                aiLifetime=ai, classes=classes, metrics=metrics, warnings=row['playabilityWarnings'],
                retainedEventWindow=dict(events=len(events), earliestTick=min((e['tick'] for e in events), default=None),
                                         latestTick=max((e['tick'] for e in events), default=None), finalTick=saved['snapshot']['tick']),
                unclassifiedRetainedFireEvents=unknown, finalSave=save_ref)


def outcomes(values):
    counts = Counter(v['result'] for v in values)
    return dict(observations=len(values), wins=counts['win'], losses=counts['loss'], unresolved=counts['unresolved'])


def describe(values):
    classes = {}
    for name in ['ai', 'air', 'orbital']:
        observations = [v['classes'][name] for v in values]
        classes[name] = dict(playerObservations=len(values), paidDeploymentTotal=sum(v['paidDeployments'] for v in observations),
                             notPurchasedObservations=sum(v['status'] == 'not-purchased' for v in observations),
                             firingConfirmedObservations=sum(v['status'] == 'confirmed-firing' for v in observations),
                             purchasedButFiringUnconfirmedObservations=sum(v['status'] == 'purchased-firing-unconfirmed' for v in observations),
                             partialActorUnknownObservations=sum(v['status']=='confirmed-firing' and v['purchasedActorsWithoutPositiveFiringEvidence']>0 for v in observations),
                             purchasedActorsWithoutPositiveFiringEvidence=sum(v['purchasedActorsWithoutPositiveFiringEvidence'] for v in observations),
                             confirmedFiringActorLowerBoundTotal=sum(v['confirmedFiringActorCountLowerBound'] or 0 for v in observations),
                             confirmedAttackCycleLowerBoundTotal=sum(v['confirmedAttackCyclesLowerBound'] or 0 for v in observations),
                             confirmedFiringActorsPerObservation=stats.stats(v['confirmedFiringActorCountLowerBound'] for v in observations),
                             confirmedAttackCyclesPerObservation=stats.stats(v['confirmedAttackCyclesLowerBound'] for v in observations))
    return dict(**outcomes(values), distinctGames=len({v['index'] for v in values}), durationSeconds=stats.stats(v['seconds'] for v in values),
                byRawVictoryReason={reason: outcomes([v for v in values if v['winReason'] == reason]) for reason in sorted({v['winReason'] for v in values})},
                byClockBin={key: outcomes([v for v in values if v['clockBin'] == key]) for key in CLOCK_BINS},
                winsByWinnerFinalMaximumResearchedBranchTier=dict(Counter(str(v['winnerFinalMaximumResearchedBranchTier']) for v in values if v['result'] == 'win')),
                aiLifetime={key: dict(total=sum(v['aiLifetime'][key] for v in values), perObservation=stats.stats(v['aiLifetime'][key] for v in values)) for key in values[0]['aiLifetime']},
                classes=classes, metrics={key: stats.stats(v['metrics'][key] for v in values) for key in values[0]['metrics']},
                firstAnyBranchT5Seconds=stats.stats(v['firstAnyBranchT5Seconds'] for v in values),
                secondAnyBranchT5Seconds=stats.stats(v['secondAnyBranchT5Seconds'] for v in values))


def group(values):
    before, after = [v['baseline'] for v in values], [v['current'] for v in values]
    result = dict(baseline=describe(before), current=describe(after), pairedChanges={})
    identifiers = [f'{v["index"]}/{v["owner"]}' for v in values]
    for section in ['metrics', 'aiLifetime']:
        result['pairedChanges'][section] = {key: paired_values([(identifier, a[section][key], b[section][key]) for identifier,a,b in zip(identifiers,before,after)]) for key in before[0][section]}
    result['pairedChanges']['technology'] = {key: paired_values([(identifier,a[key],b[key]) for identifier,a,b in zip(identifiers,before,after)]) for key in ['firstAnyBranchT5Seconds', 'secondAnyBranchT5Seconds']}
    result['pairedChanges']['observedFiringLowerBounds'] = {name: {key: paired_values([(identifier,a['classes'][name][key],b['classes'][name][key]) for identifier,a,b in zip(identifiers,before,after)]) for key in ['confirmedFiringActorCountLowerBound','confirmedAttackCyclesLowerBound']} for name in ['air','orbital']}
    return result



def air_class_games(values, generation, category):
    by_game = {}
    for paired in values:
        by_game.setdefault(paired['index'], []).append(paired[generation])
    games = []
    for index, sides in sorted(by_game.items()):
        paid = sum(side['classes'][category]['paidDeployments'] for side in sides)
        actors = {}
        for side in sides:
            for actor in side['classes'][category]['confirmedActors']:
                key = (side['owner'], actor['id'])
                old = actors.get(key)
                if old is None or actor['confirmedAttackCyclesLowerBound'] > old['confirmedAttackCyclesLowerBound']:
                    actors[key] = dict(actor)
        unknown = paid - len(actors)
        require(unknown >= 0, 'Paid/confirmed actor count mismatch')
        status = 'confirmed-firing' if actors else 'purchased-firing-unconfirmed' if paid else 'not-purchased'
        games.append(dict(index=index, playerPerspectives=len(sides), owners=[side['owner'] for side in sides],
                          paidDeployments=paid, firingStatus=status,
                          confirmedActorCountLowerBound=None if paid and not actors else len(actors),
                          confirmedAttackCyclesLowerBound=None if paid and not actors else sum(a['confirmedAttackCyclesLowerBound'] for a in actors.values()),
                          purchasedActorsWithoutPositiveFiringEvidence=unknown,
                          confirmedActors=list(actors.values()), finalSaves=[side['finalSave'] for side in sides]))
    return dict(distinctGames=len(games), playerPerspectives=len(values), paidDeploymentTotal=sum(g['paidDeployments'] for g in games),
                purchasedGames=sum(g['paidDeployments']>0 for g in games),
                firingConfirmedGames=sum(g['firingStatus']=='confirmed-firing' for g in games),
                purchasedButFiringUnconfirmedGames=sum(g['firingStatus']=='purchased-firing-unconfirmed' for g in games),
                partiallyConfirmedGames=sum(g['firingStatus']=='confirmed-firing' and g['purchasedActorsWithoutPositiveFiringEvidence']>0 for g in games),
                notPurchasedGames=sum(g['firingStatus']=='not-purchased' for g in games),
                confirmedFiringActorLowerBoundTotal=sum(len(g['confirmedActors']) for g in games),
                confirmedAttackCycleLowerBoundTotal=sum(g['confirmedAttackCyclesLowerBound'] or 0 for g in games),
                purchasedActorsWithoutPositiveFiringEvidence=sum(g['purchasedActorsWithoutPositiveFiringEvidence'] for g in games), games=games)


def air_group(values):
    return {generation:{category:air_class_games(values,generation,category) for category in ['air','orbital']} for generation in ['baseline','current']}


def build_air_report(observations, report, participation_ref):
    policies = {}
    for policy in sorted({v['policy'] for v in observations}):
        values = [v for v in observations if v['policy']==policy]
        cross = [v for v in values if v['policy']!=v['opponentPolicy']]
        policies[policy] = dict(crossStrategy30=air_group(cross), includingSelf36DistinctGames=air_group(values))
    return dict(recordedAtUtc=support.now(),scope='Paid air/orbit production and confirmed actual firing for all126 paired6828/d339 strategy games. No purchase is promoted to combat use; unknown actors remain explicit.',
                integrityPassed=True,finalBalanceAcceptance=False,caseCount=126,
                sourceIdentity=report['sourceIdentity'],participationAndSupply=participation_ref,
                full126DistinctGames=air_group(observations),policyGroups=policies,
                limitations=[
                    'Each policy appears in30 cross-strategy games and6 self games:36 distinct games,42 player perspectives. The full matrix is126 distinct games. Self-game sides are combined once per game only in these distinct-game tables.',
                    'Paid counts are lifetime accepted Deploy orders. Firing confirmation unions (owner,actorId) within each game and uses the maximum of overlapping raw/survivor/event counts, never a sum of duplicate evidence.',
                    'Surviving attackCount and retained fire events establish only lower bounds for air/orbit activity. Dead actors may have expired from the event ring. Purchased-but-unconfirmed games have null firing counts; partially confirmed games retain the residual unconfirmed purchased-actor count.',
                    'Unknown is not zero and cannot establish that a purchased actor never fired. Observed lower-bound totals count positive confirmed evidence only, not imputed missing activity.',
                    'Attack cycles are not projectile counts, hit rates, actual HP loss, economic efficiency or balance acceptance. No per-weapon hit-rate or room-absorption data exists in these raw records.',
                ])


def main():
    destination = OUT / 'participation-and-supply.json'
    air_destination = OUT / 'air-orbit-production-participation.json'
    require(not destination.exists() and not air_destination.exists(), 'Existing supplemental evidence is preserved')
    paired_report_path = OUT / 'paired-strategy-analysis.json'; paired_report = read(paired_report_path)
    require(paired_report['integrityPassed'] and paired_report['finalBalanceAcceptance'] is False, 'Strict early strategy comparison required')
    require(sha(Path(stats.__file__)) == support.AGGREGATOR_SHA, 'Reviewed statistics utility changed')
    catalog_source = ROOT / 'native-v6/src/catalog.rs'
    frozen_catalog = OLD.parent / 'source/native-v6/src/catalog.rs'
    require(sha(catalog_source) == sha(frozen_catalog) == '74bac662f9a49da83a4646cce342c364f9c3e97ab894b349b6c9fb7ca302c85d', 'Catalogue rules differ from frozen6828')
    host_identity_path = ROOT / 'Logs/v6/native-room-d33999c4-20260914-a-host-identity/host-identity.json'
    host_identity = read(host_identity_path)
    require(host_identity['rulesFingerprint'] == FPS['current'], 'Current native catalogue host identity differs')
    catalogs = [ROOT / 'game/v6/projectile-lifetime-6828-20260914-a/catalog.json', verify(host_identity['catalog'])]
    old_catalog, new_catalog = map(read, catalogs)
    require(old_catalog['rulesFingerprint'] == FPS['baseline'] and new_catalog['rulesFingerprint'] == FPS['current']
            and old_catalog['units'] == new_catalog['units'], 'Exported unit/category definitions changed')
    categories = {unit['id']: unit['category'] for unit in old_catalog['units']}
    require(len(categories) == len(old_catalog['units']) == 84, 'Catalogue IDs are not unique')
    analyses = {'baseline': read(verify(paired_report['baselineAnalysis'])), 'current': read(verify(paired_report['currentAnalysis']))}
    rows, input_refs = {}, {}
    for generation in ['baseline', 'current']:
        analysis = analyses[generation]
        require(analysis['games'] == 126 and analysis['integrityPassed'] and analysis['simulationFingerprints'] == {FPS[generation]:126}, 'Analysis rule/count mismatch')
        rows[generation], input_refs[generation] = load_rows(analysis)
    strict_path = verify(paired_report['sourceIdentity']['currentStrictAudit']); strict = read(strict_path)
    require(strict['passed'] and strict['games']==126 and strict['rulesFingerprint']==FPS['current'], 'Current canonical strict audit differs')
    require(verify(strict['analysis']) == verify(paired_report['currentAnalysis']), 'Current strict analysis hash differs')
    stats_receipt_path = CURRENT / 'statistics-receipt.json'
    stats_receipt = read(stats_receipt_path)
    require(stats_receipt['complete'] and stats_receipt['rulesFingerprint']==FPS['current'], 'Completed d339 statistics receipt required')
    for value in stats_receipt['outputs'].values(): verify(value)
    control_refs,control_cache = [],{}
    observations, save_refs = [], {'baseline': [], 'current': []}
    for index in range(126):
        a, b = rows['baseline'][index], rows['current'][index]
        require(all(a[key] == b[key] for key in ['index','seed','theme','branches','strategies','swapped','plannerOrder','planId','planSha256','planIndex']), 'Paired canonical identity differs')
        saved = {}
        for generation, folder in [('baseline', OLD), ('current', CURRENT)]:
            row = rows[generation][index]
            execution_path = folder / f'execution-{index:03}.json'; execution = read(execution_path)
            require(execution['exitCode'] == 0 and execution['rulesFingerprint'] == FPS[generation], 'Execution identity mismatch')
            require(verify(execution['raw']).name == f'results-{index:03}.jsonl', 'Raw execution path differs')
            save_reference = execution['save']; expected = folder / f'diagnostics/match-{index}.save.json'
            if generation == 'current':
                control_refs.append(dict(index=index,execution=ref(execution_path),**verified_dispatch(execution['controlAtDispatch'],control_cache)))
            path = verify(save_reference); require(path == expected, 'Unexpected original Save path')
            state = read(path); snapshot = state['snapshot']
            require(row['simulationSourceSha256'] == FPS[generation] == state['rulesFingerprint']
                    and snapshot['seed'] == row['seed'] and snapshot['theme'] == row['theme']
                    and snapshot['tick']/60 == row['seconds'] and snapshot['winner'] == row['winner']
                    and snapshot['winReason'] == row['winReason'] and state['initialAi'] is False
                    and state['administrativeEvents'] == [], 'Save/raw identity mismatch')
            saved[generation] = state; save_refs[generation].append(ref(path))
        for owner in [0,1]:
            observations.append(dict(index=index, owner=owner+1, policy=a['strategies'][owner], opponentPolicy=a['strategies'][1-owner],
                                     **{generation:player_observation(rows[generation][index], owner, saved[generation], categories, save_refs[generation][-1]) for generation in ['baseline','current']}))
    groups = {}
    for policy in sorted({v['policy'] for v in observations}):
        values = [v for v in observations if v['policy'] == policy]
        cross = [v for v in values if v['policy'] != v['opponentPolicy']]
        require(len(values) == 42 and len(cross) == 30, 'Per-policy denominators differ')
        require(len({v['index'] for v in values})==36 and len({v['index'] for v in cross})==30, 'Distinct game denominators differ')
        groups[policy] = dict(excludingSelf=group(cross), includingSelf=group(values))
    report = dict(recordedAtUtc=support.now(), scope='Actual participation evidence and supply changes for all126 paired6828/d339 strategy cases only; branch full-run progress and final acceptance are not evaluated.',
                  integrityPassed=True, finalBalanceAcceptance=False, caseCount=126, playerObservations=252,
                  sourceIdentity=dict(strictPairedReport=ref(paired_report_path), baselineAnalysis=paired_report['baselineAnalysis'], currentAnalysis=paired_report['currentAnalysis'],
                                      inputFiles=input_refs, finalSaves=save_refs, currentStrictAudit=ref(strict_path), currentStatisticsReceipt=ref(stats_receipt_path), controlAtDispatch=control_refs,
                                      currentCatalogueSource=ref(catalog_source), frozen6828CatalogueSource=ref(frozen_catalog), exportedCatalogues=[ref(p) for p in catalogs],
                                      helper=ref(Path(__file__)), statisticsUtility=ref(Path(stats.__file__))),
                  clockBinDefinitions={CLOCK_BINS[0]:'seconds <1080',CLOCK_BINS[1]:'1080 <= seconds <1440',CLOCK_BINS[2]:'1440 <= seconds <1800',CLOCK_BINS[3]:'1800 <= seconds <=2700',CLOCK_BINS[4]:'seconds >2700'},
                  policyGroups=groups, fullMatrixDistinctGames=126, policyDistinctGamesIncludingSelf=36, pairedObservations=observations,
                  limitations=[
                      'Cross-policy groups have30 related player observations per policy; including-self groups have42 perspectives from36 distinct games because six self games contribute both players. Full matrix coverage is126 distinct games. These are fixed correlated cases, not independent random samples.',
                      'AI participation uses runner lifetime per-actor firing, accepted active-skill and passive-event counters, including dead actors. Purchases, upgrades and plugins alone are not combat activity or attributed damage.',
                      'Air/orbit confirmation is a lower bound from positive surviving-unit attackCount and retained fire events mapped by owner/actor ID/subjectKind. The same actor is unioned, and overlapping counts use max, never addition. Counts are attack cycles, not projectile counts, hit rates or damage.',
                      'Dead actors without retained fire events cannot be classified as never fired. Purchased-but-unconfirmed class participation is null/unavailable, not zero. Aggregate lower-bound totals sum observed positive evidence only; they do not impute zero true activity for missing observations.',
                      'Different event-ring retention windows and survivor sets may change observed lower bounds. Their paired differences are differences in captured evidence, not unbiased differences in true lifetime air/orbit activity.',
                      'Supply counters are actual delivery/recharge and eligible weapon-ready starvation unit-seconds. Reasons overlap; compute starvation is tested only after ammo/energy suffice. They are not wall-clock idle duration or lost-DPS estimates.',
                      'Ore delivered and node-control seconds are observed throughput/control counters; multiple nodes can contribute simultaneously. Native lostValue covers destroyed units and buildings/GPU, not independent rooms/walls/links or net economic efficiency.',
                      'nativeTotals.damage-dealt is post-defense event magnitude and may include overkill; it is not clamped HP loss. Gemini-active HP damage is a separate limited metric. Shield payload is not prevented HP damage. Sparse absent native event counters mean zero recorded events.',
                      'Victory bins are clock bins only, not inferred combat stages. Winner maximum researched branch tier is final player knowledge, not a unit tier or the exact tier when a decisive event occurred.',
                      'Any-branch T5 values retain observed nulls, including all-null secondT5 when present. Timings are sampled each simulated second and paired deltas use identical observations with both values available.',
                      'These reports do not invent per-weapon hit rates or room-absorption measurements and do not infer balance acceptance.',
                  ])
    support.write_new(destination, report)
    air_report = build_air_report(observations, report, ref(destination))
    support.write_new(air_destination, air_report)
    print(json.dumps(dict(report=str(destination), airOrbitReport=str(air_destination), cases=126, finalBalanceAcceptance=False,
                          crossPolicy={policy:{generation:dict(aiFiringActors=value[generation]['aiLifetime']['firingActors']['total'], aiActiveSkills=value[generation]['aiLifetime']['activeSkills']['total'], airConfirmed=value[generation]['classes']['air']['firingConfirmedObservations'], airUnconfirmed=value[generation]['classes']['air']['purchasedButFiringUnconfirmedObservations'], orbitalConfirmed=value[generation]['classes']['orbital']['firingConfirmedObservations']) for generation in ['baseline','current']} for policy,parts in groups.items() for value in [parts['excludingSelf']]})), flush=True)


if __name__ == '__main__':
    main()
