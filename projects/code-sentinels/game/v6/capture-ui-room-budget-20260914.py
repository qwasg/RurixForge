"""Collect actual room-budget GUI evidence without approving incomplete functionality."""
from datetime import datetime, timezone
from pathlib import Path
import hashlib
import json

from PIL import Image

PROJECT = Path(__file__).resolve().parents[2]
PACK = PROJECT / 'dist/final-candidate-d33999c4-20260914/CodeSentinels-V6-Windows'
RUN = PROJECT / 'qa/v6/client/room-budget-20260914'
OUT = RUN / 'native-ui-acceptance.json'
COLD = PROJECT / 'game/v6/cold-start-runs/final-1b1738b7-20260914-a'


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def ref(path):
    path = path.resolve()
    assert path.is_relative_to(PROJECT.resolve())
    return {'path': path.relative_to(PROJECT).as_posix(), 'sha256': sha(path)}


def write(path, data):
    with path.open('x', encoding='utf-8') as stream:
        json.dump(data, stream, ensure_ascii=False, indent=2)
        stream.write('\n')


def own(state, key):
    return sorted((v for v in state[key] if v.get('owner') == 1), key=lambda v: v.get('id', v.get('owner')))


def room(state, rid):
    return next(v for v in state['rooms'] if v['id'] == rid and v['owner'] == 1)


def bpc(value):
    return value['capacityBudget'], value['potentialCapacity'], value['capacity']


def actor(state):
    value = next(v for v in state['units'] if v['id'] == 695)
    assert value['owner'] == 1 and value['kind'] == 'glm'
    return value


def canonical_command(value):
    result = json.loads(json.dumps(value))
    if result['op'] in ('room', 'convert-room'):
        result.setdefault('branch', None)
    if result['op'] == 'wire':
        result.setdefault('unitEndpoints', [])
    return result


assert not OUT.exists(), 'Preserve previous UI report'
target = read(PACK / 'v6-candidate.json')['target']
assert target['engineSha256'] == '1b1738b72e8f2533af60cc058b11894c6e47536f4acfe278b675ea010f21d2bf'
assert target['rulesFingerprint'] == 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'
assert target['payloadSha256'] == 'da4582106a5a651dde29d034d7445206c554feeee26506afb599a9aafc38bf77'
stream = read(RUN / 'native-stream.json')
assert all(stream[k] == v for k, v in target.items())
assert stream['passed'] and stream['actualNativeExecution'] and stream['rgbaFrames'] == 30 and stream['errors'] == []
cleanup = read(RUN / 'gui-process-cleanup.json')
assert cleanup['normalApplicationShutdown'] and cleanup['activeOwnedProcesses'] == []
assert stream['nativePid'] == cleanup['nativePid'] == 45824
assert stream['bridgePid'] == cleanup['bridgePid'] == 44932

names = ['merged-state', 'refit-and-upper-network-state', 'three-gpus-state', 'offline-skill-state',
         'after-compute-wire-state', 'final-saved-state', 'gui-loaded-state', 'replay-before-skill', 'replay-ended-state']
docs = {name: read(RUN / (name + '.json')) for name in names}
states = {name: value['snapshot'] for name, value in docs.items()}
saved_path = PACK / '.forge/save/v6/b0b9f0b5-74d2-4820-a6d2-57747b120201.json'
saved = read(saved_path)
for key in ('engineSha256', 'rulesVersion', 'rulesFingerprint'):
    assert saved[key] == target[key]
assert saved['save']['rulesFingerprint'] == target['rulesFingerprint']
stored = saved['save']['snapshot']
assert stored['tick'] == 26629 and stored['seed'] == 6026
orders = [row for row in saved['save']['orders'] if row['order']['owner'] == 1]
assert [row['order']['sequence'] for row in orders] == list(range(1, 32))
assert saved['save']['sequences'][0] == 31
assert all(row['receipt']['accepted'] and row['receipt']['sequence'] == row['order']['sequence'] for row in orders)
commands = {row['order']['sequence']: row['order']['command'] for row in orders}
browser = orders[16:]
assert len(browser) == 15
assert commands[17] == {'op': 'merge-rooms', 'ids': [50, 53]}
assert sum(c['op'] == 'merge-rooms' for c in commands.values()) == 1
assert commands[18] == {'op': 'remove-gpu', 'room': 50, 'bay': 0}
assert commands[19] == {'op': 'convert-room', 'id': 50, 'kind': 'data-center', 'branch': None}
assert commands[20] == {'op': 'build', 'kind': 'wind-power', 'pos': {'x': 17, 'y': 42, 'z': 0}}
assert commands[21] == {'op': 'entrance', 'kind': 'stairs', 'pos': {'x': 17, 'y': 48, 'z': 1}, 'toLevel': 0, 'width': 1}
assert commands[22]['op'] == 'room' and commands[22]['shell'] == 219
assert commands[22]['rect'] == {'x': 13, 'y': 46, 'z': 1, 'w': 4, 'h': 2}
for sequence, kind in [(23, 'power'), (24, 'compute')]:
    assert commands[sequence]['op'] == 'wire' and commands[sequence]['kind'] == kind
    assert {p['z'] for p in commands[sequence]['path']} == {0, 1}
for sequence in (25, 26, 27):
    assert commands[sequence] == {'op': 'install-gpu', 'room': 50, 'model': 'rtx-5060'}
assert commands[28] == {'op': 'deploy', 'room': 56, 'kind': 'glm', 'pos': {'x': 12, 'y': 50, 'z': 0}}
for sequence, tick in [(29, 24177), (31, 26626)]:
    assert commands[sequence]['op'] == 'skill' and commands[sequence]['id'] == 695
    assert commands[sequence]['pos'] == {'x': 12, 'y': 52, 'z': 0}
    assert orders[sequence - 1]['tick'] == tick
assert commands[30]['op'] == 'wire' and commands[30]['kind'] == 'compute'
assert commands[30]['path'][-1] == {'x': 12, 'y': 50, 'z': 0}

setup_path = RUN / 'paid-room-budget-setup.json'
setup = read(setup_path)
setup_orders = [row for row in setup if row['event'] == 'order']
assert len(setup_orders) == 16
for i, row in enumerate(setup_orders):
    assert canonical_command(row['command']) == commands[i + 1]
    for field in ('accepted', 'sequence', 'tick'):
        assert row['receipt'][field] == orders[i]['receipt'][field]
opening = next(row for row in setup if row['event'] == 'paid opening verified')
assert opening['acceptedOpeningOrders'] == 14 and opening['player']['credits'] >= 500
assert (opening['dc'], opening['secondDc'], opening['lab']) == (50, 53, 56)
ready = next(row for row in setup if row['event'] == 'ready for browser room-budget test')
assert ready['helperOrderBoundary'] == 16 and ready['shell'] == 23
checkpoint_path = PACK / ('.forge/save/v6/' + ready['saved']['id'] + '.json')
checkpoint_doc = read(checkpoint_path)
checkpoint = checkpoint_doc['save']['snapshot']
assert checkpoint_doc['rulesFingerprint'] == target['rulesFingerprint']
assert checkpoint['tick'] == ready['saved']['tick'] == 7577
assert checkpoint_doc['save']['sequences'][0] == 16
initial_rooms = [room(checkpoint, rid) for rid in (50, 53)]
for r in initial_rooms:
    assert r['rect']['w'] == 2 and r['rect']['h'] == 3 and bpc(r) == (1, 1, 1)
    assert r['progress'] == 1 and abs(r['maxHp'] - 140 * (6 / 4) ** 0.85) < 1e-8
assert room(checkpoint, 56)['kind'] == 'research-lab'
merged = room(states['merged-state'], 50)
refitted = room(states['refit-and-upper-network-state'], 50)
assert bpc(merged) == (2, 3, 2) and merged['invested'] == 276
assert merged['gpus'] == ['rtx-5060'] and not any(r['id'] == 53 for r in states['merged-state']['rooms'])
assert merged['hp'] == sum(r['hp'] for r in initial_rooms)
assert merged['maxHp'] == sum(r['maxHp'] for r in initial_rooms)
assert abs(merged['maxHp'] - 395.21698573990477) < 1e-8
assert bpc(refitted) == (3, 3, 3) and refitted['gpus'] == []
assert refitted['hp'] == merged['hp'] and refitted['maxHp'] == merged['maxHp']
assert refitted['invested'] - merged['invested'] == 78
upper = room(states['refit-and-upper-network-state'], 619)
assert upper['rect'] == {'x': 13, 'y': 46, 'z': 1, 'w': 4, 'h': 2}
assert bpc(upper) == (2, 2, 2) and abs(upper['maxHp'] - 252.35012953103245) < 1e-8
assert upper['hp'] == upper['maxHp']
three = states['three-gpus-state']
assert room(three, 50)['gpus'] == ['rtx-5060'] * 3 and bpc(room(three, 50)) == (3, 3, 3)
assert all(r['connected'] for r in own(three, 'rooms'))
player_three = own(three, 'players')[0]
assert [player_three[k] for k in ('production', 'computeCapacity', 'power', 'demand')] == [36, 900, 300, 225]

offline = states['offline-skill-state']
connected = states['after-compute-wire-state']
before = states['replay-before-skill']
end = states['replay-ended-state']
assert offline['tick'] == 24179 and actor(offline)['lastCastTick'] == 24177
assert actor(offline)['battery'] == 35 and actor(offline)['covered'] is False and actor(offline)['wired'] is False
assert connected['tick'] == 25409 and abs(actor(connected)['battery'] - 277.4) < 1e-6
assert actor(connected)['covered'] is True and actor(connected)['wired'] is False
assert before['tick'] == 26029 and actor(before)['lastCastTick'] == 24177 and actor(before)['attackCount'] == 10
assert end['tick'] == stored['tick'] == 26629 and actor(end)['lastCastTick'] == 26626
assert actor(end)['attackCount'] == 11 and actor(end)['battery'] == 300
assert actor(end)['covered'] is True and actor(end)['wired'] is False
assert actor(end)['sourceFacility'] == 56 and actor(end)['invested'] == 660
assert before['playback']['paused'] and end['playback']['paused'] and end['playback']['speed'] == 8
assert states['gui-loaded-state']['tick'] == 26643 and docs['gui-loaded-state']['session']['lastSequence'] == 31
assert bpc(room(states['gui-loaded-state'], 50)) == (3, 3, 3)
assert room(states['gui-loaded-state'], 50)['gpus'] == ['rtx-5060'] * 3
comparisons = []
for collection, count in [('players', 1), ('buildings', 6), ('rooms', 3), ('units', 3), ('links', 7)]:
    assert own(end, collection) == own(stored, collection)
    assert own(states['final-saved-state'], collection) == own(stored, collection)
    assert len(own(end, collection)) == count
    comparisons.append({'collection': collection, 'owner': 1, 'count': count, 'equal': True})
compute_events = [e for e in stored['events'] if e['subject'] == 695 and e['kind'] == 'compute-spent']
skill_events = [e for e in compute_events if e['tick'] in (24177, 26626)]
assert [(e['tick'], e['magnitude']) for e in skill_events] == [(24177, 85), (26626, 85)]
ordinary_attack_events = [e for e in compute_events if e['tick'] not in (24177, 26626)]
assert len(ordinary_attack_events) == 11 and all(e['magnitude'] == 4 for e in ordinary_attack_events)

actions_path = RUN / 'cua-actions.json'
actions = read(actions_path)
completed = ['new-solo', 'build-room', 'power-wire', 'compute-wire', 'install-gpu', 'deploy-unit', 'skill-target', 'change-layer', 'save-load', 'replay', 'merge-room', 'refit-room']
assert set(completed) <= {a['action'] for a in actions}
originals = sorted(RUN.glob('*-original.jpg'))
assert len(originals) == 10
png_dir = RUN / 'png-evidence'
png_dir.mkdir(exist_ok=False)
images = []
for source in originals:
    old_sha = sha(source)
    with Image.open(source) as image:
        assert image.format == 'JPEG'
        rgba = image.convert('RGBA')
        pixels = rgba.tobytes()
        output = png_dir / (source.stem.removesuffix('-original') + '.png')
        rgba.save(output, format='PNG')
    with Image.open(output) as converted:
        assert converted.format == 'PNG' and converted.convert('RGBA').tobytes() == pixels
    assert sha(source) == old_sha
    row = {'original': ref(source), 'originalMime': 'image/jpeg', 'png': ref(output), 'size': list(rgba.size),
           'decodedRgbaSha256': hashlib.sha256(pixels).hexdigest(), 'conversionLosslessForDecodedSource': True}
    if source.name.startswith('02-'):
        row['pixelDescription'] = 'Selected data-center50 panel visibly shows1F,2x3,6cells and197/197 health, with the GPU catalogue open. No visible budget values are inferred from this screenshot; B/P/C are established by native checkpoint data.'
    if source.name.startswith('04-'):
        row['pixelDescription'] = 'The visible paid rack-refit button quotes78; text explains retaining the existing2 racks, current space for3, eight-second work and manual clearing prerequisites.'
    images.append(row)
conversion = png_dir / 'conversion-manifest.json'
write(conversion, {'scope': 'Original JPEG bytes retained; PNG exactly preserves decoded source RGBA, not the pre-JPEG native framebuffer.', 'images': images})
extraction = RUN / 'verified-native-save-orders.json'
write(extraction, {'sourceSave': ref(saved_path), 'sourceCheckpoint': ref(checkpoint_path),
                   'scope': 'Read-back extraction from retained actual native Save files. No new commands, replay or resources were generated by this collector.',
                   'helperOrders': orders[:16], 'browserOrders': browser, 'openingObservation': opening,
                   'originalRoomSnapshots': initial_rooms, 'skillPaymentEvents': skill_events,
                   'ordinaryGlmAttackPaymentEvents': ordinary_attack_events, 'replayOwnCollections': comparisons})
cold_path = COLD / 'cold-start-acceptance.json'
cold = read(cold_path)
assert cold['passed'] and cold['normalShutdown'] and all(cold[k] == v for k, v in target.items())
cold_pngs = []
for item in cold['nativeFrameEvidence']:
    path = Path(item['path'])
    assert sha(path) == item['sha256']
    cold_pngs.append(ref(path))
assert len(cold_pngs) == 2
inputs = [actions_path, RUN / 'native-stream.json', setup_path, RUN / 'gui-process-cleanup.json', conversion, extraction, cold_path]
inputs += [RUN / (name + '.json') for name in names]
evidence = [ref(path) for path in inputs] + [row['png'] for row in images] + cold_pngs
assert sum(r['path'].endswith('.png') for r in evidence) == 12
report = {
    'kind': 'actual-native-ui-acceptance', 'schemaVersion': 2, **target,
    'recordedAtUtc': datetime.now(timezone.utc).isoformat(), 'nativeUiScopePassed': True,
    'actualNativeExecution': True, 'nativePid': 45824, 'bridgePid': 44932,
    'rgbaFrames': 30, 'errors': [], 'completedActions': completed,
    'completeFunctionalityAccepted': False,
    'pendingSignatureRequiredTargets': ['v6_combat_contracts', 'v6_door_openings', 'v6_repair_sources', 'v6_resource_contracts'],
    'scope': 'Actual new room-budget GUI actions, native order/Save/state readback and local RGBA only. The four required OS-blocked targets remain unexecuted; this does not approve complete functionality, balance, pressure, LAN or publication.',
    'browserOrdersFromSavedGame': browser,
    'orderBoundary': {'helperSequences': [1, 16], 'acceptedOpeningOrders': 14, 'browserSequences': [17, 31], 'acceptedBrowserOrders': 15},
    'ordinaryPaidOpening': {'startingCredits': 2000, 'seed': 6026, 'injectedResources': False, 'acceptedOrders': 14,
        'observedTick': opening['tick'], 'creditsRemaining': opening['player']['credits'], 'dc': 50, 'secondDc': 53, 'lab': 56, 'shell': 23,
        'basis': 'CUA recorded displayed2000 for standard solo; helper starts at actual tick883 and uses ordinary paid commands with real elapsed income. Both independent six-cell data centers are native B1/P1/C1.'},
    'roomBudget': {'initialRooms': [{'id': r['id'], 'B': r['capacityBudget'], 'P': r['potentialCapacity'], 'C': r['capacity'], 'hp': r['hp'], 'maxHp': r['maxHp'], 'invested': r['invested']} for r in initial_rooms],
        'merged': {'id': 50, 'B': 2, 'P': 3, 'C': 2, 'hp': merged['hp'], 'maxHp': merged['maxHp'], 'invested': 276},
        'refit': {'id': 50, 'nativeQuoteShown': 78, 'actualInvestedIncrease': 78, 'B': 3, 'P': 3, 'C': 3, 'hpUnchanged': True, 'maxHpUnchanged': True,
                  'manualGpuRemovalSequence': 18, 'paidConvertSequence': 19, 'installedGpuSequences': [25, 26, 27]},
        'upper': {'id': 619, 'rect': upper['rect'], 'hp': upper['hp'], 'maxHp': upper['maxHp'], 'B': 2, 'P': 2, 'C': 2},
        'threeGpuNetwork': {'production': 36, 'computeCapacity': 900, 'power': 300, 'load': 225, 'allThreeOwnRoomsConnected': True},
        'intermediateObservation': 'After manual card removal and refit, the intermediate upper-network snapshot has zero GPUs/production and connected=false. Actual connected=true is established after the three GPU installations.'},
    'skill': {'unit': 695, 'kind': 'glm', 'costPerCast': 85, 'offlineCastTick': 24177, 'offlineObservedTick': 24179,
        'offlineCacheAfterCast': 35, 'initialCacheFromDeploymentAndCua': 120, 'endpointRechargeObservedCache': actor(connected)['battery'],
        'endpointCovered': True, 'wired': False, 'laterCache': 300, 'onlineCastTick': 26626,
        'finalAttackCount': 11, 'skillPaymentEvents': skill_events, 'ordinaryAttackPaymentEvents': ordinary_attack_events},
    'saveLoad': {'saveId': saved['id'], 'savedFileSha256': sha(saved_path), 'savedTick': 26629, 'loadedPausedTick': 26643, 'loadedSequence': 31},
    'replayComparison': {'rewoundTick': 26029, 'beforeLastCastTick': 24177, 'exactEndedTick': 26629, 'endLastCastTick': 26626,
        'speed': 8, 'collections': comparisons, 'scope': 'Exact full equality of five own-player collections; no full private-authority world comparison claim.'},
    'recoveredUiObservation': {'kind': 'merge notification wait', 'waitLimitMs': 3000,
        'basis': 'Root reported that the first post-merge toast wait timed out. Actual native Save contains one accepted merge at sequence17; following native snapshot confirms correct merged data. No second merge was issued.',
        'actualAcceptedMergeOrders': 1, 'gameplayFailureInferred': False},
    'screenshots': images, 'additionalColdStartPngEvidence': cold_pngs,
    'additionalColdStartNativeRgbaFrames': cold['nativeRgbaFrames'], 'normalApplicationShutdown': True,
    'evidence': evidence,
}
write(OUT, report)
print(json.dumps({'report': ref(OUT), 'browserOrders': 15, 'rgbaFrames': 30, 'guiPngs': 10, 'directPngs': 12,
                  'openingCredits': opening['player']['credits'], 'completeFunctionalityAccepted': False}))
