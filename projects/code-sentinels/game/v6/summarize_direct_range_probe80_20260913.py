"""Audit and compare only the preregistered direct-range probe's paired 80 cases.

Run after candidate/completion-receipt.json records all 80 completed executions.
This launches Python aggregation only, never a native runner or a simulation.
Existing raw files, receipts, analyses and paired reports are never overwritten.
"""
from pathlib import Path
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import subprocess
import sys
import uuid

sys.dont_write_bytecode = True
import release_gate
import summarize_balance as aggregate

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/direct-range-probe80-20260913'
RUN = BASE / 'candidate'
BASELINE_FP = '0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a'
CANDIDATE_FP = '6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781'
BASELINE_RUNNER = '808ceb66984934b7b3569e310aa14d2b3ccdb8ca6d6ea75f009091f655463ac2'
CANDIDATE_RUNNER = 'e5ccbd4912eba1302fe39ea7f2690785d28a4e69828c13c4cd967295e107e022'
PREREG_SHA = '5929607f8108e67b753d097387888317af63893cd4e00826e7a4356ce80d781b'
AGGREGATOR_SHA = '6a26e8958522ec779a598692a4f0df5841fa251189504af9c6b33d99d601f6d8'
GATE_SHA = '8a056b911df2039008d6e5684952781918a15acfefbf8845233d0a017dbdb0c8'
SEEDS = [1000, 1005, 1010, 1014, 1019]
OPPONENTS = ['speed', 'security', 'algorithm', 'lightweight']
TIMES = ['firstAnyBranchT5Seconds', 'secondAnyBranchT5Seconds', 'primaryT5Seconds']
SECTIONS = {
    'supplyMetrics': ['energyRecharged', 'chargingUnitSeconds', 'ammoDeliveredToUnits',
                      'fuelDeliveredToUnits', 'repairMaterialDeliveredToUnits',
                      'energyStarvedFiringUnitSeconds', 'ammoStarvedFiringUnitSeconds',
                      'computeStarvedFiringUnitSeconds'],
    'defenseMetrics': ['interceptedPayload', 'interceptionEvents'],
    'delayedAreaMetrics': ['impacts', 'hpDamagingImpacts', 'actualHpDamage'],
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ref(path):
    return dict(path=path.resolve().relative_to(ROOT).as_posix(), sha256=sha(path),
                bytes=path.stat().st_size)


def verify(reference):
    path = (ROOT / reference['path']).resolve()
    require(path.is_relative_to(ROOT), 'Evidence reference leaves the project')
    require(path.is_file() and sha(path) == reference['sha256'], f'Hash mismatch: {path}')
    require('bytes' not in reference or path.stat().st_size == reference['bytes'],
            f'Byte length mismatch: {path}')
    return path


def write_new(path, document):
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(document, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write('\n')


def one_row(path):
    rows = [json.loads(line) for line in path.read_text(encoding='utf-8-sig').splitlines()
            if line.strip()]
    require(len(rows) == 1, f'Expected exactly one original row: {path}')
    return rows[0]


def canonical_case(index):
    branches = aggregate.BRANCHES
    pair = [branches[index // 200], branches[(index // 40) % 5]]
    offset, swapped = (index // 2) % 20, bool(index % 2)
    return dict(index=index, seed=1000 + offset, theme=['river', 'mining', 'highland'][offset % 3],
                branches=pair[::-1] if swapped else pair, strategies=['mixed-ai', 'mixed-ai'],
                swapped=swapped, plannerOrder=[2, 1] if swapped else [1, 2], originalPair=pair)


def check_row(row, case, plan, plan_sha, fingerprint):
    require(all(type(row.get(key)) is type(value) and row[key] == value
                for key, value in case.items()), f'Canonical metadata mismatch: {case["index"]}')
    require(row['planId'] == plan['planId'] and row['planIndex'] == case['index']
            and row['planSha256'] == plan_sha, f'Plan identity mismatch: {case["index"]}')
    require(row['rulesVersion'] == 'v6.2' and row['simulationSourceSha256'] == fingerprint,
            f'Rule identity mismatch: {case["index"]}')
    require(0 < row['seconds'] <= 3300 and (row['winner'] is not None or row['seconds'] == 3300),
            f'Invalid natural outcome/budget: {case["index"]}')


def technology(row, owner, saved):
    branch = row['branches'][owner]
    player = next(p for p in saved['snapshot']['players'] if p['owner'] == owner + 1)
    tier = player['branches'].get(branch, 0)
    first, second = row['firstT5Seconds'][owner], row['secondT5Seconds'][owner]
    counts = Counter(order['order']['command']['branch'] for order in saved['orders']
                     if order['receipt']['accepted'] and order['order']['owner'] == owner + 1
                     and order['order']['command']['op'] == 'research'
                     and first is not None and order['tick'] <= first * 60)
    eligible = [key for key, value in counts.items() if value >= 4]
    first_branch = eligible[0] if len(eligible) == 1 else None
    primary = None
    status = 'not-reached' if tier < 5 else 'branch-time-not-directly-instrumented'
    if tier >= 5 and first_branch == branch:
        primary, status = first, 'unique-possible-branch-at-first-recorded-T5'
    elif tier >= 5 and first_branch is not None and first_branch != branch and second is not None \
            and sum(value >= 5 for value in player['branches'].values()) == 2:
        primary, status = second, 'other-first-and-exactly-two-final-T5-branches'
    return dict(finalPrimaryTier=tier, primaryReached=tier >= 5, primaryT5Seconds=primary,
                primaryTimeStatus=status, firstAnyBranchT5Seconds=first,
                secondAnyBranchT5Seconds=second)


def player_observation(row, owner, saved):
    order = row['orderMetrics']
    paid = Counter(entry['order']['command']['kind'] for entry in saved['orders']
                   if entry['receipt']['accepted'] and entry['order']['owner'] == owner + 1
                   and entry['order']['command']['op'] == 'deploy')
    require(dict(paid) == order['paidDeploymentsByKind'][owner],
            f'Paid composition differs from ordinary Save orders: {row["index"]}/{owner + 1}')
    require(sum(paid.values()) == sum(order['paidDeploymentsByClass'][owner].values()),
            'Paid class counts differ from paid kind counts')
    metrics = {name: row[name][owner] for name in
               ['oreDelivered', 'nodeControlSeconds', 'lostValue', 'actualAmmoSpent',
                'actualComputeSpent', 'actualEnergySpent', 'recoverableCargoWreckAmount',
                'shotsFired', 'firstCombatSeconds', 'firstDamageSeconds']}
    for section, names in SECTIONS.items():
        metrics.update({section + '.' + name: row[section][name][owner] for name in names})
    # Native totals are sparse additive counters: an absent event key means zero events.
    for name in ['damage-dealt', 'transport-spent', 'compute-maintenance']:
        metrics['nativeTotals.' + name] = row['nativeTotals'][owner].get(name, 0.)
    actors = order['aiActors'][owner]
    for name in ['shots', 'activeSkills', 'passiveUses', 'computeSpent', 'upgrades', 'plugins']:
        metrics['aiActors.' + name] = sum(actor[name] for actor in actors)
    metrics['peakAliveAi'] = order['peakAliveAi'][owner]
    metrics['activeAiCount'] = order['activeAiCount'][owner]
    metrics['maxObservedAiTier'] = max((actor['maxTier'] for actor in actors), default=0)
    require(all(value is None or aggregate.finite_number(value) and value >= 0
                for value in metrics.values()), 'Invalid observed player metric')
    result = 'unresolved' if row['winner'] is None else 'win' if row['winner'] == owner + 1 else 'loss'
    return dict(owner=owner + 1, branch=row['branches'][owner], result=result,
                technology=technology(row, owner, saved), metrics=metrics,
                paidDeploymentsByKind=dict(paid), paidDeploymentsByClass=order['paidDeploymentsByClass'][owner])


def game_observation(row, saved, raw_reference, save_reference):
    science = row['branches'].index('science')
    return dict(seconds=row['seconds'], winner=row['winner'], winReason=row['winReason'],
                unresolved=row['winner'] is None, warnings=row['playabilityWarnings'],
                science=player_observation(row, science, saved),
                opponent=player_observation(row, 1 - science, saved),
                raw=raw_reference, finalSave=save_reference)


def paired_values(values):
    """Values are (case index, baseline, candidate); no null is coerced to zero."""
    both = [(index, old, new) for index, old, new in values
            if aggregate.finite_number(old) and aggregate.finite_number(new)]
    old_only = [index for index, old, new in values
                if aggregate.finite_number(old) and not aggregate.finite_number(new)]
    new_only = [index for index, old, new in values
                if not aggregate.finite_number(old) and aggregate.finite_number(new)]
    neither = [index for index, old, new in values
               if not aggregate.finite_number(old) and not aggregate.finite_number(new)]
    return dict(pairs=len(values), baseline=aggregate.stats(old for _, old, _ in values),
                candidate=aggregate.stats(new for _, _, new in values), matchedBoth=len(both),
                baselineOnly=len(old_only), candidateOnly=len(new_only), neither=len(neither),
                matchedIndices=[index for index, _, _ in both], baselineOnlyIndices=old_only,
                candidateOnlyIndices=new_only, neitherIndices=neither,
                candidateMinusBaselineOnMatched=aggregate.stats(new - old for _, old, new in both))


def player_summary(players):
    n = len(players)
    counts = Counter(p['result'] for p in players)
    tech = [p['technology'] for p in players]
    compositions = {}
    for field in ['paidDeploymentsByKind', 'paidDeploymentsByClass']:
        names = sorted({name for player in players for name in player[field]})
        compositions[field] = {name: dict(total=sum(p[field].get(name, 0) for p in players),
                                          perPlayer=aggregate.stats(p[field].get(name, 0) for p in players))
                               for name in names}
    return dict(playerObservations=n, branches=dict(Counter(p['branch'] for p in players)),
                wins=counts['win'], losses=counts['loss'], unresolved=counts['unresolved'],
                winFractionAllObservations=counts['win'] / n,
                winFractionCompleted=counts['win'] / (n - counts['unresolved'])
                if n > counts['unresolved'] else None,
                technology=dict(primaryReached=sum(t['primaryReached'] for t in tech),
                                primaryNotReached=sum(not t['primaryReached'] for t in tech),
                                primaryTimeUnavailableDespiteReaching=sum(t['primaryReached'] and t['primaryT5Seconds'] is None for t in tech),
                                timeStatusCounts=dict(Counter(t['primaryTimeStatus'] for t in tech)),
                                **{name: aggregate.stats(t[name] for t in tech) for name in TIMES}),
                metrics={name: aggregate.stats(p['metrics'][name] for p in players)
                         for name in players[0]['metrics']}, **compositions)


def summarize_group(cases):
    require(bool(cases), 'Empty registered comparison stratum')
    summary = dict(caseCount=len(cases), indices=[p['index'] for p in cases],
                   scope='One science and one opponent perspective per game; roles retain actual spawn owner and planner position.')
    for generation in ['baseline', 'candidate']:
        games = [p[generation] for p in cases]
        summary[generation] = dict(games=len(games), completed=sum(not g['unresolved'] for g in games),
                                   unresolved=sum(g['unresolved'] for g in games),
                                   durationSeconds=aggregate.stats(g['seconds'] for g in games),
                                   completedIn30To45Minutes=sum(not g['unresolved'] and 1800 <= g['seconds'] <= 2700 for g in games),
                                   victoryReasons=dict(Counter(g['winReason'] for g in games)),
                                   warningCounts=dict(Counter(key for g in games for key, value in g['warnings'].items() if value)),
                                   **{role: player_summary([g[role] for g in games]) for role in ['science', 'opponent']})
    summary['pairedDurationSeconds'] = paired_values([(p['index'], p['baseline']['seconds'], p['candidate']['seconds']) for p in cases])
    summary['pairedPlayers'] = {}
    for role in ['science', 'opponent']:
        result = {}
        for field, names in [('technology', TIMES), ('metrics', cases[0]['baseline'][role]['metrics'])]:
            result[field] = {name: paired_values([(p['index'], p['baseline'][role][field][name], p['candidate'][role][field][name]) for p in cases]) for name in names}
        result['primaryReachTransitions'] = dict(Counter(
            f"{p['baseline'][role]['technology']['primaryReached']}->{p['candidate'][role]['technology']['primaryReached']}" for p in cases))
        result['outcomeTransitions'] = dict(Counter(f"{p['baseline'][role]['result']}->{p['candidate'][role]['result']}" for p in cases))
        for field in ['paidDeploymentsByKind', 'paidDeploymentsByClass']:
            names = sorted({name for p in cases for gen in ['baseline', 'candidate'] for name in p[gen][role][field]})
            result[field] = {name: paired_values([(p['index'], p['baseline'][role][field].get(name, 0), p['candidate'][role][field].get(name, 0)) for p in cases]) for name in names}
        summary['pairedPlayers'][role] = result
    return summary


def reviewed_analysis(generation, paths, rows, target, directory):
    destination = directory / f'{generation}-analysis.json'
    command = [sys.executable, '-B', str(Path(aggregate.__file__).resolve()),
               *map(str, paths), '--output', str(destination), '--expected-games', '80']
    invocation = dict(command=command, cwd=str(ROOT), orderedInputs=[ref(path) for path in paths],
                      aggregator=ref(Path(aggregate.__file__)), target=target,
                      startedAtUtc=datetime.now(timezone.utc).isoformat())
    write_new(directory / f'{generation}-aggregate-invocation.json', invocation)
    flags = subprocess.CREATE_NO_WINDOW if sys.platform == 'win32' else 0
    completed = subprocess.run(command, cwd=ROOT, capture_output=True, text=True,
                               encoding='utf-8', creationflags=flags)
    write_new(directory / f'{generation}-aggregate-execution.json',
              dict(**invocation, exitCode=completed.returncode, stdout=completed.stdout,
                   stderr=completed.stderr, endedAtUtc=datetime.now(timezone.utc).isoformat()))
    require(completed.returncode == 0, f'{generation} reviewed aggregator failed: {completed.stderr}')
    require(all(sha(path) == item['sha256'] for path, item in zip(paths, invocation['orderedInputs'])),
            'Raw input changed during aggregation')
    analysis = read(destination)
    release_gate.validate_match_rows(rows, target, generation + ' preregistered80')
    release_gate.verify_analysis(analysis, rows, [item['sha256'] for item in invocation['orderedInputs']],
                                 target, generation + ' preregistered80')
    return ref(destination)


def main():
    completion_path = RUN / 'completion-receipt.json'
    require(completion_path.is_file(), 'Refusing analysis until all 80 candidate executions complete')
    completion = read(completion_path)
    require(completion.get('complete') is True and completion.get('completedCases') == 80
            and completion.get('requiredCases') == 80 and completion.get('failed') == []
            and completion.get('sourceUnchanged') is True,
            'Refusing incomplete, failed or source-drifted candidate measurement')
    destination = BASE / 'paired-analysis.json'
    require(not destination.exists(), 'Existing paired-analysis.json is preserved')
    prereg_path, build_path = BASE / 'pre-registration.json', BASE / 'runner-build-receipt.json'
    require(sha(prereg_path) == PREREG_SHA, 'Preregistration identity changed')
    require(sha(Path(aggregate.__file__)) == AGGREGATOR_SHA and sha(Path(release_gate.__file__)) == GATE_SHA,
            'Reviewed aggregator or release gate changed')
    prereg, build = read(prereg_path), read(build_path)
    clarification_path = BASE / 'intervention-scope-clarification.json'
    clarification = read(clarification_path)
    physics_reference = clarification['actualEmbeddedHostPhysicsComparison']
    require(physics_reference['sha256'] == '3b5f24ac72de35b6653b21ab5b0d84abcb025a9d4083a4386cfec3c18c5dbf4b',
            'Unexpected supporting physics comparison identity')
    verify(physics_reference)
    require(build['exitCode'] == 0 and build['sourceUnchanged'] is True
            and build['rulesFingerprint'] == build['postBuildRulesFingerprint'] == CANDIDATE_FP,
            'Candidate build identity failed')
    require(prereg['baselineRulesFingerprint'] == BASELINE_FP
            and prereg['baselineRunner']['sha256'] == BASELINE_RUNNER
            and build['artifact']['sha256'] == CANDIDATE_RUNNER, 'Unexpected runner/rule identity')
    require(completion['rulesFingerprint'] == completion['rulesFingerprintAfter'] == CANDIDATE_FP
            and completion['rulesVersion'] == 'v6.2' and completion['runner'] == build['artifact'],
            'Candidate completion identity mismatch')
    for reference in [build['preRegistration'], build['sourceBefore'], build['sourceAfter'],
                      build['artifact'], prereg['baselineRunner'], prereg['baselineSourceFreeze'],
                      completion['preRegistration'], completion['runnerBuild'], completion['orchestrator']]:
        verify(reference)
    require(read(verify(build['sourceBefore'])) == read(verify(build['sourceAfter'])), 'Build sources differ')
    compiled_before, compiled_after = RUN / 'compiled-source-before.json', RUN / 'compiled-source-after.json'
    require(read(compiled_before) == read(compiled_after), 'Measurement compiled sources differ')
    require(read(compiled_before) == [r for r in read(verify(build['sourceAfter'])) if '/tests/' not in r['path']],
            'Measured compiled sources differ from built sources')
    plan_path = verify(prereg['canonicalPlan']); plan = read(plan_path)
    require(completion['canonicalPlan'] == prereg['canonicalPlan'], 'Completion plan changed')
    require(plan['planId'] == 'v6-branch-1000-canonical-20260913'
            and len(plan['cases']) == 1000
            and all(case == canonical_case(i) for i, case in enumerate(plan['cases'])), 'Full canonical plan changed')
    indices = [i for i in range(1000) if canonical_case(i)['seed'] in SEEDS
               and canonical_case(i)['branches'].count('science') == 1]
    require(prereg['indices'] == indices and prereg['caseCount'] == len(indices) == 80
            and prereg['seeds'] == SEEDS and [c['index'] for c in prereg['cases']] == indices,
            'Selection differs from preregistered 80 IDs/order/seeds')
    require(sorted(c['index'] for c in completion['cases']) == indices, 'Completion case set differs')
    require({p.name for p in RUN.glob('results-*.jsonl')} == {f'results-{i:03}.jsonl' for i in indices},
            'Candidate raw file set differs from registered selection')
    paths, rows, paired, execution_refs = dict(baseline=[], candidate=[]), dict(baseline=[], candidate=[]), [], []
    for binding in prereg['cases']:
        i, case = binding['index'], binding['case']
        require(case == canonical_case(i) == plan['cases'][i], f'Preregistered case changed: {i}')
        baseline_path = verify(binding['preservedRaw']); original_path = verify(binding['originalRaw'])
        require(baseline_path.read_bytes() == original_path.read_bytes(), f'Baseline bytes changed: {i}')
        execution_path = RUN / f'execution-{i:03}.json'; execution = read(execution_path)
        require(execution['index'] == i and execution['exitCode'] == 0
                and execution['rulesFingerprint'] == CANDIDATE_FP and execution['runner'] == build['artifact']
                and execution['baseline'] == binding['originalRaw'], f'Candidate execution mismatch: {i}')
        candidate_path = verify(execution['raw'])
        require(candidate_path == RUN / f'results-{i:03}.jsonl', 'Unexpected candidate raw path')
        require(verify(execution['save']) == RUN / f'diagnostics/match-{i}.save.json',
                'Unexpected candidate Save path')
        command = execution['command']
        for flag, value in [('--first', str(i)), ('--limit', '1'), ('--minutes', '55'), ('--source-hash', CANDIDATE_FP)]:
            require(command[command.index(flag) + 1] == value, f'Execution argument mismatch: {i}/{flag}')
        require(Path(command[0]).resolve() == verify(build['artifact'])
                and Path(command[command.index('--plan') + 1]).resolve() == plan_path, 'Runner/plan command path mismatch')
        game = dict(index=i, case=case, seed=case['seed'], theme=case['theme'],
                    scienceOwner=case['branches'].index('science') + 1,
                    opponent=next(branch for branch in case['branches'] if branch != 'science'))
        game['sciencePlannerPosition'] = 'first' if case['plannerOrder'][0] == game['scienceOwner'] else 'second'
        for generation, path, save_reference, fingerprint in [
                ('baseline', baseline_path, binding['originalFinalSave'], BASELINE_FP),
                ('candidate', candidate_path, execution['save'], CANDIDATE_FP)]:
            row = one_row(path); check_row(row, case, plan, prereg['canonicalPlan']['sha256'], fingerprint)
            save_path = verify(save_reference); saved = read(save_path); snapshot = saved['snapshot']
            require(saved['rulesFingerprint'] == fingerprint and saved['rulesVersion'] == 'v6.2'
                    and snapshot['seed'] == row['seed'] and snapshot['theme'] == row['theme']
                    and snapshot['tick'] / 60 == row['seconds'] and snapshot['winner'] == row['winner']
                    and snapshot['winReason'] == row['winReason']
                    and saved['initialAi'] is False and saved['administrativeEvents'] == [],
                    f'Save identity/outcome/admin mismatch: {generation}/{i}')
            paths[generation].append(path); rows[generation].append(row)
            game[generation] = game_observation(row, saved, ref(path), ref(save_path))
        completed_case = next(c for c in completion['cases'] if c['index'] == i)
        require(completed_case['seconds'] == game['candidate']['seconds']
                and completed_case['unresolved'] == game['candidate']['unresolved'],
                f'Completion outcome differs from raw row: {i}')
        paired.append(game); execution_refs.append(ref(execution_path))
        if len(paired) % 20 == 0:
            print(json.dumps(dict(stage='paired-save-audit', audited=len(paired), total=80)), flush=True)
    for generation, fingerprint in [('baseline', BASELINE_FP), ('candidate', CANDIDATE_FP)]:
        require(release_gate.validate_match_rows(rows[generation], dict(rulesVersion='v6.2', rulesFingerprint=fingerprint), generation) == set(indices), 'Validated row IDs differ')
    run_id = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid.uuid4().hex[:8]
    audit_directory = BASE / 'analysis-runs' / run_id
    audit_directory.mkdir(parents=True, exist_ok=False)
    analyses = {generation: reviewed_analysis(generation, paths[generation], rows[generation],
                 dict(rulesVersion='v6.2', rulesFingerprint=fingerprint), audit_directory)
                for generation, fingerprint in [('baseline', BASELINE_FP), ('candidate', CANDIDATE_FP)]}
    opponents = {opponent: [p for p in paired if p['opponent'] == opponent] for opponent in OPPONENTS}
    require(all(len(value) == 20 for value in opponents.values()), 'Expected 20 games per opponent')
    report = dict(schemaVersion=1, recordedAtUtc=datetime.now(timezone.utc).isoformat(),
                  scope='All and only the preregistered 80 paired ordinary-Game cases; direction probe, not full-matrix or final balance acceptance.',
                  integrityPassed=True, finalBalanceAcceptance=False, caseCount=80, indices=indices,
                  baselineRulesFingerprint=BASELINE_FP, candidateRulesFingerprint=CANDIDATE_FP,
                  intervention=dict(clarification=ref(clarification_path),
                                    changes=clarification['intervention'],
                                    attributionLimit=clarification['notClaimed'],
                                    supportingEmbeddedHostPhysics=physics_reference,
                                    supportingPhysicsIncludedIn80GameStatistics=False),
                  sourceIdentity=dict(preRegistration=ref(prereg_path), canonicalPlan=ref(plan_path),
                                      baselineRunner=prereg['baselineRunner'], baselineSourceFreeze=prereg['baselineSourceFreeze'],
                                      candidateRunner=build['artifact'], candidateBuild=ref(build_path),
                                      candidateBuildSourceBefore=build['sourceBefore'], candidateBuildSourceAfter=build['sourceAfter'],
                                      candidateCompletion=ref(completion_path), compiledSourceBefore=ref(compiled_before),
                                      compiledSourceAfter=ref(compiled_after), executionReceipts=execution_refs,
                                      aggregator=ref(Path(aggregate.__file__)), gate=ref(Path(release_gate.__file__)),
                                      helper=ref(Path(__file__))),
                  reviewedAnalyses=analyses, cases=paired, all80=summarize_group(paired),
                  byOpponent={opponent: dict(**summarize_group(value),
                                            byScienceSpawnAndPlanner={f'{owner}/{position}': summarize_group([p for p in value if p['scienceOwner'] == owner and p['sciencePlannerPosition'] == position]) for owner in [1, 2] for position in ['first', 'second']}) for opponent, value in opponents.items()},
                  bySeed={str(seed): dict(allOpponents=summarize_group([p for p in paired if p['seed'] == seed]),
                                         byOpponent={opponent: summarize_group([p for p in value if p['seed'] == seed]) for opponent, value in opponents.items()}) for seed in SEEDS},
                  byScienceSpawnOwner={str(owner): summarize_group([p for p in paired if p['scienceOwner'] == owner]) for owner in [1, 2]},
                  bySciencePlannerPosition={position: summarize_group([p for p in paired if p['sciencePlannerPosition'] == position]) for position in ['first', 'second']},
                  byScienceSpawnAndPlanner={f'{owner}/{position}': summarize_group([p for p in paired if p['scienceOwner'] == owner and p['sciencePlannerPosition'] == position]) for owner in [1, 2] for position in ['first', 'second']},
                  warnings={generation: [dict(index=p['index'], warnings=p[generation]['warnings']) for p in paired if any(p[generation]['warnings'].values())] for generation in ['baseline', 'candidate']},
                  limitations=[
                      'Only these same 80 baseline/candidate cases are compared. Five preregistered seeds, swaps and shared opponents are correlated observations, not 80 independent random samples. Theme is coupled to seed.',
                      'The intervention combines full-range travel along a fixed direct-projectile aim ray and normalization to 28 world-units/second, including removal of the old incidental algorithm lead-related speed increase. These 80 outcomes do not isolate lifetime extension from velocity normalization. Supporting embedded-host physics cases are not pooled into the game statistics.',
                      'Roles are science and its actual opponent; all80 opponent pools combine four branches. Actual owner and planner order remain recorded. No self games or unrelated full-matrix aggregate enter this comparison.',
                      'Unresolved natural 55-minute outcomes remain in all-observation denominators. There is no assigned score winner. Duration includes real unresolved budget duration.',
                      'Any-branch T5 times are sampled each simulated second. Primary-branch timing uses conservative ordinary-order/final-Save attribution. Nulls and unavailable attribution remain explicit. Timing deltas use only identical case/owner observations with both times available; reach changes are separate.',
                      'nativeTotals.damage-dealt is native post-defense damage magnitude, potentially including overkill; it is not clamped actual HP loss, per-weapon damage, or proof of useful targeting. Sparse absent native event counters mean zero events.',
                      'delayedAreaMetrics.actualHpDamage is Gemini-active-only HP loss, excluding overkill; it is not all AI damage. Shield interceptedPayload is not prevented HP damage.',
                      'Native lostValue covers destroyed units and buildings/GPU, not independent room/wall/link removals or net economic efficiency. Operating ammo, compute, energy, transport and recoverable cargo are separate.',
                      'Supply starvation is eligible weapon-ready unit time failing payment; reasons overlap, compute is checked only after local ammo/energy suffice, and these are not lost-DPS estimates.',
                      'Paid compositions count accepted lifetime Deploy orders, not simultaneous force size or paid credit totals. AI shots/skills/passive events do not attribute damage; peakAliveAi is a maximum, not sustained participation.',
                      'Raw telemetry has no per-weapon hit-rate statistic or room-absorption statistic. This helper does not invent either or claim a full causal decomposition.',
                      'Passing row/aggregation integrity does not approve balance, duration, GPU, LAN, functionality or release. finalBalanceAcceptance remains false.',
                  ])
    require(sha(prereg_path) == PREREG_SHA and sha(Path(aggregate.__file__)) == AGGREGATOR_SHA
            and sha(Path(release_gate.__file__)) == GATE_SHA, 'Reviewed analysis identity changed during execution')
    write_new(destination, report)
    print(json.dumps(dict(report=str(destination), cases=80, integrityPassed=True,
                          finalBalanceAcceptance=False, reviewedAnalyses=analyses)), flush=True)


if __name__ == '__main__':
    main()
