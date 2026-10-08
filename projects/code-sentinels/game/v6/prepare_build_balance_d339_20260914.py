"""Freeze d339 inputs and normally build one independent balance executable."""
from pathlib import Path
import datetime
import hashlib
import json
import shutil
import subprocess
import build_direct_range_probe_runner_20260913 as utility

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-d33999c4-20260914'
FP = 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'
read, sha, ref, write, now = utility.read, utility.sha, utility.ref, utility.write, utility.now


def main():
    assert utility.fingerprint() == FP
    assert not BASE.exists(), 'Existing attempt preserved; do not prepare/build twice.'
    verified = []
    for name, expected in [
        ('game/v6/room-hp-capacity-fix-20260914-a/source-frozen-after.json', 'a7667246b3fb3b3ad204f51a8f969a42802cb9bb514211d44dce4a9e4b1970e6'),
        ('game/v6/room-hp-capacity-fix-20260914-a/source-freeze-regression-receipt.json', '8a08efec626c7ce4c3dc575ef838694652ee3c179315e117837206b2bf356f0c'),
        ('game/v6/native-room-d33999c4-20260914-a/native-build-receipt.json', '42d1d9a0a09d797e3f9b28bfadcee8b07f2b7a447379282fd8839551f7bdc37f'),
        ('Logs/v6/native-room-d33999c4-20260914-a-host-identity/host-identity.json', 'eac1b3c0fa9852bfeaae8a57264fe6a3c067ea62d9d9d0ff10b4effa4d58b092'),
    ]:
        path = ROOT / name
        assert sha(path) == expected, name
        verified.append(ref(path))
    BASE.mkdir()
    for name in ['artifact', 'harness', 'identity', 'control-history', 'strategy/diagnostics', 'branch/diagnostics']:
        (BASE / name).mkdir(parents=True)
    before = utility.sources()
    compiled = [item for item in before if '/tests/' not in item['path']]
    write(BASE / 'source-before-build.json', before)
    write(BASE / 'compiled-source-before.json', compiled)
    for item in compiled:
        destination = BASE / 'source' / item['path']
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / item['path'], destination)
        assert sha(destination) == item['sha256']
    originals = {
        'strategy': ROOT / 'game/v6/strategy-plan.json',
        'branch': ROOT / 'game/v6/direct-range-probe80-20260913/canonical-branch-plan.json',
    }
    expected_plan_hashes = {
        'strategy': '8ab5503d68773260e193af5fa9cf8e669cb199a13358f4dddc05270631b40f44',
        'branch': 'cae20640404e86e10dc49dc68983a79e2852cd42a04df6ac64edba09dcc39eb8',
    }
    plans = {}
    for name, source in originals.items():
        assert sha(source) == expected_plan_hashes[name]
        destination = BASE / name / f'{name}-plan.json'
        shutil.copy2(source, destination)
        assert sha(source) == sha(destination)
        cases = read(destination)['cases']
        assert len(cases) == (126 if name == 'strategy' else 1000)
        plans[name] = ref(destination)
    write(BASE / 'preparation.json', dict(
        preparedAtUtc=now(), rulesFingerprint=FP, rulesVersion='v6.2',
        canonicalPlans=plans, nativeEvidence=verified,
        allCasesFresh=True, reusedRows=0, plannedStrategyCases=126, plannedBranchCases=1000,
        compiledSource=ref(BASE / 'compiled-source-before.json'),
        sourceArchive='source', setupScript=ref(Path(__file__).resolve()),
        scope='New d339 execution inputs only. Existing0ac/6828 data retained as history, never relabelled.',
    ))
    target = ROOT / 'game/v6/balance-d33999c4-target-20260914'
    command = ['cargo', 'build', '--release', '--manifest-path', str(ROOT / 'game/v6/balance-runner/Cargo.toml'), '--target-dir', str(target), '-j', '4']
    invocation = dict(command=command, cwd=str(ROOT), startedAtUtc=now(), rulesFingerprint=FP,
                      plannedArtifact=str(BASE / 'artifact/sentinels-v6-balance-runner.exe'),
                      sourceBefore=ref(BASE / 'source-before-build.json'))
    write(BASE / 'build-invocation.json', invocation)
    with (BASE / 'build.stdout.log').open('x', encoding='utf-8') as stdout, (BASE / 'build.stderr.log').open('x', encoding='utf-8') as stderr:
        process = subprocess.Popen(command, cwd=ROOT, stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
        write(BASE / 'build-process.json', dict(pid=process.pid, startedAtUtc=invocation['startedAtUtc'], command=command))
        code = process.wait()
    after = utility.sources()
    write(BASE / 'source-after-build.json', after)
    result = dict(**invocation, endedAtUtc=now(), exitCode=code, sourceUnchanged=before == after,
                  rulesFingerprintAfter=utility.fingerprint(), sourceAfter=ref(BASE / 'source-after-build.json'))
    if code == 0:
        original = target / 'release/sentinels-v6-balance-runner.exe'
        frozen = BASE / 'artifact/sentinels-v6-balance-runner.exe'
        shutil.copy2(original, frozen)
        assert sha(original) == sha(frozen)
        result['artifact'] = ref(frozen)
    write(BASE / 'runner-build-receipt.json', result)
    assert code == 0 and before == after and utility.fingerprint() == FP
    print(json.dumps(dict(buildPassed=True, rulesFingerprint=FP, artifact=result['artifact'],
                          identityStarted=False, matrixStarted=False)), flush=True)


if __name__ == '__main__':
    main()
