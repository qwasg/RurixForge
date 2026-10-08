"""Meshy batch generation + acceptance for the Code Sentinels V6 model set.

Every model listed in meshy-manifest.json is generated through the RurixForge forge-agentd
REST face (`POST /api/forge/gen/mesh`, backend meshy, Smart Topology T2 image-to-3D) using the
high-resolution render of the authored rough model as the geometry reference and the manifest
prompt as texture guidance. Each artifact is then structurally validated (GLB parse), accepted
into the asset pipeline through `mcp__gen-model__gen_accept` (Content/<destFolder>/<id>.glb +
.meta provenance + .rxmesh build) and recorded under meshy-batch-<date>/results/<id>.json.

Sub-commands
  balance                       print the live Meshy credit balance (key never printed)
  run [--ids a,b] [--limit N] [--workers 4] [--budget 1000] [--reserve 40] [--no-accept] [--dry-run]
  accept [--ids a,b]            (re)run gen_accept for generated-but-unaccepted results
  validate [--ids a,b]          re-run GLB validation against the results
  sheets                        build per-model contact sheets (reference + 4 Meshy views) + overview grid
  report                        write results.json + summary markdown for the batch

The API key is read only to query the credit balance (DPAPI keystore, same user) and is never
logged or echoed; generation itself goes through forge-agentd which holds the key.
"""
import argparse
import base64
import ctypes
import ctypes.wintypes as wt
import json
import struct
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[1]
WORKSPACE_ROOT = PROJECT.parents[1]
AGENTD = 'http://127.0.0.1:8103'
MESHY_BALANCE_URL = 'https://api.meshy.ai/openapi/v1/balance'

MANIFEST_PATH = HERE / 'meshy-manifest.json'
MANIFEST = json.loads(MANIFEST_PATH.read_text(encoding='utf-8'))
BATCH = HERE / MANIFEST['batch']
REFS = BATCH / 'refs'
RESULTS = BATCH / 'results'
SHEETS = BATCH / 'sheets'
for p in (RESULTS, SHEETS):
    p.mkdir(parents=True, exist_ok=True)

GEN = MANIFEST['generation']
WORKSPACE_ID = MANIFEST['workspaceId']
DEST_FOLDER = MANIFEST['destFolder']
PALETTE = MANIFEST['palette']
CREDITS_PER_MODEL = int(GEN.get('creditsPerModel', 15))
LOCK = threading.Lock()

if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')


def now_iso():
    return datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


def log(msg):
    with LOCK:
        print('[%s] %s' % (datetime.now().strftime('%H:%M:%S'), msg), flush=True)


# ---------------------------------------------------------------- HTTP helpers

def http_json(method, url, body=None, timeout=60, headers=None):
    data = None
    hdrs = {'Content-Type': 'application/json'}
    if headers:
        hdrs.update(headers)
    if body is not None:
        data = json.dumps(body).encode('utf-8')
    req = urllib.request.Request(url, data=data, method=method, headers=hdrs)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read()
            return resp.status, (json.loads(raw.decode('utf-8')) if raw else None)
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            doc = json.loads(raw.decode('utf-8')) if raw else None
        except Exception:
            doc = {'error': {'code': 'NON_JSON', 'message': raw[:300].decode('utf-8', 'replace')}}
        return e.code, doc


# ---------------------------------------------------------------- Meshy balance (DPAPI keystore)

class _BLOB(ctypes.Structure):
    _fields_ = [('cbData', wt.DWORD), ('pbData', ctypes.POINTER(ctypes.c_char))]


def _dpapi_unprotect(cipher: bytes) -> bytes:
    buf = ctypes.create_string_buffer(cipher, len(cipher))
    inp = _BLOB(len(cipher), ctypes.cast(buf, ctypes.POINTER(ctypes.c_char)))
    out = _BLOB()
    ok = ctypes.windll.crypt32.CryptUnprotectData(ctypes.byref(inp), None, None, None, None, 0, ctypes.byref(out))
    if not ok:
        raise OSError('CryptUnprotectData failed')
    try:
        return ctypes.string_at(out.pbData, out.cbData)
    finally:
        ctypes.windll.kernel32.LocalFree(out.pbData)


def _meshy_key():
    doc = json.loads((WORKSPACE_ROOT / 'data' / 'keystore.json').read_text(encoding='utf-8'))
    plain = _dpapi_unprotect(base64.b64decode(doc['dpapi']))
    return json.loads(plain.decode('utf-8'))['keys']['meshy']


def meshy_balance():
    """Live credit balance. The key is used for the Authorization header only."""
    key = _meshy_key()
    status, doc = http_json('GET', MESHY_BALANCE_URL, headers={'Authorization': 'Bearer ' + key})
    if status != 200 or not isinstance(doc, dict) or 'balance' not in doc:
        raise RuntimeError('balance query failed: HTTP %s' % status)
    return int(doc['balance'])


# ---------------------------------------------------------------- manifest helpers

def models_by_priority():
    return sorted(MANIFEST['models'], key=lambda m: m['priority'])


def model_map():
    return {m['id']: m for m in MANIFEST['models']}


def result_path(mid):
    return RESULTS / (mid + '.json')


def load_result(mid):
    p = result_path(mid)
    return json.loads(p.read_text(encoding='utf-8')) if p.exists() else None


def save_result(mid, doc):
    doc['updatedAt'] = now_iso()
    result_path(mid).write_text(json.dumps(doc, indent=2, ensure_ascii=False), encoding='utf-8')


def full_prompt(model):
    text = model['prompt'].strip() + ' ' + PALETTE
    if len(text) > 600:
        text = text[:597].rsplit(' ', 1)[0] + '...'
    return text


# ---------------------------------------------------------------- GLB validation

CHUNK_JSON = 0x4E4F534A
CHUNK_BIN = 0x004E4942
SAFE_EXTENSIONS = {
    'KHR_materials_emissive_strength', 'KHR_materials_specular', 'KHR_materials_ior',
    'KHR_texture_transform', 'KHR_materials_unlit', 'KHR_lights_punctual', 'KHR_materials_clearcoat',
}


def parse_glb(path: Path):
    data = path.read_bytes()
    if len(data) < 12:
        raise ValueError('file too small (%d bytes)' % len(data))
    magic, version, length = struct.unpack_from('<4sII', data, 0)
    if magic != b'glTF':
        raise ValueError('bad magic %r' % magic)
    if version != 2:
        raise ValueError('unsupported glTF version %d' % version)
    off = 12
    doc = None
    bin_len = 0
    while off + 8 <= min(length, len(data)):
        clen, ctype = struct.unpack_from('<II', data, off)
        off += 8
        chunk = data[off:off + clen]
        off += clen
        if ctype == CHUNK_JSON:
            doc = json.loads(chunk.decode('utf-8'))
        elif ctype == CHUNK_BIN:
            bin_len = len(chunk)
    if doc is None:
        raise ValueError('no JSON chunk')
    return doc, bin_len, len(data)


def glb_stats(path: Path):
    doc, bin_len, size = parse_glb(path)
    accessors = doc.get('accessors', [])
    meshes = doc.get('meshes', [])
    tris = 0
    verts = 0
    has_normal = True
    has_uv = True
    prims = 0
    mins = [float('inf')] * 3
    maxs = [float('-inf')] * 3
    for mesh in meshes:
        for prim in mesh.get('primitives', []):
            prims += 1
            attrs = prim.get('attributes', {})
            mode = prim.get('mode', 4)
            if 'POSITION' not in attrs:
                continue
            pos = accessors[attrs['POSITION']]
            verts += pos.get('count', 0)
            if 'min' in pos and 'max' in pos:
                mins = [min(a, b) for a, b in zip(mins, pos['min'])]
                maxs = [max(a, b) for a, b in zip(maxs, pos['max'])]
            count = accessors[prim['indices']]['count'] if 'indices' in prim else pos.get('count', 0)
            if mode == 4:
                tris += count // 3
            elif mode in (5, 6):
                tris += max(count - 2, 0)
            has_normal = has_normal and 'NORMAL' in attrs
            has_uv = has_uv and 'TEXCOORD_0' in attrs
    materials = doc.get('materials', [])
    base_color = sum(1 for m in materials if 'baseColorTexture' in m.get('pbrMetallicRoughness', {}))
    mr = sum(1 for m in materials if 'metallicRoughnessTexture' in m.get('pbrMetallicRoughness', {}))
    normal_tex = sum(1 for m in materials if 'normalTexture' in m)
    images = doc.get('images', [])
    image_bytes = 0
    for img in images:
        if 'bufferView' in img:
            image_bytes += doc['bufferViews'][img['bufferView']].get('byteLength', 0)
    extent = [round(b - a, 4) if a != float('inf') else None for a, b in zip(mins, maxs)]
    return {
        'fileBytes': size,
        'binBytes': bin_len,
        'generator': doc.get('asset', {}).get('generator'),
        'meshes': len(meshes),
        'primitives': prims,
        'triangles': tris,
        'vertices': verts,
        'hasNormals': has_normal and prims > 0,
        'hasTexcoords': has_uv and prims > 0,
        'materials': len(materials),
        'baseColorTextured': base_color,
        'metallicRoughnessTextured': mr,
        'normalTextured': normal_tex,
        'images': len(images),
        'imageMimeTypes': sorted({img.get('mimeType', '?') for img in images}),
        'imageBytes': image_bytes,
        'extensionsUsed': doc.get('extensionsUsed', []),
        'extensionsRequired': doc.get('extensionsRequired', []),
        'boundsMin': [round(v, 4) for v in mins] if mins[0] != float('inf') else None,
        'boundsMax': [round(v, 4) for v in maxs] if maxs[0] != float('-inf') else None,
        'extent': extent,
        'nodes': len(doc.get('nodes', [])),
        'animations': len(doc.get('animations', [])),
    }


def judge(stats, target_polycount, preview_count, pbr_expected=True):
    """PASS / WARN / FAIL with explicit reasons."""
    fails, warns = [], []
    if stats['meshes'] < 1 or stats['primitives'] < 1:
        fails.append('no mesh primitives')
    if stats['triangles'] < 500:
        fails.append('too few triangles (%d)' % stats['triangles'])
    if stats['triangles'] > target_polycount * 3:
        warns.append('triangles %d far above target %d' % (stats['triangles'], target_polycount))
    elif stats['triangles'] > int(target_polycount * 1.5):
        warns.append('triangles %d above target %d' % (stats['triangles'], target_polycount))
    if not stats['hasNormals']:
        fails.append('missing NORMAL attribute')
    if not stats['hasTexcoords']:
        fails.append('missing TEXCOORD_0 attribute')
    if stats['materials'] < 1 or stats['baseColorTextured'] < 1:
        fails.append('no base color texture')
    if stats['images'] < 1 or stats['imageBytes'] < 10_000:
        fails.append('no embedded texture images')
    if pbr_expected and (stats['metallicRoughnessTextured'] < 1 or stats['normalTextured'] < 1):
        warns.append('PBR maps incomplete (metallicRoughness=%d normal=%d)' % (
            stats['metallicRoughnessTextured'], stats['normalTextured']))
    if stats['extent'] is None or any(e is None or e <= 1e-4 for e in stats['extent']):
        fails.append('degenerate bounds %s' % stats['extent'])
    else:
        ratio = max(stats['extent']) / max(min(stats['extent']), 1e-6)
        if ratio > 25:
            warns.append('extreme aspect ratio %.1f' % ratio)
    unsafe = [e for e in stats['extensionsRequired'] if e not in SAFE_EXTENSIONS]
    if unsafe:
        fails.append('requires unsupported extensions %s' % unsafe)
    if preview_count < 4:
        warns.append('only %d/4 preview views saved' % preview_count)
    verdict = 'FAIL' if fails else ('WARN' if warns else 'PASS')
    return {'verdict': verdict, 'fails': fails, 'warns': warns}


# ---------------------------------------------------------------- generation

def texture_guidance(model):
    """'image' = Meshy textures from the reference render itself (default, keeps the authored
    palette); 'prompt' = the manifest prompt is sent as texture_prompt (text-guided texturing).
    The first five turrets of this batch used 'prompt' and came back washed-out white, so the
    manifest default was switched to 'image'."""
    return model.get('textureGuidance', GEN.get('textureGuidance', 'image'))


def cropped_reference(model, ref: Path) -> Path:
    """Small props occupy a fraction of the 1024px bake frame (ortho_scale 3.6 is sized for the
    airfield); crop to the alpha bounding box with 8% padding and resize back to a 1024 square so
    Meshy gets the full resolution on the object. Saved next to the original for provenance."""
    from PIL import Image
    img = Image.open(ref).convert('RGBA')
    bbox = img.getchannel('A').getbbox()
    if not bbox:
        return ref
    x0, y0, x1, y1 = bbox
    side = max(x1 - x0, y1 - y0)
    pad = int(side * 0.08)
    side += 2 * pad
    cx, cy = (x0 + x1) // 2, (y0 + y1) // 2
    canvas = Image.new('RGBA', (side, side), (0, 0, 0, 0))
    canvas.paste(img, (side // 2 - cx, side // 2 - cy), img)
    target = int(model.get('refSize', 1024))
    canvas = canvas.resize((target, target), Image.LANCZOS)
    out = ref.with_name(model['id'] + '.crop.png')
    canvas.save(out)
    return out


def build_request(model):
    ref = REFS / (model['id'] + '.png')
    if not ref.exists():
        raise FileNotFoundError('reference render missing: %s' % ref)
    if model.get('cropRef'):
        ref = cropped_reference(model, ref)
    b64 = base64.b64encode(ref.read_bytes()).decode('ascii')
    return {
        'workspaceId': WORKSPACE_ID,
        'backend': MANIFEST['backend'],
        # An empty prompt makes the adapter omit texture_prompt, so Meshy textures from the image.
        'prompt': full_prompt(model) if texture_guidance(model) == 'prompt' else '',
        'imageDataUrl': 'data:image/png;base64,' + b64,
        'modelType': GEN['modelType'],
        'aiModel': GEN['aiModel'],
        'targetPolycount': int(model.get('polycount', GEN['defaultPolycount'])),
        'texture': bool(GEN['texture']),
        'pbr': bool(GEN['pbr']),
        'textureResolution': GEN['textureResolution'],
        'timeoutSec': int(GEN['timeoutSec']),
    }


def redact_request(body):
    out = dict(body)
    out['imageDataUrl'] = '<data:image/png;base64, %d chars>' % len(body['imageDataUrl'])
    return out


def archive_previous(model):
    """Before a --redo run: keep the previous generation as evidence (results/<id>.v<N>.json and
    the previously accepted GLB under rejected/), so the superseded model stays reviewable
    while Content/<destFolder>/<id>.glb is replaced by the new acceptance."""
    import shutil
    mid = model['id']
    prev = load_result(mid)
    if not prev:
        return None
    n = 1
    while (RESULTS / ('%s.v%d.json' % (mid, n))).exists():
        n += 1
    prev['supersededAt'] = now_iso()
    prev['supersededReason'] = model.get('retryReason', 'manual redo')
    (RESULTS / ('%s.v%d.json' % (mid, n))).write_text(json.dumps(prev, indent=2, ensure_ascii=False), encoding='utf-8')
    rejected = BATCH / 'rejected'
    rejected.mkdir(exist_ok=True)
    asset = prev.get('accept', {}).get('assetPath')
    if asset and (PROJECT / 'Content' / asset).exists():
        shutil.copy2(PROJECT / 'Content' / asset, rejected / ('%s.v%d.glb' % (mid, n)))
    for pv in prev.get('previews', []):
        src = PROJECT / pv
        if src.exists():
            shutil.copy2(src, rejected / ('%s.v%d.%s' % (mid, n, src.name.rsplit('.', 2)[-2] + '.png')))
    result_path(mid).unlink()
    log('%s: previous generation archived as v%d (%s)' % (mid, n, prev.get('supersededReason')))
    return n


def strip_data_urls(resp):
    if not isinstance(resp, dict):
        return resp
    out = json.loads(json.dumps(resp))
    for art in out.get('artifacts', []):
        art.pop('dataUrl', None)
        for pv in art.get('previews', []):
            pv.pop('dataUrl', None)
    return out


class Budget:
    def __init__(self, budget, reserve, spent):
        self.budget = budget
        self.reserve = reserve
        self.spent = spent
        self.inflight = 0
        self.lock = threading.Lock()

    def reserve_task(self):
        with self.lock:
            projected = self.spent + (self.inflight + 1) * CREDITS_PER_MODEL
            if projected > self.budget - self.reserve:
                return False
            self.inflight += 1
            return True

    def settle(self, consumed):
        with self.lock:
            self.inflight -= 1
            self.spent += consumed


def generate_one(model, budget: Budget, do_accept=True, attempt_limit=2):
    mid = model['id']
    prev = load_result(mid) or {'id': mid, 'attempts': []}
    if not budget.reserve_task():
        log('%s: budget guard stop (spent=%d inflight=%d)' % (mid, budget.spent, budget.inflight))
        return mid, 'budget-stop'
    body = build_request(model)
    consumed = 0
    outcome = 'error'
    for attempt in range(1, attempt_limit + 1):
        started = time.time()
        log('%s: generate attempt %d (polycount=%d)' % (mid, attempt, body['targetPolycount']))
        status, doc = http_json('POST', AGENTD + '/api/forge/gen/mesh', body, timeout=int(GEN['timeoutSec']) * 2 + 600)
        elapsed = round(time.time() - started, 1)
        record = {'attempt': attempt, 'startedAt': now_iso(), 'elapsedSec': elapsed, 'httpStatus': status}
        if status == 200 and doc and doc.get('artifacts'):
            art = doc['artifacts'][0]
            consumed = int(art.get('meta', {}).get('consumedCredits') or CREDITS_PER_MODEL)
            record['ok'] = True
            prev['attempts'].append(record)
            prev.update({
                'status': 'generated',
                'textureGuidance': texture_guidance(model),
                'manifestPrompt': model['prompt'],
                'request': redact_request(body),
                'response': strip_data_urls(doc),
                'meshFileRef': art['fileRef'],
                'previews': [pv['fileRef'] for pv in art.get('previews', [])],
                'meta': art.get('meta', {}),
                'consumedCredits': consumed,
                'generatedAt': now_iso(),
                'elapsedSec': elapsed,
            })
            save_result(mid, prev)
            log('%s: generated in %ss, task=%s credits=%s previews=%d' % (
                mid, elapsed, art.get('meta', {}).get('taskId'), consumed, len(prev['previews'])))
            outcome = 'generated'
            break
        err = (doc or {}).get('error', {}) if isinstance(doc, dict) else {}
        code = err.get('code', 'HTTP_%s' % status)
        message = err.get('message', str(doc)[:300])
        record.update({'ok': False, 'errorCode': code, 'errorMessage': message})
        prev['attempts'].append(record)
        prev['status'] = 'error'
        prev['lastError'] = {'code': code, 'message': message}
        save_result(mid, prev)
        log('%s: attempt %d failed after %ss: %s %s' % (mid, attempt, elapsed, code, message[:200]))
        if code == 'GEN_BACKEND_NOT_CONFIGURED' or code == 'GEN_BAD_PARAMS' or code == 'WORKSPACE_NOT_FOUND':
            break
        if attempt < attempt_limit:
            time.sleep(90 if code == 'GEN_RATE_LIMITED' else 20)
    budget.settle(consumed)
    if outcome != 'generated':
        return mid, outcome
    validate_one(model, prev)
    if do_accept:
        accept_one(model, prev)
    return mid, prev.get('status', outcome)


# ---------------------------------------------------------------- validation + acceptance

def validate_one(model, res):
    mid = model['id']
    mesh_ref = res.get('meshFileRef')
    path = PROJECT / mesh_ref if mesh_ref else None
    try:
        if not path or not path.exists():
            raise FileNotFoundError('artifact missing: %s' % mesh_ref)
        stats = glb_stats(path)
        previews = [p for p in res.get('previews', []) if (PROJECT / p).exists()]
        verdict = judge(stats, int(model.get('polycount', GEN['defaultPolycount'])), len(previews), GEN['pbr'])
        sidecar = PROJECT / (mesh_ref + '.json')
        verdict['sidecarPresent'] = sidecar.exists()
        res['glb'] = stats
        res['validation'] = verdict
        log('%s: validation %s tris=%d verts=%d images=%d %s' % (
            mid, verdict['verdict'], stats['triangles'], stats['vertices'], stats['images'],
            '; '.join(verdict['fails'] + verdict['warns'])))
    except Exception as e:
        res['validation'] = {'verdict': 'FAIL', 'fails': ['glb parse error: %s' % e], 'warns': []}
        log('%s: validation FAIL %s' % (mid, e))
    save_result(mid, res)
    return res['validation']['verdict']


def mcp_call(tool, arguments, timeout=600):
    body = {'tool': tool, 'workspaceId': WORKSPACE_ID, 'arguments': arguments}
    status, doc = http_json('POST', AGENTD + '/api/forge/mcp/call', body, timeout=timeout)
    if status != 200:
        raise RuntimeError('mcp call %s HTTP %s: %s' % (tool, status, json.dumps(doc)[:300]))
    text = doc.get('content', [{}])[0].get('text', '{}') if isinstance(doc, dict) else '{}'
    try:
        payload = json.loads(text)
    except Exception:
        payload = {'raw': text}
    if isinstance(doc, dict) and doc.get('isError'):
        raise RuntimeError('%s: %s' % (payload.get('error'), payload.get('message')))
    return payload


def meta_origin(meta_path: Path):
    """assetd .meta files are YAML; read `provenance: / origin:` without a YAML dependency."""
    try:
        lines = meta_path.read_text(encoding='utf-8').splitlines()
    except Exception:
        return None
    in_prov = False
    for line in lines:
        if line.startswith('provenance:'):
            in_prov = True
            continue
        if in_prov:
            if line and not line.startswith(' '):
                break
            s = line.strip()
            if s.startswith('origin:'):
                return s.split(':', 1)[1].strip().strip('"\'')
    return None


def accept_one(model, res):
    mid = model['id']
    if res.get('validation', {}).get('verdict') == 'FAIL':
        res['status'] = 'rejected'
        res['accept'] = {'skipped': True, 'reason': 'validation FAIL'}
        save_result(mid, res)
        log('%s: NOT accepted (validation FAIL)' % mid)
        return False
    try:
        started = time.time()
        payload = mcp_call('mcp__gen-model__gen_accept', {
            'meshFileRef': res['meshFileRef'],
            'destFolder': DEST_FOLDER,
            'name': mid,
        })
        asset_path = payload.get('assetPath')
        content_root = PROJECT / 'Content'
        asset_abs = content_root / asset_path if asset_path else None
        meta_abs = content_root / (asset_path + '.meta') if asset_path else None
        artifact = payload.get('artifact')
        artifact_abs = PROJECT / '.forge' / 'cache' / artifact if artifact else None
        checks = {
            'assetExists': bool(asset_abs and asset_abs.exists()),
            'metaExists': bool(meta_abs and meta_abs.exists()),
            'artifactExists': bool(artifact_abs and artifact_abs.exists()),
        }
        origin = meta_origin(meta_abs) if checks['metaExists'] else None
        checks['provenanceGenModel'] = origin == 'gen-model'
        res['accept'] = {
            'ok': all(checks.values()),
            'assetPath': asset_path,
            'guid': payload.get('guid'),
            'artifact': artifact,
            'cacheHit': payload.get('cacheHit'),
            'checks': checks,
            'provenanceOrigin': origin,
            'elapsedSec': round(time.time() - started, 1),
            'acceptedAt': now_iso(),
        }
        res['status'] = 'accepted' if res['accept']['ok'] else 'accept-incomplete'
        save_result(mid, res)
        log('%s: accepted -> %s (artifact=%s, %ss)' % (mid, asset_path, artifact, res['accept']['elapsedSec']))
        return res['accept']['ok']
    except Exception as e:
        res['accept'] = {'ok': False, 'error': str(e)[:500]}
        res['status'] = 'accept-error'
        save_result(mid, res)
        log('%s: accept ERROR %s' % (mid, str(e)[:300]))
        return False


# ---------------------------------------------------------------- recovery

def recover_orphans(do_accept=True):
    """Adopt artifacts that forge-agentd finished after this client was stopped.

    The blocking generation task inside forge-agentd runs to completion even when the HTTP
    client disconnects, so the GLB, its sidecar and the preview PNGs still land in
    .forge/tmp/gen/. Sidecars carry the exact prompt that was sent, which is unique per model
    while texture guidance is 'prompt'; match on it and adopt the artifact.
    """
    gen_dir = PROJECT / '.forge' / 'tmp' / 'gen'
    known_refs = {r.get('meshFileRef') for r in (load_result(m['id']) for m in MANIFEST['models']) if r}
    prompt_to_model = {full_prompt(m): m for m in MANIFEST['models']}
    adopted = []
    for sidecar in sorted(gen_dir.glob('gen-*.glb.json')):
        glb_rel = ('.forge/tmp/gen/' + sidecar.name[:-5])
        if glb_rel in known_refs:
            continue
        try:
            sc = json.loads(sidecar.read_text(encoding='utf-8'))
        except Exception:
            continue
        if sc.get('backendId') != 'meshy':
            continue
        model = prompt_to_model.get((sc.get('prompt') or '').strip())
        if model is None:
            log('recover: %s has no matching manifest prompt, left alone' % sidecar.name)
            continue
        mid = model['id']
        existing = load_result(mid)
        if existing and existing.get('status') not in (None, 'error'):
            log('recover: %s already has a result (%s), orphan %s kept in tmp' % (mid, existing['status'], sidecar.name))
            continue
        meta = sc.get('meta', {})
        previews = [('.forge/tmp/gen/' + p.name) for p in sorted(gen_dir.glob(sidecar.name[:-5] + '.*.png'))]
        res = existing or {'id': mid, 'attempts': []}
        res['attempts'].append({'attempt': len(res['attempts']) + 1, 'ok': True, 'recovered': True,
                                'note': 'client stopped mid-flight; artifact adopted from .forge/tmp/gen sidecar'})
        res.update({
            'status': 'generated',
            'textureGuidance': 'prompt',
            'manifestPrompt': model['prompt'],
            'request': {'recovered': True, 'prompt': sc.get('prompt'), 'targetPolycount': meta.get('targetPolycount')},
            'meshFileRef': glb_rel,
            'previews': previews,
            'meta': meta,
            'consumedCredits': int(meta.get('consumedCredits') or CREDITS_PER_MODEL),
            'generatedAt': sc.get('generatedAt'),
        })
        save_result(mid, res)
        log('recover: adopted %s -> %s (task=%s previews=%d)' % (sidecar.name, mid, meta.get('taskId'), len(previews)))
        validate_one(model, res)
        if do_accept:
            accept_one(model, res)
        adopted.append(mid)
    log('recover: %d adopted %s' % (len(adopted), adopted))
    return adopted


# ---------------------------------------------------------------- palette drift (rough vs meshy, same camera/lights)

ISO_DIR = BATCH / 'iso'
BAKES = PROJECT / 'Content/UI/v6/model-bakes'
ISO_VIEWS = ['s', 'w', 'n', 'e']


def _mean_rgba(paths):
    """Alpha-weighted mean sRGB (0..1) and mean alpha coverage over a set of RGBA renders."""
    from PIL import Image
    sum_rgb = [0.0, 0.0, 0.0]
    sum_a = 0.0
    pixels = 0
    for p in paths:
        if not p.exists():
            continue
        img = Image.open(p).convert('RGBA')
        raw = img.tobytes()
        pixels += len(raw) // 4
        for i in range(0, len(raw), 4):
            a = raw[i + 3]
            if a == 0:
                continue
            w = a / 255.0
            sum_rgb[0] += raw[i] / 255.0 * w
            sum_rgb[1] += raw[i + 1] / 255.0 * w
            sum_rgb[2] += raw[i + 2] / 255.0 * w
            sum_a += w
    if sum_a == 0:
        return None, 0.0
    return [round(c / sum_a, 4) for c in sum_rgb], round(sum_a / max(pixels, 1), 4)


def palette_metrics(mid):
    """Compare the authored rough bake (Content/UI/v6/model-bakes/<id>/{s,w,n,e}.png) with the
    Meshy model rendered by render_meshy_iso.py under the identical camera and lights.
    deltaL = luminance difference (0..1), deltaRGB = euclidean distance of mean colours,
    coverageRatio = silhouette pixel coverage meshy/rough (size normalisation sanity)."""
    rough_paths = [BAKES / mid / (v + '.png') for v in ISO_VIEWS]
    meshy_paths = [ISO_DIR / ('%s.%s.png' % (mid, v)) for v in ISO_VIEWS]
    if not all(p.exists() for p in meshy_paths) or not all(p.exists() for p in rough_paths):
        return None
    r_rgb, r_cov = _mean_rgba(rough_paths)
    m_rgb, m_cov = _mean_rgba(meshy_paths)
    if r_rgb is None or m_rgb is None:
        return None
    lum = lambda c: 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    d_rgb = sum((a - b) ** 2 for a, b in zip(r_rgb, m_rgb)) ** 0.5
    out = {
        'roughMeanRGB': r_rgb, 'meshyMeanRGB': m_rgb,
        'deltaRGB': round(d_rgb, 4), 'deltaL': round(lum(m_rgb) - lum(r_rgb), 4),
        'coverageRatio': round(m_cov / r_cov, 3) if r_cov else None,
    }
    # Calibrated on this batch: |dL| around 0.10 is a modest, visually acceptable shift under the
    # identical bake lighting (e.g. plasma-cannon -0.11 looks right); 0.15+ is a noticeable
    # lighter/darker body colour worth a human look; 0.25+ is a clearly different palette.
    flags = []
    if abs(out['deltaL']) > 0.25:
        flags.append('strong palette drift %+.2f luminance (%s than authored palette)' % (out['deltaL'], 'lighter' if out['deltaL'] > 0 else 'darker'))
    elif abs(out['deltaL']) > 0.15:
        flags.append('palette drift %+.2f luminance (%s than authored palette, review)' % (out['deltaL'], 'lighter' if out['deltaL'] > 0 else 'darker'))
    elif out['deltaRGB'] > 0.25:
        flags.append('colour drift dRGB=%.2f (review)' % out['deltaRGB'])
    if out['coverageRatio'] and (out['coverageRatio'] < 0.6 or out['coverageRatio'] > 1.7):
        flags.append('silhouette coverage ratio %.2f' % out['coverageRatio'])
    out['flags'] = flags
    return out


def attach_palette_metrics(ids=None):
    iso_index_path = ISO_DIR / 'iso-index.json'
    iso_index = json.loads(iso_index_path.read_text(encoding='utf-8')) if iso_index_path.exists() else {}
    n = 0
    for model in models_by_priority():
        mid = model['id']
        if ids and mid not in ids:
            continue
        res = load_result(mid)
        if not res or res.get('status') != 'accepted':
            continue
        iso = iso_index.get(mid)
        metrics = palette_metrics(mid)
        if iso:
            res['blenderImport'] = {k: iso.get(k) for k in (
                'triangles', 'materials', 'images', 'meshyExtentXYZ', 'roughExtentXYZ', 'normalizeScale',
                'normalizedHeight', 'strip', 'importError')}
        if metrics:
            res['palette'] = metrics
            v = res.setdefault('validation', {'verdict': 'PASS', 'fails': [], 'warns': []})
            v['warns'] = [w for w in v.get('warns', []) if not w.startswith(('luminance drift', 'palette drift', 'strong palette drift', 'colour drift', 'silhouette coverage'))]
            v['warns'].extend(metrics['flags'])
            if v['verdict'] != 'FAIL':
                v['verdict'] = 'WARN' if v['warns'] else 'PASS'
        save_result(mid, res)
        n += 1
    log('palette metrics attached for %d models' % n)


# ---------------------------------------------------------------- sheets + report

def build_sheets(ids=None):
    from PIL import Image, ImageDraw, ImageFont
    font = ImageFont.truetype('C:/Windows/Fonts/consola.ttf', 18)
    small = ImageFont.truetype('C:/Windows/Fonts/consola.ttf', 15)
    tile = 320
    pad = 12
    fronts = []
    for model in models_by_priority():
        mid = model['id']
        if ids and mid not in ids:
            continue
        res = load_result(mid)
        if not res or res.get('status') in (None, 'error'):
            continue
        views = [('reference (rough model)', REFS / (mid + '.png'))]
        for pv in res.get('previews', []):
            label = Path(pv).suffix and Path(pv).stem.rsplit('.', 1)[-1]
            views.append(('meshy ' + label, PROJECT / pv))
        w = pad + len(views) * (tile + pad)
        strip_path = ISO_DIR / (mid + '.png')
        strip_img = None
        if strip_path.exists():
            try:
                strip_img = Image.open(strip_path).convert('RGBA')
                scale = (w - 2 * pad) / strip_img.width
                strip_img = strip_img.resize((w - 2 * pad, max(1, int(strip_img.height * scale))))
            except Exception:
                strip_img = None
        strip_h = (strip_img.height + 24 + pad) if strip_img else 0
        h = tile + 3 * pad + 24 + 2 * 22 + strip_h
        sheet = Image.new('RGB', (w, h), (30, 33, 38))
        draw = ImageDraw.Draw(sheet)
        if strip_img:
            y0 = tile + 3 * pad + 24 + 2 * 22
            bg = Image.new('RGBA', strip_img.size, (58, 62, 70, 255))
            bg.paste(strip_img, (0, 0), strip_img)
            sheet.paste(bg.convert('RGB'), (pad, y0))
            draw.text((pad + 4, y0 + strip_img.height + 2),
                      'same bake camera + lights: rough se | meshy s / w / n / e (Blender glTF import, footprint-normalised)',
                      fill=(200, 205, 210), font=small)
        x = pad
        for label, path in views:
            box = Image.new('RGB', (tile, tile), (58, 62, 70))
            if path.exists():
                try:
                    img = Image.open(path).convert('RGBA')
                    img.thumbnail((tile, tile))
                    bg = Image.new('RGBA', (tile, tile), (58, 62, 70, 255))
                    bg.paste(img, ((tile - img.width) // 2, (tile - img.height) // 2), img)
                    box = bg.convert('RGB')
                except Exception:
                    pass
            sheet.paste(box, (x, pad))
            draw.text((x + 4, pad + tile + 4), label, fill=(200, 205, 210), font=small)
            x += tile + pad
        g = res.get('glb', {})
        v = res.get('validation', {})
        acc = res.get('accept', {})
        line1 = '%s  |  %s  |  tris=%s verts=%s mats=%s images=%s  |  %s' % (
            mid, res.get('status'), g.get('triangles'), g.get('vertices'), g.get('materials'), g.get('images'),
            v.get('verdict'))
        pal = res.get('palette') or {}
        line2 = 'task=%s credits=%s guidance=%s accepted=%s %s  dL=%s dRGB=%s cov=%s' % (
            res.get('meta', {}).get('taskId'), res.get('consumedCredits'), res.get('textureGuidance'), acc.get('ok'),
            acc.get('assetPath') or '', pal.get('deltaL'), pal.get('deltaRGB'), pal.get('coverageRatio'))
        notes = '; '.join(v.get('fails', []) + v.get('warns', []))
        draw.text((pad, pad + tile + 30), line1, fill=(235, 235, 235), font=font)
        draw.text((pad, pad + tile + 54), line2 + ('  |  ' + notes if notes else ''), fill=(180, 200, 220), font=small)
        sheet.save(SHEETS / (mid + '.png'))
        front = next((PROJECT / p for p in res.get('previews', []) if p.endswith('.front.png')), None)
        fronts.append((mid, front, v.get('verdict')))
    if fronts:
        cols = 8
        cell = 200
        rows = (len(fronts) + cols - 1) // cols
        grid = Image.new('RGB', (cols * cell, rows * (cell + 22)), (30, 33, 38))
        draw = ImageDraw.Draw(grid)
        for i, (mid, front, verdict) in enumerate(fronts):
            cx = (i % cols) * cell
            cy = (i // cols) * (cell + 22)
            if front and front.exists():
                img = Image.open(front).convert('RGBA')
                img.thumbnail((cell - 8, cell - 8))
                bg = Image.new('RGBA', (cell - 8, cell - 8), (58, 62, 70, 255))
                bg.paste(img, ((cell - 8 - img.width) // 2, (cell - 8 - img.height) // 2), img)
                grid.paste(bg.convert('RGB'), (cx + 4, cy + 4))
            color = (120, 220, 140) if verdict == 'PASS' else ((240, 200, 90) if verdict == 'WARN' else (240, 110, 110))
            draw.text((cx + 6, cy + cell), '%s %s' % (mid[:22], verdict or ''), fill=color, font=small)
        grid.save(BATCH / 'overview-front-views.png')
    log('sheets: %d built -> %s' % (len(fronts), SHEETS))


def refresh_accept_checks(res):
    """Re-verify the on-disk acceptance facts (asset, .meta with gen-model provenance, .rxmesh)."""
    acc = res.get('accept') or {}
    asset = acc.get('assetPath')
    if not asset:
        return acc
    asset_abs = PROJECT / 'Content' / asset
    meta_abs = PROJECT / 'Content' / (asset + '.meta')
    artifact = acc.get('artifact')
    origin = meta_origin(meta_abs) if meta_abs.exists() else None
    acc['checks'] = {
        'assetExists': asset_abs.exists(),
        'metaExists': meta_abs.exists(),
        'artifactExists': bool(artifact and (PROJECT / '.forge' / 'cache' / artifact).exists()),
        'provenanceGenModel': origin == 'gen-model',
    }
    acc['provenanceOrigin'] = origin
    acc['ok'] = all(acc['checks'].values())
    res['accept'] = acc
    return acc


def write_report(balance_before=None, balance_after=None):
    rows = []
    totals = {'accepted': 0, 'generated': 0, 'rejected': 0, 'error': 0, 'other': 0, 'credits': 0,
              'pass': 0, 'warn': 0, 'fail': 0, 'acceptChecksOk': 0}
    for model in models_by_priority():
        res = load_result(model['id'])
        if not res:
            rows.append({'id': model['id'], 'status': 'not-run', 'group': model['group']})
            continue
        if res.get('accept', {}).get('assetPath'):
            acc = refresh_accept_checks(res)
            if acc['ok']:
                totals['acceptChecksOk'] += 1
            elif res.get('status') == 'accepted':
                res['status'] = 'accept-incomplete'
            save_result(model['id'], res)
        st = res.get('status', 'error')
        key = st if st in totals else ('error' if st.startswith(('error', 'accept')) else 'other')
        totals[key] = totals.get(key, 0) + 1
        totals['credits'] += int(res.get('consumedCredits') or 0)
        verdict = res.get('validation', {}).get('verdict')
        if verdict:
            totals[verdict.lower()] += 1
        rows.append({
            'id': model['id'], 'group': model['group'], 'status': st, 'verdict': verdict,
            'triangles': res.get('glb', {}).get('triangles'), 'vertices': res.get('glb', {}).get('vertices'),
            'images': res.get('glb', {}).get('images'), 'fileBytes': res.get('glb', {}).get('fileBytes'),
            'previews': len(res.get('previews', [])), 'credits': res.get('consumedCredits'),
            'taskId': res.get('meta', {}).get('taskId'), 'assetPath': res.get('accept', {}).get('assetPath'),
            'artifact': res.get('accept', {}).get('artifact'),
            'provenanceOrigin': res.get('accept', {}).get('provenanceOrigin'),
            'acceptChecks': res.get('accept', {}).get('checks'),
            'textureGuidance': res.get('textureGuidance'),
            'blenderImport': res.get('blenderImport'),
            'palette': res.get('palette'),
            'notes': '; '.join(res.get('validation', {}).get('fails', []) + res.get('validation', {}).get('warns', [])),
            'error': (res.get('lastError') or {}).get('message') if st == 'error' else res.get('accept', {}).get('error'),
        })
    superseded = []
    for p in sorted(RESULTS.glob('*.v*.json')):
        try:
            old = json.loads(p.read_text(encoding='utf-8'))
        except Exception:
            continue
        totals['credits'] += int(old.get('consumedCredits') or 0)
        superseded.append({
            'id': old.get('id'), 'file': p.name, 'taskId': old.get('meta', {}).get('taskId'),
            'credits': old.get('consumedCredits'), 'reason': old.get('supersededReason'),
            'textureGuidance': old.get('textureGuidance'),
        })
    # Meshy tasks that were created (and billed) but whose polling died on a transient TLS error;
    # the adapter reported GEN_BACKEND_ERROR and the runner re-created the task.
    orphan_tasks = []
    for p in sorted(RESULTS.glob('*.json')):
        try:
            r = json.loads(p.read_text(encoding='utf-8'))
        except Exception:
            continue
        for att in r.get('attempts', []):
            msg = att.get('errorMessage') or ''
            if not att.get('ok') and 'image-to-3d/' in msg:
                tid = msg.split('image-to-3d/', 1)[1].split(':', 1)[0].strip()
                orphan_tasks.append({'id': r.get('id'), 'taskId': tid, 'error': msg[:160], 'creditsLikelyBilled': CREDITS_PER_MODEL})
    totals['creditsOrphanedTasks'] = sum(o['creditsLikelyBilled'] for o in orphan_tasks)
    summary = {
        'batch': MANIFEST['batch'], 'generatedAt': now_iso(), 'generation': GEN, 'destFolder': DEST_FOLDER,
        'balanceBefore': balance_before, 'balanceAfter': balance_after, 'totals': totals,
        'skipped': MANIFEST['skipped'], 'models': rows, 'superseded': superseded, 'orphanTasks': orphan_tasks,
    }
    (BATCH / 'results.json').write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding='utf-8')
    write_markdown_table(rows, BATCH / 'results-table.md')
    log('report: %s' % (BATCH / 'results.json'))
    return summary


def write_markdown_table(rows, path):
    """Per-model acceptance table (Chinese headers) for the batch report."""
    status_zh = {
        'accepted': '已入库', 'generated': '已生成未入库', 'rejected': '校验失败未入库',
        'error': '生成失败', 'accept-error': '入库失败', 'accept-incomplete': '入库不完整', 'not-run': '未运行',
    }
    lines = ['| # | 模型 | 分组 | 状态 | 校验 | 三角面 | 顶点 | 贴图 | GLB | 贴图引导 | Blender 导入 | ΔL | ΔRGB | 覆盖比 | credits | Meshy task | 入库路径 | 备注 |',
             '|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|']
    for i, r in enumerate(rows, 1):
        size = r.get('fileBytes')
        size_s = '%.1f MB' % (size / 1048576) if size else ''
        note = r.get('notes') or ''
        if r.get('error'):
            note = (note + '; ' if note else '') + str(r['error'])[:120]
        bi = r.get('blenderImport') or {}
        pal = r.get('palette') or {}
        import_s = ''
        if bi:
            import_s = ('失败: %s' % bi['importError'][:60]) if bi.get('importError') else (
                'OK %s tris/%s img, ×%s' % (bi.get('triangles'), bi.get('images'), bi.get('normalizeScale')))
        task_s = ('`%s`' % r['taskId'][:36]) if r.get('taskId') else ''
        lines.append('| %d | `%s` | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |' % (
            i, r['id'], r.get('group', ''), status_zh.get(r.get('status'), r.get('status')), r.get('verdict') or '',
            r.get('triangles') if r.get('triangles') is not None else '', r.get('vertices') if r.get('vertices') is not None else '',
            r.get('images') if r.get('images') is not None else '', size_s, r.get('textureGuidance') or '', import_s,
            ('%+.2f' % pal['deltaL']) if pal.get('deltaL') is not None else '',
            ('%.2f' % pal['deltaRGB']) if pal.get('deltaRGB') is not None else '',
            pal.get('coverageRatio') if pal.get('coverageRatio') is not None else '',
            r.get('credits') if r.get('credits') is not None else '', task_s,
            ('`Content/%s`' % r['assetPath']) if r.get('assetPath') else '', note.replace('|', '/')))
    path.write_text('\n'.join(lines) + '\n', encoding='utf-8')


# ---------------------------------------------------------------- CLI

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('command', choices=['balance', 'run', 'accept', 'validate', 'sheets', 'report', 'recover', 'palette'])
    ap.add_argument('--ids')
    ap.add_argument('--limit', type=int)
    ap.add_argument('--workers', type=int, default=4)
    ap.add_argument('--budget', type=int, default=1000, help='max credits this batch may consume in total')
    ap.add_argument('--reserve', type=int, default=40, help='credits to keep unspent for retries')
    ap.add_argument('--no-accept', action='store_true')
    ap.add_argument('--dry-run', action='store_true')
    ap.add_argument('--retry-errors', action='store_true', help='include models whose last run errored')
    ap.add_argument('--redo', action='store_true', help='regenerate even if already generated (costs credits)')
    args = ap.parse_args()
    ids = set(args.ids.split(',')) if args.ids else None
    mm = model_map()
    if ids:
        unknown = ids - set(mm)
        if unknown:
            sys.exit('unknown ids: %s' % sorted(unknown))

    if args.command == 'balance':
        print('meshy balance: %d credits' % meshy_balance())
        return

    if args.command == 'recover':
        recover_orphans(not args.no_accept)
        return

    if args.command == 'palette':
        attach_palette_metrics(ids)
        return

    if args.command == 'validate':
        for model in models_by_priority():
            if ids and model['id'] not in ids:
                continue
            res = load_result(model['id'])
            if res and res.get('meshFileRef'):
                validate_one(model, res)
        return

    if args.command == 'accept':
        for model in models_by_priority():
            if ids and model['id'] not in ids:
                continue
            res = load_result(model['id'])
            if res and res.get('meshFileRef') and not res.get('accept', {}).get('ok'):
                if not res.get('validation'):
                    validate_one(model, res)
                accept_one(model, res)
        return

    if args.command == 'sheets':
        build_sheets(ids)
        return

    if args.command == 'report':
        write_report()
        return

    # run
    queue = []
    spent = 0
    for model in models_by_priority():
        res = load_result(model['id'])
        if res and res.get('consumedCredits'):
            spent += int(res['consumedCredits'])
        if ids and model['id'] not in ids:
            continue
        if res and res.get('status') not in (None, 'error') and not args.redo:
            continue
        if res and args.redo and res.get('status') not in (None, 'error'):
            if not ids:
                sys.exit('--redo requires explicit --ids (it re-spends credits)')
            archive_previous(model)
        if res and res.get('status') == 'error' and not args.retry_errors and not ids:
            continue
        if not (REFS / (model['id'] + '.png')).exists():
            log('%s: reference render missing, skipped' % model['id'])
            continue
        queue.append(model)
    if args.limit:
        queue = queue[:args.limit]
    try:
        balance_before = meshy_balance()
    except Exception as e:
        balance_before = None
        log('balance query failed (%s); relying on recorded consumption only' % e)
    log('queue=%d workers=%d budget=%d reserve=%d already-spent=%d balance=%s' % (
        len(queue), args.workers, args.budget, args.reserve, spent, balance_before))
    if balance_before is not None and balance_before < CREDITS_PER_MODEL:
        sys.exit('insufficient Meshy balance')
    if args.dry_run:
        for m in queue:
            print(' ', m['priority'], m['id'], 'polycount=%s' % m.get('polycount', GEN['defaultPolycount']),
                  'prompt=%d chars' % len(full_prompt(m)))
        return
    budget = Budget(args.budget, args.reserve, spent)
    if balance_before is not None:
        # never let projected spend exceed the live balance either
        budget.budget = min(budget.budget, spent + balance_before)
    outcomes = {}
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = {pool.submit(generate_one, m, budget, not args.no_accept): m['id'] for m in queue}
        for fut in as_completed(futures):
            mid = futures[fut]
            try:
                _, outcome = fut.result()
            except Exception as e:
                outcome = 'exception: %s' % e
                log('%s: EXCEPTION %s' % (mid, e))
            outcomes[mid] = outcome
    try:
        balance_after = meshy_balance()
    except Exception:
        balance_after = None
    counts = {}
    for v in outcomes.values():
        counts[v] = counts.get(v, 0) + 1
    log('run finished: %s spent=%d balance=%s' % (counts, budget.spent, balance_after))
    write_report(balance_before, balance_after)


if __name__ == '__main__':
    main()
