"""Bind the actual affected-test rerun to the retained earlier native suite."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re

project = Path(__file__).resolve().parents[2]
run = project / 'game/v6/final-contracts-0acffa83-20260912-a'
output = run / 'contract-execution-resolved-forward-20260913.json'
assert not output.exists(), 'Preserve previous evidence'
read = lambda p: json.loads(p.read_text(encoding='utf-8-sig'))
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
ref = lambda p: {'path': p.relative_to(project).as_posix(), 'sha256': sha(p)}
spec = importlib.util.spec_from_file_location('contracts', project / 'game/v6/run_native_contracts.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
fingerprint = module.fingerprint()
assert fingerprint == '0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a'
original_path = run / 'contract-execution.json'
original = read(original_path)
assert original['rulesFingerprint'] == fingerprint and original['sourceUnchanged']
assert original['passedTests'] == 253 and original['failedTests'] == 1
assert not original['unexecutedTargets'] and not original['missingTargets']
target = 'tests/v6_forward_energy.rs'
assert [r['target'] for r in original['targets'] if r['failed']] == [target]
old_source = run / 'v6_forward_energy-before-explicit-engagement.rs'
test_source = project / 'native-v6/tests/v6_forward_energy.rs'
before = read(run / 'source-before.json')
test_record = next(r for r in before if r['path'] == 'native-v6/tests/v6_forward_energy.rs')
assert sha(old_source) == test_record['sha256']
changed = [r['path'] for r in before if sha(project / r['path']) != r['sha256']]
assert changed == ['native-v6/tests/v6_forward_energy.rs'], changed
log_path = project / 'game/v6/forward-charge-real-engagement-20260913.log'
log = log_path.read_text(encoding='utf-8-sig')
assert 'test result: ok. 1 passed; 0 failed; 0 ignored;' in log
assert 'FAILED' not in log and 'never executed' not in log
execution = module.parse(log)
assert len(execution) == 1 and execution[0]['target'] == target
assert execution[0]['status'] == 'executed' and execution[0]['passed'] == 1
shot = re.search(r'ordinary firing prerequisite unit=(\d+) attacks=(\d+) -> (\d+) energy=([\d.]+) -> ([\d.]+) at ([\d.]+)s', log)
charge = re.search(r'actual forward charge unit=(\d+) pos=Pos \{ x: (-?\d+), y: (-?\d+), level: (-?\d+) \} ([\d.]+) -> ([\d.]+) at ([\d.]+)s', log)
assert shot and charge and shot[1] == charge[1]
assert int(shot[3]) > int(shot[2]) and float(shot[5]) < float(shot[4])
assert 0 < float(charge[6]) - float(charge[5]) <= 35 / 60 + 1e-8
assert float(charge[7]) > float(shot[6])
now = module.sources()
source_path = run / 'source-after-forward-resolution-20260913.json'
assert not source_path.exists()
source_path.write_text(json.dumps(now, ensure_ascii=False, indent=2), encoding='utf-8')
effective = []
for row in original['targets']:
    if row['target'] == target:
        resolved = dict(execution[0])
        resolved['earlierFailedExecution'] = row
        resolved['testSource'] = ref(test_source)
        effective.append(resolved)
    else:
        effective.append(row)
report = {
    'kind': 'native-contract-execution-resolved', 'finalEligible': False,
    'scope': 'Current native Game correctness coverage across retained release suite and one affected-test rerun. This is not a single all-green invocation or host/UI/balance/performance/LAN release approval.',
    'rulesFingerprint': fingerprint, 'profile': 'release', 'actualNativeExecution': True,
    'passedTests': sum(r['passed'] for r in effective),
    'failedTests': sum(r['failed'] for r in effective),
    'ignoredTests': sum(r['ignored'] for r in effective),
    'unexecutedTargets': [], 'missingTargets': [], 'targets': effective,
    'completeRequiredTestCoverage': True, 'changedSourcesSinceFullRun': changed,
    'gameSourcesUnchanged': True,
    'resolution': {
        'reason': 'The new escort policy completes the paid forward station before the tanks fire. The charging fixture now orders an already purchased tank into ordinary combat to establish real energy consumption before returning to the original paid station. Original charging, payment, route, and surplus-power assertions remain.',
        'noResourceOrVictoryRuleChanges': True,
        'shots': {'unit': int(shot[1]), 'before': int(shot[2]), 'after': int(shot[3]), 'energyBefore': float(shot[4]), 'energyAfter': float(shot[5]), 'seconds': float(shot[6])},
        'recharge': {'unit': int(charge[1]), 'position': [int(charge[i]) for i in (2, 3, 4)], 'before': float(charge[5]), 'after': float(charge[6]), 'seconds': float(charge[7])},
        'originalAggregateStillRetained': {'passed': 253, 'failed': 1},
    },
    'evidence': [ref(p) for p in (original_path, run / 'cargo-tests.log', old_source,
                                  test_source, log_path, source_path)],
}
assert report['passedTests'] == 254 and report['failedTests'] == 0 and report['ignoredTests'] == 2
output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps({'path': str(output), 'sha256': sha(output), 'passed': 254, 'failed': 0, 'ignored': 2, 'gameFingerprint': fingerprint}))
