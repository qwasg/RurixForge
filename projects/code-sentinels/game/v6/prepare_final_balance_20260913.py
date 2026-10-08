"""Freeze final measurement plans and audit six original cases without simulation."""
from pathlib import Path
import datetime
import hashlib
import json
import shutil

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'game/v6/final-balance-0acffa83-20260913'
PILOT = ROOT / 'game/v6/escort-two-pilot-20260912'
FP = '0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a'
RUNNER_SHA = '808ceb66984934b7b3569e310aa14d2b3ccdb8ca6d6ea75f009091f655463ac2'
STRATEGY_SHA = '8ab5503d68773260e193af5fa9cf8e669cb199a13358f4dddc05270631b40f44'
BRANCHES = ['speed', 'security', 'algorithm', 'science', 'lightweight']


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def read(p):
    return json.loads(p.read_text(encoding='utf-8-sig'))


def write(p, v):
    with p.open('x', encoding='utf-8', newline='\n') as f:
        json.dump(v, f, ensure_ascii=False, indent=2)
        f.write('\n')


def ref(p):
    return dict(path=p.relative_to(ROOT).as_posix(), sha256=sha(p), bytes=p.stat().st_size)


def rule_fingerprint():
    base = ROOT / 'native-v6'
    paths = sorted([*base.joinpath('src').rglob('*.rs'), *[base / n for n in ['Cargo.toml', 'Cargo.lock', 'build.rs']]], key=lambda p: p.relative_to(base).as_posix())
    digest = hashlib.sha256(b'code-sentinels-native-rules-v1\0')
    for p in paths:
        name = p.relative_to(base).as_posix().encode()
        data = p.read_bytes()
        digest.update(len(name).to_bytes(8, 'little')); digest.update(name)
        digest.update(len(data).to_bytes(8, 'little')); digest.update(data)
    return digest.hexdigest()


def prepare():
    assert not OUT.exists(), 'Existing measurement is preserved; do not prepare twice.'
    candidate = read(PILOT / 'candidate-manifest.json')
    assert candidate['rulesFingerprint'] == FP == rule_fingerprint()
    source_records, test_drift = [], []
    for item in candidate['files']:
        current = ROOT / item['path']
        frozen = PILOT / 'source' / item['path']
        assert sha(frozen) == item['sha256'], item['path']
        actual = ref(current)
        if actual['sha256'] != item['sha256']:
            assert item['path'] == 'native-v6/tests/v6_forward_energy.rs', item['path']
            test_drift.append(dict(**actual, frozenSha256=item['sha256'], scope='Root-resolved test fixture only; outside native rules fingerprint and runner sources.'))
        source_records.append(actual)
    for a in candidate['artifacts']:
        assert sha(PILOT / a['frozen']) == a['sha256']
    assert sha(PILOT / candidate['artifacts'][0]['frozen']) == RUNNER_SHA
    strategy_path = ROOT / 'game/v6/strategy-plan.json'
    assert sha(strategy_path) == STRATEGY_SHA
    strategy = read(strategy_path)
    assert strategy['planId'] == 'v6-strategy-126-v1'
    expected = {c['index']: c for c in strategy['cases']}
    assert set(expected) == set(range(126))

    original_files = [PILOT / 'results-096.jsonl', PILOT / 'results-099.jsonl']
    raw_lines = [line for p in original_files for line in p.read_bytes().splitlines(keepends=True) if line.strip()]
    rows = [json.loads(line) for line in raw_lines]
    assert len(rows) == 6 and {r['index'] for r in rows} == set(range(96, 102))
    combined = [json.loads(l) for l in (PILOT / 'results-matched-six.jsonl').read_bytes().splitlines() if l.strip()]
    assert combined == rows
    reuse = []
    for r in rows:
        i = r['index']; case = expected[i]
        assert all(r[k] == case[k] for k in ['index', 'seed', 'theme', 'branches', 'strategies', 'swapped', 'plannerOrder'])
        assert r['originalPair'] == case.get('originalPair')
        assert r['planId'] == strategy['planId'] and r['planIndex'] == i and r['planSha256'] == STRATEGY_SHA
        assert r['rulesVersion'] == 'v6.2' and r['simulationSourceSha256'] == FP
        execution = read(PILOT / f'execution-{96 if i < 99 else 99:03}.json')
        assert execution['exitCode'] == 0 and execution['rulesFingerprint'] == FP and execution['runnerSha256'] == RUNNER_SHA
        args = execution['command']
        assert args[args.index('--minutes') + 1] == '55'
        assert args[args.index('--plan') + 1] == str(strategy_path)
        assert Path(args[0]) == PILOT / candidate['artifacts'][0]['frozen']
        saved_path = PILOT / f'diagnostics/match-{i}.save.json'
        saved = read(saved_path)
        assert saved['rulesFingerprint'] == FP and saved['rulesVersion'] == r['rulesVersion']
        assert saved['snapshot']['tick'] / 60 == r['seconds']
        assert saved['snapshot']['winner'] == r['winner'] and saved['snapshot']['winReason'] == r['winReason']
        assert not saved['initialAi'] and not saved['administrativeEvents']
        assert r['seconds'] <= 3300 and r['winner'] in [1, 2]
        replay_path = PILOT / f'replay-execution-{i}.json'
        replay = read(replay_path)
        assert replay['exitCode'] == 0 and replay['rulesFingerprint'] == FP and replay['sourceSaveSha256'] == sha(saved_path)
        observations_path = PILOT / f'formation-{i}-observation.jsonl'
        observations = [json.loads(l) for l in observations_path.read_text(encoding='utf-8').splitlines() if l.strip()]
        final = observations[-1]
        assert final['type'] == 'complete' and final['matchedOriginalSnapshot'] is True
        assert final['tick'] == saved['snapshot']['tick'] and final['winner'] == r['winner']
        reuse.append(dict(index=i, originalRaw=ref(original_files[0 if i < 99 else 1]), rawLineSha256=hashlib.sha256(raw_lines[rows.index(r)]).hexdigest(), save=ref(saved_path), replayExecution=ref(replay_path), replayObservation=ref(observations_path), exactFullSaveReplay=True, canonicalMetadataMatched=True))

    OUT.mkdir()
    (OUT / 'strategy').mkdir(); (OUT / 'branch').mkdir(); (OUT / 'artifact').mkdir()
    shutil.copy2(PILOT / candidate['artifacts'][0]['frozen'], OUT / 'artifact/sentinels-v6-balance-runner.exe')
    shutil.copy2(strategy_path, OUT / 'strategy/strategy-plan.json')
    assert sha(OUT / 'strategy/strategy-plan.json') == STRATEGY_SHA
    source_records.sort(key=lambda r: r['path'])
    write(OUT / 'source-before.json', source_records)
    write(OUT / 'reuse-six-audit.json', dict(scope='Original six receipts are reused unchanged; no relabelling, no repeated counts. These are measurement inputs, not balance approval.', rulesFingerprint=FP, runnerSha256=RUNNER_SHA, planSha256=STRATEGY_SHA, reusedCases=reuse, testFixtureDrift=test_drift))
    # Preserve the original files exactly, rather than reserializing their rows.
    for p in original_files:
        shutil.copy2(p, OUT / 'strategy' / ('reused-' + p.name))
        assert sha(p) == sha(OUT / 'strategy' / ('reused-' + p.name))
    branch_cases = []
    for i in range(1000):
        pair = [BRANCHES[i // 200], BRANCHES[(i // 40) % 5]]
        seed_index = (i // 2) % 20; swapped = bool(i % 2)
        branch_cases.append(dict(index=i, seed=1000 + seed_index, theme=['river', 'mining', 'highland'][seed_index % 3], branches=list(reversed(pair)) if swapped else pair.copy(), strategies=['mixed-ai', 'mixed-ai'], swapped=swapped, plannerOrder=[2, 1] if swapped else [1, 2], originalPair=pair))
    assert len({json.dumps([c[k] for k in ['seed', 'theme', 'branches', 'strategies', 'plannerOrder']], sort_keys=True) for c in branch_cases}) == 1000
    write(OUT / 'branch/branch-plan.json', dict(schemaVersion=1, planId='v6-branch-1000-canonical-20260913', scope='Exact schema-2 branch enumeration. Prepared experiment plan only, not observations.', cases=branch_cases))
    manifest = dict(schemaVersion=1, createdAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), scope='Frozen 0ac final CPU measurement; no final balance, performance or LAN approval.', rulesVersion='v6.2', rulesFingerprint=FP, runner=ref(OUT / 'artifact/sentinels-v6-balance-runner.exe'), originalCandidate=ref(PILOT / 'candidate-manifest.json'), sourceBefore=ref(OUT / 'source-before.json'), reuseAudit=ref(OUT / 'reuse-six-audit.json'), aggregator=ref(ROOT / 'game/v6/summarize_balance.py'), approvedContract=ref(ROOT / 'V6-APPROVED-PLAN.md'), releaseContract=ref(ROOT / 'game/v6/RELEASE-ACCEPTANCE-SCHEMA.md'), maxConcurrentProcesses=16, simulatedMinutesBudget=55, simulationHz=60, injectedResources=False, tickSkipping=False, forcedWinner=False, matrices={})
    for name, plan_path, indices, required, reused in [('strategy', OUT / 'strategy/strategy-plan.json', [i for i in range(126) if i not in range(96, 102)], 126, 6), ('branch', OUT / 'branch/branch-plan.json', list(range(1000)), 1000, 0)]:
        jobs = []
        for i in indices:
            folder = OUT / name
            args = [str(OUT / 'artifact/sentinels-v6-balance-runner.exe'), '--plan', str(plan_path), '--first', str(i), '--limit', '1', '--minutes', '55', '--source-hash', FP, '--stop-on-failure', 'true', '--out', str(folder / f'results-{i:03}.jsonl'), '--diagnostics', str(folder / 'diagnostics')]
            jobs.append(dict(index=i, command=args))
        write(OUT / name / 'execution-plan.json', dict(matrix=name, requiredTotalCases=required, reusedOriginalCases=reused, freshCases=len(indices), plan=ref(plan_path), cwd=str(ROOT), maxConcurrentProcesses=16, jobs=jobs))
        manifest['matrices'][name] = dict(executionPlan=ref(OUT / name / 'execution-plan.json'), inputPlan=ref(plan_path), requiredCases=required, reusedCases=reused, freshCases=len(indices))
    write(OUT / 'measurement-manifest.json', manifest)
    print(json.dumps(dict(prepared=str(OUT), rulesFingerprint=FP, reusedCases=6, strategyFreshCases=120, branchFreshCases=1000, identityPassed=True, simulationsStarted=False)))


if __name__ == '__main__':
    prepare()
