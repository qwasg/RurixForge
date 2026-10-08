"""Bind the actual exclusive H1b173/Rd339 pressure run to retained raw evidence."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
from collections import Counter
from statistics import median
import csv
import hashlib
import json

from PIL import Image

PROJECT = Path(__file__).resolve().parents[2]
RUN = PROJECT / 'game/v6/pressure-1b1738b7-d33999c4-20260914-a'
RAW = PROJECT / 'dist/final-candidate-d33999c4-20260914/CodeSentinels-V6-Windows/Logs/v6/pressure-1b1738b7-d33999c4-20260914-a'
TARGET = PROJECT / 'game/v6/performance-final-1b1738b7-20260914.json'
IDENTITY = PROJECT / 'Logs/v6/native-room-d33999c4-20260914-a-host-identity/host-identity.json'
BUILD = PROJECT / 'game/v6/native-room-d33999c4-20260914-a/native-build-receipt.json'


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def sha(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()


def ref(path):
    return {'path': path.relative_to(PROJECT).as_posix(), 'sha256': sha(path)}


def write_new(path, value):
    with path.open('x', encoding='utf-8') as output:
        json.dump(value, output, indent=2, ensure_ascii=False)
        output.write('\n')


preflight = read(RUN / 'preflight.json')
target = preflight['target']
identity = read(IDENTITY)
exit_receipt = read(RUN / 'process-exit.json')
simulation = read(RAW / 'simulation-report.json')
gpu = read(RAW / 'gpu-report.json')
telemetry = read(RAW / 'gpu-telemetry.json')
assert not TARGET.exists(), 'Preserve existing evidence'
assert identity['engineSha256'] == gpu['nativeHash'] == target['engineSha256'] == read(BUILD)['artifact']['sha256']
assert identity['rulesFingerprint'] == target['rulesFingerprint']
assert exit_receipt['exitCode'] == 0
assert exit_receipt['engineSha256Before'] == exit_receipt['engineSha256After'] == target['engineSha256']
assert exit_receipt['sourceSha256Before'] == exit_receipt['sourceSha256After']
assert exit_receipt['balanceControlSha256Before'] == exit_receipt['balanceControlSha256After'] == preflight['balanceControlSha256']
assert sha(PROJECT / 'game/v6/pressure-probe.mjs') == exit_receipt['driverSha256'] == preflight['driverSha256']
assert sha(RUN / 'source-before.json') == sha(RUN / 'source-after.json') == exit_receipt['sourceSha256Before']
assert len(read(RUN / 'source-before.json')) == 175
assert read(RUN / 'owned-process-cleanup.json')['allOwnedProcessesClosed'] is True

counts = simulation['counts']
layers = {str(n) for n in range(-2, 6)}
checks = {
    'simulationCounts': all(counts[k] >= value for k, value in {
        'shells': 128, 'rooms': 512, 'units': 200,
        'minimumProjectilesBeforeStep': 600}.items()),
    'movingUnits': simulation['minimumMovingUnits'] >= 200,
    'simulationDuration': simulation['wallSeconds'] >= 60,
    'simulationSamples': simulation['samples'] >= 3600 and simulation['ticks'] >= 3600,
    'actualWeaponAttacks': simulation['actualWeaponAttacks'] > 0,
    'allSimulationLayers': set(simulation['activeLayerSamples']) == layers
        and all(value > 0 for value in simulation['activeLayerSamples'].values()),
    'tickP99': simulation['stepMs']['p99'] <= 16.7,
    'gpuDuration': gpu['wallSeconds'] >= 60,
    'gpuFrames': gpu['frameCount'] >= 1800,
    'gpuOverallFps': gpu['observedFps'] >= 30,
    'allGpuLayers': set(gpu['perLayer']) == layers,
    'everyGpuLayer': all(row['seconds'] >= 7 and row['frames'] > 0 and row['fps'] >= 30
        for row in gpu['perLayer'].values()),
    'gpuErrors': gpu['errors'] == [],
    'noTruncation': gpu['truncatedFrames'] == 0,
    'rgbaDiagnostics': len(gpu['frameDiagnostics']) >= 2 and all(
        row['width'] == 1280 and row['height'] == 720 and row['meshFallbacks'] == 0
        and row['truncated'] is False and bool(row['deviceName'])
        for row in gpu['frameDiagnostics']),
}

window_summary = None
if telemetry.get('measurementStartUtc') and (RAW / 'gpu-telemetry.csv').exists():
    start = datetime.fromisoformat(telemetry['measurementStartUtc'].replace('Z', '+00:00'))
    end = datetime.fromisoformat(telemetry['measurementEndUtc'].replace('Z', '+00:00'))
    offset = timezone(timedelta(minutes=telemetry['localUtcOffsetMinutes']))
    rows = []
    with (RAW / 'gpu-telemetry.csv').open(encoding='utf-8-sig', newline='') as source:
        for row in csv.DictReader(source):
            row = {k.strip(): v.strip() for k, v in row.items()}
            at = datetime.strptime(row['timestamp'], '%Y/%m/%d %H:%M:%S.%f').replace(tzinfo=offset)
            if start <= at <= end:
                rows.append(row)

    def column(name):
        values = [float(row[name].split()[0]) for row in rows
                  if row[name] not in ('[N/A]', 'N/A', '[Not Supported]')]
        return {'observedSamples': len(values), 'missingSamples': len(rows) - len(values),
                'min': min(values) if values else None,
                'median': median(values) if values else None,
                'max': max(values) if values else None}

    window_summary = {
        'scope': 'Read-only telemetry within recorded GPU window. Missing samples remain null. No system settings changed; observations do not prove causality.',
        'measurementStartUtc': telemetry['measurementStartUtc'],
        'measurementEndUtc': telemetry['measurementEndUtc'], 'sampleCount': len(rows),
        'pstates': dict(Counter(row['pstate'] for row in rows)),
        'columns': {name: column(name) for name in [
            'temperature.gpu', 'clocks.current.graphics [MHz]', 'clocks.current.memory [MHz]',
            'power.draw [W]', 'power.limit [W]', 'utilization.gpu [%]',
            'utilization.memory [%]', 'memory.used [MiB]', 'memory.total [MiB]']},
    }
    write_new(RAW / 'gpu-window-summary.json', window_summary)

pngs = RAW / 'current-readback-png'
pngs.mkdir(exist_ok=False)
conversions = []
for ppm in sorted(RAW.glob('layer-*.ppm')):
    pixels = Image.open(ppm).convert('RGB')
    dest = pngs / (ppm.stem + '.png')
    pixels.save(dest)
    decoded = Image.open(dest).convert('RGB')
    assert pixels.size == decoded.size == (1280, 720)
    assert pixels.tobytes() == decoded.tobytes()
    conversions.append({'original': ref(ppm), 'png': ref(dest), 'width': 1280, 'height': 720,
                        'decodedRgbSha256': hashlib.sha256(pixels.tobytes()).hexdigest(),
                        'decodedPixelsEqual': True})
assert len(conversions) == 8
write_new(pngs / 'conversion-manifest.json', conversions)

files = [RAW / name for name in [
    'simulation-report.json', 'gpu-report.json', 'phase-profile.json',
    'pressure-save.json', 'render-frames.json', 'gpu-telemetry.json',
    'gpu-telemetry.csv', 'gpu-window-summary.json'] if (RAW / name).exists()]
files += sorted(RAW.glob('layer-*.ppm')) + sorted(pngs.glob('*'))
files += [RUN / name for name in ['preflight.json', 'balance-status-before.json',
    'source-before.json', 'source-after.json', 'actual-pressure-process.json',
    'driver.log', 'process-exit.json', 'owned-process-cleanup.json']]
files += [IDENTITY, BUILD, Path(__file__), PROJECT / 'game/v6/pressure-probe.mjs']
files += [PROJECT / name for name in [
    'game/v6/candidate-d33999c4-20260914-a/exact-candidate-verification.json',
    'game/v6/pressure-fixture-budget-fix-20260914-a/source-change-receipt.json',
    'game/v6/pressure-fixture-budget-fix-20260914-a/native-generation-validation.json',
    'game/v6/final-balance-d33999c4-20260914/pauses/pressure-20260914-a/cpu-handoff.json',
    'game/v6/room-hp-capacity-fix-20260914-a/root-pressure-window-processes.json']]
report = {
    'schemaVersion': 2, 'kind': 'native-performance-acceptance',
    'generatedAtUtc': datetime.now(timezone.utc).isoformat(),
    'passed': all(checks.values()), 'finalEligible': all(checks.values()), **target,
    'scope': 'Actual exclusive CPU60s and GPU eight-layer60s strict pressure acceptance for this H1b173/Rd339 candidate and its own Content. Performance scope only; no complete functionality, balance, LAN or global release approval.',
    'completeFunctionalityAccepted': False, 'globalReleaseApproved': False,
    'pendingSignatureRequiredTargets': ['v6_combat_contracts', 'v6_resource_contracts', 'v6_door_openings', 'v6_repair_sources'],
    'checks': checks, 'simulation': simulation, 'gpu': gpu,
    'failedLayers': {key: row['fps'] for key, row in gpu['perLayer'].items() if row['fps'] < 30},
    'gpuWindow': window_summary, 'evidence': [ref(path) for path in files],
    'notes': [
        'The synthetic 128shell/512room/200moving/600minimum-projectile fixture is explicitly unearned. It is generated for the actual current native rules; no old real Save was relabeled.',
        'Only synthetic room capacity metadata changed for current strict validation: floor(16*catalogue rate), budget and potential equal capacity. The 8-layer layout, count, algorithm and all sustained thresholds remain unchanged.',
        'Pre-step projectile replenishment and resource maintenance are pressure-only fixtures excluded from Game::step timing.',
        'Maximum native tick 69.4342ms and observed interframe gap 126.5065ms remain recorded. Threshold acceptance does not imply absence of stalls.',
        'Render/backend phase rings can include preparation and are not strictly the measured FPS window; their raw time scopes remain intact. GPU pass durations may overlap and are not summed; separate CPU readback duration is unknown.',
        'Balance revision1 was naturally paused and drained for measurement, with only its idle control waiter. Post-measurement report generation occurs after the performance window was released.',
        'Old failed FA31 concurrent LAN evidence remains separate and unchanged. No 2700second LAN was launched or accepted by this report.',
    ],
}
write_new(TARGET, report)
print(json.dumps({'path': str(TARGET), 'sha256': sha(TARGET), 'passed': report['passed'],
                  'checks': checks, 'fps': gpu['observedFps'],
                  'minimumLayerFps': min(row['fps'] for row in gpu['perLayer'].values()),
                  'tickP99': simulation['stepMs']['p99'], 'maximumTickMs': simulation['stepMs']['max'],
                  'maximumFrameGapMs': gpu['maxInterframeGapMs'], 'evidenceFiles': len(files)}))
