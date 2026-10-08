"""Finalize completed 6828 statistics; Python analysis only, never native games.

Requires the shared run's complete 126-fresh + 920-fresh + 80-reused receipt.
Preserves exact raw order, runs the reviewed aggregator through runpy to avoid
Windows argv limits, checks existing release gates, then runs both summaries.
Existing outputs are never overwritten; incomplete finalization receipts remain.
"""
from pathlib import Path
from datetime import datetime, timezone
import contextlib
import hashlib
import json
import os
import runpy
import sys
import traceback

sys.dont_write_bytecode = True
import release_gate
import summarize_balance

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-6828c5b7-20260914'
FP = '6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781'
RUNNER_SHA = 'e5ccbd4912eba1302fe39ea7f2690785d28a4e69828c13c4cd967295e107e022'
AGGREGATOR_SHA = '6a26e8958522ec779a598692a4f0df5841fa251189504af9c6b33d99d601f6d8'
GATE_SHA = '8a056b911df2039008d6e5684952781918a15acfefbf8845233d0a017dbdb0c8'
MATRICES = {'strategy': (126, 126, 0), 'branch': (1000, 920, 80)}
SUMMARIES = {
    'strategy': ('summarize_final_strategy_6828_20260914.py', 'paired-strategy-analysis.json'),
    'branch': ('summarize_final_branches_6828_20260914.py', 'branch-review-data.json'),
}


def now():
    return datetime.now(timezone.utc).isoformat()


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
    require(path.is_relative_to(ROOT), 'Reference leaves the project')
    require(path.is_file() and sha(path) == reference['sha256'], f'Hash mismatch: {path}')
    require('bytes' not in reference or path.stat().st_size == reference['bytes'],
            f'Byte count mismatch: {path}')
    return path


def write_new(path, value):
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write('\n')


def python_stage(script, arguments, out, prefix, metadata):
    """A logical Python invocation, executed in-process with receipts and logs."""
    stdout_path, stderr_path = out / f'{prefix}.stdout.log', out / f'{prefix}.stderr.log'
    invocation = dict(command=[sys.executable, str(script), *arguments], cwd=str(ROOT),
                      transport='In-process runpy with exact sys.argv; no OS command-line expansion.',
                      script=ref(script), startedAtUtc=now(), **metadata)
    write_new(out / f'{prefix}-invocation.json', invocation)
    original_argv, original_cwd = sys.argv, Path.cwd()
    code = 0
    with stdout_path.open('x', encoding='utf-8', newline='\n') as stdout, \
            stderr_path.open('x', encoding='utf-8', newline='\n') as stderr:
        try:
            verify(invocation['script'])
            sys.argv = [str(script), *arguments]
            os.chdir(ROOT)
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                try:
                    runpy.run_path(str(script), run_name='__main__')
                except SystemExit as exit_result:
                    code = 0 if exit_result.code is None else exit_result.code if isinstance(exit_result.code, int) else 1
                    if code:
                        print(str(exit_result), file=stderr)
                except BaseException:
                    code = 1
                    traceback.print_exc(file=stderr)
        except BaseException:
            code = 1
            traceback.print_exc(file=stderr)
        finally:
            sys.argv = original_argv
            os.chdir(original_cwd)
    execution = dict(**invocation, exitCode=code, endedAtUtc=now(),
                     stdout=ref(stdout_path), stderr=ref(stderr_path))
    write_new(out / f'{prefix}-execution.json', execution)
    print(json.dumps(dict(stage=prefix, matrix=out.name, exitCode=code,
                          execution=str(out / f'{prefix}-execution.json'))), flush=True)
    require(code == 0, f'{out.name}/{prefix} failed; original logs and all outputs are retained')
    require(sha(script) == invocation['script']['sha256'], 'Python stage source changed while running')
    return ref(out / f'{prefix}-execution.json')


def validate_completion(manifest):
    path = BASE / 'completion-receipt.json'
    require(path.is_file(), 'Refusing finalization until the shared full run completes')
    completed = read(path)
    require(completed.get('complete') is True and completed.get('failures') == []
            and completed.get('unchangedDuringMeasurement') is True
            and completed.get('rulesFingerprint') == FP
            and completed.get('completedStrategyCases') == 126
            and completed.get('completedFreshBranchCases') == 920
            and completed.get('reusedBranchCases') == 80, 'Shared completion is incomplete or inconsistent')
    require(verify(completed['manifest']) == BASE / 'measurement-manifest.json', 'Completion manifest mismatch')
    verify(completed['orchestrator'])
    before = read(verify(manifest['sourceBefore']))
    require(before == read(BASE / 'source-after.json'), 'Shared source records changed during measurement')
    for item in before:
        frozen = BASE / 'source' / item['path']
        require(frozen.is_file() and sha(frozen) == item['sha256'], f'Frozen compiled source mismatch: {frozen}')
    return completed


def collect_matrix(name, manifest, reused):
    out = BASE / name
    required, fresh, reused_count = MATRICES[name]
    definition = manifest['matrices'][name]
    require((definition['requiredCases'], definition['freshCases'], definition['reusedCases'])
            == (required, fresh, reused_count), f'{name}: unexpected planned counts')
    plan_path = verify(definition['inputPlan']); plan = read(plan_path)
    execution_plan = read(verify(definition['executionPlan']))
    require(execution_plan['plan'] == definition['inputPlan']
            and execution_plan['requiredTotalCases'] == required
            and execution_plan['freshCases'] == fresh
            and execution_plan['reusedOriginalCases'] == reused_count, f'{name}: execution plan differs')
    completion_path = out / 'completion-receipt.json'; completion = read(completion_path)
    require(completion['complete'] is True and completion['failures'] == []
            and completion['unchangedDuringMeasurement'] is True and completion['rulesFingerprint'] == FP
            and completion['matrix'] == name and completion['completedFreshCases'] == fresh
            and completion['requiredFreshCases'] == fresh and completion['reusedCases'] == reused_count,
            f'{name}: per-matrix completion mismatch')
    require(verify(completion['manifest']) == BASE / 'measurement-manifest.json', 'Per-matrix manifest mismatch')
    require(read(out / 'source-after.json') == read(BASE / 'source-after.json'), 'Per-matrix source records differ')
    jobs = {job['index']: job for job in execution_plan['jobs']}
    require(len(jobs) == len(execution_plan['jobs']) == fresh
            and sorted(c['index'] for c in completion['completed']) == sorted(jobs),
            f'{name}: missing or duplicate fresh completion IDs')
    completed_cases = {c['index']: c for c in completion['completed']}
    # Required order: all original reused files first, then all fresh files.
    reused_paths = sorted(out.glob('reused-results-*.jsonl'))
    fresh_paths = sorted(out.glob('results-*.jsonl'))
    require(len(reused_paths) == reused_count and len(fresh_paths) == fresh, f'{name}: raw file counts differ')
    require({p.name for p in fresh_paths} == {f'results-{i:03}.jsonl' for i in jobs}, 'Unexpected fresh raw filenames')
    if name == 'branch':
        require(set(jobs).isdisjoint(reused) and set(jobs) | set(reused) == set(range(required)),
                'Branch reuse and fresh IDs overlap or omit cases')
        require({p.name for p in reused_paths} == {f'reused-results-{i:03}.jsonl' for i in reused},
                'Unexpected reused raw filenames')
    else:
        require(set(jobs) == set(range(required)), 'Strategy must be 126 fresh cases')
    paths = reused_paths + fresh_paths
    rows, inputs, execution_refs = [], [], []
    for path in paths:
        parsed = [json.loads(line) for line in path.read_text(encoding='utf-8-sig').splitlines() if line.strip()]
        require(len(parsed) == 1, f'{name}: each file must retain exactly one canonical raw row')
        row = parsed[0]; index = row['index']; case = plan['cases'][index]
        require(row['index'] == case['index'] and all(row[key] == case[key]
                for key in ['seed', 'theme', 'branches', 'strategies', 'swapped', 'plannerOrder'])
                and row['originalPair'] == case.get('originalPair')
                and row['planId'] == plan['planId'] and row['planIndex'] == index
                and row['planSha256'] == definition['inputPlan']['sha256']
                and row['simulationSourceSha256'] == FP and row['rulesVersion'] == 'v6.2',
                f'{name}: raw identity mismatch at {index}')
        require(0 < row['seconds'] <= 3300 and (row['winner'] is not None or row['seconds'] == 3300),
                f'{name}: altered natural outcome/budget at {index}')
        if path.name.startswith('reused-'):
            record = reused[index]
            require(verify(record['preservedRaw']) == path, 'Reused raw path mismatch')
            require(verify(record['preservedSave']) == out / f'diagnostics/match-{index}.save.json', 'Reused Save path mismatch')
        else:
            execution_path = out / f'execution-{index:03}.json'; execution = read(execution_path)
            require(execution['exitCode'] == 0 and execution['index'] == index and execution['matrix'] == name
                    and execution['rulesFingerprint'] == FP and execution['runner'] == manifest['runner']
                    and execution['command'] == jobs[index]['command'], 'Fresh execution identity mismatch')
            require(verify(execution['raw']) == path
                    and verify(execution['save']) == out / f'diagnostics/match-{index}.save.json', 'Fresh output hash/path mismatch')
            require(completed_cases[index]['seconds'] == row['seconds']
                    and completed_cases[index]['unresolved'] == (row['winner'] is None), 'Fresh completion outcome mismatch')
            execution_refs.append(ref(execution_path))
        rows.append(row); inputs.append(ref(path))
    require(len(rows) == required and {r['index'] for r in rows} == set(range(required)), 'Matrix row IDs differ')
    return dict(paths=paths, rows=rows, inputs=inputs, plan=plan, planPath=plan_path,
                executionReceipts=execution_refs, completion=ref(completion_path))


def main():
    manifest_path = BASE / 'measurement-manifest.json'; manifest = read(manifest_path)
    require(manifest['rulesFingerprint'] == FP and manifest['rulesVersion'] == 'v6.2'
            and manifest['runner']['sha256'] == RUNNER_SHA, 'Unexpected frozen 6828 identity')
    aggregator_path = verify(manifest['aggregator']); verify(manifest['runner'])
    require(sha(aggregator_path) == AGGREGATOR_SHA == sha(Path(summarize_balance.__file__))
            and sha(Path(release_gate.__file__)) == GATE_SHA, 'Reviewed aggregator or gate changed')
    validate_completion(manifest)
    # Preflight all destinations before creating either analysis; failures retain receipts.
    destinations = [BASE / 'stats-finalization-start.json', BASE / 'stats-finalization-completion.json']
    for name, (_, summary_output) in SUMMARIES.items():
        out = BASE / name
        destinations += [out / item for item in ['analysis.json', 'strict-integrity-check.json', summary_output]]
        for prefix in ['aggregate', 'summary']:
            destinations += [out / f'{prefix}{suffix}' for suffix in
                             ['-invocation.json', '-execution.json', '.stdout.log', '.stderr.log']]
    require(not any(path.exists() for path in destinations),
            'Existing finalization outputs preserved; do not overwrite or delete partial evidence')
    reuse_path = verify(manifest['reuseAudit']); reuse_audit = read(reuse_path)
    require(reuse_audit['rulesFingerprint'] == FP and reuse_audit['runnerSha256'] == RUNNER_SHA,
            'Reuse audit identity differs')
    verify(reuse_audit['strictPairedAudit'])
    reused = {record['index']: record for record in reuse_audit['cases']}
    require(len(reused) == len(reuse_audit['cases']) == 80, 'Exactly 80 unique original rows required')
    for record in reused.values():
        for key in ['originalRaw', 'preservedRaw', 'originalSave', 'preservedSave', 'originalExecution']:
            verify(record[key])
        require(record['originalRaw']['sha256'] == record['preservedRaw']['sha256']
                and record['originalSave']['sha256'] == record['preservedSave']['sha256'], 'Reused bytes changed')
    matrices = {name: collect_matrix(name, manifest, reused) for name in MATRICES}
    summary_sources = {name: ref(ROOT / 'game/v6' / item[0]) for name, item in SUMMARIES.items()}
    start = dict(startedAtUtc=now(), scope='Statistics and descriptive reports only; no native invocation or final balance approval.',
                 manifest=ref(manifest_path), sharedCompletion=ref(BASE / 'completion-receipt.json'),
                 helper=ref(Path(__file__)), aggregator=ref(aggregator_path), gate=ref(Path(release_gate.__file__)),
                 summaryHelpers=summary_sources, finalBalanceAcceptance=False)
    write_new(BASE / 'stats-finalization-start.json', start)
    stages, audits = {}, {}
    target = dict(rulesVersion='v6.2', rulesFingerprint=FP)
    try:
        for name, matrix in matrices.items():
            out = BASE / name
            arguments = [str(path) for path in matrix['paths']] + ['--output', str(out / 'analysis.json'),
                         '--expected-games', str(MATRICES[name][0])]
            if name == 'branch':
                arguments.append('--require-branch-matrix')
            stages[name + '/aggregate'] = python_stage(aggregator_path, arguments, out, 'aggregate',
                                                       dict(inputs=matrix['inputs'], target=target))
            for reference in matrix['inputs']:
                verify(reference)
            analysis = read(out / 'analysis.json')
            if name == 'strategy':
                release_gate.strategy_check(matrix['rows'], target, matrix['plan'], sha(matrix['planPath']))
            else:
                release_gate.matrix_check(matrix['rows'], target)
            release_gate.verify_analysis(analysis, matrix['rows'], [r['sha256'] for r in matrix['inputs']],
                                         target, '6828 final ' + name)
            audit = dict(recordedAtUtc=now(), scope='Existing canonical checks and exact reviewed aggregator recomputation. No criterion or final balance approval.',
                         matrix=name, **target, games=len(matrix['rows']), completed=analysis['completed'],
                         unresolved=analysis['unresolved'], passed=True, finalBalanceAcceptance=False,
                         runner=manifest['runner'], inputs=matrix['inputs'], analysis=ref(out / 'analysis.json'),
                         gate=ref(Path(release_gate.__file__)), aggregate=manifest['aggregator'],
                         plan=ref(matrix['planPath']), completion=matrix['completion'],
                         freshExecutionReceipts=matrix['executionReceipts'], reuseAudit=ref(reuse_path) if name == 'branch' else None)
            write_new(out / 'strict-integrity-check.json', audit)
            audits[name] = ref(out / 'strict-integrity-check.json')
            print(json.dumps(dict(stage='strict-integrity', matrix=name, games=len(matrix['rows']), passed=True)), flush=True)
        # Both canonical/aggregate gates must pass before either descriptive helper runs.
        for name, (_, output_name) in SUMMARIES.items():
            script = verify(summary_sources[name]); out = BASE / name
            stages[name + '/summary'] = python_stage(script, [], out, 'summary', dict(strictIntegrityCheck=audits[name]))
            summary = read(out / output_name)
            require(summary['integrityPassed'] is True and summary['finalBalanceAcceptance'] is False,
                    'Supplemental report must not manufacture final acceptance')
        require(sha(aggregator_path) == AGGREGATOR_SHA and sha(Path(release_gate.__file__)) == GATE_SHA,
                'Reviewed Python sources changed during finalization')
        result = dict(**start, endedAtUtc=now(), complete=True, stages=stages, strictIntegrityChecks=audits,
                      analyses={name: ref(BASE / name / 'analysis.json') for name in MATRICES},
                      summaries={name: ref(BASE / name / item[1]) for name, item in SUMMARIES.items()})
    except BaseException as error:
        write_new(BASE / 'stats-finalization-completion.json',
                  dict(**start, endedAtUtc=now(), complete=False, stages=stages,
                       strictIntegrityChecks=audits, error=repr(error), traceback=traceback.format_exc()))
        raise
    write_new(BASE / 'stats-finalization-completion.json', result)
    print(json.dumps(dict(stage='statistics-complete', strategyCases=126, branchCases=1000,
                          finalBalanceAcceptance=False, receipt=str(BASE / 'stats-finalization-completion.json'))), flush=True)


if __name__ == '__main__':
    main()
