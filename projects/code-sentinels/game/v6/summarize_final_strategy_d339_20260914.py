"""Supplemental paired 6828/d339 strategy observations; never creates acceptance."""
from pathlib import Path
import argparse
import collections
import datetime
import hashlib
import json
import statistics
import sys
sys.dont_write_bytecode = True
import release_gate
import summarize_balance

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-d33999c4-20260914'
CURRENT = BASE / 'strategy'
OLD = ROOT / 'game/v6/final-balance-6828c5b7-20260914/strategy/early-review'
CURRENT_FP = 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'
BASELINE_FP = '6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781'


def read(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def ref(p): return dict(path=p.resolve().relative_to(ROOT).as_posix(), sha256=sha(p))


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


def paired_group(baseline, current):
    assert len(baseline) == len(current)
    metrics = {}
    for key in baseline[0]:
        if key == 'result': continue
        pairs = [(a[key], b[key]) for a,b in zip(baseline,current)]
        both = [(a,b) for a,b in pairs if a is not None and b is not None]
        metrics[key] = dict(observations=len(pairs), baselineAvailable=sum(a is not None for a,b in pairs), currentAvailable=sum(b is not None for a,b in pairs), matchedBoth=len(both), baselineOnly=sum(a is not None and b is None for a,b in pairs), currentOnly=sum(a is None and b is not None for a,b in pairs), neither=sum(a is None and b is None for a,b in pairs), currentMinusBaselineOnMatched=distribution([b-a for a,b in both]))
    return dict(scope='Identical case and side observations only; numeric time deltas require both values. Conditional marginal medians are not paired effects.', observations=len(baseline), outcomeTransitions=dict(collections.Counter(a['result']+'->'+b['result'] for a,b in zip(baseline,current))), metrics=metrics)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--audit-directory', type=Path, default=CURRENT,
                        help='Directory containing completion/analysis/strict audit and receiving the report; Saves stay in the original strategy/diagnostics.')
    audit_directory = parser.parse_args().audit_directory.resolve()
    assert audit_directory.is_relative_to(CURRENT), 'Audit directory must be within the current strategy directory'
    manifest = read(BASE / 'measurement-manifest.json')
    assert manifest['rulesFingerprint'] == CURRENT_FP and manifest['rulesVersion'] == 'v6.2'
    assert sha(Path(summarize_balance.__file__)) == manifest['aggregator']['sha256']
    assert sha(ROOT / manifest['runner']['path']) == manifest['runner']['sha256']
    completion = read(audit_directory / 'completion-receipt.json')
    assert completion['complete']
    analysis = read(audit_directory / 'analysis.json')
    assert analysis['integrityPassed'] and analysis['games'] == 126
    current_paths = [ROOT / r['path'] for r in analysis['inputFiles']]
    for r, p in zip(analysis['inputFiles'], current_paths): assert sha(p) == r['sha256']
    baseline_completion = read(OLD / 'completion-receipt.json')
    assert baseline_completion['complete'] and baseline_completion['requiredCases'] == 126
    baseline_save_refs = {(ROOT / value['path']).resolve():value for value in baseline_completion['finalSaves']}
    assert len(baseline_save_refs) == 126
    baseline_analysis = read(OLD / 'analysis.json')
    assert baseline_analysis['integrityPassed'] and baseline_analysis['games'] == 126
    old_paths = [ROOT / item['path'] for item in baseline_analysis['inputFiles']]
    for item,path in zip(baseline_analysis['inputFiles'],old_paths): assert sha(path) == item['sha256']
    rows, old = load_rows(current_paths), load_rows(old_paths)
    plan_path = ROOT / manifest['matrices']['strategy']['inputPlan']['path']
    plan = read(plan_path); cases = {c['index']: c for c in plan['cases']}
    expected_sha = manifest['matrices']['strategy']['inputPlan']['sha256']
    assert sha(plan_path) == expected_sha
    for folder, report, records, paths, fp in [(audit_directory,analysis,rows,current_paths,CURRENT_FP),(OLD,baseline_analysis,old,old_paths,BASELINE_FP)]:
        strict = read(folder / 'strict-integrity-check.json')
        assert strict['passed'] and strict['games'] == 126 and strict['analysis']['sha256'] == sha(folder / 'analysis.json')
        assert strict['rulesFingerprint'] == fp and strict['gate']['sha256'] == sha(Path(release_gate.__file__))
        target = dict(rulesVersion='v6.2', rulesFingerprint=fp)
        ordered_rows = [json.loads(line) for path in paths for line in path.read_text(encoding='utf-8-sig').splitlines() if line.strip()]
        release_gate.strategy_check(ordered_rows,target,plan,expected_sha)
        release_gate.verify_analysis(report,ordered_rows,[sha(path) for path in paths],target,'d339 strategy paired '+folder.name)
    groups, primary, paired = {}, [], []
    for i in range(126):
        r, previous, case = rows[i], old[i], cases[i]
        for row in [r, previous]:
            assert all(row[k] == case[k] for k in ['index', 'seed', 'theme', 'branches', 'strategies', 'swapped', 'plannerOrder'])
            assert row['planId'] == plan['planId'] and row['planIndex'] == i and row['planSha256'] == expected_sha
        assert r['simulationSourceSha256'] == manifest['rulesFingerprint']
        assert previous['simulationSourceSha256'] == BASELINE_FP
        baseline_save_path = OLD.parent / f'diagnostics/match-{i}.save.json'
        assert sha(baseline_save_path) == baseline_save_refs[baseline_save_path]['sha256']
        baseline_saved = read(baseline_save_path)
        assert baseline_saved['rulesFingerprint'] == BASELINE_FP and baseline_saved['initialAi'] is False and baseline_saved['administrativeEvents'] == []
        assert baseline_saved['snapshot']['tick']/60 == previous['seconds'] and baseline_saved['snapshot']['winner'] == previous['winner'] and baseline_saved['snapshot']['winReason'] == previous['winReason']
        save_path = CURRENT / f'diagnostics/match-{i}.save.json'
        saved = read(save_path)
        assert saved['rulesFingerprint'] == r['simulationSourceSha256']
        assert saved['snapshot']['tick']/60 == r['seconds'] and saved['snapshot']['winner'] == r['winner']
        assert saved['snapshot']['seed'] == r['seed'] and saved['snapshot']['theme'] == r['theme'] and saved['snapshot']['winReason'] == r['winReason']
        assert saved['initialAi'] is False and saved['administrativeEvents'] == []
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
        paired.append(dict(index=i, strategies=r['strategies'], branches=r['branches'], seed=r['seed'], theme=r['theme'], baselineWinner=previous['winner'], currentWinner=r['winner'], baselineSeconds=previous['seconds'], currentSeconds=r['seconds'], baselineWinReason=previous['winReason'], currentWinReason=r['winReason'], plannerOrder=r['plannerOrder'], swapped=r['swapped'], baselineFirstAnyBranchT5Seconds=previous['firstT5Seconds'], currentFirstAnyBranchT5Seconds=r['firstT5Seconds'], baselineSecondAnyBranchT5Seconds=previous['secondT5Seconds'], currentSecondAnyBranchT5Seconds=r['secondT5Seconds']))
    primary_groups = {}
    for style in plan['strategies']:
        for include_self in [True,False]:
            values = [p for p in primary if p['strategy']==style and (include_self or p['strategy']!=p['opponent'])]
            primary_groups[f'{style}/{"including-self" if include_self else "excluding-self"}'] = dict(observations=len(values), primaryReached=sum(p['finalPrimaryTier']>=5 for p in values), primaryNotReached=sum(p['finalPrimaryTier']<5 for p in values), primaryTimeUnavailableDespiteReaching=sum(p['finalPrimaryTier']>=5 and p['primaryT5Seconds'] is None for p in values), primaryT5Seconds=distribution([p['primaryT5Seconds'] for p in values]), firstAnyBranchT5Seconds=distribution([p['firstAnyBranchT5Seconds'] for p in values]), secondAnyBranchT5Seconds=distribution([p['secondAnyBranchT5Seconds'] for p in values]))
    report = dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), scope='Complete approved 126-case 6828/d339 comparison only. Supplemental observation report; final statistical integrity comes from unchanged summarize_balance.py.', integrityPassed=True, currentRulesFingerprint=manifest['rulesFingerprint'], baselineRulesFingerprint=old[0]['simulationSourceSha256'], runner=manifest['runner'], canonicalPlan=ref(plan_path), currentAnalysis=ref(audit_directory / 'analysis.json'), currentInputs=[ref(p) for p in current_paths], baselineInputs=[ref(p) for p in old_paths], groups={key:{generation:describe(v) for generation,v in values.items()} for key,values in groups.items()}, primaryTechnology=primary_groups, primaryObservations=primary, pairedCases=paired, warnings=[dict(index=i, warnings=r['playabilityWarnings']) for i,r in rows.items() if any(r['playabilityWarnings'].values())], finalBalanceAcceptance=False, limitations=['Cross-strategy comparisons contain 30 observations per policy over three theme/seed contexts. Related swaps and shared seeds are not independent random samples.', 'Self comparisons measure initiative and must not be interpreted as policy superiority.', 'First/second T5 metrics refer to any branch. Preferred-branch timing is reported only when ordinary research orders and actual final Save identify the branch unambiguously.', 'Null technology times and unfinished matches are retained, never converted to zero, losses or forced winners.', 'Destroyed asset value excludes ongoing ammunition/compute/energy spend; those metrics are reported separately.', 'This report does not choose a universal 50 percent win-rate target or approve the 30-45 minute duration criterion.'])
    report['pairedGroupMetrics'] = {key:paired_group(value['baseline'],value['current']) for key,value in groups.items()}
    report['baselineAnalysis'] = ref(OLD / 'analysis.json')
    report['baselineCompletion'] = ref(OLD / 'completion-receipt.json')
    report['baselineFinalSaves'] = [baseline_save_refs[OLD.parent / f'diagnostics/match-{i}.save.json'] for i in range(126)]
    report['currentAnalysis'] = ref(audit_directory / 'analysis.json')
    report['currentCompletion'] = ref(audit_directory / 'completion-receipt.json')
    report['auditDirectory'] = audit_directory.relative_to(ROOT).as_posix()
    if audit_directory != CURRENT:
        report['scope'] += ' ' + completion.get('scope', 'Separate strategy-only audit; no shared branch completion claim.')
    report['sourceIdentity'] = dict(manifest=ref(BASE / 'measurement-manifest.json'), helper=ref(Path(__file__)), aggregator=ref(Path(summarize_balance.__file__)), gate=ref(Path(release_gate.__file__)), currentStrictAudit=ref(audit_directory / 'strict-integrity-check.json'), baselineStrictAudit=ref(OLD / 'strict-integrity-check.json'))
    report['limitations'].extend([
        'T5 is sampled once per simulated second. Each distribution is conditional on reaching; paired timing deltas use only identical case/side observations where both times exist, and reach/non-reach counts remain separate.',
        'Primary-branch time attribution describes current d339. All current126 Saves are new strategy/diagnostics files. Historical6828 inputs retain their exact completed early-review analysis order; all126 corresponding original6828 Saves are hash/outcome verified. No pilot fallback or incomplete branch aggregate is used.',
        'Native lostValue counts destroyed units and buildings/GPU, not independent room/wall/link removals or net economic efficiency. Costs and recoverable cargo remain separate; both-perspective self pooling mechanically balances wins/losses and exchange.',
        'This comparison is historical6828 versus d339, whose room budget/topology accounting is changed. Both inherit the earlier direct-flight correction; this comparison does not estimate that earlier intervention or isolate a causal mechanism.',
        'Raw telemetry has no per-weapon hit-rate or room-absorption statistic. Actor shots/skills do not attribute HP damage; no such causal decomposition is claimed.',
    ])
    destination = audit_directory / 'paired-strategy-analysis.json'
    with destination.open('x', encoding='utf-8', newline='\n') as f:
        json.dump(report, f, ensure_ascii=False, indent=2); f.write('\n')
    print(json.dumps(dict(report=str(destination), games=126, crossStrategy={k:{g:{m:v[m] for m in ['observations','wins','losses','unresolved']} for g,v in values.items()} for k,values in report['groups'].items() if k.endswith('/excluding-self')}, warnings=report['warnings']), ensure_ascii=True))


if __name__ == '__main__': main()
