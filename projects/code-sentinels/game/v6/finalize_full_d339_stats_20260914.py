"""Finalize completed d339 matrices independently, retaining earlier outputs.

Usage: python -B finalize_full_d339_stats_20260914.py --matrix strategy|branch|all
Only completed selected matrices are processed. Repeated calls verify existing
canonical analyses, strict audits and statistical receipts without overwriting.
Python-only runpy aggregation avoids Windows argv limits; no native launch.
"""
from pathlib import Path
import argparse
import json
import sys
import uuid

sys.dont_write_bytecode = True
import finalize_full_6828_stats_20260914 as io
import balance_dispatch_control_d339_20260914 as dispatch_control
import release_gate
import summarize_balance

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-d33999c4-20260914'
FP = 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'
RUNNER_SHA = '351c95e776c40df103cf1d6eb940f2749eb89cc0fd994dcc3b9c8f878e4bffb4'
COUNTS = {'strategy':126, 'branch':1000}
SUMMARIES = {'strategy':('summarize_final_strategy_d339_20260914.py','paired-strategy-analysis.json'),
             'branch':('summarize_final_branches_d339_20260914.py','branch-review-data.json')}
read, sha, ref, verify = io.read, io.sha, io.ref, io.verify
require, write_new, now = io.require, io.write_new, io.now


def verified_dispatch(state, event_cache):
    require(state['mode'] == 'resume' and state['dispatchAllowed'] is True
            and state['stopRequested'] is False and state['controlValid'] is True
            and state['validationError'] is None, 'Case dispatch was not authorized by its bound control snapshot')
    event_path = Path(state['auditEventPath']).resolve()
    require(event_path.is_relative_to(BASE / 'control-history/events'), 'Control event is outside this measurement')
    key = (str(event_path), state['auditEventSha256'])
    if key not in event_cache:
        require(sha(event_path) == state['auditEventSha256'], 'Bound control audit event hash changed')
        event = read(event_path)
        require(event['controlPath'] == str(BASE / 'control.json') and event['valid'] is True
                and event['schemaValid'] is True and event['effectiveMode'] == 'resume'
                and event['stopRequestedAfter'] is False, 'Control event was not a valid resume')
        raw_path = (BASE / 'control-history' / event['rawFile']).resolve()
        require(raw_path.is_relative_to(BASE / 'control-history/raw') and sha(raw_path) == event['rawSha256'],
                'Original authorizing control bytes changed')
        document = dispatch_control.parse_control(raw_path.read_bytes())
        require(document == event['parsedControl'] and document['command'] == 'resume'
                and document['revision'] == event['acceptedRevisionAfter'], 'Control event/bytes revision differs')
        previous = event['acceptedRevisionBefore']
        require(previous is None or document['revision'] > previous, 'Control event revision is not monotonic')
        event_cache[key] = (event, document, ref(event_path), ref(raw_path))
    event, document, event_ref, raw_ref = event_cache[key]
    require(state['rawSha256'] == event['rawSha256']
            and state['revision'] == state['lastAcceptedRevision'] == document['revision']
            and state['requestedCommand'] == document['command']
            and state['requestedAtUtc'] == document['requestedAtUtc'] and state['reason'] == document['reason'],
            'Dispatch snapshot does not identify the exact observed raw control')
    return dict(event=event_ref, raw=raw_ref, revision=document['revision'])


def verify_manifest(manifest):
    require(Path(manifest['workspaceRoot']).resolve() == ROOT
            and manifest['rulesFingerprint'] == FP and manifest['rulesVersion'] == 'v6.2'
            and manifest['runner']['sha256'] == RUNNER_SHA
            and manifest['allCasesFresh'] is True and manifest['reusedRows'] == 0, 'Unexpected all-fresh d339 identity')
    for item in manifest['immutableInputs']:
        verify(item)
    verify(manifest['runnerBuild']); verify(manifest['runner'])
    require(sha(verify(manifest['aggregator'])) == io.AGGREGATOR_SHA == sha(Path(summarize_balance.__file__))
            and sha(verify(manifest['gate'])) == io.GATE_SHA == sha(Path(release_gate.__file__)),
            'Reviewed aggregator/gate changed')
    control_source = next(item for item in manifest['harness'] if item['path'].endswith('/balance_dispatch_control_d339_20260914.py'))
    require(sha(Path(dispatch_control.__file__)) == control_source['sha256'], 'Control parser differs from the measured frozen module')
    sources = read(verify(manifest['compiledSource']))
    for item in sources:
        frozen = BASE / 'source' / item['path']
        require(sha(frozen) == item['sha256'], 'Frozen compiled source no longer matches its record')
    identity_path = BASE / 'identity/execution.json'; identity = read(identity_path)
    require(identity['processStarted'] is True and identity['exitCode'] == 0 and identity['passed'] is True
            and identity['compiledFingerprintAssertionPassed'] is True
            and identity['expectedCompiledRulesFingerprint'] == FP
            and identity['runner'] == manifest['runner'] and identity['command'] == manifest['identityCommand'],
            'One-shot native identity execution did not pass')
    verify(identity['mapAudit'])
    require(read(verify(identity['mapAudit']))['passed'] is True, 'Native identity map result failed')
    verified_dispatch(identity['control'], {})
    return sources, ref(identity_path)


def collect(name, manifest, sources):
    out, expected = BASE / name, COUNTS[name]
    completion_path = out / 'completion-receipt.json'
    require(completion_path.is_file(), f'{name} is not complete; no statistics may be finalized yet')
    completion = read(completion_path)
    require(completion['complete'] is True and completion['matrix'] == name
            and completion['completedFreshCases'] == completion['requiredFreshCases'] == expected
            and completion['reusedCases'] == 0 and completion['failures'] == []
            and completion['unchangedDuringMeasurement'] is True
            and completion['rulesFingerprint'] == FP, 'Per-matrix completion is inconsistent')
    require(verify(completion['manifest']) == BASE / 'measurement-manifest.json', 'Completion manifest changed')
    require(read(out / 'source-after.json') == sources, 'Matrix compiled sources changed during measurement')
    definition = manifest['matrices'][name]
    require((definition['requiredCases'],definition['freshCases'],definition['reusedCases']) == (expected,expected,0),
            'Unexpected matrix case/fresh/reuse counts')
    plan_path = verify(definition['inputPlan']); plan = read(plan_path)
    execution_plan = read(verify(definition['executionPlan']))
    require(execution_plan['plan'] == definition['inputPlan'], 'Execution and input plan differ')
    jobs = {job['index']:job for job in execution_plan['jobs']}
    require(len(jobs) == len(execution_plan['jobs']) == expected and set(jobs) == set(range(expected)),
            'Execution plan has missing/duplicate/noncanonical indices')
    completed = {item['index']:item for item in completion['completed']}
    require(len(completed) == len(completion['completed']) == expected and set(completed) == set(jobs),
            'Completion result IDs differ from the fresh execution plan')
    paths = sorted(out.glob('results-*.jsonl'))
    require({path.name for path in paths} == {f'results-{i:03}.jsonl' for i in jobs}
            and not list(out.glob('reused-results-*.jsonl')), 'Raw set is not exactly the fresh matrix')
    rows, inputs, executions, saves, controls = [], [], [], [], []
    cache = {}
    for path in paths:
        parsed = [json.loads(line) for line in path.read_text(encoding='utf-8-sig').splitlines() if line.strip()]
        require(len(parsed) == 1, 'Fresh raw file must contain exactly one case')
        row = parsed[0]; index = row['index']
        require(type(index) is int and index in jobs and path.name == f'results-{index:03}.jsonl', 'Raw index/file differs')
        case = plan['cases'][index]; job = jobs[index]
        require(job['case'] == case and all(row[key] == case[key] for key in ['index','seed','theme','branches','strategies','swapped','plannerOrder'])
                and row['originalPair'] == case.get('originalPair') and row['planId'] == plan['planId']
                and row['planIndex'] == index and row['planSha256'] == definition['inputPlan']['sha256']
                and row['simulationSourceSha256'] == FP and row['rulesVersion'] == 'v6.2', 'Canonical raw/plan identity differs')
        invocation_path = out / f'invocation-{index:03}.json'; invocation = read(invocation_path)
        execution_path = out / f'execution-{index:03}.json'; execution = read(execution_path)
        require(execution['exitCode'] == 0 and execution['index'] == index and execution['matrix'] == name
                and execution['runner'] == manifest['runner'] and execution['rulesFingerprint'] == FP
                and execution['command'] == job['command'], 'Actual execution identity/command differs')
        require(all(execution[key] == value for key,value in invocation.items()), 'Invocation/execution bindings differ')
        process = read(out / f'process-{index:03}.json')
        require(process['pid'] == execution['pid'] and process['index'] == index and process['matrix'] == name,
                'Native process receipt differs')
        require(verify(execution['raw']) == path and verify(completed[index]['raw']) == path
                and verify(completed[index]['execution']) == execution_path, 'Completion/raw/execution hashes differ')
        save_path = verify(execution['save'])
        require(save_path == out / f'diagnostics/match-{index}.save.json'
                and verify(completed[index]['save']) == save_path, 'Save reference differs')
        saved = read(save_path); snapshot = saved['snapshot']
        require(saved['rulesFingerprint'] == FP and saved['rulesVersion'] == 'v6.2'
                and snapshot['seed'] == row['seed'] and snapshot['theme'] == row['theme']
                and snapshot['tick']/60 == row['seconds'] and snapshot['winner'] == row['winner']
                and snapshot['winReason'] == row['winReason'] and saved['initialAi'] is False
                and saved['administrativeEvents'] == [], 'Actual Save identity/terminal reason/admin history differs')
        require(0 < row['seconds'] <= 3300 and (row['winner'] is not None or row['seconds'] == 3300)
                and completed[index]['seconds'] == row['seconds'] and completed[index]['winner'] == row['winner']
                and completed[index]['unresolved'] == (row['winner'] is None), 'Natural outcome/budget differs')
        controls.append(dict(index=index, invocation=ref(invocation_path),
                             **verified_dispatch(execution['controlAtDispatch'], cache)))
        rows.append(row); inputs.append(ref(path)); executions.append(ref(execution_path)); saves.append(ref(save_path))
    return dict(rows=rows, inputs=inputs, paths=paths, plan=plan, planPath=plan_path,
                completion=ref(completion_path), executions=executions, saves=saves, controls=controls)


def gate_rows(name, matrix, target):
    if name == 'strategy':
        release_gate.strategy_check(matrix['rows'], target, matrix['plan'], sha(matrix['planPath']))
    else:
        release_gate.matrix_check(matrix['rows'], target)


def finalize_matrix(name, matrix, manifest, identity, attempt):
    out = BASE / name; target = dict(rulesVersion='v6.2',rulesFingerprint=FP)
    analysis_path, strict_path = out / 'analysis.json', out / 'strict-integrity-check.json'
    script_name, summary_name = SUMMARIES[name]
    script, summary_path = ROOT / 'game/v6' / script_name, out / summary_name
    receipt_path = out / 'statistics-receipt.json'
    existing_receipt = read(receipt_path) if receipt_path.exists() else None
    if existing_receipt:
        require(existing_receipt['complete'] is True and existing_receipt['rulesFingerprint'] == FP,
                'Existing statistics receipt is incomplete or belongs to other rules')
        for item in existing_receipt['outputs'].values(): verify(item)
        verify(existing_receipt['summaryHelper']); verify(existing_receipt['matrixCompletion'])
    gate_rows(name, matrix, target)
    analysis_reused = analysis_path.exists()
    if not analysis_reused:
        require(not strict_path.exists() and not summary_path.exists(), 'Derived output exists without its analysis; preserve and investigate')
        arguments = [str(path) for path in matrix['paths']] + ['--output',str(analysis_path),'--expected-games',str(COUNTS[name])]
        if name == 'branch': arguments.append('--require-branch-matrix')
        io.python_stage(verify(manifest['aggregator']), arguments, out, 'aggregate',
                        dict(inputs=matrix['inputs'], target=target, scope='Completed selected matrix only.'))
    for item in matrix['inputs']: verify(item)
    analysis = read(analysis_path)
    release_gate.verify_analysis(analysis, matrix['rows'], [item['sha256'] for item in matrix['inputs']], target, 'd339 '+name)
    if strict_path.exists():
        strict = read(strict_path)
        require(strict['passed'] is True and strict['games'] == COUNTS[name] and strict['rulesFingerprint'] == FP
                and verify(strict['analysis']) == analysis_path and verify(strict['gate']) == Path(release_gate.__file__).resolve()
                and [item['sha256'] for item in strict['inputs']] == [item['sha256'] for item in matrix['inputs']],
                'Existing strict audit does not bind the same analysis and ordered raw rows')
    else:
        strict = dict(recordedAtUtc=now(), scope='This completed matrix only; shared work may continue. Existing canonical/aggregate checks passed without a balance decision.',
                      matrix=name, **target, passed=True, games=COUNTS[name], completed=analysis['completed'],
                      unresolved=analysis['unresolved'], finalBalanceAcceptance=False, runner=manifest['runner'],
                      inputs=matrix['inputs'], analysis=ref(analysis_path), gate=manifest['gate'], aggregate=manifest['aggregator'],
                      plan=ref(matrix['planPath']), completion=matrix['completion'], nativeIdentity=identity,
                      executionReceipts=matrix['executions'], finalSaves=matrix['saves'], controlAtDispatchEvidence=matrix['controls'])
        write_new(strict_path, strict)
    summary_reused = summary_path.exists()
    if not summary_reused:
        io.python_stage(script, [], out, 'summary', dict(strictIntegrityCheck=ref(strict_path)))
    summary = read(summary_path)
    require(summary['integrityPassed'] is True and summary['finalBalanceAcceptance'] is False, 'Supplemental output claimed unsupported acceptance')
    if name == 'strategy':
        require(summary['currentRulesFingerprint'] == FP and len(summary['pairedCases']) == 126
                and verify(summary['currentAnalysis']) == analysis_path
                and summary['baselineRulesFingerprint'] == '6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781',
                'Strategy comparison identity/baseline differs')
        recorded_inputs = summary['currentInputs']
    else:
        require(summary['rulesFingerprint'] == FP and summary['games'] == 1000 and verify(summary['analysis']) == analysis_path,
                'Branch supplemental identity differs')
        recorded_inputs = summary['inputFiles']
    require([item['sha256'] for item in recorded_inputs] == [item['sha256'] for item in matrix['inputs']], 'Supplemental ordered input identity differs')
    verify(summary['sourceIdentity']['helper'])
    result = dict(recordedAtUtc=now(), scope='Completed matrix statistical verification only; no final balance/release acceptance.',
                  complete=True, matrix=name, rulesFingerprint=FP, games=COUNTS[name], allFresh=True, reusedRows=0,
                  matrixCompletion=matrix['completion'], manifest=ref(BASE/'measurement-manifest.json'),
                  compiledSource=manifest['compiledSource'], nativeIdentity=identity, summaryHelper=ref(script),
                  finalizer=ref(Path(__file__)), outputs=dict(analysis=ref(analysis_path), strictIntegrityCheck=ref(strict_path), summary=ref(summary_path)),
                  finalBalanceAcceptance=False)
    if existing_receipt:
        require(existing_receipt['outputs'] == result['outputs'], 'Existing canonical statistical outputs changed')
    else:
        write_new(receipt_path, result)
    verified = dict(matrix=name, analysisReused=analysis_reused, summaryReused=summary_reused,
                    statisticsReceipt=ref(receipt_path), outputs=result['outputs'])
    write_new(attempt / f'{name}-verification.json', verified)
    return verified


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--matrix', choices=['strategy','branch','all'], default='all')
    option = parser.parse_args().matrix
    selected = list(COUNTS) if option == 'all' else [option]
    manifest = read(BASE / 'measurement-manifest.json')
    sources, identity = verify_manifest(manifest)
    # Refuse an incomplete selected matrix before producing any statistical output.
    matrices = {name:collect(name,manifest,sources) for name in selected}
    attempt = BASE / 'statistics-runs' / uuid.uuid4().hex
    attempt.mkdir(parents=True,exist_ok=False)
    start = dict(startedAtUtc=now(), requestedMatrix=option, selectedMatrices=selected,
                 scope='Only completed selected matrices; previously generated canonical outputs are verified and preserved.',
                 manifest=ref(BASE/'measurement-manifest.json'), finalizer=ref(Path(__file__)),
                 finalBalanceAcceptance=False)
    write_new(attempt/'invocation.json',start)
    results = {}
    try:
        for name,matrix in matrices.items():
            results[name] = finalize_matrix(name,matrix,manifest,identity,attempt)
        verify(manifest['aggregator']); verify(manifest['gate'])
    except BaseException as error:
        write_new(attempt/'completion.json',dict(**start,endedAtUtc=now(),complete=False,matrices=results,error=repr(error)))
        raise
    write_new(attempt/'completion.json',dict(**start,endedAtUtc=now(),complete=True,matrices=results))
    print(json.dumps(dict(stage='selected-statistics-complete',matrix=option,completedMatrices=selected,
                          receipt=str(attempt/'completion.json'),finalBalanceAcceptance=False)),flush=True)


if __name__ == '__main__':
    main()
