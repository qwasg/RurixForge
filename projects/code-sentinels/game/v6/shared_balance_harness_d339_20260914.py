"""Frozen ordinary-Game harness with auditable, non-destructive dispatch controls."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import subprocess
import sys
import time

from balance_dispatch_control_d339_20260914 import DispatchControl
from balance_dispatch_queue_d339_20260914 import DispatchQueue

OWNED_PROCESSES = []
EMERGENCY_CONTEXT = {}


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def write_new(path, value):
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write('\n')


class CaseProcess:
    def __init__(self, process, stdout, stderr, invocation):
        self.process = process
        self.stdout = stdout
        self.stderr = stderr
        self.invocation = invocation
        self.started_monotonic = time.monotonic()

    def poll(self):
        return self.process.poll()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', required=True, type=Path)
    args = parser.parse_args()
    manifest_path = args.manifest.resolve()
    base = manifest_path.parent
    manifest = read(manifest_path)
    root = Path(manifest['workspaceRoot']).resolve()
    fp = manifest['rulesFingerprint']
    assert base.is_relative_to(root)
    EMERGENCY_CONTEXT.update(base=base, rulesFingerprint=fp)

    def reference(path):
        return {'path': path.resolve().relative_to(root).as_posix(), 'sha256': sha(path), 'bytes': path.stat().st_size}

    def verify(record):
        path = (root / record['path']).resolve()
        assert path.is_relative_to(root) and sha(path) == record['sha256'], str(path)
        return path

    def compiled_records():
        paths = []
        for directory in [root / 'native-v6', root / 'game/v6/balance-runner']:
            paths.extend(directory.joinpath('src').rglob('*.rs'))
            paths.extend(path for path in [directory / 'Cargo.toml', directory / 'Cargo.lock', directory / 'build.rs'] if path.is_file())
        return sorted([reference(path) for path in paths], key=lambda record: record['path'])

    expected_sources = read(verify(manifest['compiledSource']))

    def verify_frozen_inputs():
        for record in manifest['immutableInputs']:
            verify(record)
        assert compiled_records() == expected_sources, 'Compiled Game/runner source drift detected.'

    verify_frozen_inputs()
    executable = verify(manifest['runner'])
    definitions = {name: read(verify(value['executionPlan'])) for name, value in manifest['matrices'].items()}
    canonical = {name: read(verify(definition['plan'])) for name, definition in definitions.items()}
    jobs = read(verify(manifest['dispatchPlan']))['jobs']
    assert len(jobs) == 1126 and len({(job['matrix'], job['index']) for job in jobs}) == 1126
    assert sum(job['matrix'] == 'strategy' for job in jobs) == 126
    assert sum(job['matrix'] == 'branch' for job in jobs) == 1000
    expected_identity = [str(executable), '--source-hash', fp, '--maps', 'true', '--seeds', '1', '--out', str(base / 'identity/map-audit.json')]
    assert manifest['identityCommand'] == expected_identity
    for job in jobs:
        name, index = job['matrix'], job['index']
        expected_case = next(case for case in canonical[name]['cases'] if case['index'] == index)
        assert job['case'] == expected_case
        expected_command = [str(executable), '--plan', str(verify(definitions[name]['plan'])), '--first', str(index),
                            '--limit', '1', '--minutes', '55', '--source-hash', fp, '--stop-on-failure', 'true',
                            '--out', str(base / name / f'results-{index:03}.jsonl'), '--diagnostics', str(base / name / 'diagnostics')]
        assert job['command'] == expected_command
    start = dict(startedAtUtc=now(), pid=os.getpid(), rulesFingerprint=fp, rulesVersion=manifest['rulesVersion'],
                 manifest=reference(manifest_path), harness=manifest['harness'], plannedStrategyCases=126,
                 plannedBranchCases=1000, reusedRows=0, maxGameWorkers=16,
                 scope='All fresh ordinary-Game d339 cases. Controls affect dispatch only; no pause is added to simulated match time.')
    write_new(base / 'start-receipt.json', start)
    controller = DispatchControl(base / 'control.json', base / 'control-history')
    events_path = base / 'harness-events.jsonl'

    def event(kind, **values):
        with events_path.open('a', encoding='utf-8', newline='\n') as stream:
            stream.write(json.dumps(dict(observedAtUtc=now(), kind=kind, **values), ensure_ascii=False) + '\n')

    def status(stage, queue=None, identity_pid=None):
        active = [] if queue is None else [dict(pid=entry['handle'].process.pid, matrix=entry['job']['matrix'],
                                                  index=entry['job']['index']) for entry in queue.active]
        completed = [] if queue is None else queue.completed
        counts = {name: sum(row['matrix'] == name for row in completed) for name in ['strategy', 'branch']}
        controller.write_status(base / 'status.json', details=dict(
            stage=stage, orchestratorPid=os.getpid(), identityPid=identity_pid, activeProcesses=active,
            activeGameWorkers=len(active), completed=counts, dispatchedCases=0 if queue is None else queue.cursor,
            remainingUnstarted=1126 if queue is None else len(queue.remaining),
            pausedAndDrained=stage == 'paused-and-drained', rulesFingerprint=fp,
            simulationPaused=False, finalBalanceAcceptance=False,
        ))

    # A pause requested before first launch also prevents the one identity launch.
    while True:
        state = controller.poll()
        if state.stopRequested:
            status('stopped-before-identity')
            write_new(base / 'stop-after-active-receipt.json', dict(**start, endedAtUtc=now(), status='stopped-before-identity',
                      completedCases=0, notStartedCases=[dict(matrix=job['matrix'], index=job['index']) for job in jobs],
                      complete=False, finalBalanceAcceptance=False, control=state.serialize()))
            return
        if state.dispatchAllowed:
            break
        status('paused-and-drained')
        time.sleep(1)

    # Exactly one pre-registered normal identity launch. A policy block is not retried.
    identity = manifest['identityCommand']
    identity_invocation = dict(command=identity, cwd=str(root), startedAtUtc=now(), runner=manifest['runner'],
                               expectedCompiledRulesFingerprint=fp, control=state.serialize(),
                               scope='Existing runner source-hash assertion followed by a 3-theme/1-seed native map audit; no match simulation ticks.')
    write_new(base / 'identity/invocation.json', identity_invocation)
    identity_code = None
    stdout = (base / 'identity/stdout.log').open('x', encoding='utf-8')
    stderr = (base / 'identity/stderr.log').open('x', encoding='utf-8')
    try:
        process = subprocess.Popen(identity, cwd=root, stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
    except OSError as exc:
        stdout.close()
        stderr.close()
        blocked = dict(**identity_invocation, endedAtUtc=now(), status='identity-launch-blocked', processStarted=False,
                       winerror=getattr(exc, 'winerror', None), errno=exc.errno, errorType=type(exc).__name__, message=str(exc),
                       matrixCasesStarted=0, noRetry=True, noAlternativeNamePathOrArgumentsAttempted=True)
        write_new(base / 'identity/execution.json', blocked)
        write_new(base / 'blocked-receipt.json', blocked)
        event('identity-launch-blocked', winerror=blocked['winerror'], noRetry=True)
        status('identity-launch-blocked')
        print(json.dumps(blocked), flush=True)
        return
    # Ownership is retained immediately after Popen, before any fallible audit I/O.
    OWNED_PROCESSES.append(process)
    try:
        write_new(base / 'identity/process.json', dict(pid=process.pid, command=identity, startedAtUtc=identity_invocation['startedAtUtc']))
        status('identity-running', identity_pid=process.pid)
        identity_code = process.wait()
    finally:
        stdout.close()
        stderr.close()
    identity_report = base / 'identity/map-audit.json'
    identity_passed = identity_code == 0 and identity_report.is_file() and read(identity_report).get('passed') is True
    identity_receipt = dict(**identity_invocation, endedAtUtc=now(), exitCode=identity_code, processStarted=True,
                            passed=identity_passed, mapAudit=reference(identity_report) if identity_report.exists() else None,
                            compiledFingerprintAssertionPassed=identity_passed)
    write_new(base / 'identity/execution.json', identity_receipt)
    if not identity_passed:
        write_new(base / 'blocked-receipt.json', dict(**identity_receipt, status='identity-verification-not-passed', matrixCasesStarted=0, noRetry=True))
        status('identity-verification-not-passed')
        print(json.dumps(dict(type='identity-not-passed', exitCode=identity_code, matrixCasesStarted=0)), flush=True)
        return
    verify_frozen_inputs()
    print(json.dumps(dict(type='identity-passed', rulesFingerprint=fp, runnerSha256=manifest['runner']['sha256'],
                          actualNativeIdentityExecution=True, maxGameWorkers=16)), flush=True)

    def launch(job, control_state):
        out = base / job['matrix']
        index = job['index']
        invocation = dict(matrix=job['matrix'], index=index, command=job['command'], cwd=str(root), startedAtUtc=now(),
                          rulesFingerprint=fp, runner=manifest['runner'], controlAtDispatch=control_state.serialize())
        write_new(out / f'invocation-{index:03}.json', invocation)
        stdout = (out / f'run-{index:03}.stdout.log').open('x', encoding='utf-8')
        stderr = (out / f'run-{index:03}.stderr.log').open('x', encoding='utf-8')
        try:
            process = subprocess.Popen(job['command'], cwd=root, stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
        except OSError as exc:
            stdout.close()
            stderr.close()
            write_new(out / f'launch-error-{index:03}.json', dict(**invocation, observedAtUtc=now(), processStarted=False,
                      winerror=getattr(exc, 'winerror', None), errno=exc.errno, error=str(exc), noRetry=True))
            raise
        OWNED_PROCESSES.append(process)
        handle = CaseProcess(process, stdout, stderr, invocation)
        handle.post_launch_audit_error = None
        try:
            write_new(out / f'process-{index:03}.json', dict(pid=process.pid, matrix=job['matrix'], index=index, startedAtUtc=invocation['startedAtUtc']))
        except Exception as exc:
            handle.post_launch_audit_error = str(exc)
            queue.request_drain(f'Post-launch audit error for {job["matrix"]}/{index}: {exc}')
            print(json.dumps(dict(type='post-launch-audit-error-draining', pid=process.pid, error=str(exc))), file=sys.stderr, flush=True)
        return handle

    def finish(job, handle, code):
        handle.stdout.close()
        handle.stderr.close()
        out, index, case = base / job['matrix'], job['index'], job['case']
        raw, saved = out / f'results-{index:03}.jsonl', out / f'diagnostics/match-{index}.save.json'
        receipt = dict(**handle.invocation, pid=handle.process.pid, exitCode=code, endedAtUtc=now(),
                       wallSeconds=time.monotonic() - handle.started_monotonic,
                       raw=reference(raw) if raw.exists() else None, save=reference(saved) if saved.exists() else None)
        write_new(out / f'execution-{index:03}.json', receipt)
        assert code == 0, f'{job["matrix"]}/{index}: actual runner exit {code}'
        row, save = read(raw), read(saved)
        assert all(row[key] == case[key] for key in ['index', 'seed', 'theme', 'branches', 'strategies', 'swapped', 'plannerOrder'])
        assert row['originalPair'] == case.get('originalPair')
        assert row['planIndex'] == index and row['planSha256'] == definitions[job['matrix']]['plan']['sha256']
        assert row['planId'] == canonical[job['matrix']]['planId']
        assert row['simulationSourceSha256'] == fp and row['rulesVersion'] == manifest['rulesVersion']
        assert save['rulesFingerprint'] == fp and save['snapshot']['tick'] / 60 == row['seconds']
        assert save['snapshot']['winner'] == row['winner'] and save['snapshot']['winReason'] == row['winReason']
        assert not save['initialAi'] and save['administrativeEvents'] == []
        assert 0 < row['seconds'] <= 3300 and (row['winner'] in [1, 2] or row['seconds'] == 3300)
        if handle.post_launch_audit_error:
            raise RuntimeError('Native outcome retained, but post-launch process receipt failed: ' + handle.post_launch_audit_error)
        return dict(matrix=job['matrix'], index=index, seconds=row['seconds'], winner=row['winner'],
                    unresolved=row['winner'] is None, raw=receipt['raw'], save=receipt['save'], execution=reference(out / f'execution-{index:03}.json'))

    queue = DispatchQueue(jobs, launch, finish, max_workers=16)
    matrix_written = set()
    last_stage, last_status, last_integrity = None, 0.0, 0.0
    began = time.monotonic()
    event('measurement-start', plannedCases=1126, allFresh=True)
    while not queue.finished:
        try:
            state = controller.poll()
            if state.dispatchAllowed and not queue.stop_requested and time.monotonic() - last_integrity >= 30:
                try:
                    verify_frozen_inputs()
                except Exception as exc:
                    queue.request_drain(str(exc))
                    event('frozen-input-or-source-drift-drain-request', error=str(exc))
                last_integrity = time.monotonic()
            outcomes = queue.cycle(controller)
        except KeyboardInterrupt:
            queue.request_drain('KeyboardInterrupt received; finish active games normally.')
            event('interrupt-drain-request')
            continue
        for outcome in outcomes:
            counts = {name: sum(row['matrix'] == name for row in queue.completed) for name in ['strategy', 'branch']}
            print(json.dumps(dict(type='case-complete', strategyCompleted=counts['strategy'], branchCompleted=counts['branch'],
                                  matrix=outcome['matrix'], index=outcome['index'], seconds=outcome['seconds'], unresolved=outcome['unresolved'])), flush=True)
        counts = {name: [row for row in queue.completed if row['matrix'] == name] for name in ['strategy', 'branch']}
        for name, required in [('strategy', 126), ('branch', 1000)]:
            if name not in matrix_written and len(counts[name]) == required:
                current_sources = compiled_records()
                write_new(base / name / 'source-after.json', current_sources)
                if current_sources != expected_sources:
                    queue.request_drain('Compiled source drift at matrix boundary.')
                    event('matrix-outcomes-recorded-with-source-drift', matrix=name, cases=required)
                else:
                    write_new(base / name / 'completion-receipt.json', {
                        **start, 'matrix': name, 'endedAtUtc': now(), 'complete': True,
                        'completedFreshCases': required, 'requiredFreshCases': required, 'reusedCases': 0,
                        'failures': [], 'unchangedDuringMeasurement': True, 'completed': counts[name],
                        'scope': 'This complete matrix only; other shared-queue work may remain.',
                    })
                matrix_written.add(name)
                if current_sources == expected_sources:
                    event('matrix-complete', matrix=name, cases=required)
        if queue.stop_requested or queue.failures:
            stage = 'stop-requested-draining' if queue.active else 'stopped-after-active'
        elif not queue.last_control.dispatchAllowed:
            stage = 'dispatch-paused-draining' if queue.active else 'paused-and-drained'
        else:
            stage = 'measuring'
        if stage != last_stage:
            event('dispatch-stage-change', previous=last_stage, stage=stage, active=len(queue.active),
                  unstarted=len(queue.remaining), control=queue.last_control.serialize())
            last_stage = stage
        if outcomes or time.monotonic() - last_status >= 5 or stage != 'measuring':
            status(stage, queue)
            last_status = time.monotonic()
        if not queue.finished:
            time.sleep(1 if not queue.active else 0.25)
    after = compiled_records()
    write_new(base / 'source-after.json', after)
    complete = queue.complete and after == expected_sources
    final = dict(**start, endedAtUtc=now(), elapsedWallSeconds=time.monotonic() - began,
                 status='completed' if complete else 'stopped-after-active' if not queue.failures else 'drained-after-error',
                 complete=complete, completedStrategyCases=sum(row['matrix'] == 'strategy' for row in queue.completed),
                 completedFreshBranchCases=sum(row['matrix'] == 'branch' for row in queue.completed), reusedBranchCases=0,
                 unchangedDuringMeasurement=after == expected_sources, failures=queue.failures,
                 unstartedCases=[dict(matrix=job['matrix'], index=job['index']) for job in queue.remaining],
                 finalControl=queue.last_control.serialize(), activeGameWorkers=0, finalBalanceAcceptance=False)
    write_new(base / ('completion-receipt.json' if complete else 'stop-after-active-receipt.json'), final)
    status(final['status'], queue)
    event('harness-ended', status=final['status'], complete=complete, activeGameWorkers=0)
    print(json.dumps(dict(type='harness-ended', **final)), flush=True)


def emergency_drain(error):
    """A final safety net for non-Game exceptions; never terminates a process."""
    outcomes = []
    for process in OWNED_PROCESSES:
        while process.poll() is None:
            try:
                process.wait(timeout=1)
            except subprocess.TimeoutExpired:
                pass
            except KeyboardInterrupt:
                print('Continuing natural drain of already launched processes.', file=sys.stderr, flush=True)
        outcomes.append(dict(pid=process.pid, actualExitCode=process.returncode))
    receipt = dict(observedAtUtc=now(), status='orchestrator-error-naturally-drained',
                   errorType=type(error).__name__, error=str(error), processes=outcomes,
                   activeOwnedProcesses=0, noProcessesTerminated=True, noGameOutcomesFabricated=True,
                   rulesFingerprint=EMERGENCY_CONTEXT.get('rulesFingerprint'), finalBalanceAcceptance=False)
    if EMERGENCY_CONTEXT.get('base'):
        try:
            write_new(EMERGENCY_CONTEXT['base'] / 'emergency-drain-receipt.json', receipt)
        except Exception as write_error:
            receipt['receiptWriteError'] = str(write_error)
    print(json.dumps(receipt), file=sys.stderr, flush=True)


if __name__ == '__main__':
    try:
        main()
    except BaseException as failure:
        emergency_drain(failure)
        raise
