"""Complete canonical branch observations, with self-matchups kept separate."""
from pathlib import Path
import collections
import datetime
import hashlib
import json
import statistics
import sys
sys.dont_write_bytecode = True
import summarize_balance
import release_gate

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-d33999c4-20260914'
OUT = BASE / 'branch'
BRANCHES = summarize_balance.BRANCHES
CURRENT_FP = 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'


def read(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def ref(p): return dict(path=p.resolve().relative_to(ROOT).as_posix(), sha256=sha(p))


def supplemental(entries):
    report = summarize_balance.describe(entries)
    report['trackedLossMetricScope'] = 'Native lostValue increments for destroyed units and buildings/GPU only. Independent room, wall and link removals are not added here. Operating expenditure is separate; this is not exhaustive destruction valuation or net economic efficiency.'
    report['twoSidedPoolingCaveat'] = 'Groups containing both perspectives of every game mechanically have pooled exchange1 and completed win fraction50 percent. Those pooled values do not measure branch balance.'
    report['additionalMetricDefinitions'] = {'peakTwoToFourAlive': 'Number of observations whose maximum simultaneous living AI count was between2 and4; not sustained force size.', 'ai.maxTier': 'Highest observed AI actor tier; zero means no observed AI actor.', 'defenseMetrics.interceptedPayload': 'Shield-absorb payload, not HP damage prevented.', 'delayedAreaMetrics': 'Currently Gemini active is the only delayed-area producer; this is not all AI damage.', 'supplyMetrics': 'Weapon-ready units with eligible targets failing actual payment. Reasons may overlap; compute starvation is checked only after local ammo/energy suffice.'}
    extra = {}
    for section, names in {
        'supplyMetrics': ['energyRecharged', 'chargingUnitSeconds', 'ammoDeliveredToUnits', 'fuelDeliveredToUnits', 'repairMaterialDeliveredToUnits', 'energyStarvedFiringUnitSeconds', 'ammoStarvedFiringUnitSeconds', 'computeStarvedFiringUnitSeconds'],
        'defenseMetrics': ['interceptedPayload', 'interceptionEvents'],
        'delayedAreaMetrics': ['impacts', 'hpDamagingImpacts', 'actualHpDamage'],
    }.items():
        for name in names:
            extra[f'{section}.{name}'] = summarize_balance.stats(summarize_balance.per_owner(row[section], name, owner) for row,owner in entries)
    extra['actualEnergySpent'] = summarize_balance.stats(row['actualEnergySpent'][owner] for row,owner in entries)
    for name in ['firstCombatSeconds', 'firstDamageSeconds']:
        extra[name] = summarize_balance.stats(row[name][owner] for row,owner in entries)
    for tier in range(1, 6):
        extra[f'firstAnyBranchT{tier}Seconds'] = summarize_balance.stats(row['firstTierSeconds'][owner][tier-1] for row,owner in entries)
    classes = ['ai', 'vehicle', 'air', 'orbital']
    for name in classes:
        extra['paidDeployments.' + name] = summarize_balance.stats(row['orderMetrics']['paidDeploymentsByClass'][owner].get(name,0) for row,owner in entries)
        extra['firstPaidClassSeconds.' + name] = summarize_balance.stats(row['orderMetrics']['firstPaidClassSeconds'][owner].get(name) for row,owner in entries)
    for name in ['passiveUses', 'computeSpent', 'maxTier']:
        extra['ai.' + name] = summarize_balance.stats((max((a[name] for a in row['orderMetrics']['aiActors'][owner]), default=0) if name == 'maxTier' else sum(a[name] for a in row['orderMetrics']['aiActors'][owner])) for row,owner in entries)
    report['additionalMetrics'] = extra
    report['aiParticipation'] = dict(playerObservations=len(entries), paidAtLeastOne=sum(row['orderMetrics']['paidDeploymentsByClass'][owner].get('ai',0)>0 for row,owner in entries), anyActiveAi=sum(row['orderMetrics']['activeAiCount'][owner]>0 for row,owner in entries), peakTwoToFourAlive=sum(2<=row['orderMetrics']['peakAliveAi'][owner]<=4 for row,owner in entries), zeroAlivePeak=sum(row['orderMetrics']['peakAliveAi'][owner]==0 for row,owner in entries), zeroActorShots=sum(not any(a['shots']>0 for a in row['orderMetrics']['aiActors'][owner]) for row,owner in entries), zeroActorActiveSkills=sum(not any(a['activeSkills']>0 for a in row['orderMetrics']['aiActors'][owner]) for row,owner in entries))
    return report


def primary_technology(row, owner, saved):
    preferred = row['branches'][owner]
    player = next(p for p in saved['snapshot']['players'] if p['owner'] == owner+1)
    tier = player['branches'].get(preferred, 0)
    first, second = row['firstT5Seconds'][owner], row['secondT5Seconds'][owner]
    counts = collections.Counter(o['order']['command']['branch'] for o in saved['orders'] if o['receipt']['accepted'] and o['order']['owner'] == owner+1 and o['order']['command']['op'] == 'research' and first is not None and o['tick'] <= first*60)
    eligible = [branch for branch,count in counts.items() if count>=4]
    first_branch = eligible[0] if len(eligible)==1 else None
    primary_time = None
    status = 'not-reached' if tier<5 else 'branch-time-not-directly-instrumented'
    if tier>=5 and first_branch==preferred:
        primary_time=first; status='unique-possible-branch-at-first-recorded-T5'
    elif tier>=5 and first_branch is not None and first_branch!=preferred and second is not None and sum(t>=5 for t in player['branches'].values())==2:
        primary_time=second; status='other-first-and-exactly-two-final-T5-branches'
    return dict(index=row['index'], owner=owner+1, branch=preferred, opponent=row['branches'][1-owner], mirror=preferred==row['branches'][1-owner], seed=row['seed'], plannerOrder=row['plannerOrder'], finalPrimaryTier=tier, primaryT5Seconds=primary_time, primaryTimeStatus=status, firstAnyBranchT5Seconds=first, secondAnyBranchT5Seconds=second)


def primary_summary(values):
    return dict(scope='T5 is sampled once per simulated second. Timing distributions are conditional on reaching observations; null/non-reaching cases remain counted and between-group medians are not paired treatment effects.', playerObservations=len(values), primaryReached=sum(v['finalPrimaryTier']>=5 for v in values), primaryNotReached=sum(v['finalPrimaryTier']<5 for v in values), primaryTimeUnavailableDespiteReaching=sum(v['finalPrimaryTier']>=5 and v['primaryT5Seconds'] is None for v in values), primaryT5Seconds=summarize_balance.stats(v['primaryT5Seconds'] for v in values), firstAnyBranchT5Seconds=summarize_balance.stats(v['firstAnyBranchT5Seconds'] for v in values), secondAnyBranchT5Seconds=summarize_balance.stats(v['secondAnyBranchT5Seconds'] for v in values))


def main():
    manifest = read(BASE / 'measurement-manifest.json')
    assert manifest['rulesFingerprint'] == CURRENT_FP and manifest['rulesVersion'] == 'v6.2'
    assert sha(ROOT / manifest['runner']['path']) == manifest['runner']['sha256']
    assert sha(Path(summarize_balance.__file__)) == manifest['aggregator']['sha256']
    assert read(OUT / 'completion-receipt.json')['complete']
    analysis = read(OUT / 'analysis.json')
    assert analysis['integrityPassed'] and analysis['games']==1000
    strict_audit = read(OUT / 'strict-integrity-check.json')
    assert strict_audit['passed'] and strict_audit['games']==1000 and strict_audit['analysis']['sha256']==sha(OUT/'analysis.json')
    assert strict_audit['rulesFingerprint'] == CURRENT_FP and strict_audit['gate']['sha256'] == sha(Path(release_gate.__file__))
    rows, inputs, ordered_rows = {}, [], []
    for item in analysis['inputFiles']:
        p=ROOT / item['path']; assert sha(p)==item['sha256']
        inputs.append(ref(p))
        for line in p.read_text(encoding='utf-8').splitlines():
            if not line.strip(): continue
            row=json.loads(line); assert row['index'] not in rows; rows[row['index']]=row; ordered_rows.append(row)
    assert set(rows)==set(range(1000))
    target = dict(rulesVersion='v6.2',rulesFingerprint=CURRENT_FP)
    release_gate.matrix_check(ordered_rows,target)
    release_gate.verify_analysis(analysis,ordered_rows,[item['sha256'] for item in inputs],target,'d339 full branch supplemental')
    entries=[(rows[i], owner) for i in range(1000) for owner in (0,1)]
    assert all(r['simulationSourceSha256']==manifest['rulesFingerprint'] for r in rows.values())
    cross=[(r,o) for r,o in entries if r['branches'][0]!=r['branches'][1]]
    mirrors=[(r,o) for r,o in entries if r['branches'][0]==r['branches'][1]]
    assert len(cross)==1600 and len(mirrors)==400
    groups={}
    for branch in BRANCHES:
        all_branch=[(r,o) for r,o in entries if r['branches'][o]==branch]
        cross_branch=[(r,o) for r,o in cross if r['branches'][o]==branch]
        assert len(all_branch)==400 and len(cross_branch)==320
        groups[branch]=dict(includingSelf=supplemental(all_branch), excludingSelf=supplemental(cross_branch), selfOnly=supplemental([(r,o) for r,o in mirrors if r['branches'][o]==branch]), byOpponent={opponent:supplemental([(r,o) for r,o in cross_branch if r['branches'][1-o]==opponent]) for opponent in BRANCHES if opponent!=branch}, bySpawnOwner={str(owner+1):supplemental([(r,o) for r,o in cross_branch if o==owner]) for owner in (0,1)}, byPlannerPosition={position:supplemental([(r,o) for r,o in cross_branch if (r['plannerOrder'][0]==o+1)==(position=='first')]) for position in ['first','second']})
    primary=[]; save_refs=[]; unresolved=[]
    for i,row in sorted(rows.items()):
        p=OUT / f'diagnostics/match-{i}.save.json'; saved=read(p)
        assert saved['rulesFingerprint']==manifest['rulesFingerprint'] and saved['snapshot']['tick']/60==row['seconds'] and saved['snapshot']['winner']==row['winner']
        assert saved['snapshot']['seed']==row['seed'] and saved['snapshot']['theme']==row['theme'] and saved['snapshot']['winReason']==row['winReason']
        assert saved['initialAi'] is False and saved['administrativeEvents']==[]
        save_refs.append(ref(p)); primary.extend(primary_technology(row,owner,saved) for owner in (0,1))
        if row['winner'] is None:
            unresolved.append(dict(index=i, seed=row['seed'], theme=row['theme'], branches=row['branches'], plannerOrder=row['plannerOrder'], seconds=row['seconds'], warnings=row['playabilityWarnings'], firstCombatSeconds=row['firstCombatSeconds'], firstDamageSeconds=row['firstDamageSeconds'], firstT5Seconds=row['firstT5Seconds'], secondT5Seconds=row['secondT5Seconds'], credits=row['credits'], nativeTotals=row['nativeTotals'], nodeControlSeconds=row['nodeControlSeconds'], supplyMetrics=row['supplyMetrics'], orderMetrics=row['orderMetrics'], finalNodes=[n for n in saved['snapshot']['resources'] if n['kind']=='node'], finalSave=ref(p)))
        if (i+1)%100==0: print(json.dumps(dict(stage='final-save-audit', audited=i+1,total=1000)),flush=True)
    per_seed={str(seed):dict(games=sum(r['seed']==seed for r in rows.values()), completed=sum(r['seed']==seed and r['winner'] is not None for r in rows.values()), durationSeconds=summarize_balance.stats(r['seconds'] for r in rows.values() if r['seed']==seed), branchesExcludingSelf={branch:supplemental([(r,o) for r,o in cross if r['seed']==seed and r['branches'][o]==branch]) for branch in BRANCHES}) for seed in range(1000,1020)}
    report=dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), scope='Complete1000 canonical ordinary-Game branch matrix. Descriptive observations only; existing canonical/aggregate gate check is separate and no balance decision is manufactured.', rulesFingerprint=manifest['rulesFingerprint'], rulesVersion=manifest['rulesVersion'], runner=manifest['runner'], analysis=ref(OUT/'analysis.json'), inputFiles=inputs, games=1000, completed=analysis['completed'], unresolved=analysis['unresolved'], integrityPassed=True, durationSeconds=analysis['durationSeconds'], completedIn30To45Minutes=analysis['completedIn30To45Minutes'], victoryReasons=analysis['victoryReasons'], playabilityWarningCounts=analysis['playabilityWarningCounts'], branchGroups=groups, allCrossBranch=supplemental(cross), allSelfOnly=supplemental(mirrors), spawnOwnersExcludingSelf={str(owner+1):supplemental([(r,o) for r,o in cross if o==owner]) for owner in (0,1)}, planningPositionsExcludingSelf={position:supplemental([(r,o) for r,o in cross if (r['plannerOrder'][0]==o+1)==(position=='first')]) for position in ['first','second']}, seeds=per_seed, primaryTechnology={branch:{'includingSelf':primary_summary([p for p in primary if p['branch']==branch]),'excludingSelf':primary_summary([p for p in primary if p['branch']==branch and not p['mirror']])} for branch in BRANCHES}, primaryObservations=primary, finalSaves=save_refs, unresolvedDetails=unresolved, finalBalanceAcceptance=False, limitations=['Self-matchups cannot measure superiority between branches; the main cross-branch view excludes all200 self games and preserves800 cross games /1600 player observations.', 'Each branch has320 cross observations over20 seeds and4 opponents, with both spawn sides and planner positions. This is a fixed designed experiment; correlated mirrored scenarios are not320 independent random samples.', 'Themes are cyclically assigned to seeds, so theme effects are not independent of seed.', 'AI participation means actual actor shots, active skills or passive events. Purchases alone are not combat use and shots are not a claim of attributed damage.', 'First/second T5 refers to any branch. Preferred-branch times are inferred only when accepted ordinary research orders and final Save make the identity unambiguous; all null values are retained.', 'Destroyed asset exchange uses ratio of sums and excludes running ammo/compute/energy costs; those costs are separate metrics.', 'Supply starvation is eligible weapon-ready unit time failing real payment. Reasons can overlap and these counters are not a direct damage-loss estimate.', 'Unresolved cases retain the real55-minute budget outcome with no assigned score winner.'])
    report['strictIntegrityCheck'] = ref(OUT/'strict-integrity-check.json')
    report['sourceIdentity'] = dict(manifest=ref(BASE/'measurement-manifest.json'), helper=ref(Path(__file__)), aggregator=ref(Path(summarize_balance.__file__)), gate=ref(Path(release_gate.__file__)))
    report['limitations'].extend([
        'All1000 rows are fresh d339 cases in exact analysis-recorded order, with zero reused rows. Every final Save is read from this new branch/diagnostics directory.',
        'Native lostValue omits independently removed rooms/walls/links. These statistics do not establish full economic exchange, and pooled two-sided exchange1/completed win fraction50 percent are mechanical identities.',
        'T5 is sampled every simulated second. Timing distributions are conditional on reaching; nulls remain counted and marginal medians are not paired effects.',
        'Supply compute starvation is checked only after local ammo and energy suffice. Shield payload is not HP prevented; delayed-area HP damage is Gemini active only; peak2-to4 AI is a maximum criterion, not sustained force size.',
        'Raw data has no per-weapon hit-rate or room-absorption measurement. This is descriptive d339 room-budget-rule evidence, not an isolated test of the earlier direct-flight correction or a final balance decision.',
    ])
    dest=OUT/'branch-review-data.json'
    with dest.open('x',encoding='utf-8',newline='\n') as f: json.dump(report,f,ensure_ascii=False,indent=2); f.write('\n')
    print(json.dumps(dict(report=str(dest),games=1000,completed=report['completed'],unresolved=report['unresolved'],branchesExcludingSelf={b:{k:groups[b]['excludingSelf'][k] for k in ['playerObservations','wins','losses','unresolved','winFractionAllObservations']} for b in BRANCHES},duration=report['durationSeconds'],finalBalanceAcceptance=False)),flush=True)


if __name__ == '__main__': main()
