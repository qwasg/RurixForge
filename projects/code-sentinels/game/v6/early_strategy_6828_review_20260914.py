"""Audit completed strategy126 separately while the shared branch run continues.

Writes only strategy/early-review. Original raw files, Saves, final matrix
completion receipts and finalizer outputs are never created or modified here.
Only Python aggregation/analysis is executed; no native game or compiler runs.
"""
from pathlib import Path
import hashlib
import json
import sys

sys.dont_write_bytecode = True
import finalize_full_6828_stats_20260914 as support
import release_gate
import summarize_balance

ROOT, BASE, FP = support.ROOT, support.BASE, support.FP
CURRENT = BASE / 'strategy'
EARLY = CURRENT / 'early-review'
read, sha, ref, verify = support.read, support.sha, support.ref, support.verify
require, write_new, now = support.require, support.write_new, support.now
SCOPE = 'Only strategy126 is complete. The shared branch measurement is still in progress at evidence collection; this separate early audit does not approve branch1000 or final balance/release.'


def current_fingerprint():
    native = ROOT / 'native-v6'
    paths = sorted([*native.joinpath('src').rglob('*.rs'),
                    *[native / name for name in ['Cargo.toml', 'Cargo.lock', 'build.rs']]],
                   key=lambda path: path.relative_to(native).as_posix())
    digest = hashlib.sha256(b'code-sentinels-native-rules-v1\0')
    for path in paths:
        name, data = path.relative_to(native).as_posix().encode(), path.read_bytes()
        digest.update(len(name).to_bytes(8, 'little')); digest.update(name)
        digest.update(len(data).to_bytes(8, 'little')); digest.update(data)
    return digest.hexdigest()


def check_sources(manifest):
    records = read(verify(manifest['sourceBefore']))
    for item in records:
        verify(item)
        frozen = BASE / 'source' / item['path']
        require(sha(frozen) == item['sha256'], f'Frozen compiled source changed: {item["path"]}')
    require(current_fingerprint() == FP, 'Live native rules fingerprint differs from frozen 6828')
    return records


def main():
    require(not (BASE / 'completion-receipt.json').exists(),
            'Shared run already ended; use the normal full finalizer instead of an early audit')
    require(not EARLY.exists(), 'Existing early-review evidence is preserved; do not overwrite it')
    manifest_path = BASE / 'measurement-manifest.json'; manifest = read(manifest_path)
    start = read(BASE / 'start-receipt.json')
    require(verify(start['manifest']) == manifest_path, 'Shared start manifest differs')
    verify(start['orchestrator'])
    require(manifest['rulesVersion'] == 'v6.2' and manifest['rulesFingerprint'] == FP
            and manifest['runner']['sha256'] == support.RUNNER_SHA, 'Unexpected measurement identity')
    runner = verify(manifest['runner']); aggregator = verify(manifest['aggregator'])
    require(sha(aggregator) == support.AGGREGATOR_SHA == sha(Path(summarize_balance.__file__))
            and sha(Path(release_gate.__file__)) == support.GATE_SHA, 'Reviewed aggregator or gate changed')
    definition = manifest['matrices']['strategy']
    require((definition['requiredCases'], definition['freshCases'], definition['reusedCases']) == (126, 126, 0),
            'Early strategy audit requires exactly126 fresh cases and no reused strategy rows')
    plan_path = verify(definition['inputPlan']); plan = read(plan_path)
    execution_plan = read(verify(definition['executionPlan']))
    jobs = {job['index']: job for job in execution_plan['jobs']}
    require(len(execution_plan['jobs']) == 126 and set(jobs) == set(range(126))
            and execution_plan['plan'] == definition['inputPlan'], 'Execution plan IDs/identity differ')
    raw_paths = sorted(CURRENT.glob('results-*.jsonl'))
    require({p.name for p in raw_paths} == {f'results-{i:03}.jsonl' for i in range(126)}
            and not list(CURRENT.glob('reused-results-*.jsonl')), 'Not all126 unique fresh raw files exist')
    expected_executions = {CURRENT / f'execution-{i:03}.json' for i in range(126)}
    require(all(path.is_file() for path in expected_executions), 'Not all126 execution receipts exist')
    source_before = check_sources(manifest)
    rows, inputs, executions, saves = [], [], [], []
    for path in raw_paths:
        parsed = [json.loads(line) for line in path.read_text(encoding='utf-8-sig').splitlines() if line.strip()]
        require(len(parsed) == 1, f'Expected one unchanged actual row: {path}')
        row = parsed[0]; index = row['index']
        require(type(index) is int and index in jobs and path.name == f'results-{index:03}.jsonl', 'Raw index/filename mismatch')
        job, case = jobs[index], plan['cases'][index]
        execution_path = CURRENT / f'execution-{index:03}.json'; execution = read(execution_path)
        require(execution['exitCode'] == 0 and execution['matrix'] == 'strategy'
                and execution['index'] == index and execution['rulesFingerprint'] == FP
                and execution['runner'] == manifest['runner'] and execution['command'] == job['command'],
                f'Execution did not complete with the approved runner/command: {index}')
        require(verify(execution['raw']) == path, 'Execution raw hash/path mismatch')
        save_path = verify(execution['save'])
        require(save_path == CURRENT / f'diagnostics/match-{index}.save.json', 'Save path mismatch')
        require(all(row[key] == case[key] for key in ['index', 'seed', 'theme', 'branches', 'strategies', 'swapped', 'plannerOrder'])
                and row['originalPair'] == case.get('originalPair')
                and row['rulesVersion'] == 'v6.2' and row['simulationSourceSha256'] == FP
                and row['planId'] == plan['planId'] and row['planIndex'] == index
                and row['planSha256'] == definition['inputPlan']['sha256'], f'Raw canonical/plan/rule metadata differs: {index}')
        require(0 < row['seconds'] <= 3300 and (row['winner'] is not None or row['seconds'] == 3300), 'Natural outcome/budget differs')
        saved = read(save_path); snapshot = saved['snapshot']
        require(saved['rulesFingerprint'] == FP and saved['rulesVersion'] == 'v6.2'
                and snapshot['tick'] / 60 == row['seconds'] and snapshot['winner'] == row['winner']
                and snapshot['winReason'] == row['winReason'] and snapshot['seed'] == row['seed']
                and snapshot['theme'] == row['theme'] and saved['initialAi'] is False
                and saved['administrativeEvents'] == [], f'Actual Save identity/outcome/admin mismatch: {index}')
        rows.append(row); inputs.append(ref(path)); executions.append(ref(execution_path)); saves.append(ref(save_path))
    require(len(rows) == 126 and {r['index'] for r in rows} == set(range(126)), 'Early strategy IDs are not unique/complete')
    target = dict(rulesVersion='v6.2', rulesFingerprint=FP)
    release_gate.strategy_check(rows, target, plan, definition['inputPlan']['sha256'])
    require(check_sources(manifest) == source_before, 'Compiled sources changed during collection')
    require(not (BASE / 'completion-receipt.json').exists(), 'Shared run ended during collection; use normal finalizer')
    EARLY.mkdir()
    completion = dict(recordedAtUtc=now(), scope=SCOPE, matrix='strategy', **target,
                      complete=True, requiredCases=126, completedFreshCases=126, reusedCases=0,
                      sharedRunComplete=False, unchangedDuringCollection=True, failures=[],
                      manifest=ref(manifest_path), sharedStart=ref(BASE / 'start-receipt.json'),
                      sourceBefore=manifest['sourceBefore'], runner=manifest['runner'],
                      inputPlan=definition['inputPlan'], executionReceipts=executions,
                      inputs=inputs, finalSaves=saves, helper=ref(Path(__file__)),
                      pythonStageSupport=ref(Path(support.__file__)), finalBalanceAcceptance=False)
    write_new(EARLY / 'completion-receipt.json', completion)
    arguments = [str(path) for path in raw_paths] + ['--output', str(EARLY / 'analysis.json'), '--expected-games', '126']
    support.python_stage(aggregator, arguments, EARLY, 'aggregate', dict(scope=SCOPE, inputs=inputs, target=target))
    for item in inputs:
        verify(item)
    analysis = read(EARLY / 'analysis.json')
    release_gate.strategy_check(rows, target, plan, definition['inputPlan']['sha256'])
    release_gate.verify_analysis(analysis, rows, [item['sha256'] for item in inputs], target, 'Early complete6828 strategy126')
    require(check_sources(manifest) == source_before, 'Compiled sources changed during aggregation')
    strict = dict(recordedAtUtc=now(), scope=SCOPE, matrix='strategy', **target, passed=True,
                  games=126, completed=analysis['completed'], unresolved=analysis['unresolved'],
                  finalBalanceAcceptance=False, runner=manifest['runner'], inputs=inputs,
                  analysis=ref(EARLY / 'analysis.json'), gate=ref(Path(release_gate.__file__)),
                  aggregate=manifest['aggregator'], plan=ref(plan_path), completion=ref(EARLY / 'completion-receipt.json'))
    write_new(EARLY / 'strict-integrity-check.json', strict)
    summary = ROOT / 'game/v6/summarize_final_strategy_6828_20260914.py'
    support.python_stage(summary, ['--audit-directory', str(EARLY)], EARLY, 'summary',
                         dict(scope=SCOPE, strictIntegrityCheck=ref(EARLY / 'strict-integrity-check.json')))
    report = read(EARLY / 'paired-strategy-analysis.json')
    require(report['integrityPassed'] is True and report['finalBalanceAcceptance'] is False
            and len(report['pairedCases']) == 126, 'Unexpected early comparison result')
    require(check_sources(manifest) == source_before, 'Compiled sources changed during supplemental review')
    write_new(EARLY / 'review-completion-receipt.json',
              dict(recordedAtUtc=now(), scope=SCOPE, complete=True, strategyCases=126,
                   strictIntegrityCheck=ref(EARLY / 'strict-integrity-check.json'),
                   pairedAnalysis=ref(EARLY / 'paired-strategy-analysis.json'),
                   liveCompiledSourcesStillMatch=True, finalBalanceAcceptance=False))
    print(json.dumps(dict(stage='early-strategy-review-complete', strategyCases=126,
                          branchCompletionClaimed=False, report=str(EARLY / 'paired-strategy-analysis.json'),
                          finalBalanceAcceptance=False)), flush=True)


if __name__ == '__main__':
    main()
