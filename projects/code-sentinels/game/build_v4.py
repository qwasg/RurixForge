"""Author V4 map data, native scene and sprite atlases without running a game.

The terrain bitmap and Rust MAP_DATA are generated from the same arrays. This
file does not load a DLL or call gameplay exports. Existing V2 assets stay intact.
"""
import argparse
import copy
import json
import math
import random
import re
from PIL import Image, ImageDraw, ImageFilter, ImageOps
import build_native as b

ROOT = b.ROOT
TEX = b.TEX
SPR = b.SPR
MODULE = 'Content/Scripts/sentinels_v4.rs'
b.MODULE = MODULE
W, H, TILE = 32, 20, 80
DATA = ROOT / 'Content/Data/maps-v4.json'
ART = ROOT / 'Content/UI/v4/art'
BUILDING_ART = ['command-core', 'data-center', 'wind-power', 'hydro-power', 'coal-power', 'nuclear-power', 'mobile-relay', 'research-lab', 'resource-extractor', 'cudad-wall']


def authored_maps():
    layouts = []
    for level in range(1, 4):
        cells = [0] * (W * H)
        rng = random.Random(4100 + level * 719)

        def set_cell(x, y, terrain):
            if 0 <= x < W and 0 <= y < H:
                cells[y * W + x] = terrain

        def ellipse(cx, cy, rx, ry, terrain):
            for y in range(max(0, int(cy - ry)), min(H, int(cy + ry) + 1)):
                for x in range(max(0, int(cx - rx)), min(W, int(cx + rx) + 1)):
                    if ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1:
                        set_cell(x, y, terrain)

        # Two home plateaus, valuable forward high ground, and a river with
        # several crossing choices give both economic and military destinations.
        for cx, cy, rx, ry in [(4, 3, 4, 2.5), (27, 16, 3.5, 2.5), (10, 12, 2.5, 2), (23, 5, 3, 2)]:
            ellipse(cx, cy, rx, ry, 4)
        for y in range(H):
            mid = 15 + round(math.sin((y + level) * .48))
            width = 2 if level != 2 else 3
            for x in range(mid, mid + width):
                set_cell(x, y, 2)
        if level == 2:
            ellipse(12, 3, 3.5, 2.4, 2)
            ellipse(20, 16, 3.4, 2.1, 2)
        if level == 3:
            for cx, cy in [(8, 4), (24, 14), (20, 3)]:
                ellipse(cx, cy, 3, 2.2, 4)
        for x, start, end in [(10, 1, 8), (22, 11, 19)]:
            for y in range(start, end):
                if y not in (5, 10, 15):
                    set_cell(x + int(y % 3 == 0), y, 1)
        for _ in range(20):
            x, y = rng.randrange(2, 30), rng.randrange(H)
            if cells[y * W + x] == 0 and y not in [5, 9, 10, 11, 15]:
                set_cell(x, y, 1)
        # Three east-west arteries are authored after ridges and river.
        for y in [5, 10, 15]:
            for x in range(W):
                set_cell(x, y, 3)
        for x in [3, 28]:
            for y in range(2, 19):
                if cells[y * W + x] != 1:
                    set_cell(x, y, 3)
        # Starter deposits on each side, plus richer contested extraction sites.
        for x, y in [(6, 8), (6, 13), (25, 8), (25, 13), (12, 11), (20, 8)]:
            for dx, dy in [(0, 0), (1, 0), (0, 1)]:
                set_cell(x + dx, y + dy, 5)
        for x, y in [(8, 16), (24, 3), (11, 3), (21, 16)]:
            for dx, dy in [(0, 0), (1, 0), (0, 1)]:
                set_cell(x + dx, y + dy, 6)
        for stage in range(3):
            terrain = cells.copy()
            if stage >= 1:
                # The north crossing closes; a flank crossing opens at row 8.
                for x in range(14, 19):
                    terrain[5 * W + x] = 2 if level == 2 else 1
                    terrain[8 * W + x] = 3
                for y in [3, 4, 6, 7]:
                    terrain[y * W + 10] = 0
            if stage >= 2:
                # Boss collapse opens a southern bridge and breaks a ridge.
                for x in range(12, 21):
                    terrain[17 * W + x] = 3
                for y in range(12, 18):
                    terrain[y * W + 22] = 0
                for x in range(14, 19):
                    terrain[5 * W + x] = 4 if level == 3 else 3
            for base in [323, 348]:
                bx, by = base % W, base // W
                for dy in range(2):
                    for dx in range(2):
                        terrain[(by + dy) * W + bx + dx] = 7
            for entry in [191, 511, 25, 160, 480, 6]:
                terrain[entry] = 3
                x, y = entry % W, entry // W
                for yy in range(y, min(H, y + 3)):
                    terrain[yy * W + x] = 3
            layouts.append({'level': level, 'stage': stage, 'seed': 4100 + level * 719, 'cells': terrain})
    return {'version': 4, 'width': W, 'height': H, 'baseCells': [323, 348],
            'spawnCells': [[191, 511, 25], [160, 480, 6]],
            'terrainNames': ['平地', '岩壁', '水域', '道路', '高地', '矿脉', '煤层', '基地候选地'], 'layouts': layouts}


def paint_maps(doc):
    with Image.open(TEX / 'open-battlefield.png') as source:
        ground = ImageOps.fit(source.convert('RGBA'), (W * TILE, H * TILE), method=Image.Resampling.LANCZOS)
    with Image.open(TEX / 'terrain-tiles.png') as source:
        materials = source.convert('RGBA')
    patches = {}
    for terrain, index in {1: 0, 2: 1, 3: 2, 4: 3, 5: 3, 6: 5, 7: 2}.items():
        patches[terrain] = materials.crop(((index % 3) * 512, (index // 3) * 512, (index % 3 + 1) * 512, (index // 3 + 1) * 512)).resize((TILE * 3, TILE * 3), Image.Resampling.LANCZOS)
    for layout in doc['layouts']:
        cells = layout['cells']; image = ground.copy()
        for terrain, patch in patches.items():
            surface = Image.new('RGBA', image.size)
            for yy in range(0, image.height, patch.height):
                for xx in range(0, image.width, patch.width):
                    surface.paste(patch, (xx, yy))
            mask = Image.new('L', image.size); draw = ImageDraw.Draw(mask)
            pad = 3 if terrain in [1, 2, 4] else 6
            for cell, value in enumerate(cells):
                if value != terrain: continue
                x, y = cell % W * TILE, cell // W * TILE
                draw.rounded_rectangle((x + pad, y + pad, x + TILE - pad, y + TILE - pad), radius=12, fill=255)
                if cell % W < W - 1 and cells[cell + 1] == terrain:
                    draw.rectangle((x + TILE // 2, y + pad, x + TILE * 1.5, y + TILE - pad), fill=255)
                if cell // W < H - 1 and cells[cell + W] == terrain:
                    draw.rectangle((x + pad, y + TILE // 2, x + TILE - pad, y + TILE * 1.5), fill=255)
            mask = mask.filter(ImageFilter.GaussianBlur(1.5))
            image = Image.composite(surface, image, mask)
        draw = ImageDraw.Draw(image)
        for cell, terrain in enumerate(cells):
            cx, cy = (cell % W + .5) * TILE, (cell // W + .5) * TILE
            if terrain == 5:
                for ox, oy, radius in [(-15, 7, 12), (8, -7, 16), (19, 13, 9)]:
                    draw.regular_polygon((cx + ox, cy + oy, radius), 5, rotation=15, fill='#6ac7bf', outline='#c8f0cc', width=2)
                draw.line([(cx - 28, cy + 25), (cx + 28, cy + 25)], fill='#d5b978', width=3)
            elif terrain == 6:
                for ox, oy in [(-16, 8), (1, -7), (18, 10)]:
                    draw.regular_polygon((cx + ox, cy + oy, 15), 6, fill='#293138', outline='#947b68', width=2)
                draw.line([(cx - 24, cy + 27), (cx + 24, cy + 27)], fill='#c28b68', width=3)
            elif terrain == 4 and cell // W < H - 1 and cells[cell + W] != 4:
                draw.line([(cx - 26, cy + 32), (cx + 26, cy + 32)], fill='#b9bd99', width=3)
        for side, base in enumerate(doc['baseCells']):
            x, y = base % W * TILE, base // W * TILE
            color = '#62cad4' if side == 0 else '#d29a94'
            draw.rounded_rectangle((x + 3, y + 3, x + TILE * 2 - 3, y + TILE * 2 - 3), radius=9, fill='#1a363a', outline=color, width=4)
            for ox, oy in [(12, 12), (TILE * 2 - 28, 12), (12, TILE * 2 - 16), (TILE * 2 - 28, TILE * 2 - 16)]:
                draw.line([(x + ox, y + oy), (x + ox + 16, y + oy)], fill=color, width=4)
        for entries in doc['spawnCells']:
            for cell in entries:
                cx, cy = (cell % W + .5) * TILE, (cell // W + .5) * TILE
                draw.regular_polygon((cx, cy, 17), 6, outline='#d8a0a0', width=3)
        image = image.convert('RGB')
        target = TEX / f'v4-terrain-{layout["level"]}-{layout["stage"]}.png'
        image.save(target); b.meta(target, 'texture')


def build_maps():
    doc = authored_maps(); b.json_file(DATA, doc)
    code = 'const MAP_DATA:[[u8;N];9]=[\n' + ',\n'.join('[' + ','.join(map(str, row['cells'])) + ']' for row in doc['layouts']) + '\n];'
    source = ROOT / MODULE
    text = source.read_text(encoding='utf8')
    text, count = re.subn(r'// MAP_DATA_BEGIN[^\n]*\n.*?// MAP_DATA_END', '// MAP_DATA_BEGIN (generated from Content/Data/maps-v4.json)\n' + code + '\n// MAP_DATA_END', text, flags=re.S)
    if count != 1: raise RuntimeError('Native terrain injection marker is missing or ambiguous')
    source.write_text(text, encoding='utf8')
    paint_maps(doc)
    print('Authored 9 terrain layers and native MAP_DATA from one 32x20 map document.')


def building_atlas():
    size = 384
    atlas = Image.new('RGBA', (size * 5, size * 2))
    frames = {}
    for index, name in enumerate(BUILDING_ART):
        source = ART / (name + '.png')
        with Image.open(source) as raw:
            cut = raw.convert('RGBA'); box = cut.getbbox()
            if box: cut = cut.crop(box)
            cut.thumbnail((350, 350), Image.Resampling.LANCZOS)
            ox, oy = index % 5 * size, index // 5 * size
            atlas.alpha_composite(cut, (ox + (size - cut.width) // 2, oy + (size - cut.height) // 2))
            frames[f'frame_{index}'] = {'bbox': [ox, oy, size, size]}
    target = TEX / 'v4-buildings.png'; atlas.save(target)
    sprite = b.sprite_doc('V4Buildings', target, frames, {'idle': {'frames': list(frames), 'fps': 1, 'loop': True}}, (.5, .5))
    b.json_file(ROOT / 'game/v4/building-atlas.json', {'sprite': 'Content/Sprites/V4Buildings.rxsprite',
        'texture': str(target.relative_to(ROOT)), 'frameSize': [size, size], 'frameNames': BUILDING_ART,
        'method': 'Proportional alpha-preserving atlas assembly of the selected built-in imagegen assets'})
    return sprite


def build_scene():
    doc = json.loads(DATA.read_text(encoding='utf8'))
    buildings = building_atlas()
    old_scene = json.loads((ROOT / 'Content/Scenes/Main.rxscene').read_text(encoding='utf8'))
    templates = {e['name']: e for e in old_scene['entities']}
    entities, bindings = [], []

    def component(kind, props): return {'type': kind, 'enabled': True, 'props': props}
    def entity(name, components=None, position=(-100., -100., 0.), scale=(1., 1., 1.), binding=None):
        index = len(entities) + 1
        entities.append({'id': index, 'name': name, 'transform': {'translation': list(position), 'rotation': [0., 0., 0., 1.], 'scale': list(scale)}, 'components': components or []})
        if binding:
            kind, data = binding
            bindings.append({'entityId': index, 'kind': kind, 'data': (data + [0] * 6)[:6]})
        return index
    def clone(name, template, visual, kind=2):
        components = copy.deepcopy(templates[template]['components'])
        entity(name, components, binding=(kind, [visual]))
    def sprite(texture='', sp='', ppu=80., order=0, blend='alpha'):
        return component('Sprite', {'texture': texture, 'sprite': sp, 'clip': 'idle' if sp else '', 'frame': 0., 'tint': [1., 1., 1., 1.],
            'flipX': False, 'flipY': False, 'pixelsPerUnit': ppu, 'sortingOrder': float(order), 'chromaKey': 'none', 'blendMode': blend})
    def state(name, keys): entity(name, position=(0., 0., 0.), binding=(0, keys))

    entity('C4_Controller', [component('Script', {'graphRef': 'Content/Graphs/V4BatchController.rxgraph', 'module': '', 'props': {}})], (0., 0., 0.))
    entity('Camera', [component('Camera', {'projection': 'orthographic', 'orthoSize': 10., 'near': .1, 'far': 100., 'fov': 60.})], (0., 0., 10.))
    background = TEX / 'open-battlefield.png'
    with Image.open(background) as image: width, height = image.size
    entity('C4_BeyondMap', [sprite(texture=b.meta(background, 'texture'), ppu=80., order=-100, blend='opaque')], (0., 0., 0.), (H * 16 / 9 * 80 / width, H * 80 / height, 1.))
    for index, layout in enumerate(doc['layouts']):
        target = TEX / f'v4-terrain-{layout["level"]}-{layout["stage"]}.png'
        entity(f'C4_Terrain{index}', [sprite(texture=b.meta(target, 'texture'), order=-80, blend='opaque')], binding=(1, [500 + index]))
    for slot in range(32):
        for kind in range(1, 5): clone(f'C4_Actor{slot}_{kind}', f'CS_Actor0_{kind}', slot * 4 + kind - 1, 3 if kind >= 3 else 1)
    for slot in range(64): clone(f'C4_EnemyVisual{slot}', 'CS_Enemy0', 200 + slot)
    for slot in range(24): clone(f'C4_Pulse{slot}', 'CS_Pulse0', 300 + slot)
    for slot in range(48): entity(f'C4_Structure{slot}', [sprite(sp=buildings, ppu=300., order=12)], binding=(2, [1000 + slot]))
    for slot in range(32): clone(f'C4_GPUVisual{slot}', 'CS_GPUVisual0', 1100 + slot)
    for slot in range(64):
        clone(f'C4_VFXOverlay{slot}', 'CS_VFXOverlay0', 400 + slot)
        entity(f'C4_VFX{slot}', [component('ParticleEmitter', {})], binding=(4, [9000 + slot * 10 + j for j in range(5)]))
    for name, offset in [('C4_State', 0), ('C4_Economy', 6), ('C4_Status', 12), ('C4_Network', 18), ('C4_Progress', 24)]: state(name, list(range(offset, offset + 6)))
    for slot in range(48):
        base = 1000 + slot * 20
        for suffix, offset in [('', 0), ('Meta', 6), ('Extra', 12)]: state(f'C4_Building{suffix}{slot}', list(range(base + offset, base + offset + 6)))
    for slot in range(32):
        for suffix, offset in [('', 0), ('Meta', 6)]: state(f'C4_Gpu{suffix}{slot}', list(range(2000 + slot * 12 + offset, 2000 + slot * 12 + offset + 6)))
        for suffix, offset in [('', 0), ('Meta', 6), ('Combat', 12), ('Pos', 18)]: state(f'C4_Unit{suffix}{slot}', list(range(3000 + slot * 24 + offset, 3000 + slot * 24 + offset + 6)))
    for slot in range(64):
        base = 4000 + slot * 8
        state(f'C4_Link{slot}', list(range(base, base + 6))); state(f'C4_LinkMeta{slot}', [base + 6, base + 7, -1, -1, -1, -1])
        base = 5000 + slot * 8
        state(f'C4_EnemyStat{slot}', [base, base + 1, base + 2, base + 3, base + 4, -1]); state(f'C4_EnemyPos{slot}', [base + 5, base + 6, base + 7, -1, -1, -1])
    for row in range(H):
        for name, offset in [('Map', 6000), ('Wire', 7000), ('Wall', 8000)]: state(f'C4_{name}Row{row}', list(range(offset + row * 6, offset + row * 6 + 6)))
    graph = b.Graph('V4BatchController')
    def frame(dt): return graph.n('call.native_frame', module=graph.c(MODULE), fn=graph.c('cs4_frame'), dt=dt, bindings=graph.c(bindings), animatorParam=graph.c('attacking'))
    start = graph.n('event.on_start'); reset = graph.call('cs4_reset', graph.c([])); graph.link(start, reset); graph.link(reset, frame(graph.c(0.)))
    update = graph.n('event.on_update'); graph.link(update, frame(graph.p(update, 'dt')))
    event = graph.n('event.on_input'); save = graph.n('var.set', name=graph.p(event, 'action'), value=graph.p(event, 'value')); graph.link(event, save)
    value = graph.n('var.get', name=graph.c('cs4')); command = graph.call('cs4_input', graph.p(value, 'out')); graph.link(save, command)
    clear = graph.n('var.set', name=graph.c('cs4'), value=graph.c(0.)); graph.link(command, clear); graph.emit()
    scene = ROOT / 'Content/Scenes/Command.rxscene'
    b.json_file(scene, {'name': 'Code Sentinels V4 — 算力边疆', 'mode': '2d', 'gravity': [0, 0, 0], 'next_id': len(entities) + 1, 'entities': entities})
    b.meta(scene, 'scene')
    b.json_file(ROOT / 'game/v4/scene-build.json', {'scene': str(scene), 'entities': len(entities), 'bindings': len(bindings), 'nativeCallsPerFrame': 1,
        'module': MODULE, 'mapSize': [W, H], 'terrainLayers': 9, 'newBuildingArt': 10, 'retainedI2VSkills': 144,
        'generationOnly': True, 'gameOrTestsExecuted': False})
    print(json.dumps({'scene': str(scene), 'entities': len(entities), 'newBuildingArt': 10}, ensure_ascii=False))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--maps-only', action='store_true'); mode.add_argument('--scene-only', action='store_true')
    args = parser.parse_args()
    if not args.scene_only: build_maps()
    if not args.maps_only: build_scene()
