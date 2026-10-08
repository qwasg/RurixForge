"""Register genuine I2V atlases and build the V5 native scene.

This is artifact assembly only: no game launch, DLL execution or gameplay test.
The existing V4 scene, map arrays and playable release are preserved.
"""
import copy
import json
import pathlib
import re
import shutil
from PIL import Image
import build_native as b

ROOT = b.ROOT
REPO = ROOT.parents[1]
MODULE = 'Content/Scripts/sentinels_v5.rs'
BUILDINGS = ['command-core', 'data-center', 'wind-power', 'hydro-power', 'coal-power', 'nuclear-power',
             'mobile-relay', 'research-lab', 'resource-extractor', 'cudad-wall', 'vscode-turret', 'pycharm-turret']
FX = [('kinetic-hit', 32, 32, 'additive'), ('plasma-hit', 32, 24, 'additive'), ('heavy-impact', 48, 24, 'alpha'),
      ('shield-hit', 32, 32, 'additive'), ('power-arc', 32, 24, 'additive'), ('repair', 48, 24, 'additive'),
      ('upgrade', 48, 24, 'additive'), ('collapse-explosion', 48, 24, 'alpha')]
SOURCE = ROOT / 'Content/Animations/v5'
PUBLIC = REPO / 'packages/client/public/games/code-sentinels/animation-v5'


def read(path): return json.loads(path.read_text(encoding='utf-8-sig'))


def import_atlas(group, name, expected_frames):
    path = SOURCE / group / (name + '.png')
    data = read(path.with_suffix('.json'))
    boxes = data.get('boxes', data.get('frames'))
    if not isinstance(boxes, list) or len(boxes) != expected_frames:
        raise ValueError(f'{name}: expected {expected_frames} real frames; not padding missing footage')
    if not data.get('provenance'):
        raise ValueError(f'{name}: genuine video provenance is required')
    with Image.open(path) as image:
        if image.size != (data['width'], data['height']) or image.mode != 'RGBA':
            raise ValueError(f'{name}: image and atlas metadata do not agree')
    for box in boxes:
        x, y, w, h = box
        if min(x, y) < 0 or (w, h) != (256, 256) or x + w > data['width'] or y + h > data['height']:
            raise ValueError(f'{name}: frame rectangle leaves its actual texture')
    frames = {f'frame_{i:03}': {'bbox': box} for i, box in enumerate(boxes)}
    clips = {}
    for key, clip in data['clips'].items():
        start, end = clip['start'], clip['endExclusive']
        if not 0 <= start < end <= expected_frames:
            raise ValueError(f'{name}: invalid {key} clip')
        clips[key] = {'frames': [f'frame_{i:03}' for i in range(start, end)], 'fps': clip['fps'], 'loop': bool(clip['loop'])}
    sprite_name = 'V5-' + name
    guid = b.sprite_doc(sprite_name, path, frames, clips, (.5, .5))
    # Public animation metadata is intentionally limited to display fields;
    # provider responses and signed reference URLs stay in the project pipeline.
    out = PUBLIC / group; out.mkdir(parents=True, exist_ok=True)
    shutil.copy2(path, out / path.name)
    b.json_file(out / (name + '.json'), {key: data[key] for key in
        ['id', 'image', 'width', 'height', 'frames', 'boxes', 'frameCount', 'clips', 'normalizationSpan', 'pivot'] if key in data})
    return guid, data


def main():
    required = [(SOURCE / 'buildings' / (name + suffix)) for name in BUILDINGS for suffix in ['.png', '.json']]
    required += [(SOURCE / 'effects' / (name + suffix)) for name, *_ in FX for suffix in ['.png', '.json']]
    missing = [p.relative_to(ROOT).as_posix() for p in required if not p.is_file()]
    if missing:
        raise FileNotFoundError('V5 media production is incomplete; no release scene was written:\n' + '\n'.join(missing))
    body_ids, effect_ids, inventory = [], [], []
    for kind, name in enumerate(BUILDINGS, 1):
        guid, data = import_atlas('buildings', name, 128)
        expected = {'land': (0, 32, 16, False), 'work': (32, 80, 16, True), 'destroy': (80, 128, 24, False)}
        for phase, spec in expected.items():
            clip = data['clips'].get(phase, {})
            if tuple(clip.get(key) for key in ['start', 'endExclusive', 'fps', 'loop']) != spec:
                raise ValueError(f'{name}: {phase} must match the native 128-frame lifecycle contract')
        body_ids.append(guid)
        inventory.append({'kind': kind, 'id': name, 'frames': 128, 'sprite': f'Content/Sprites/V5-{name}.rxsprite', 'provenance': data['provenance']})
    for kind, (name, count, fps, blend) in enumerate(FX, 1):
        guid, data = import_atlas('effects', name, count)
        effect_ids.append(guid)
        inventory.append({'fxKind': kind, 'id': name, 'frames': count, 'fps': fps, 'blend': blend,
                          'sprite': f'Content/Sprites/V5-{name}.rxsprite', 'provenance': data['provenance']})

    scene = copy.deepcopy(read(ROOT / 'Content/Scenes/Command.rxscene'))
    old_graph = read(ROOT / 'Content/Graphs/V4BatchController.rxgraph')
    bindings = copy.deepcopy(next(node['inputs']['bindings']['const'] for node in old_graph['nodes'] if node['type'] == 'call.native_frame'))
    scene['entities'] = [e for e in scene['entities'] if not e['name'].startswith('C4_GPUVisual')]
    kept = {e['id'] for e in scene['entities']}
    bindings = [binding for binding in bindings if binding['entityId'] in kept]
    by_entity = {binding['entityId']: binding for binding in bindings}

    def variant_props(guids, stride, order, blend, ppu):
        return {'texture': '', 'sprite': guids[0], 'spriteVariants': guids, 'variantStride': stride,
                'clip': '', 'frame': 0., 'tint': [1., 1., 1., 1.], 'flipX': False, 'flipY': False,
                'pixelsPerUnit': ppu, 'sortingOrder': float(order), 'chromaKey': 'none', 'blendMode': blend}
    for entity in scene['entities']:
        if entity['name'] == 'C4_Controller':
            entity['name'] = 'C5_Controller'
            entity['components'][0]['props']['graphRef'] = 'Content/Graphs/V5BatchController.rxgraph'
        if entity['name'].startswith('C4_Structure') or re.fullmatch(r'C4_Actor\d+_[12]', entity['name']):
            sprite = next(c for c in entity['components'] if c['type'] == 'Sprite')
            order = 15 if 'Actor' in entity['name'] else 12
            sprite['props'] = variant_props(body_ids, 128, order, 'alpha', 256.)
            by_entity[entity['id']]['kind'] = 2

    next_id = max(e['id'] for e in scene['entities']) + 1
    def append(name, components, binding_kind, data, position=(-100., -100., 0.)):
        nonlocal next_id
        entity = {'id': next_id, 'name': name, 'transform': {'translation': list(position), 'scale': [1., 1., 1.], 'rotation': [0., 0., 0., 1.]}, 'components': components}
        scene['entities'].append(entity)
        bindings.append({'entityId': next_id, 'kind': binding_kind, 'data': (list(data) + [0] * 6)[:6]})
        next_id += 1
    def sprite(props): return {'type': 'Sprite', 'enabled': True, 'props': props}
    def state(name, keys): append(name, [], 0, keys, (0., 0., 0.))
    for slot in range(24): append(f'C5_DestroyedVisual{slot}', [sprite(variant_props(body_ids, 128, 14, 'alpha', 256.))], 2, [1200 + slot])
    for slot in range(64):
        append(f'C5_ImpactAdditive{slot}', [sprite(variant_props(effect_ids, 48, 32, 'additive', 128.))], 2, [1300 + slot])
        append(f'C5_ImpactAlpha{slot}', [sprite(variant_props(effect_ids, 48, 26, 'alpha', 128.))], 2, [1400 + slot])
    state('C5_Global', list(range(12000, 12006)))
    for slot in range(48): state(f'C5_BuildingAnim{slot}', list(range(13000 + slot * 8, 13006 + slot * 8)))
    for slot in range(32): state(f'C5_UnitAnim{slot}', list(range(14000 + slot * 8, 14006 + slot * 8)))
    for slot in range(256):
        state(f'C5_Event{slot}', list(range(16000 + slot * 12, 16006 + slot * 12)))
        state(f'C5_EventMeta{slot}', list(range(16006 + slot * 12, 16012 + slot * 12)))

    b.MODULE = MODULE
    graph = b.Graph('V5BatchController')
    def frame(dt): return graph.n('call.native_frame', module=graph.c(MODULE), fn=graph.c('cs5_frame'), dt=dt, bindings=graph.c(bindings), animatorParam=graph.c('attacking'))
    start = graph.n('event.on_start'); reset = graph.call('cs5_reset', graph.c([])); graph.link(start, reset); graph.link(reset, frame(graph.c(0.)))
    update = graph.n('event.on_update'); graph.link(update, frame(graph.p(update, 'dt')))
    event = graph.n('event.on_input'); save = graph.n('var.set', name=graph.p(event, 'action'), value=graph.p(event, 'value')); graph.link(event, save)
    value = graph.n('var.get', name=graph.c('cs5')); command = graph.call('cs5_input', graph.p(value, 'out')); graph.link(save, command)
    clear = graph.n('var.set', name=graph.c('cs5'), value=graph.c(0.)); graph.link(command, clear); graph.emit()
    scene['name'] = 'Code Sentinels V5 — 动态战线'; scene['next_id'] = next_id
    output = ROOT / 'Content/Scenes/CommandV5.rxscene'
    b.json_file(output, scene); b.meta(output, 'scene')
    b.json_file(ROOT / 'game/v5/media-inventory.json', {'version': 5, 'buildingFamilies': 12, 'buildingClips': 36, 'impactClips': 8,
        'newVideoFrames': 1536 + sum(item[1] for item in FX), 'items': inventory})
    b.json_file(ROOT / 'game/v5/scene-build.json', {'version': 5, 'scene': str(output), 'entities': len(scene['entities']), 'bindings': len(bindings),
        'bodyAtlasVariants': len(body_ids), 'effectAtlasVariants': len(effect_ids), 'rendererFeature': 'spriteVariants-v1', 'nativeModule': MODULE,
        'gameStarted': False, 'testsRun': False, 'originalCharacterAndSkillAtlasesRetained': True})
    print(json.dumps({'scene': str(output), 'entities': len(scene['entities']), 'mediaFrames': 1856}, ensure_ascii=False))


if __name__ == '__main__': main()
