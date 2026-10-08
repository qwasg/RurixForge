from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json

project = Path(__file__).resolve().parents[2]
raw = project / 'game/v6/lan-runs/final-fa31b360-concurrent-20260913-a'
context = project / 'game/v6/lan-run-contexts/final-fa31b360-concurrent-20260913-a'
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
read = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
ref = lambda path: {'path': path.relative_to(project).as_posix(), 'sha256': sha(path)}
samples = [json.loads(line) for line in (raw / 'samples.jsonl').read_text(encoding='utf-8-sig').splitlines() if line.strip()]
valid = [s for s in samples if s['seconds'] > 30 and s['activityIntervalMs'] > 0 and s['stable'] and s.get('fps')]
violating = [s for s in valid if min(s['fps']) < 30]
assert violating and violating[0]['sampleId'] == 67
failure = read(raw / 'failure.json')
process_path = project / 'game/v6/final-fa31b360-concurrent-20260913-a-process-exit.json'
process = read(process_path)
shutdown = read(context / 'processes-after-stop.json')
assert process['exitCode'] == 1 and process['executableUnchanged']
assert failure['message'] == 'Owned diagnostic stopped by explicit stop-file request'
assert shutdown['ownedProcessesClosed']
files = sorted(p for p in raw.rglob('*') if p.is_file())
files += sorted(p for p in context.rglob('*') if p.is_file())
files += [process_path, project / 'game/v6/final-fa31b360-concurrent-20260913-a-driver.log', Path(__file__)]
report = {
    'kind': 'native-lan-failed-attempt', 'schemaVersion': 2,
    'generatedAtUtc': datetime.now(timezone.utc).isoformat(),
    'actualNativeExecution': True, 'passed': False, 'finalEligible': False,
    **process['target'], 'requestedSeconds': 2700,
    'observedCombatWallSeconds': samples[-1]['seconds'], 'measurementWallSeconds': samples[-1]['wallSeconds'],
    'samples': len(samples), 'validFpsSamples': len(valid),
    'minimumValidFps': [min(s['fps'][i] for s in valid) for i in range(2)],
    'fpsViolatingSamples': len(violating), 'firstViolation': violating[0],
    'minimumObservedTps': min(s['authorityTicksPerSecond'] for s in samples if 'authorityTicksPerSecond' in s),
    'maximumObservedTps': max(s['authorityTicksPerSecond'] for s in samples if 'authorityTicksPerSecond' in s),
    'qualificationFailure': 'Original valid sample FPS below the unchanged 30 FPS per-renderer threshold.',
    'driverExitReason': failure['message'],
    'driverReachedEndOfActivityFpsAssertion': False,
    'ownedProcessesClosed': True, 'concurrentBalanceWorkersObserved': [16],
    'balanceWorkersStoppedOrChanged': False,
    'scope': 'Deliberate concurrent-load attempt. Failed FPS eligibility was observed from original already-persisted valid rows. Root then requested cooperative stop through the unchanged driver stop-file option. This did not reach 2700 active seconds or the end-of-activity FPS assertion. All original rows remain in evidence; no failing interval is discarded or reclassified. An idle retry must be an independent attempt.',
    'evidence': [ref(path) for path in files],
}
output = project / 'game/v6/lan-concurrent-fa31b360-failed-20260913.json'
with output.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, indent=2)
    stream.write('\n')
print(json.dumps({'path': str(output), 'sha256': sha(output), 'activeSeconds': report['observedCombatWallSeconds'], 'samples': len(samples), 'minimumValidFps': report['minimumValidFps'], 'violatingSamples': len(violating), 'ownedProcessesClosed': True}))
