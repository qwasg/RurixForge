"""Replay the former F3 budget-exhausted scenario using the frozen observer."""
from pathlib import Path
import datetime
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'game/v6/final-balance-0acffa83-20260913/strategy'
PILOT = ROOT / 'game/v6/escort-two-pilot-20260912'
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
read = lambda p: json.loads(p.read_text(encoding='utf-8'))
now = lambda: datetime.datetime.now(datetime.timezone.utc).isoformat()
manifest = read(PILOT / 'candidate-manifest.json')
artifact = next(a for a in manifest['artifacts'] if 'formation-purchase-observer' in a['frozen'])
exe = PILOT / artifact['frozen']
assert sha(exe) == artifact['sha256']
saved = OUT / 'diagnostics/match-110.save.json'
observed = OUT / 'formation-110-observation.jsonl'
command = [str(exe), '110', str(saved), str(observed)]
invocation = dict(command=command, cwd=str(ROOT.parents[1]), startedAtUtc=now(), rulesFingerprint=manifest['rulesFingerprint'], observerSha256=sha(exe), sourceSaveSha256=sha(saved))
with (OUT / 'replay-110-invocation.json').open('x', encoding='utf-8') as f: json.dump(invocation,f,indent=2)
with (OUT / 'replay-110.stdout.log').open('x', encoding='utf-8') as stdout, (OUT / 'replay-110.stderr.log').open('x', encoding='utf-8') as stderr:
    result = subprocess.run(command, cwd=ROOT.parents[1], stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
receipt = dict(**invocation, exitCode=result.returncode, endedAtUtc=now())
with (OUT / 'replay-110-execution.json').open('x', encoding='utf-8') as f: json.dump(receipt,f,indent=2)
assert result.returncode == 0
lines = [json.loads(l) for l in observed.read_text(encoding='utf-8').splitlines()]
assert lines[0]['sourceSaveSha256'] == sha(saved) and lines[0]['rulesFingerprint'] == manifest['rulesFingerprint']
assert lines[-1]['matchedOriginalSnapshot'] and lines[-1]['type'] == 'complete'
print(json.dumps(dict(index=110, exactFullSnapshotReplay=True, outerSerializedSaveEqualityTested=False, final=lines[-1], observationSha256=sha(observed))))
