from pathlib import Path
from datetime import datetime, timezone
import json
from release_gate import EvidenceStore, read_json, sha, validate_host, validate_build, validate_performance

project = Path(__file__).resolve().parents[2]
run = project / 'game/v6/native-final-0acffa83-20260913-a'
host_path = project / 'Logs/v6/native-final-0acffa83-20260913-a-host-identity/host-identity.json'
build_path = run / 'native-build-receipt.json'
performance_path = project / 'game/v6/performance-final-fa31b360-20260913.json'
target = {k: read_json(host_path)[k] for k in ['engineSha256', 'rulesVersion', 'rulesFingerprint']}
store = EvidenceStore(project)
validate_host(read_json(host_path), target, store)
validate_build(read_json(build_path), target, store, project.parents[1])
performance = validate_performance(read_json(performance_path), target)
refs = [{'path': path.relative_to(project).as_posix(), 'sha256': sha(path)} for path in [host_path, build_path, performance_path, project / 'game/v6/release_gate.py']]
report = {'kind': 'native-evidence-schema-validation', 'completedAtUtc': datetime.now(timezone.utc).isoformat(),
          'target': target, 'passed': True, 'validatedSections': ['hostIdentity', 'nativeBuild', 'performance'],
          'performance': performance, 'inputs': refs,
          'scope': 'Actual read-only release-gate validation of these three completed observations. This is not a complete release approval; functionality, balance, LAN and isolated cold start remain separately required.'}
output = run / 'evidence-gate-validation.json'
with output.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, indent=2)
    stream.write('\n')
print(json.dumps({'path': str(output), 'sha256': sha(output), **report}))
