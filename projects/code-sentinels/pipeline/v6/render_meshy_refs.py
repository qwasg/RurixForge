"""Render high-resolution front three-quarter reference images from the authored V6 rough
.blend models (Content/Models/v6), for use as Meshy image-to-3D references.

Run with Blender in background mode:
  blender --background --python render_meshy_refs.py -- [--ids a,b] [--size 1024] [--samples 48]
                                                       [--manifest meshy-manifest.json] [--out DIR] [--force]

The saved .blend files already contain the orthographic bake camera and the two area lights
(see bake_models.py); this script only rotates the assembly root to the original bake
direction 'se' (root z = -90 deg, front-right three-quarter) and re-renders at higher resolution.
Objects listed in a model's manifest `hideObjects` (substring match, e.g. the unmodified
official software icon plates) are excluded from the render so the AI does not repaint logos.
"""
import bpy, math, sys, json
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[1]
MODELS = PROJECT / 'Content/Models/v6'

args = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []


def opt(name, default=None):
    return args[args.index(name) + 1] if name in args else default


manifest_path = Path(opt('--manifest', str(HERE / 'meshy-manifest.json')))
manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
out = Path(opt('--out', str(HERE / manifest['batch'] / 'refs')))
out.mkdir(parents=True, exist_ok=True)
size = int(opt('--size', str(manifest.get('reference', {}).get('size', 1024))))
samples = int(opt('--samples', '48'))
angle = float(opt('--angle', '-90'))
force = '--force' in args
wanted = opt('--ids')
wanted = wanted.split(',') if wanted else None
by_id = {m['id']: m for m in manifest['models']}
ids = wanted or [m['id'] for m in manifest['models']]

index_path = out / 'refs-index.json'
index = json.loads(index_path.read_text(encoding='utf-8')) if index_path.exists() else {}

for id in ids:
    target = out / (id + '.png')
    if target.exists() and not force:
        print('REF_SKIP ' + id, flush=True)
        continue
    blend = MODELS / (id + '.blend')
    if not blend.exists():
        print('REF_MISSING ' + id, flush=True)
        continue
    bpy.ops.wm.open_mainfile(filepath=str(blend))
    scene = bpy.context.scene
    root = bpy.data.objects.get(id + ' assembly')
    if root is None:
        empties = [o for o in scene.objects if o.parent is None and o.type == 'EMPTY' and o.children]
        root = empties[0] if empties else None
    if root is None or scene.camera is None:
        print('REF_NOROOT ' + id, flush=True)
        continue
    model_angle = float(by_id.get(id, {}).get('refAngle', angle))
    root.rotation_euler.z = math.radians(model_angle)
    hidden = []
    for needle in by_id.get(id, {}).get('hideObjects', []):
        for ob in scene.objects:
            if needle in ob.name:
                ob.hide_render = True
                hidden.append(ob.name)
    scene.render.engine = 'CYCLES'
    scene.cycles.device = 'CPU'
    scene.cycles.samples = samples
    scene.cycles.use_denoising = True
    scene.render.threads_mode = 'AUTO'
    scene.render.resolution_x = size
    scene.render.resolution_y = size
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = 'PNG'
    scene.render.image_settings.color_mode = 'RGBA'
    scene.render.film_transparent = True
    scene.render.filepath = str(target)
    bpy.ops.render.render(write_still=True)
    index[id] = {
        'id': id,
        'source': blend.relative_to(PROJECT).as_posix(),
        'ref': target.relative_to(PROJECT).as_posix(),
        'view': 'orthographic bake camera from the saved .blend, root z=%g deg (%s)' % (
            model_angle, 'front-right three-quarter, bake direction se' if model_angle == -90 else 'per-model refAngle override'),
        'size': [size, size],
        'samples': samples,
        'engine': 'Blender %s Cycles CPU' % bpy.app.version_string,
        'hiddenObjects': hidden,
    }
    index_path.write_text(json.dumps(index, indent=2, ensure_ascii=False), encoding='utf-8')
    print('REF_RENDERED ' + id, flush=True)
print('REF_DONE', flush=True)
