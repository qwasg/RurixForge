"""Execute one approved final matrix with at most sixteen immutable-runner workers."""
from pathlib import Path
import argparse
import concurrent.futures
import datetime
import hashlib
import json
import os
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-0acffa83-20260913'


def read(p):
    return json.loads(p.read_text(encoding='utf-8-sig'))


def sha(p):
    with p.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write(p, value):
    with p.open('x', encoding='utf-8', newline='\n') as f:
        json.dump(value, f, ensure_ascii=False, indent=2)
        f.write('\n')


def ref(p):
    return dict(path=p.relative_to(ROOT).as_posix(), sha256=sha(p), bytes=p.stat().st_size)


def verify(reference):
    path = ROOT / reference['path']
    assert sha(path) == reference['sha256'], str(path)
    return path


def current_sources(records):
    return [ref(ROOT / r['path']) for r in records]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('matrix', choices=['strategy', 'branch'])
    parser.add_argument('--authorization', required=True, help='Concrete root/native CPU-release instruction, retained in receipt.')
    args = parser.parse_args()
    manifest = read(BASE / 'measurement-manifest.json')
    out = BASE / args.matrix
    plan_ref = manifest['matrices'][args.matrix]['executionPlan']
    plan = read(verify(plan_ref))
    cases = {c['index']: c for c in read(verify(plan['plan']))['cases']}
    verify(manifest['runner']); verify(manifest['aggregator']); verify(manifest['reuseAudit'])
    before_path = verify(manifest['sourceBefore'])
    before = read(before_path)
    assert current_sources(before) == before, 'Frozen source changed before measurement.'
    orchestrator_snapshot = out / 'orchestrator-source.py'
    with orchestrator_snapshot.open('xb') as f:
        f.write(Path(__file__).read_bytes())
    start = dict(startedAtUtc=now(), pid=os.getpid(), matrix=args.matrix, authorization=args.authorization, executionPlan=plan_ref, manifest=ref(BASE / 'measurement-manifest.json'), orchestrator=ref(orchestrator_snapshot), maxConcurrentProcesses=16, requiredCases=plan['requiredTotalCases'], freshCases=plan['freshCases'], reusedCases=plan['reusedOriginalCases'])
    write(out / 'start-receipt.json', start)
    print(json.dumps(dict(type='matrix-start', **start)), flush=True)

    def run(job):
        i = job['index']; started = now(); timer = time.monotonic()
        invocation = dict(index=i, command=job['command'], cwd=str(ROOT), startedAtUtc=started, rulesFingerprint=manifest['rulesFingerprint'], runner=manifest['runner'])
        write(out / f'invocation-{i:03}.json', invocation)
        with (out / f'run-{i:03}.stdout.log').open('x', encoding='utf-8') as stdout, (out / f'run-{i:03}.stderr.log').open('x', encoding='utf-8') as stderr:
            proc = subprocess.Popen(job['command'], cwd=ROOT, stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
            write(out / f'process-{i:03}.json', dict(index=i, pid=proc.pid, startedAtUtc=started, invocation=ref(out / f'invocation-{i:03}.json')))
            code = proc.wait()
        result_path = out / f'results-{i:03}.jsonl'
        save_path = out / f'diagnostics/match-{i}.save.json'
        receipt = dict(**invocation, pid=proc.pid, exitCode=code, endedAtUtc=now(), wallSeconds=time.monotonic()-timer, raw=ref(result_path) if result_path.exists() else None, save=ref(save_path) if save_path.exists() else None, stdout=ref(out / f'run-{i:03}.stdout.log'), stderr=ref(out / f'run-{i:03}.stderr.log'))
        write(out / f'execution-{i:03}.json', receipt)
        assert code == 0, f'case {i}: runner exit {code}; receipts preserved'
        rows = [json.loads(line) for line in result_path.read_text(encoding='utf-8').splitlines() if line.strip()]
        assert len(rows) == 1
        r = rows[0]; case = cases[i]
        for key in ['index', 'seed', 'theme', 'branches', 'strategies', 'swapped', 'plannerOrder']:
            assert r[key] == case[key], (i, key)
        assert r['planIndex'] == i and r['planSha256'] == plan['plan']['sha256']
        assert r['originalPair'] == case.get('originalPair')
        assert r['simulationSourceSha256'] == manifest['rulesFingerprint'] and r['rulesVersion'] == manifest['rulesVersion']
        save = read(save_path)
        assert save['rulesFingerprint'] == manifest['rulesFingerprint']
        assert save['snapshot']['tick'] / 60 == r['seconds'] and 0 < r['seconds'] <= 3300
        assert save['snapshot']['winner'] == r['winner'] and save['snapshot']['winReason'] == r['winReason']
        assert not save['initialAi'] and not save['administrativeEvents']
        assert r['winner'] in [1, 2] or r['seconds'] == 3300
        return dict(index=i, seconds=r['seconds'], unresolved=r['winner'] is None, wallSeconds=receipt['wallSeconds'])

    jobs = iter(plan['jobs']); finished = []; failures = []
    began = time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
        pending = {pool.submit(run, next(jobs)): None for _ in range(min(16, len(plan['jobs'])))}
        while pending:
            done, _ = concurrent.futures.wait(pending, timeout=30, return_when=concurrent.futures.FIRST_COMPLETED)
            for future in done:
                pending.pop(future)
                try:
                    result = future.result(); finished.append(result)
                    print(json.dumps(dict(type='case-complete', matrix=args.matrix, completedFresh=len(finished), totalFresh=plan['freshCases'], **result)), flush=True)
                except BaseException as e:
                    failures.append(repr(e)); print(json.dumps(dict(type='case-failed', error=repr(e))), flush=True)
                if not failures:
                    job = next(jobs, None)
                    if job is not None:
                        pending[pool.submit(run, job)] = None
            if not done:
                print(json.dumps(dict(type='matrix-progress', matrix=args.matrix, completedFresh=len(finished), totalFresh=plan['freshCases'], activeWorkers=len(pending), elapsedWallSeconds=time.monotonic()-began)), flush=True)
    after = current_sources(before)
    write(out / 'source-after.json', after)
    unchanged = after == before
    completion = dict(**start, endedAtUtc=now(), elapsedWallSeconds=time.monotonic()-began, completedFreshCases=len(finished), failures=failures, unchangedDuringMeasurement=unchanged, sourceAfter=ref(out / 'source-after.json'), completed=finished, complete=len(finished) == plan['freshCases'] and not failures and unchanged)
    write(out / 'completion-receipt.json', completion)
    assert completion['complete'], 'Measurement incomplete; no aggregate acceptance generated.'
    inputs = sorted(out.glob('reused-results-*.jsonl')) + sorted(out.glob('results-*.jsonl'))
    assert len(inputs) == plan['freshCases'] + (2 if args.matrix == 'strategy' else 0)
    command = [sys.executable, str(verify(manifest['aggregator'])), *[str(p) for p in inputs], '--output', str(out / 'analysis.json'), '--expected-games', str(plan['requiredTotalCases'])]
    if args.matrix == 'branch': command.append('--require-branch-matrix')
    # The 1000 absolute input paths exceed Windows' process argv limit. A tiny
    # launcher reads this exact invocation and runs the reviewed script in-process.
    helper = ROOT / 'game/v6/aggregate_final_balance_20260913.py'
    execution_command = [sys.executable, str(helper), str(out / 'aggregate-invocation.json')]
    write(out / 'aggregate-invocation.json', dict(command=command, executionCommand=execution_command, helper=ref(helper), cwd=str(ROOT), startedAtUtc=now(), inputs=[ref(p) for p in inputs]))
    result = subprocess.run(execution_command, cwd=ROOT, capture_output=True, text=True, encoding='utf-8', creationflags=subprocess.CREATE_NO_WINDOW)
    write(out / 'aggregate-execution.json', dict(command=command, executionCommand=execution_command, helper=ref(helper), exitCode=result.returncode, endedAtUtc=now(), stdout=result.stdout, stderr=result.stderr))
    print(result.stdout, flush=True)
    assert result.returncode == 0, result.stderr
    report = read(out / 'analysis.json')
    assert report['integrityPassed'] and report['games'] == plan['requiredTotalCases']
    print(json.dumps(dict(type='matrix-complete', matrix=args.matrix, games=report['games'], completed=report['completed'], unresolved=report['unresolved'], analysis=ref(out / 'analysis.json'), finalBalanceAcceptance=False)), flush=True)


if __name__ == '__main__':
    main()
