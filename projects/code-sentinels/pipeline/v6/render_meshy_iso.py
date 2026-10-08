"""Acceptance renders for the Meshy-generated V6 models.

For every accepted model in meshy-batch-<date>/results/, import Content/Models/v6-meshy/<id>.glb
with Blender's strict glTF importer (this alone is a real import check: materials, embedded
textures, normals), normalise it to the authored rough model's footprint (largest horizontal
extent of Content/Models/v6/<id>.glb), stand it on z=0, and render four cardinal views with the
same orthographic 30-degree bake camera and lights used by bake_models.py. The strip lets the
rough model and the AI model be compared under identical lighting, and doubles as a preview of
how the mesh would bake into the eight-direction sprite pipeline.

Run:  blender --background --python render_meshy_iso.py -- [--ids a,b] [--size 384] [--samples 16]
"""
import bpy, math, sys, json
from pathlib import Path
from mathutils import Vector

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[1]
ROUGH = PROJECT / 'Content/Models/v6'
MANIFEST = json.loads((HERE / 'meshy-manifest.json').read_text(encoding='utf-8'))
BATCH = HERE / MANIFEST['batch']
ACCEPTED = PROJECT / 'Content' / MANIFEST['destFolder']
OUT = BATCH / 'iso'
OUT.mkdir(parents=True, exist_ok=True)

args = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []


def opt(name, default=None):
    return args[args.index(name) + 1] if name in args else default


size = int(opt('--size', '384'))
samples = int(opt('--samples', '16'))
wanted = opt('--ids')
wanted = wanted.split(',') if wanted else None
force = '--force' in args
DIRS = [('s', -135), ('w', -225), ('n', -315), ('e', -405)]


def reset_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
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
    scene.view_settings.view_transform = 'AgX'
    world = bpy.data.worlds.new('World')
    scene.world = world
    world.color = (.17, .19, .22)
    return scene


def look(ob, point):
    ob.rotation_euler = (Vector(point) - ob.location).to_track_quat('-Z', 'Y').to_euler()


def add_camera_and_lights(scene):
    bpy.ops.object.camera_add(location=(6, -6, 5.54898))
    camera = bpy.context.object
    camera.data.type = 'ORTHO'
    camera.data.ortho_scale = 3.6
    look(camera, (0, 0, .65))
    scene.camera = camera
    for name, loc, power, sz, col in [('key', (-3, -4, 7), 800, 5, (1, .90, .75)), ('fill', (4, 1, 5), 650, 4, (.55, .82, 1))]:
        bpy.ops.object.light_add(type='AREA', location=loc)
        o = bpy.context.object
        o.name = name
        o.data.energy = power
        o.data.shape = 'DISK'
        o.data.size = sz
        o.data.color = col
        look(o, (0, 0, .5))


def import_glb(path):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(path))
    return [o for o in bpy.data.objects if o not in before]


def world_bounds(objects):
    mins = Vector((float('inf'),) * 3)
    maxs = Vector((float('-inf'),) * 3)
    bpy.context.view_layer.update()
    for ob in objects:
        if ob.type != 'MESH':
            continue
        for corner in ob.bound_box:
            w = ob.matrix_world @ Vector(corner)
            mins = Vector(min(a, b) for a, b in zip(mins, w))
            maxs = Vector(max(a, b) for a, b in zip(maxs, w))
    return mins, maxs


def mesh_stats(objects):
    tris = 0
    mats = set()
    images = set()
    for ob in objects:
        if ob.type != 'MESH':
            continue
        me = ob.data
        me.calc_loop_triangles()
        tris += len(me.loop_triangles)
        for m in me.materials:
            if m is None:
                continue
            mats.add(m.name)
            if m.use_nodes:
                for n in m.node_tree.nodes:
                    if n.type == 'TEX_IMAGE' and n.image is not None:
                        images.add(n.image.name)
    return {'triangles': tris, 'materials': len(mats), 'images': len(images)}


index_path = OUT / 'iso-index.json'
index = json.loads(index_path.read_text(encoding='utf-8')) if index_path.exists() else {}
results_dir = BATCH / 'results'
ids = []
for p in sorted(results_dir.glob('*.json')):
    if '.v' in p.stem:  # superseded generations (<id>.v<N>.json) keep their own evidence under rejected/
        continue
    res = json.loads(p.read_text(encoding='utf-8'))
    if res.get('status') == 'accepted' and (wanted is None or res['id'] in wanted):
        ids.append(res['id'])

for mid in ids:
    strip_path = OUT / (mid + '.png')
    if strip_path.exists() and not force and mid in index:
        print('ISO_SKIP ' + mid, flush=True)
        continue
    glb = ACCEPTED / (mid + '.glb')
    rough = ROUGH / (mid + '.glb')
    if not glb.exists():
        print('ISO_MISSING ' + mid, flush=True)
        continue
    scene = reset_scene()
    add_camera_and_lights(scene)
    entry = {'id': mid, 'glb': glb.relative_to(PROJECT).as_posix()}
    rough_extent = None
    if rough.exists():
        rough_objs = import_glb(rough)
        rmin, rmax = world_bounds(rough_objs)
        rough_extent = [round(v, 4) for v in (rmax - rmin)]
        entry['roughExtentXYZ'] = rough_extent
        for ob in rough_objs:
            bpy.data.objects.remove(ob, do_unlink=True)
    try:
        objs = import_glb(glb)
    except Exception as e:
        entry['importError'] = str(e)
        index[mid] = entry
        index_path.write_text(json.dumps(index, indent=2, ensure_ascii=False), encoding='utf-8')
        print('ISO_IMPORT_FAIL ' + mid, flush=True)
        continue
    mn, mx = world_bounds(objs)
    raw_extent = [round(v, 4) for v in (mx - mn)]
    entry['meshyExtentXYZ'] = raw_extent
    entry.update(mesh_stats(objs))
    # normalise: match the rough model's largest horizontal footprint, stand on z=0, centre xy
    root = bpy.data.objects.new(mid + ' meshy root', None)
    scene.collection.objects.link(root)
    for ob in objs:
        if ob.parent is None:
            ob.parent = root
    horiz_meshy = max(raw_extent[0], raw_extent[1], 1e-6)
    horiz_rough = max(rough_extent[0], rough_extent[1]) if rough_extent else 1.6
    scale = horiz_rough / horiz_meshy
    root.scale = (scale, scale, scale)
    bpy.context.view_layer.update()
    mn2, mx2 = world_bounds(objs)
    center = (mn2 + mx2) / 2
    root.location = Vector((-center.x, -center.y, -mn2.z))
    entry['normalizeScale'] = round(scale, 4)
    entry['normalizedHeight'] = round((mx2.z - mn2.z), 4)
    frames = []
    for name, deg in DIRS:
        root.rotation_euler.z = math.radians(deg)
        scene.render.filepath = str(OUT / ('%s.%s.png' % (mid, name)))
        bpy.ops.render.render(write_still=True)
        frames.append(scene.render.filepath)
    entry['frames'] = [Path(f).name for f in frames]
    # stitch strip: rough model 'se' bake reference (from Content/UI/v6/model-bakes) + four views
    bake_dir = PROJECT / 'Content/UI/v6/model-bakes' / mid
    strip_imgs = []
    ref = bake_dir / 'se.png'
    if ref.exists():
        strip_imgs.append(('rough se', ref))
    for f, (name, _) in zip(frames, DIRS):
        strip_imgs.append(('meshy ' + name, Path(f)))
    w = size * len(strip_imgs)
    strip = bpy.data.images.new('strip', w, size, alpha=True)
    pixels = [0.0] * (w * size * 4)
    for i, (label, path) in enumerate(strip_imgs):
        img = bpy.data.images.load(str(path))
        if img.size[0] != size or img.size[1] != size:
            img.scale(size, size)
        px = list(img.pixels)
        for y in range(size):
            row_src = y * size * 4
            row_dst = (y * w + i * size) * 4
            pixels[row_dst:row_dst + size * 4] = px[row_src:row_src + size * 4]
        bpy.data.images.remove(img)
    strip.pixels = pixels
    strip.filepath_raw = str(strip_path)
    strip.file_format = 'PNG'
    strip.save()
    entry['strip'] = strip_path.relative_to(PROJECT).as_posix()
    entry['stripLabels'] = [l for l, _ in strip_imgs]
    index[mid] = entry
    index_path.write_text(json.dumps(index, indent=2, ensure_ascii=False), encoding='utf-8')
    print('ISO_RENDERED ' + mid, flush=True)
print('ISO_DONE', flush=True)
