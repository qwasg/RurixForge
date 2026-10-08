# Stage 5 fixtures for scripts/f5-side-by-side.ps1 (and manual checks):
#   crates/godot-host/tests/fixtures/stage5/models/  - forge.toml, Content/Models/*.rxmodel, Content/Textures/decal_red.png(+.meta)
#   crates/godot-host/tests/fixtures/stage5/scenes/  - env_panel / fog / gi / volumes / particles (.json, same format as stage4)
# Usage: python scripts/f5-make-fixtures.py   (idempotent; rewrites every file)
import json, os, struct, zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'crates', 'godot-host', 'tests', 'fixtures', 'stage5')
MODELS = os.path.join(ROOT, 'models')
SCENES = os.path.join(ROOT, 'scenes')


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8', newline='\n') as f:
        f.write(text)


def material(guid, name, base, metallic=0.0, roughness=1.0, emissive=(0.0, 0.0, 0.0)):
    return {'guid': guid, 'name': name, 'baseColor': list(base), 'metallic': metallic, 'roughness': roughness,
            'emissive': list(emissive), 'baseColorTexture': None, 'normalTexture': None, 'metallicRoughnessTexture': None,
            'occlusionTexture': None, 'emissiveTexture': None, 'normalScale': 1.0, 'occlusionStrength': 1.0,
            'doubleSided': False, 'alphaMode': 'OPAQUE', 'alphaCutoff': 0.5, 'unlit': False}


def quad_model(idx, name, mat):
    guid = 'f5a00000-%04x-4000-8000-000000000000' % idx
    mat = dict(mat, guid='f5a00000-%04x-4000-8000-000000000100' % idx)
    prim = {'id': 'q', 'positions': [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0]],
            'normals': [[0.0, 0.0, 1.0]] * 4, 'tangents': [[1.0, 0.0, 0.0, 1.0]] * 4,
            'uv0': [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], 'indices': [0, 1, 2, 0, 2, 3],
            'joints': [], 'weights': [], 'material': 0}
    node = {'id': 'q', 'name': 'q', 'children': [], 'primitives': [0], 'translation': [0.0, 0.0, 0.0],
            'rotation': [0.0, 0.0, 0.0, 1.0], 'scale': [1.0, 1.0, 1.0], 'matrix': None, 'skin': None, 'collision': False}
    m = {'version': 1, 'guid': guid, 'revision': 1, 'name': name, 'sourceId': 'stage5/' + name,
         'sourceHash': 'stage5-fixture-v1-' + name, 'kind': 'prop', 'roots': [0], 'primitives': [prim], 'nodes': [node],
         'materials': [mat], 'textures': [], 'skins': [], 'animations': [], 'idleClip': '', 'walkClip': ''}
    write(os.path.join(MODELS, 'Content', 'Models', name + '.rxmodel'), json.dumps(m, separators=(',', ':')))
    return 'Models/%s.rxmodel' % name


def png(w, h, px):
    raw = b''.join(b'\x00' + bytes(px[y * w * 4:(y + 1) * w * 4]) for y in range(h))
    def chunk(t, d):
        return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')


write(os.path.join(MODELS, 'forge.toml'),
      '[project]\nname = "stage5-models"\nengine-version = "0.1.0"\nrurix-ref = "v1.0.1-dist"\n'
      'entry-scene = "Content/Scenes/Main.rxscene"\nmode = "3d"\n\n[dirs]\ncontent = "Content"\nscripts = "Content/Scripts"\n')
WHITE = quad_model(1, 'white', material('', 'white', (1.0, 1.0, 1.0, 1.0)))
RED = quad_model(2, 'red', material('', 'red', (1.0, 0.1, 0.1, 1.0)))
GREEN = quad_model(3, 'green', material('', 'green', (0.1, 1.0, 0.1, 1.0)))
MIRROR = quad_model(4, 'mirror', material('', 'mirror', (0.95, 0.95, 0.95, 1.0), metallic=1.0, roughness=0.05))
LAMP = quad_model(5, 'lamp', material('', 'lamp', (0.0, 0.0, 0.0, 1.0), emissive=(4.0, 4.0, 4.0)))
REDLAMP = quad_model(6, 'redlamp', material('', 'redlamp', (0.0, 0.0, 0.0, 1.0), emissive=(3.0, 0.0, 0.0)))
tex_dir = os.path.join(MODELS, 'Content', 'Textures')
os.makedirs(tex_dir, exist_ok=True)
with open(os.path.join(tex_dir, 'decal_red.png'), 'wb') as f:
    f.write(png(4, 4, [255, 0, 0, 255] * 16))
DECAL = '5e5e5e5e-0000-4000-8000-f5dec0000001'
write(os.path.join(tex_dir, 'decal_red.png.meta'), 'guid: %s\ntype: texture\nimporter: png\n' % DECAL)
import math

ID = [0.0, 0.0, 0.0, 1.0]


def rot_x(a):
    return [math.sin(a / 2), 0.0, 0.0, math.cos(a / 2)]


def rot_y(a):
    return [0.0, math.sin(a / 2), 0.0, math.cos(a / 2)]


def ent(name, model=None, t=(0.0, 0.0, 0.0), rot=ID, scale=(1.0, 1.0, 1.0), comps=()):
    c = [{'type': 'ModelRenderer', 'enabled': True, 'props': {'model': model}}] if model else []
    c += [{'type': k, 'enabled': True, 'props': p} for k, p in comps]
    return {'name': name, 'translation': list(t), 'rotation': list(rot), 'scale': list(scale), 'components': c}


def scene(name, doc, entities, camera, probes, labels, extra=None):
    s = {'name': name, 'doc': doc, 'project': 'models', 'entities': entities, 'camera': camera,
         'width': 640, 'height': 360, 'probes': probes, 'probeLabels': labels}
    s.update(extra or {})
    write(os.path.join(SCENES, name + '.json'), json.dumps(s, ensure_ascii=False, indent=1))


def ortho(half):
    return {'target': [0.0, 0.0, 0.0], 'yaw': 0.0, 'pitch': 0.0, 'dist': 10.0, 'ortho': True, 'orthoSize': half}


def persp(target, pitch, dist):
    return {'target': target, 'yaw': 0.0, 'pitch': pitch, 'dist': dist, 'fovY': 50.0, 'ortho': False}


FLOOR = rot_x(-math.pi / 2)
scene('env_panel', 'Environment: sky background + sky ambient/reflection, AgX, glow, SSAO (rurix ignores Environment; comparison only)', [
    ent('floor', WHITE, (0.0, -1.0, -1.0), FLOOR, (6.0, 6.0, 1.0)),
    ent('wall', WHITE, (0.0, 1.0, -3.0), ID, (6.0, 4.0, 1.0)),
    ent('red', RED, (-1.0, 0.0, -1.5)),
    ent('green', GREEN, (1.0, 0.0, -1.5)),
    ent('lamp', LAMP, (0.0, 1.2, -2.9), ID, (0.6, 0.6, 1.0)),
    ent('env', comps=[('Environment', {'background': 'sky', 'ambientSource': 'sky', 'reflectionSource': 'sky', 'tonemap': 'agx',
                                        'glowEnabled': True, 'glowIntensity': 0.8, 'ssaoEnabled': True})]),
], persp([0.0, -0.3, -2.0], 20.0, 6.0), [[320, 300], [320, 120], [200, 190], [440, 190], [320, 95]],
    ['floor', 'wall', 'red', 'green', 'lamp'], {'warmFrames': 3})
scene('fog', 'Environment exponential fog: five white cards at z = 0 / -6 / -12 / -24 / -48 (ortho camera, rurix ignores Environment)', [
    ent('c%d' % i, WHITE, (-2.0 + i, 0.0, z), ID, (0.8, 0.8, 1.0)) for i, z in enumerate([0.0, -6.0, -12.0, -24.0, -48.0])
] + [ent('env', comps=[('Environment', {'ambientSource': 'color', 'ambientColor': [0.6, 0.6, 0.6, 1.0], 'reflectionSource': 'disabled',
                                         'fogEnabled': True, 'fogDensity': 0.04, 'fogLightColor': [0.4, 0.6, 0.9, 1.0]})])],
    ortho(1.5), [[80, 180], [200, 180], [320, 180], [440, 180], [560, 180]], ['z0', 'z-6', 'z-12', 'z-24', 'z-48'])
# 240 warm frames: SDFGI integrates over time; measured 2026-09-29 (F+ D3D12 / Vulkan) consecutive frames still
# differ by 1 LSB at frame 62 / 122 and are byte-identical from frame 242 on (02 §9.5 Stage 5 step 7).
scene('gi', 'SDFGI: sunlit floor + red back wall + green side wall, ambient off; 240 warm frames (rurix ignores Environment)', [
    ent('floor', WHITE, (0.0, -1.0, -1.0), FLOOR, (6.0, 6.0, 1.0)),
    ent('wall', RED, (0.0, 1.0, -3.0), ID, (6.0, 4.0, 1.0)),
    ent('side', GREEN, (-3.0, 1.0, -1.0), rot_y(math.pi / 2), (6.0, 4.0, 1.0)),
    ent('sun', t=(0.0, 3.0, 0.0), rot=rot_x(-0.6), comps=[('Light', {'kind': 'directional', 'color': [1.0, 1.0, 1.0], 'intensity': 2.0, 'castShadow': True})]),
    ent('env', comps=[('Environment', {'ambientSource': 'disabled', 'reflectionSource': 'disabled', 'sdfgiEnabled': True, 'sdfgiEnergy': 2.0})]),
], persp([0.0, -0.5, -2.0], 25.0, 6.0), [[320, 300], [320, 120], [120, 200]], ['floor', 'wall', 'side'], {'warmFrames': 240})
scene('volumes', 'ReflectionProbe (mirror reflects a red lamp behind the camera) / Decal (red) / FogVolume (red emission); Forward+ shows all three', [
    ent('mirror', MIRROR, (-0.2, 0.0, 0.0), ID, (1.6, 1.6, 1.0)),
    ent('lamp', REDLAMP, (0.0, 0.0, 15.0), rot_y(math.pi), (40.0, 40.0, 1.0)),
    ent('panel', WHITE, (2.4, 0.0, 0.0), ID, (1.6, 1.6, 1.0)),
    ent('decal', t=(2.4, 0.0, 0.0), rot=rot_x(math.pi / 2), comps=[('Decal', {'size': [1.0, 1.0, 1.0], 'textureAlbedo': DECAL})]),
    ent('back', WHITE, (-2.6, 0.0, -2.0), ID, (1.8, 1.8, 1.0)),
    ent('fogvol', t=(-2.6, 0.0, 0.0), comps=[('FogVolume', {'size': [1.2, 1.2, 1.2], 'density': 2.0, 'emission': [1.0, 0.0, 0.0, 1.0]})]),
    ent('env', comps=[('Environment', {'ambientSource': 'color', 'ambientColor': [0.5, 0.5, 0.5, 1.0], 'reflectionSource': 'disabled',
                                        'volumetricFogEnabled': True, 'volumetricFogDensity': 0.0, 'volumetricFogTemporalReprojection': False}),
                      ('ReflectionProbe', {'size': [60.0, 60.0, 60.0], 'updateMode': 'always'})]),
], ortho(2.0), [[296, 180], [464, 180], [112, 180]], ['mirror', 'decal', 'fogvol'], {'warmFrames': 8})
for age in (0.5, 0.9):
    scene('particles_%02d' % int(age * 10), 'ParticleEmitter kinds 1-4 (jet / parabola / wobble / spiral) at age %.1f s, life 1.2 s; FORGE_GPU_PARTICLES=1 on both backends' % age, [
        {'name': 'p%d' % k, 'translation': list(c), 'scale': [age, 1.2, float(k)], 'components': [{'type': 'ParticleEmitter', 'enabled': True, 'props': {}}]}
        for k, c in [(1, (-2.5, 1.0, 0.0)), (2, (0.5, 1.0, 0.0)), (3, (-2.5, -1.2, 0.0)), (4, (1.0, -1.2, 0.0))]
    ], ortho(3.0), [], [], {'env': {'FORGE_GPU_PARTICLES': '1'}})
print('wrote', ROOT)
