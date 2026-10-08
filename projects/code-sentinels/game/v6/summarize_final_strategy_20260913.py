"""Supplemental paired F3/current strategy observations; never creates acceptance."""
from pathlib import Path
import collections
import datetime
import hashlib
import json
import statistics

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-0acffa83-20260913'
CURRENT = BASE / 'strategy'
OLD = ROOT / 'game/v6/strategy-wire-air126-f3f576-20260912'
PILOT = ROOT / 'game/v6/escort-two-pilot-20260912'


def read(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def ref(p): return dict(path=p.relative_to(ROOT).as_posix(), sha256=sha(p))


def load_rows(paths):
    rows = {}
    for p in paths:
        for line in p.read_text(encoding='utf-8').splitlines():
            if not line.strip(): continue
            row = json.loads(line)
            assert row['index'] not in rows
            rows[row['index']] = row
    assert set(rows) == set(range(126))
    return rows


def distribution(values):
    finite = [v for v in values if v is not None]
    return dict(observations=len(values), available=len(finite), null=len(values)-len(finite), minimum=min(finite) if finite else None, maximum=max(finite) if finite else None, median=statistics.median(finite) if finite else None, mean=statistics.mean(finite) if finite else None)


def observation(row, side):
    order = row['orderMetrics']
    return dict(result='unresolved' if row['winner'] is None else 'win' if row['winner'] == side+1 else 'loss', seconds=row['seconds'], firstAnyBranchT5Seconds=row['firstT5Seconds'][side], secondAnyBranchT5Seconds=row['secondT5Seconds'][side], oreDelivered=row['oreDelivered'][side], nodeControlSeconds=row['nodeControlSeconds'][side], ownDestroyedAssetValue=row['lostValue'][side], enemyDestroyedAssetValue=row['lostValue'][1-side], actualAmmoSpent=row['actualAmmoSpent'][side], actualComputeSpent=row['actualComputeSpent'][side], actualEnergySpent=row['actualEnergySpent'][side], recoverableCargoWreckAmount=row['recoverableCargoWreckAmount'][side], peakAliveAi=order['peakAliveAi'][side], activeAiCount=order['activeAiCount'][side], aiShots=sum(a['shots'] for a in order['aiActors'][side]), aiSkills=sum(a['activeSkills'] for a in order['aiActors'][side]), aiUpgrades=sum(a['upgrades'] for a in order['aiActors'][side]), aiPlugins=sum(a['plugins'] for a in order['aiActors'][side]), airPurchases=order['paidDeploymentsByClass'][side].get('air', 0), orbitalPurchases=order['paidDeploymentsByClass'][side].get('orbital', 0))


def describe(values):
    own = sum(v['ownDestroyedAssetValue'] for v in values)
    wins = sum(v['result'] == 'win' for v in values)
    losses = sum(v['result'] == 'loss' for v in values)
    return dict(observations=len(values), wins=wins, losses=losses, unresolved=len(values)-wins-losses, winFractionAllObservations=wins/len(values), destroyedAssetEnemyOverOwnRatioOfSums=sum(v['enemyDestroyedAssetValue'] for v in values)/own if own else None, metrics={key: distribution([v[key] for v in values]) for key in values[0] if key != 'result'})


def main():
    manifest = read(BASE / 'measurement-manifest.json')
    assert read(CURRENT / 'completion-receipt.json')['complete']
    analysis = read(CURRENT / 'analysis.json')
    assert analysis['integrityPassed'] and analysis['games'] == 126
    current_paths = [ROOT / r['path'] for r in analysis['inputFiles']]
    for r, p in zip(analysis['inputFiles'], current_paths): assert sha(p) == r['sha256']
    old_paths = sorted(OLD.glob('results-*.jsonl'))
    rows, old = load_rows(current_paths), load_rows(old_paths)
    plan_path = ROOT / 'game/v6/strategy-plan.json'
    plan = read(plan_path); cases = {c['index']: c for c in plan['cases']}
    expected_sha = manifest['matrices']['strategy']['inputPlan']['sha256']
    assert sha(plan_path) == expected_sha
    groups, primary, paired = {}, [], []
    for i in range(126):
        r, previous, case = rows[i], old[i], cases[i]
        for row in [r, previous]:
            assert all(row[k] == case[k] for k in ['index', 'seed', 'theme', 'branches', 'strategies', 'swapped', 'plannerOrder'])
            assert row['planId'] == plan['planId'] and row['planIndex'] == i and row['planSha256'] == expected_sha
        assert r['simulationSourceSha256'] == manifest['rulesFingerprint']
        assert previous['simulationSourceSha256'] == 'f3f576ef578a2268ec14a8259ba721a855e865289e76c1dd9e65bd595042b9a5'
        save_path = PILOT / f'diagnostics/match-{i}.save.json' if i in range(96,102) else CURRENT / f'diagnostics/match-{i}.save.json'
        saved = read(save_path)
        assert saved['rulesFingerprint'] == r['simulationSourceSha256']
        assert saved['snapshot']['tick']/60 == r['seconds'] and saved['snapshot']['winner'] == r['winner']
        for side, style in enumerate(r['strategies']):
            other = r['strategies'][1-side]
            keys = [f'{style}/including-self', f'{style}/vs/{other}', f'{style}/excluding-self' if style != other else f'{style}/self']
            for key in keys:
                group = groups.setdefault(key, dict(baseline=[], current=[]))
                group['baseline'].append(observation(previous, side)); group['current'].append(observation(r, side))
            preferred = r['branches'][side]
            player = next(p for p in saved['snapshot']['players'] if p['owner'] == side+1)
            tier = player['branches'].get(preferred, 0)
            first, second = r['firstT5Seconds'][side], r['secondT5Seconds'][side]
            counts = collections.Counter(o['order']['command']['branch'] for o in saved['orders'] if o['receipt']['accepted'] and o['order']['owner'] == side+1 and o['order']['command']['op'] == 'research' and first is not None and o['tick'] <= first*60)
            eligible = [b for b,n in counts.items() if n >= 4]
            first_branch = eligible[0] if len(eligible) == 1 else None
            primary_time = None
            reason = 'not-reached' if tier < 5 else 'branch-time-not-directly-instrumented'
            if tier >= 5 and first_branch == preferred:
                primary_time = first; reason = 'unique-possible-branch-at-first-recorded-T5'
            elif tier >= 5 and first_branch is not None and first_branch != preferred and second is not None and sum(t >= 5 for t in player['branches'].values()) == 2:
                primary_time = second; reason = 'other-first-and-exactly-two-final-T5-branches'
            primary.append(dict(index=i, side=side+1, strategy=style, opponent=other, primaryBranch=preferred, finalPrimaryTier=tier, primaryT5Seconds=primary_time, primaryTimeStatus=reason, firstAnyBranchT5Seconds=first, secondAnyBranchT5Seconds=second, finalSave=ref(save_path)))
        paired.append(dict(index=i, strategies=r['strategies'], branches=r['branches'], seed=r['seed'], theme=r['theme'], baselineWinner=previous['winner'], currentWinner=r['winner'], baselineSeconds=previous['seconds'], currentSeconds=r['seconds'], baselineWinReason=previous['winReason'], currentWinReason=r['winReason']))
    primary_groups = {}
    for style in plan['strategies']:
        for include_self in [True,False]:
            values = [p for p in primary if p['strategy']==style and (include_self or p['strategy']!=p['opponent'])]
            primary_groups[f'{style}/{"including-self" if include_self else "excluding-self"}'] = dict(observations=len(values), primaryReached=sum(p['finalPrimaryTier']>=5 for p in values), primaryNotReached=sum(p['finalPrimaryTier']<5 for p in values), primaryTimeUnavailableDespiteReaching=sum(p['finalPrimaryTier']>=5 and p['primaryT5Seconds'] is None for p in values), primaryT5Seconds=distribution([p['primaryT5Seconds'] for p in values]), firstAnyBranchT5Seconds=distribution([p['firstAnyBranchT5Seconds'] for p in values]), secondAnyBranchT5Seconds=distribution([p['secondAnyBranchT5Seconds'] for p in values]))
    report = dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), scope='Complete approved 126-case F3/current comparison only. Supplemental observation report; final statistical integrity comes from unchanged summarize_balance.py.', integrityPassed=True, currentRulesFingerprint=manifest['rulesFingerprint'], baselineRulesFingerprint=old[0]['simulationSourceSha256'], runner=manifest['runner'], canonicalPlan=ref(plan_path), currentAnalysis=ref(CURRENT / 'analysis.json'), currentInputs=[ref(p) for p in current_paths], baselineInputs=[ref(p) for p in old_paths], groups={key:{generation:describe(v) for generation,v in values.items()} for key,values in groups.items()}, primaryTechnology=primary_groups, primaryObservations=primary, pairedCases=paired, warnings=[dict(index=i, warnings=r['playabilityWarnings']) for i,r in rows.items() if any(r['playabilityWarnings'].values())], finalBalanceAcceptance=False, limitations=['Cross-strategy comparisons contain 30 observations per policy over three theme/seed contexts. Related swaps and shared seeds are not independent random samples.', 'Self comparisons measure initiative and must not be interpreted as policy superiority.', 'First/second T5 metrics refer to any branch. Preferred-branch timing is reported only when ordinary research orders and actual final Save identify the branch unambiguously.', 'Null technology times and unfinished matches are retained, never converted to zero, losses or forced winners.', 'Destroyed asset value excludes ongoing ammunition/compute/energy spend; those metrics are reported separately.', 'This report does not choose a universal 50 percent win-rate target or approve the 30-45 minute duration criterion.'])
    destination = CURRENT / 'paired-strategy-analysis.json'
    with destination.open('x', encoding='utf-8', newline='\n') as f:
        json.dump(report, f, ensure_ascii=False, indent=2); f.write('\n')
    print(json.dumps(dict(report=str(destination), games=126, crossStrategy={k:{g:{m:v[m] for m in ['observations','wins','losses','unresolved']} for g,v in values.items()} for k,values in report['groups'].items() if k.endswith('/excluding-self')}, warnings=report['warnings']), ensure_ascii=True))


if __name__ == '__main__': main()
