from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import shutil

PROJECT = Path(__file__).resolve().parents[2]
REPOSITORY = PROJECT.parents[1]
RUN = PROJECT / 'game/v6/native-final-0acffa83-20260913-a'
IDENTITY = PROJECT / 'Logs/v6/native-final-0acffa83-20260913-a-host-identity/host-identity.json'
HOST = 'fa31b3608a0f418f6bed58d0cac70c2c09885ba2d9912733c1c8aff07c0f1a1e'
RULES = '0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a'
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
read = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
ref = lambda path: {'path': path.relative_to(PROJECT).as_posix(), 'sha256': sha(path)}
def write_new(path, value):
    with path.open('x', encoding='utf-8') as output:
        json.dump(value, output, ensure_ascii=False, indent=2)
        output.write('\n')

result = read(RUN / 'build-result.json')
identity = read(IDENTITY)
assert result['buildExit'] == 0 and identity['engineSha256'] == HOST
assert identity['rulesFingerprint'] == RULES
before, after = RUN / 'source-before.json', RUN / 'source-after.json'
assert before.read_bytes() == after.read_bytes()
rows = read(after)
for row in rows:
    source = REPOSITORY / row['path']
    assert source.stat().st_size == row['bytes'] and sha(source) == row['sha256'].lower(), row['path']
names = {row['path'] for row in rows}
critical = [REPOSITORY / 'Cargo.toml', REPOSITORY / 'Cargo.lock']
for relative in ('projects/code-sentinels/native-v6', 'crates/engine-host'):
    base = REPOSITORY / relative
    critical.append(base / 'Cargo.toml')
    critical.extend(p for p in [base / 'build.rs'] if p.exists())
    critical.extend((base / 'src').rglob('*.rs'))
assert all(p.relative_to(REPOSITORY).as_posix() in names for p in critical)
receipt = {
    'schemaVersion': 2, 'kind': 'native-build-receipt',
    'generatedAtUtc': datetime.now(timezone.utc).isoformat(),
    'startedAtUtc': result['startedAtUtc'], 'completedAtUtc': result['completedAtUtc'],
    'buildExit': 0, 'artifact': result['artifact'],
    'rulesVersion': identity['rulesVersion'], 'rulesFingerprint': RULES,
    'source': {'scope': 'Complete current native game and engine-host Rust source, Cargo/build inputs, all current native tests, and recorded local dependency sources.',
               'unchangedDuringBuild': True, 'fileCount': len(rows),
               'beforeManifest': ref(before)['path'], 'beforeSha256': sha(before),
               'afterManifest': ref(after)['path'], 'afterSha256': sha(after)},
    'buildCommand': result['buildCommand'], 'buildLog': ref(RUN / 'native-build.log'),
    'observedIdentity': ref(IDENTITY),
    'previousRuntime': 'game/v6/native-final-0acffa83-20260913-a/previous-4bf9973c-runtime',
    'previousReceipt': ref(RUN / 'previous-native-build-receipt.json'),
    'scope': 'Normal native build and real catalogue observation for the final frozen rule sources. Performance, gameplay, LAN and portable acceptance are separate observations.'
}
write_new(RUN / 'native-build-receipt.json', receipt)
for source in (RUN / 'runtime').iterdir():
    if source.is_file():
        shutil.copy2(source, PROJECT / 'game/v6/runtime-bin' / source.name)
assert sha(PROJECT / 'game/v6/runtime-bin/engine-host.exe') == HOST
shutil.copy2(RUN / 'native-build-receipt.json', PROJECT / 'game/v6/native-build-receipt.json')
print(json.dumps({'nativeBuild': ref(RUN / 'native-build-receipt.json'), 'hostIdentity': ref(IDENTITY), 'artifactSha256': HOST, 'rulesFingerprint': RULES, 'sourceFrozen': True, 'genericRuntimeMatches': True}))
