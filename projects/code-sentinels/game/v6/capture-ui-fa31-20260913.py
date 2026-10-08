"""Verify actual CUA actions, native receipts and lossless screenshot conversions."""
import hashlib
import json
from pathlib import Path
from PIL import Image

project = Path(__file__).resolve().parents[2]
pack = project / 'dist/final-candidate-0acffa83-20260913/CodeSentinels-V6-Windows'
run = project / 'qa/v6/client/fa31-20260913'
output = run / 'native-ui-acceptance.json'
assert not output.exists(), 'Preserve prior evidence'
read = lambda p: json.loads(p.read_text(encoding='utf-8-sig'))
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
ref = lambda p: {'path': p.relative_to(project).as_posix(), 'sha256': sha(p)}
target = read(pack / 'v6-candidate.json')['target']
stream = read(run / 'native-stream.json')
assert stream['passed'] and stream['rgbaFrames'] >= 30 and stream['errors'] == []
for key, value in target.items(): assert stream[key] == value, key
before = read(run / 'replay-before-skill.json')['snapshot']
end_doc = read(run / 'replay-ended-state.json')
end = end_doc['snapshot']
save_path = pack / '.forge/save/v6/8a25a2eb-2332-466b-b605-0c1ee42bdc58.json'
saved = read(save_path)
stored = saved['save']['snapshot']
for key in ('rulesVersion', 'rulesFingerprint', 'engineSha256'):
    assert saved[key].lower() == target[key].lower(), key
assert end_doc['session']['replay'] is True
assert before['tick'] == 34431 and end['tick'] == stored['tick'] == 35031
assert before['playback']['paused'] and end['playback']['paused']
assert end['playback']['speed'] == 8
assert next(u for u in before['units'] if u['id'] == 693)['lastCastTick'] is None
actor = next(u for u in end['units'] if u['id'] == 693)
assert actor['lastCastTick'] == 35019 and actor['attackCount'] == 53
assert actor['covered'] is True and actor['wired'] is False and actor['battery'] == 300
comparisons = []
for key in ('players', 'buildings', 'rooms', 'units', 'links'):
    own = lambda state: sorted([v for v in state[key] if v.get('owner') == 1], key=lambda v: v.get('id', v.get('owner')))
    assert own(end) == own(stored), key
    comparisons.append({'collection': key, 'owner': 1, 'count': len(own(end)), 'equal': True})
orders = [r for r in saved['save']['orders'] if r['order']['owner'] == 1 and r['order']['sequence'] > 15]
assert len(orders) == 9 and all(r['receipt']['accepted'] for r in orders)
commands = [r['order']['command'] for r in orders]
for op in ('entrance', 'room', 'build', 'install-gpu', 'deploy', 'skill'):
    assert any(c['op'] == op for c in commands), op
assert any(c['op'] == 'room' and c['rect'] == {'x':13,'y':46,'z':1,'w':4,'h':2} for c in commands)
for kind in ('power', 'compute'):
    assert any(c['op'] == 'wire' and c['kind'] == kind and {p['z'] for p in c['path']} == {0,1} for c in commands), kind
assert any(c['op'] == 'install-gpu' and c['room'] == 343 and c['model'] == 'rtx-5060' for c in commands)
assert any(c['op'] == 'deploy' and c['kind'] == 'glm' and c['pos'] == {'x':12,'y':50,'z':0} for c in commands)
assert next(c for c in commands if c['op'] == 'skill')['id'] == 693
actions_path = run / 'cua-actions.json'
actions = read(actions_path)
completed = ['new-solo','build-room','power-wire','compute-wire','install-gpu','deploy-unit','skill-target','change-layer','save-load','replay']
assert set(completed) <= {r['action'] for r in actions}
setup = read(run / 'paid-layer-setup.json')
opening = next(r for r in setup if r['event'] == 'paid opening verified')
assert opening['player']['credits'] >= 500
assert sum(r['event'] == 'order' for r in setup[:setup.index(opening)]) == 13
assert all(r['receipt']['accepted'] for r in setup if r['event'] == 'order')
png_dir = run / 'png-evidence'
png_dir.mkdir(exist_ok=False)
images = []
for source in sorted(run.glob('*-original.jpg')):
    with Image.open(source) as image:
        mime = Image.MIME[image.format]
        rgba = image.convert('RGBA')
        pixels = rgba.tobytes()
        destination = png_dir / (source.stem.removesuffix('-original') + '.png')
        rgba.save(destination, format='PNG')
    with Image.open(destination) as converted:
        assert converted.format == 'PNG' and converted.convert('RGBA').tobytes() == pixels
    images.append({'original': ref(source), 'originalMime': mime, 'png': ref(destination),
                   'size': list(rgba.size), 'decodedRgbaSha256': hashlib.sha256(pixels).hexdigest(),
                   'conversionLosslessForDecodedSource': True})
assert len(images) == 7
manifest_path = png_dir / 'conversion-manifest.json'
manifest_path.write_text(json.dumps({'scope':'Original CUA screenshot bytes are JPEG. PNG conversion preserves their decoded RGBA, not an uncompressed native-frame claim.', 'images':images}, ensure_ascii=False, indent=2), encoding='utf-8')
report = {
    'kind': 'actual-native-ui-acceptance', 'schemaVersion': 2, **target,
    'scope': 'Actual CUA browser actions with ordinary native orders. API setup is identified separately; test does not replace full balance, stress performance or 45-minute LAN.',
    'actualNativeExecution': True, 'rgbaFrames': stream['rgbaFrames'], 'errors': [],
    'completedActions': completed, 'browserOrdersFromSavedGame': orders,
    'browserOrderBoundary': 'After the15 accepted API setup/research/upper-shell orders. The final saved replay contains the nine subsequent CUA-driven orders, and excludes the earlier preserved collapsed-session attempt.',
    'ordinaryPaidOpening': {'startingCredits':2000, 'standardOrdinarySoloSession':True,
        'injectedResources':False, 'acceptedOrders':13, 'creditsRemaining':opening['player']['credits'],
        'observedTick':opening['tick'], 'scope':'Standard2000-credit native session created in the UI; the setup helper uses paid commands and real elapsed time, including passive income before its first order.'},
    'replayComparison': {'saveId': save_path.stem, 'savedFileSha256': sha(save_path),
        'rewoundTick':34431, 'exactEndedTick':35031, 'speed':8, 'collections':comparisons,
        'scope':'Exact saved-tick equality of all five own-player collections; not claimed to compare the full private authority world or every serialized Save field.'},
    'skill': {'unit':693, 'lastCastTick':35019, 'attackCount':53, 'supply':'Fixed compute line, covered=true; native wired=false. Earlier offline cache76 then0 observed in CUA.', 'costFromCatalog':85},
    'historicalUiAttempt': {'preservedSaveId':'aa331959-7fac-4859-97d3-e4f174823066',
        'note':'Underdefended shells were destroyed by actual enemy attacks during a long first inspection. The real earned T2 checkpoint was loaded via the GUI; no lost buildings were silently reconstructed or injected.'},
    'screenshots': images,
    'evidence': [ref(p) for p in (actions_path, run/'native-stream.json', run/'paid-layer-setup.json',
        run/'replay-before-skill.json',run/'replay-ended-state.json',run/'gui-process-cleanup.json',manifest_path)]
        + [entry['png'] for entry in images],
}
output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps({'output':str(output),'sha256':sha(output),'browserOrders':len(orders),'rgbaFrames':stream['rgbaFrames'],'pngs':len(images),'creditsRemaining':opening['player']['credits']}))
