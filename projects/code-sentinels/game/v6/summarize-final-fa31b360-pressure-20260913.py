"""Retain strict observations from the final frozen FA31/0ac sustained pressure run."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
from collections import Counter
from statistics import median
import csv
import hashlib
import json

from PIL import Image

PROJECT = Path(__file__).resolve().parents[2]
RAW = PROJECT / 'Logs/v6/pressure-fa31b360-0acffa83-20260913'
TARGET = PROJECT / 'game/v6/performance-final-fa31b360-20260913.json'
IDENTITY = PROJECT / 'Logs/v6/native-final-0acffa83-20260913-a-host-identity/host-identity.json'
BUILD = PROJECT / 'game/v6/native-final-0acffa83-20260913-a/native-build-receipt.json'


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ref(path):
    return {'path': path.relative_to(PROJECT).as_posix(), 'sha256': sha(path)}


def write_new(path, value):
    with path.open('x', encoding='utf-8') as output:
        json.dump(value, output, indent=2, ensure_ascii=False)
        output.write('\n')


identity = read(IDENTITY)
simulation = read(RAW / 'simulation-report.json')
gpu = read(RAW / 'gpu-report.json')
telemetry = read(RAW / 'gpu-telemetry.json')
assert identity['engineSha256'] == gpu['nativeHash'] == read(BUILD)['artifact']['sha256']
assert read(RAW / 'process-exit.json')['exitCode'] == 0
assert not TARGET.exists(), 'Preserve previous pressure evidence'

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
        'scope': 'Read-only samples inside the recorded GPU window. No system settings changed; missing values remain null. These samples do not establish causality.',
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
for ppm in sorted(RAW.glob('layer-*.ppm')):
    pixels = Image.open(ppm).convert('RGB')
    dest = pngs / (ppm.stem + '.png')
    pixels.save(dest)
    assert pixels.tobytes() == Image.open(dest).convert('RGB').tobytes()

files = [RAW / name for name in [
    'simulation-report.json', 'gpu-report.json', 'phase-profile.json',
    'pressure-save.json', 'render-frames.json', 'process-exit.json',
    'gpu-telemetry.json', 'gpu-telemetry.csv', 'gpu-window-summary.json']
    if (RAW / name).exists()]
files += sorted(RAW.glob('layer-*.ppm')) + sorted(pngs.glob('*.png'))
files += [IDENTITY, BUILD, Path(__file__), PROJECT / 'game/v6/pressure-probe.mjs']
report = {
    'schemaVersion': 2, 'kind': 'native-performance-acceptance',
    'generatedAtUtc': datetime.now(timezone.utc).isoformat(),
    'passed': all(checks.values()), 'finalEligible': all(checks.values()),
    'engineSha256': identity['engineSha256'], 'rulesVersion': identity['rulesVersion'],
    'rulesFingerprint': identity['rulesFingerprint'],
    'scope': 'Final frozen FA31/0ac runtime pressure measurement using the unchanged pressure fixture and strict sustained thresholds. This report binds only this actual binary and rule identity.',
    'checks': checks, 'simulation': simulation, 'gpu': gpu,
    'failedLayers': {key: row['fps'] for key, row in gpu['perLayer'].items() if row['fps'] < 30},
    'gpuWindow': window_summary, 'evidence': [ref(path) for path in files],
    'notes': [
        'Actual pre-step projectile replenishment and resource maintenance are disclosed pressure-only fixtures, excluded from Game::step timing.',
        'Maximum tick and interframe gaps remain recorded even when P99 and FPS thresholds pass.',
        'GPU passes may overlap; their individual durations are not summed into wall-clock latency. Separate CPU readback duration remains unknown.',
        'Pixel equivalence is a separate independently recorded observation. This report cannot approve gameplay, balance, LAN, portable startup, or newer binaries.',
    ],
}
write_new(TARGET, report)
print(json.dumps({'path': str(TARGET), 'sha256': sha(TARGET), 'passed': report['passed'],
                  'checks': checks, 'fps': gpu['observedFps'],
                  'layerFps': {key: row['fps'] for key, row in gpu['perLayer'].items()},
                  'tickP99': simulation['stepMs']['p99']}))

