#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Stage 4 夹具生成器(RurixForge Godot 后端):确定性的 .rxmodel 模型包 + 场景配方。

用法: python scripts/f4-make-fixtures.py [--out DIR]
输出(缺省 crates/godot-host/tests/fixtures/stage4/):
  models/forge.toml                     最小项目(mode = "3d")
  models/Content/Models/*.rxmodel       assetd::model::ModelBundle 的 serde JSON(camelCase),贴图 rgba 内嵌
  scenes/*.json                         场景配方,给 scripts/f4-side-by-side.ps1 与测试用
确定性:固定 GUID、浮点统一 round(6)、键顺序固定、LF、无 BOM;重跑两次逐字节相同。
每个模型包写盘前按 crates/assetd/src/model/importer.rs validate_bundle 的全部检查自检,不过就退出 1。
配方里的两个约定(运行脚本负责解释):
  - entities[].idMod8 = k:先不带组件建实体,返回 id % 8 != k 就再建(不挂组件),满足后逐个 component.add;
  - props 里形如 "@名字" 的字符串 = 同一配方里先建的同名实体的 id(Parent.entity 用)。
"""
import argparse
import hashlib
import json
import math
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_OUT = os.path.join(REPO, 'crates', 'godot-host', 'tests', 'fixtures', 'stage4')


def r6(x):
    v = round(float(x), 6)
    return 0.0 if v == 0 else v  # 不出现 -0.0


def vr(v):
    return [r6(c) for c in v]


def norm(v):
    n = math.sqrt(sum(c * c for c in v))
    return [c / n for c in v]


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def quat_arc(u, v):
    """把单位向量 u 转到 v 的最短弧单位四元数 [x, y, z, w]。"""
    c = cross(u, v)
    w = 1.0 + dot(u, v)
    if w < 1e-9:
        ax = norm(cross(u, [1.0, 0.0, 0.0]) if abs(u[0]) < 0.9 else cross(u, [0.0, 1.0, 0.0]))
        return [ax[0], ax[1], ax[2], 0.0]
    q = [c[0], c[1], c[2], w]
    n = math.sqrt(dot(q, q))
    return [x / n for x in q]


# ---------- 与 validate_bundle 同一套检查(importer.rs:659 起) ----------
REQ = {
    'bundle': ['version', 'guid', 'revision', 'name', 'sourceId', 'sourceHash', 'kind', 'roots', 'primitives',
               'nodes', 'materials', 'textures', 'skins', 'animations'],
    'prim': ['id', 'positions', 'normals', 'tangents', 'uv0', 'indices', 'joints', 'weights', 'material'],
    'node': ['id', 'name', 'children', 'primitives', 'translation', 'rotation', 'scale', 'matrix', 'skin'],
    'mat': ['guid', 'name', 'baseColor', 'metallic', 'roughness', 'emissive', 'baseColorTexture', 'normalTexture',
            'metallicRoughnessTexture', 'occlusionTexture', 'emissiveTexture', 'normalScale', 'occlusionStrength',
            'doubleSided', 'alphaMode', 'alphaCutoff', 'unlit'],
    'tex': ['id', 'guid', 'assetPath', 'width', 'height', 'rgba', 'wrapS', 'wrapT', 'magFilter', 'minFilter'],
    'skin': ['name', 'joints', 'inverseBindMatrices', 'skeleton'],
}


def fin(vals):
    return all(math.isfinite(v) for v in vals)


def flat(rows):
    return [x for r in rows for x in r]


def validate(b):
    def need(kind, o):
        miss = [k for k in REQ[kind] if k not in o]
        if miss:
            raise ValueError('%s 缺字段 %s' % (kind, miss))
    need('bundle', b)
    if b['version'] != 1 or not b['guid'] or not b['primitives']:
        raise ValueError('invalid rxmodel header')
    nodes = b['nodes']
    parents = [0] * len(nodes)
    for n in nodes:
        need('node', n)
        for c in n['children']:
            if c >= len(nodes):
                raise ValueError('node child index out of range')
            parents[c] += 1
            if parents[c] > 1:
                raise ValueError('node has multiple parents')
    state = [0] * len(nodes)

    def visit(i):
        if state[i] == 1:
            raise ValueError('node hierarchy cycle')
        if state[i] == 2:
            return
        state[i] = 1
        for c in nodes[i]['children']:
            visit(c)
        state[i] = 2
    for i in range(len(nodes)):
        visit(i)
    ids = [n['id'] for n in nodes]
    if len(set(ids)) != len(ids):
        raise ValueError('template node identity collision')
    if any(r >= len(nodes) for r in b['roots']):
        raise ValueError('invalid scene root')

    np, nm, nt, ns = len(b['primitives']), len(b['materials']), len(b['textures']), len(b['skins'])
    for p in b['primitives']:
        need('prim', p)
        if not fin(flat(p['positions']) + flat(p['normals']) + flat(p['tangents']) + flat(p['uv0']) + flat(p['weights'])):
            raise ValueError('non-finite mesh attribute')
        cnt = len(p['positions'])
        if (cnt == 0 or len(p['indices']) % 3 != 0 or any(i >= cnt for i in p['indices'])
                or (p['material'] is not None and p['material'] >= nm)):
            raise ValueError('invalid primitive reference')
        if any(len(p[k]) not in (0, cnt) for k in ('normals', 'tangents', 'uv0', 'joints', 'weights')):
            raise ValueError('invalid primitive attribute length')
    for n in nodes:
        if any(i >= np for i in n['primitives']) or (n['skin'] is not None and n['skin'] >= ns):
            raise ValueError('invalid node reference')
        if not fin(n['translation'] + n['rotation'] + n['scale'] + (n['matrix'] or [])):
            raise ValueError('non-finite node transform')
    for m in b['materials']:
        need('mat', m)
        refs = [m[k] for k in ('baseColorTexture', 'normalTexture', 'metallicRoughnessTexture', 'occlusionTexture',
                               'emissiveTexture') if m[k] is not None]
        if any(t >= nt for t in refs):
            raise ValueError('invalid material texture reference')
        if not fin(m['baseColor'] + m['emissive'] + [m['metallic'], m['roughness'], m['normalScale'],
                                                     m['occlusionStrength'], m['alphaCutoff']]):
            raise ValueError('non-finite material parameter')
    for t in b['textures']:
        need('tex', t)
        w, h = t['width'], t['height']
        if w == 0 or h == 0 or w > 8192 or h > 8192 or len(t['rgba']) != w * h * 4:
            raise ValueError('invalid texture dimensions or bytes')
        if any((not isinstance(x, int)) or x < 0 or x > 255 for x in t['rgba']):
            raise ValueError('rgba 须为 0..255 整数(Vec<u8>)')
    for s in b['skins']:
        need('skin', s)
        if (len(s['joints']) != len(s['inverseBindMatrices']) or any(j >= len(nodes) for j in s['joints'])
                or (s['skeleton'] is not None and s['skeleton'] >= len(nodes))
                or not fin(flat(s['inverseBindMatrices']))):
            raise ValueError('invalid skin')
    for n in nodes:
        if n['skin'] is None:
            continue
        for pi in n['primitives']:
            p = b['primitives'][pi]
            if (not p['joints'] or len(p['joints']) != len(p['weights'])
                    or any(j >= len(b['skins'][n['skin']]['joints']) for j in flat(p['joints']))):
                raise ValueError('invalid skin vertex palette')
    for a in b['animations']:
        for c in a['channels']:
            factor = 3 if c['interpolation'] == 'CUBICSPLINE' else 1
            ts = c['times']
            if (c['node'] >= len(nodes) or not ts or len(c['values']) != len(ts) * factor
                    or not fin(flat(c['values'])) or any((not math.isfinite(t)) or t < 0 for t in ts)
                    or any(ts[i] >= ts[i + 1] for i in range(len(ts) - 1))
                    or c['path'] not in ('translation', 'rotation', 'scale')
                    or c['interpolation'] not in ('LINEAR', 'STEP', 'CUBICSPLINE')):
                raise ValueError('invalid animation channel')
    if b['kind'] == 'character':
        raise ValueError('夹具只用 kind = prop(character 另有 Idle/Walk 约束)')


# ---------- 构件 ----------
QUAD_POS = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0]]
QUAD_UV = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]  # 左下 右下 右上 左上
NEAREST, CLAMP = 9728, 33071


def guid(model, sub):
    return 'f4a00000-%04x-4000-8000-%012x' % (model, sub)


def quad(pid, material, skinned=False):
    return {'id': pid, 'positions': QUAD_POS, 'normals': [[0.0, 0.0, 1.0]] * 4,
            'tangents': [[1.0, 0.0, 0.0, 1.0]] * 4, 'uv0': QUAD_UV, 'indices': [0, 1, 2, 0, 2, 3],
            'joints': [[0, 0, 0, 0]] * 4 if skinned else [],
            'weights': [[1.0, 0.0, 0.0, 0.0]] * 4 if skinned else [], 'material': material}


def node(nid, prims, t=(0.0, 0.0, 0.0), r=(0.0, 0.0, 0.0, 1.0), children=(), skin=None):
    return {'id': nid, 'name': nid, 'children': list(children), 'primitives': list(prims), 'translation': vr(t),
            'rotation': vr(r), 'scale': [1.0, 1.0, 1.0], 'matrix': None, 'skin': skin, 'collision': False}


def material(g, name, base=(1.0, 1.0, 1.0, 1.0), metallic=0.0, roughness=1.0, emissive=(0.0, 0.0, 0.0),
             albedo=None, normal=None, occlusion=None, normal_scale=1.0, occlusion_strength=1.0,
             double_sided=False, alpha_mode='OPAQUE', alpha_cutoff=0.5, unlit=False):
    return {'guid': g, 'name': name, 'baseColor': vr(base), 'metallic': r6(metallic), 'roughness': r6(roughness),
            'emissive': vr(emissive), 'baseColorTexture': albedo, 'normalTexture': normal,
            'metallicRoughnessTexture': None, 'occlusionTexture': occlusion, 'emissiveTexture': None,
            'normalScale': r6(normal_scale), 'occlusionStrength': r6(occlusion_strength), 'doubleSided': double_sided,
            'alphaMode': alpha_mode, 'alphaCutoff': r6(alpha_cutoff), 'unlit': unlit}


def texture(tid, g, path, w, h, rgba):
    return {'id': tid, 'guid': g, 'assetPath': path, 'width': w, 'height': h, 'rgba': rgba,
            'wrapS': CLAMP, 'wrapT': CLAMP, 'magFilter': NEAREST, 'minFilter': NEAREST}



def bundle(k, name, nodes, prims, mats, texs=(), skins=(), anims=()):
    child = {c for n in nodes for c in n['children']}
    return {'version': 1, 'guid': guid(k, 0), 'revision': 1, 'name': name, 'sourceId': 'stage4/' + name,
            'sourceHash': 'stage4-fixture-v1-' + name, 'kind': 'prop',
            'roots': [i for i in range(len(nodes)) if i not in child], 'primitives': prims, 'nodes': nodes,
            'materials': mats, 'textures': list(texs), 'skins': list(skins), 'animations': list(anims),
            'idleClip': '', 'walkClip': ''}


def image(w, h, px):
    out = []
    for y in range(h):  # 第 0 行 = 贴图顶边(uv v = 0)
        for x in range(w):
            out.extend(px(x, y))
    return out


def enc_normal(v):
    """切线空间法线 → RGBA8:(n·0.5 + 0.5)·255 四舍五入(半数进位)。"""
    return [int(math.floor((c * 0.5 + 0.5) * 255.0 + 0.5)) for c in norm(v)] + [255]


def m_plane():
    k = 1
    return bundle(k, 'plane', [node('card', [0])], [quad('card', 0)], [material(guid(k, 0x100), 'white')])


def m_checker():
    k = 2

    def px(x, y):
        if y < 4:
            return [255, 0, 0, 255] if x < 4 else [0, 0, 255, 255]
        return [0, 255, 0, 255] if x < 4 else [255, 255, 255, 255]
    tex = texture('albedo', guid(k, 0x200), 'Models/checker/albedo.png', 8, 8, image(8, 8, px))
    mat = material(guid(k, 0x100), 'checker', roughness=0.8, albedo=0)
    return bundle(k, 'checker', [node('card', [0])], [quad('card', 0)], [mat], [tex])


def m_normal_y():
    k = 3
    up, down = enc_normal([0.0, 0.5, 1.0]), enc_normal([0.0, -0.5, 1.0])
    texs = [texture('n_up', guid(k, 0x200), 'Models/normal_y/n_up.png', 4, 4, up * 16),
            texture('n_down', guid(k, 0x201), 'Models/normal_y/n_down.png', 4, 4, down * 16)]
    mats = [material(guid(k, 0x100), 'n_up', normal=0), material(guid(k, 0x101), 'n_down', normal=1)]
    nodes = [node('n_up', [0]), node('n_down', [1], t=(1.2, 0.0, 0.0))]
    return bundle(k, 'normal_y', nodes, [quad('n_up', 0), quad('n_down', 1)], mats, texs)


def m_ao():
    k = 4

    def px(x, y):
        r = 0 if x < 2 else 255
        return [r, r, r, 255]
    tex = texture('occlusion', guid(k, 0x200), 'Models/ao/occlusion.png', 4, 4, image(4, 4, px))
    mat = material(guid(k, 0x100), 'ao', occlusion=0, occlusion_strength=0.5)
    return bundle(k, 'ao', [node('card', [0])], [quad('card', 0)], [mat], [tex])


def m_skinned():
    """照 crates/engine-host/src/render_core/extract3d.rs 测试里的 fixture():skinned quad + bone + prop。"""
    k = 5
    ib = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.25, 0.0, 0.0, 1.0]
    nodes = [node('skinned', [0], skin=0), node('bone', [], t=(0.25, 0.0, 0.0), children=[2]),
             node('prop', [1], t=(0.0, 1.0, 0.0))]
    mat = material(guid(k, 0x100), 'default', base=(0.8, 0.3, 0.2, 1.0), roughness=0.6, double_sided=True)
    skin = {'name': 's', 'joints': [1], 'inverseBindMatrices': [ib], 'skeleton': 1}
    chan = {'node': 1, 'path': 'translation', 'times': [0.0, 2.0],
            'values': [[0.25, 0.0, 0.0, 0.0], [1.25, 0.0, 0.0, 0.0]], 'interpolation': 'LINEAR'}
    anim = {'name': 'walk', 'duration': 2.0, 'channels': [chan]}
    return bundle(k, 'skinned', nodes, [quad('a', 0, True), quad('b', 0, True)], [mat], [], [skin], [anim])


def m_mats():
    k = 6
    tex = texture('mask', guid(k, 0x200), 'Models/mats/mask.png', 4, 4,
                  image(4, 4, lambda x, y: [255, 255, 255, 0 if x < 2 else 255]))

    def g(j):
        return guid(k, 0x100 + j)
    mats = [material(g(0), 'emissive', base=(0.2, 0.2, 0.2, 1.0), emissive=(1.0, 0.5, 0.0)),
            material(g(1), 'mask', albedo=0, alpha_mode='MASK', alpha_cutoff=0.5),
            material(g(2), 'blend', base=(1.0, 0.0, 0.0, 0.5), alpha_mode='BLEND'),
            material(g(3), 'unlit', base=(0.0, 1.0, 0.0, 1.0), unlit=True),
            material(g(4), 'metal', base=(0.9, 0.9, 0.9, 1.0), metallic=1.0, roughness=0.3),
            material(g(5), 'double_sided', base=(0.3, 0.5, 0.9, 1.0), double_sided=True)]
    names = [m['name'] for m in mats]
    # 沿 x 居中排开、间距 1.2;最后一块绕 Y 转 180°(相机看到背面)。
    nodes = [node(names[j], [j], t=((j - 2.5) * 1.2, 0.0, 0.0),
                  r=(0.0, 1.0, 0.0, 0.0) if j == 5 else (0.0, 0.0, 0.0, 1.0)) for j in range(6)]
    return bundle(k, 'mats', nodes, [quad(names[j], j) for j in range(6)], mats, [tex])


MODELS = [m_plane, m_checker, m_normal_y, m_ao, m_skinned, m_mats]



# ---------- 场景配方 ----------
L = norm([0.45, 0.8, 0.35])  # 模型腿缺省灯方向(从表面指向光源)
Z = [0.0, 0.0, 1.0]
P = norm([Z[i] - L[i] * dot(L, Z) for i in range(3)])
NDOTL = [1.0, 0.8, 0.6, 0.4, 0.2]
ORTHO = {'target': [0.0, 0.0, 0.0], 'yaw': 0.0, 'pitch': 0.0, 'dist': 10.0, 'ortho': True, 'orthoSize': 1.5}
W, H, FOV = 640, 360, 50.0
DOC_REFS = ('entities[] 是 entity.create 的参数;idMod8 = k:先不带组件建实体,id % 8 != k 就再建(不挂组件),'
            '满足后逐个 component.add;props 里 "@名字" = 同配方先建的同名实体 id。')


def front(dist, target=(0.0, 0.0, 0.0)):
    return {'target': vr(target), 'yaw': 0.0, 'pitch': 0.0, 'dist': r6(dist), 'fovY': FOV, 'ortho': False}


def project_px(cam, x, y):
    """yaw 0 / pitch 0 透视相机下 z = 0 平面上一点的像素坐标(左上原点)。"""
    k = (H / 2.0) / (cam['dist'] * math.tan(math.radians(cam['fovY']) / 2.0))
    return [int(round(W / 2.0 + (x - cam['target'][0]) * k)), int(round(H / 2.0 - (y - cam['target'][1]) * k))]


def comp(t, props):
    return {'type': t, 'enabled': True, 'props': props}


def model(name):
    return comp('ModelRenderer', {'model': 'Models/%s.rxmodel' % name})


def light(kind, color, intensity, shadow=False):
    return comp('Light', {'kind': kind, 'color': vr(color), 'intensity': r6(intensity), 'castShadow': shadow})


def ent(name, t, comps, r=None, s=None, **extra):
    e = {'name': name, 'translation': vr(t)}
    if r is not None:
        e['rotation'] = vr(r)
    if s is not None:
        e['scale'] = vr(s)
    e['components'] = comps
    e.update(extra)
    return e


def card_rotations():
    """第 i 块:+Z 转到 nᵢ = cosθᵢ·l + sinθᵢ·p(cosθᵢ = NdotL),最短弧。"""
    out = []
    for c in NDOTL:
        s = math.sqrt(max(0.0, 1.0 - c * c))
        out.append(quat_arc(Z, norm([c * L[i] + s * P[i] for i in range(3)])))
    return out


def graycard(name, mesh):
    ents = []
    for i, q in enumerate(card_rotations()):
        t = (float(i - 2), 0.0, 0.0)
        if mesh:
            ents.append(ent('card-%d' % i, t, [comp('MeshRenderer', {'mesh': 'cube', 'material': ''})],
                            r=q, s=(0.8, 0.8, 0.8), idMod8=6))
        else:
            ents.append(ent('card-%d' % i, t, [model('plane')], r=q, s=(1.0, 1.0, 1.0)))
    r = {'name': name, 'project': 'models'}
    if mesh:
        r['doc'] = DOC_REFS + ' MeshRenderer.material 是必填字段,空串 = 走 id 调色板(id % 8 == 6 → [0.60,0.60,0.66])。'
    r.update({'entities': ents, 'camera': ORTHO, 'width': W, 'height': H,
              'probes': [[320 + (i - 2) * 120, 180] for i in range(5)],
              'probeLabels': ['NdotL=%.1f' % c for c in NDOTL]})
    return r


def materials():
    cam = front(6.0)
    ents = [ent('checker', (-1.8, 0.7, 0.0), [model('checker')]),
            ent('normal_y', (-0.6, 0.7, 0.0), [model('normal_y')]),  # 两块:x = -0.6(n.y+)与 +0.6(n.y-)
            ent('ao', (1.8, 0.7, 0.0), [model('ao')]),
            ent('mats', (0.0, -0.7, 0.0), [model('mats')])]  # 六块:x = -3.0 … +3.0
    pts = [('checker.red', -2.05, 0.95), ('checker.blue', -1.55, 0.95), ('checker.green', -2.05, 0.45),
           ('checker.white', -1.55, 0.45), ('normal_y.up', -0.6, 0.7), ('normal_y.down', 0.6, 0.7),
           ('ao.r0', 1.55, 0.7), ('ao.r255', 2.05, 0.7), ('mats.emissive', -3.0, -0.7),
           ('mats.mask.a0', -2.05, -0.7), ('mats.mask.a255', -1.55, -0.7), ('mats.blend', -0.6, -0.7),
           ('mats.unlit', 0.6, -0.7), ('mats.metal', 1.8, -0.7), ('mats.doubleSidedBack', 3.0, -0.7)]
    return {'name': 'materials', 'project': 'models', 'entities': ents, 'camera': cam, 'width': W, 'height': H,
            'probes': [project_px(cam, x, y) for _, x, y in pts], 'probeLabels': [n for n, _, _ in pts]}



def lights():
    cam = front(5.5)
    sun = norm([-0.3, 0.6, 0.75])  # 从表面指向光源;Godot 方向光沿自身 −Z 照射 ⇒ 节点 +Z 转到 sun
    ents = [ent('backdrop', (0.0, 0.0, -0.3), [model('plane')], s=(7.0, 4.0, 1.0)),
            ent('plane-l', (-1.8, 0.0, 0.0), [model('plane')]),
            ent('checker-l', (-0.6, 0.0, 0.0), [model('checker')]),
            ent('plane-r', (0.6, 0.0, 0.0), [model('plane')]),
            ent('checker-r', (1.8, 0.0, 0.0), [model('checker')]),
            ent('sun', (0.0, 3.0, 3.0), [light('directional', (1.0, 0.92, 0.8), 0.8)], r=quat_arc(Z, sun)),
            ent('point-red', (-1.2, 0.6, 1.0), [light('point', (1.0, 0.25, 0.2), 2.0)]),
            ent('spot-rig', (1.2, 0.0, 2.0), []),
            ent('spot-blue', (0.0, 0.4, 0.0), [light('spot', (0.3, 0.55, 1.0), 3.0, True),
                                               comp('Parent', {'entity': '@spot-rig'})])]
    pts = [('plane-l', -1.8, 0.0), ('checker-l.white', -0.35, -0.25), ('plane-r', 0.6, 0.0),
           ('checker-r.white', 2.05, -0.25), ('backdrop.left', -3.0, 1.5), ('backdrop.right', 3.0, 1.5)]
    return {'name': 'lights', 'project': 'models', 'doc': DOC_REFS, 'entities': ents, 'camera': cam,
            'width': W, 'height': H, 'probes': [project_px(cam, x, y) for _, x, y in pts],
            'probeLabels': [n for n, _, _ in pts]}


def anim():
    cam = front(5.0, (0.25, 0.0, 0.0))
    ents = [ent('walk-0.5', (-1.5, -0.5, 0.0), [model('skinned'), comp('Animator', {'clip': 'walk', 'time': 0.5})]),
            ent('walk-1.5', (1.0, -0.5, 0.0), [model('skinned'), comp('Animator', {'clip': 'walk', 'time': 1.5})])]
    # bone.x(t) = 0.25 + 0.5·t;蒙皮 quad 平移 bone.x − 0.25,prop 在 (bone.x, 1)。
    pts = [('t0.5.skinned', -1.25, -0.5), ('t0.5.prop', -1.0, 0.5), ('t1.5.skinned', 1.75, -0.5),
           ('t1.5.prop', 2.0, 0.5)]
    return {'name': 'anim', 'project': 'models', 'entities': ents, 'camera': cam, 'width': W, 'height': H,
            'probes': [project_px(cam, x, y) for _, x, y in pts], 'probeLabels': [n for n, _, _ in pts]}


def recipes():
    maze = {'name': 'maze', 'project': 'demo', 'load': 'Content/Scenes/maze.rxscene',
            'camera': {'target': [0.0, 0.5, 0.0], 'yaw': 35.0, 'pitch': 28.0, 'dist': 9.0}, 'width': W, 'height': H}
    pz = {'name': 'pz_mvp', 'project': 'demo', 'load': 'Content/Scenes/pz_mvp_phase1.rxscene', 'width': W, 'height': H}
    return [maze, graycard('graycard_model', False), graycard('graycard_mesh', True), materials(), lights(), anim(), pz]


FORGE_TOML = '''[project]
name = "stage4-models"
engine-version = "0.1.0"
rurix-ref = "v1.0.1-dist"
entry-scene = "Content/Scenes/Main.rxscene"
# Stage 4 夹具项目(scripts/f4-make-fixtures.py 生成,勿手改):只放 Content/Models/*.rxmodel。
mode = "3d"

[dirs]
content = "Content"
scripts = "Content/Scripts"
'''


def dump_recipe(r):
    parts = []
    for k, v in r.items():
        if k == 'entities':
            body = ',\n'.join('    ' + json.dumps(e, ensure_ascii=False, allow_nan=False) for e in v)
            parts.append('  "entities": [\n' + body + '\n  ]')
        else:
            parts.append('  %s: %s' % (json.dumps(k), json.dumps(v, ensure_ascii=False, allow_nan=False)))
    return '{\n' + ',\n'.join(parts) + '\n}\n'


def write(path, text):
    """内容相同就不重写(mtime 也不动);LF、UTF-8 无 BOM。"""
    data = text.encode('utf-8')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if os.path.isfile(path):
        with open(path, 'rb') as f:
            if f.read() == data:
                return False
    with open(path, 'wb') as f:
        f.write(data)
    return True


def main():
    ap = argparse.ArgumentParser(description='Stage 4 夹具生成器(确定性)')
    ap.add_argument('--out', default=DEFAULT_OUT, help='输出目录(缺省 crates/godot-host/tests/fixtures/stage4)')
    out = os.path.abspath(ap.parse_args().out)
    files = {'models/forge.toml': FORGE_TOML}
    for make in MODELS:
        b = make()
        try:
            validate(b)
        except ValueError as e:
            print('validate_bundle 自检失败 %s: %s' % (b['name'], e), file=sys.stderr)
            return 1
        text = json.dumps(b, separators=(',', ':'), ensure_ascii=False, allow_nan=False) + '\n'
        files['models/Content/Models/%s.rxmodel' % b['name']] = text
    for r in recipes():
        files['scenes/%s.json' % r['name']] = dump_recipe(r)
    changed = 0
    for rel in sorted(files):
        changed += write(os.path.join(out, *rel.split('/')), files[rel])
        print('%s  %s' % (hashlib.sha256(files[rel].encode('utf-8')).hexdigest(), rel))
    print('%d 个文件,%d 个有改动 -> %s' % (len(files), changed, out))
    return 0


if __name__ == '__main__':
    sys.exit(main())
