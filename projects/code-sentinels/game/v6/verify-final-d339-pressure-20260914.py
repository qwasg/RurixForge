"""Read-only validation of the completed performance scope; writes a new receipt."""
from pathlib import Path
from datetime import datetime, timezone
import json

import release_gate as gate

PROJECT = Path(__file__).resolve().parents[2]
RUN = PROJECT / 'game/v6/pressure-1b1738b7-d33999c4-20260914-a'
REPORT = PROJECT / 'game/v6/performance-final-1b1738b7-20260914.json'
OUTPUT = RUN / 'performance-gate-verification.json'
target = gate.read_json(RUN / 'preflight.json')['target']
document = gate.read_json(REPORT)
facts = gate.validate_performance(document, target)
store = gate.EvidenceStore(PROJECT)
store.supporting(document['evidence'], 'performance-supporting')
gate.identity(document, target, 'performance payload', payload=True)
raw = Path(gate.read_json(RUN / 'process-exit.json')['output'])
gate.require(document['simulation'] == gate.read_json(raw / 'simulation-report.json'), 'simulation differs from raw')
gate.require(document['gpu'] == gate.read_json(raw / 'gpu-report.json'), 'GPU differs from raw')
gate.require(document['globalReleaseApproved'] is False and document['completeFunctionalityAccepted'] is False, 'scope expanded')
gate.require(set(document['pendingSignatureRequiredTargets']) == {'v6_combat_contracts', 'v6_resource_contracts', 'v6_door_openings', 'v6_repair_sources'}, 'required blocked targets omitted')
gate.require(gate.read_json(RUN / 'owned-process-cleanup.json')['allOwnedProcessesClosed'] is True, 'process cleanup unverified')
source_rows = gate.read_json(RUN / 'source-after.json')
for row in source_rows:
    path = (PROJECT.parents[1] / row['path']).resolve()
    gate.require(path.is_relative_to(PROJECT.parents[1]) and path.is_file(), 'source path missing or outside repository')
    gate.require(gate.sha(path) == row['sha256'].lower(), 'source changed: ' + row['path'])
receipt = {
    'schemaVersion': 2, 'kind': 'native-performance-scope-verification',
    'observedAtUtc': datetime.now(timezone.utc).isoformat(), 'passed': True,
    'validator': {'path': 'game/v6/release_gate.py', 'sha256': gate.sha(PROJECT / 'game/v6/release_gate.py'),
                  'function': 'validate_performance'},
    'verificationScript': {'path': Path(__file__).relative_to(PROJECT).as_posix(), 'sha256': gate.sha(Path(__file__))},
    'report': {'path': REPORT.relative_to(PROJECT).as_posix(), 'sha256': gate.sha(REPORT)},
    'target': target, 'facts': facts, 'supportingReferencesVerified': len(store.references),
    'actualCurrentSourceFilesVerified': len(source_rows), 'rawSimulationEqual': True, 'rawGpuEqual': True,
    'completeFunctionalityAccepted': False, 'globalReleaseApproved': False,
    'pendingSignatureRequiredTargets': document['pendingSignatureRequiredTargets'],
    'scope': 'Only completed pressure evidence and immutable references are validated. No test, native, GPU, LAN or packaging process is launched.'
}
with OUTPUT.open('x', encoding='utf-8') as stream:
    json.dump(receipt, stream, indent=2)
    stream.write('\n')
print(json.dumps({'path': str(OUTPUT), 'sha256': gate.sha(OUTPUT), 'passed': True, 'facts': facts,
                  'references': len(store.references), 'sources': len(source_rows)}))
