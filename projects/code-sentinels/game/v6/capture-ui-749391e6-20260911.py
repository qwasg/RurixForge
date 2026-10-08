"""Read actual CUA/native results; never performs gameplay or changes a receipt."""
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import urlopen

project = Path(__file__).resolve().parents[2]
package = project / 'dist/final-candidate-749391e6-20260911/CodeSentinels-V6-Windows'
out = project / 'qa/v6/client/native-ui-749391e6-20260911.json'
assert not out.exists(), 'Keep previous evidence'
read = lambda p: json.loads(p.read_text(encoding='utf-8-sig'))
ref = lambda p: {'path': p.relative_to(project).as_posix(), 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()}
target = read(package / 'v6-candidate.json')['target']
stream_file = project / 'qa/v6/client/native-stream-749391e6-20260911.json'
stream = read(stream_file)
assert stream['passed'] and stream['rgbaFrames'] >= 30 and not stream['errors']
for key, value in target.items():
    assert stream[key] == value
state = json.load(urlopen('http://127.0.0.1:60725/api/v6/status'))
saved_file = package / '.forge/save/v6/3376206a-88f4-49b7-af0f-170ee25e0b85.json'
saved = read(saved_file)
snapshot, original = state['snapshot'], saved['save']['snapshot']
assert state['session']['replay'] is True
assert snapshot['tick'] == original['tick'] == 23345
assert snapshot['playback']['paused'] is True
for key in ('rulesVersion', 'rulesFingerprint', 'engineSha256'):
    assert saved[key].lower() == target[key].lower()
compared = []
for collection in ('players', 'buildings', 'rooms', 'units', 'links'):
    own = lambda value: sorted((item for item in value.get(collection, []) if item.get('owner') == 1), key=lambda item: item.get('id', item.get('owner', 0)))
    assert own(snapshot) == own(original), collection
    compared.append({'collection': collection, 'count': len(own(snapshot)), 'owner': 1, 'equal': True})
glm = next(unit for unit in snapshot['units'] if unit['id'] == 670)
assert glm['battery'] == 35 and glm['lastCastTick'] == 23001 and not glm['covered']
room = next(item for item in snapshot['rooms'] if item['id'] == 333)
assert room['rect'] == {'x': 13, 'y': 46, 'z': 1, 'w': 4, 'h': 2}
logs = [json.loads(line) for line in (package / 'Logs/v6/bridge.jsonl').read_text(encoding='utf-8').splitlines()]
orders = [row for row in logs if row.get('event') == 'order']
browser_orders = [row for row in orders if row['order']['sequence'] > 15]
accepted = [row for row in browser_orders if row['result']['accepted']]
for op in ('entrance', 'room', 'build', 'install-gpu', 'deploy', 'skill'):
    assert any(row['order']['command']['op'] == op for row in accepted), op
for kind in ('power', 'compute'):
    assert any(row['order']['command']['op'] == 'wire' and row['order']['command']['kind'] == kind and {p['z'] for p in row['order']['command']['path']} == {0, 1} for row in accepted), kind
skill = next(row for row in accepted if row['order']['command']['op'] == 'skill')
assert skill['result']['tick'] == 23001
screens = [project / 'qa/v6/client' / name for name in (
    'upper-room-network-749391e6-20260911.png', 'offline-glm-skill-749391e6-20260911.png',
    'loaded-battle-749391e6-20260911.png', 'replay-upper-floor-749391e6-20260911.png',
    'replay-rewind-cache-749391e6-20260911.png', 'replay-final-state-749391e6-20260911.png')]
assert all(p.is_file() for p in screens)
report = {
    'schemaVersion': 2, 'kind': 'native-ui-acceptance', **target,
    'testedAt': datetime.now(timezone.utc).isoformat(), 'finalEligible': True,
    'scope': 'Actual CUA browser operation on the current candidate, plus separately counted local native RGBA. UI-only acceptance; not global release, full hidden-authority equality, balance, performance, or LAN acceptance.',
    'actualNativeExecution': True, 'rgbaFrames': stream['rgbaFrames'], 'errors': stream['errors'],
    'completedActions': ['new-solo', 'build-room', 'power-wire', 'compute-wire', 'install-gpu', 'deploy-unit', 'skill-target', 'change-layer', 'save-load', 'replay'],
    'evidence': [ref(stream_file), ref(project / 'game/v6/ui-final-paid-opening-749391e6-20260911.json'), *map(ref, screens)],
    'preparation': 'The CUA lobby started a normal solo match. The separately recorded script issued ordinary paid commands and waited for real T2 research and upper-shell construction. It never inserted resources, replaced state, or advanced the game clock.',
    'browserOrders': browser_orders,
    'expandedRoom': {'id': 333, 'rect': room['rect'], 'observedRackCapacity': 2, 'observedPowerSupply': 300, 'observedPowerLoad': 170, 'observedComputePerSecond': 24, 'observedConnectedComputeCapacity': 600},
    'offlineSkill': {'unit': 670, 'kind': 'glm', 'batteryBefore': 120, 'batteryAfter': 35, 'spent': 85, 'covered': False, 'tick': 23001, 'receipt': skill},
    'saveLoadReplay': {'saveId': saved['id'], 'savedTick': 23345, 'loadedDisplayedTime': '06:43', 'loadedTimeScope': 'Live load continued normal time; screenshot displays06:43. Exact equality is checked at the saved replay tick, not the later running load.', 'replaySpeed': 8, 'rewindTick': 22745, 'rewindBattery': 120, 'replayFinalBattery': 35, 'ownerStateComparisons': compared, 'wholeAuthorityStateCompared': False},
    'observationNotes': ['One rejected initial stairs click used coordinates from the older1280x720 CSS viewport. The new1366x986 page letterboxes the same1280x720 native canvas; actual observed canvas bounds were then used. This agent input mistake is retained in browserOrders.', 'Paused live UI requests a lower stream rate; UI FPS displays are not pressure results.', 'All current source rules and payload identities were verified by the separate native stream observer.'],
}
out.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps({'output': str(out), 'acceptedBrowserOrders': len(accepted), 'rejectedBrowserOrders': len(browser_orders)-len(accepted), 'rgbaFrames': stream['rgbaFrames'], 'comparisons': compared}))
