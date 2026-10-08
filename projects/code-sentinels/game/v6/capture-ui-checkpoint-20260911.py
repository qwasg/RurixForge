"""Read back the actual browser-driven checkpoint; never issues gameplay commands."""
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import urlopen

project = Path(__file__).resolve().parents[2]
package = project / 'dist/final-candidate-bfdcd05d-20260911/CodeSentinels-V6-Windows'
out = project / 'qa/v6/client/browser-checkpoint-bfdcd05d-20260911.json'
assert not out.exists(), 'Preserve previous evidence'
read = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
ref = lambda path: {'path': path.relative_to(project).as_posix(), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
state = json.load(urlopen('http://127.0.0.1:53155/api/v6/status'))
saved_path = package / '.forge/save/v6/0cecb8bf-6af8-48fc-a28b-96b45d19862d.json'
saved = read(saved_path)
assert state['session']['replay'] is True
assert state['snapshot']['tick'] == saved['save']['snapshot']['tick'] == 11542
assert state['snapshot']['playback']['paused'] is True
snapshot = state['snapshot']
original = saved['save']['snapshot']
compared = []
for collection in ('players', 'buildings', 'rooms', 'units', 'links'):
    own = lambda value: sorted((item for item in value.get(collection, []) if item.get('owner') == 1), key=lambda item: item.get('id', item.get('owner', 0)))
    assert own(snapshot) == own(original), f'{collection}: saved owner state differs from UI replay'
    compared.append({'collection': collection, 'owner': 1, 'count': len(own(snapshot)), 'equal': True})
glm = next(unit for unit in snapshot['units'] if unit['id'] == 247)
assert glm['battery'] == 35 and glm['lastCastTick'] == 9773 and glm['covered'] is False
logs = [json.loads(line) for line in (package / 'Logs/v6/bridge.jsonl').read_text(encoding='utf-8').splitlines()]
orders = [entry for entry in logs if entry.get('event') == 'order']
skill = [entry for entry in orders if entry['order']['command']['op'] == 'skill' and entry['result']['accepted'] and entry['result']['tick'] == 9773]
assert len(skill) == 1
screens = [project / 'qa/v6/client' / name for name in ('glm-offline-skill-bfdcd05d-20260911.png', 'replay-offline-glm-bfdcd05d-20260911.png', 'replay-rewind-bfdcd05d-20260911.png')]
assert all(path.is_file() for path in screens)
report = {
    'schemaVersion': 2, 'kind': 'native-browser-checkpoint', 'testedAt': datetime.now(timezone.utc).isoformat(),
    **read(package / 'v6-candidate.json')['target'], 'finalEligible': False,
    'scope': 'Actual CUA browser actions and read-only native state verification on the bfdcd05d development checkpoint. Later bot rules have changed; this is historical evidence, not final-version acceptance. Screenshots are observations, not a measured frame counter or a performance result.',
    'actualNativeExecution': True, 'nativePid': 11704,
    'completedActions': ['new-solo', 'build-room', 'power-wire', 'compute-wire', 'install-gpu', 'deploy-unit', 'skill-target', 'change-layer', 'save-load', 'replay'],
    'ordinaryPreparation': ref(project / 'game/v6/ui-final-paid-opening-bfdcd05d-20260911.json'),
    'browserOrders': orders,
    'offlineSkill': {'unit': 247, 'kind': 'glm', 'initialCarriedCompute': 120, 'remainingCarriedCompute': glm['battery'], 'spent': 85, 'covered': False, 'lastCastTick': glm['lastCastTick'], 'receipt': skill[0]},
    'replay': {'saveId': saved['id'], 'tick': snapshot['tick'], 'speed': 8, 'completedNaturally': True, 'rewindObservedTick': 10942, 'ownerStateComparisons': compared, 'wholeAuthorityStateComparison': False},
    'screenshots': [ref(path) for path in screens],
    'limitations': ['No final host fingerprint claim.', 'CPU balance shards were active; screenshot FPS is not performance acceptance.', 'Optional later wiring attempt was cancelled without an order after rapid resume/tool input; not counted as a successful wire.', 'A normal enemy attack destroyed the first test branch base while debugging; saved separately as action 11:28.'],
}
out.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps({'output': str(out), 'ownerCollectionsVerified': compared, 'browserOrders': len(orders), 'skillTick': glm['lastCastTick']}, ensure_ascii=False))
