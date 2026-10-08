"""File-only counterpart of Forge pack.rs for authoring a portable package.

Used when Windows application control disallows launching the development
service. It never executes a blocked binary or alters application-control rules.
The runtime comes from the already delivered V3 portable engine.
"""
import json
import pathlib
import re
import shutil
from collections import deque

ROOT = pathlib.Path(__file__).resolve().parents[1]
GUID = re.compile(r'^[a-fA-F0-9]{8}(?:-[a-fA-F0-9]{4}){3}-[a-fA-F0-9]{12}$')


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)


def build_pack(scene, output, runtime=None):
    if output.exists():
        raise RuntimeError(f'Package already exists: {output}')
    index = {}
    for meta in (ROOT / 'Content').rglob('*.meta'):
        text = meta.read_text(encoding='utf-8-sig')
        if text.lstrip().startswith('{'):
            guid = json.loads(text).get('guid')
        else:
            match = re.search(r'^guid:\s*["\']?([a-fA-F0-9-]+)', text, re.M)
            guid = match.group(1) if match else None
        if guid:
            index[guid] = meta.relative_to(ROOT).as_posix()[:-5]
    queue = deque([scene.relative_to(ROOT).as_posix()])
    closure = set()
    while queue:
        relative = queue.popleft()
        if relative in closure:
            continue
        source = (ROOT / relative).resolve()
        if not source.is_relative_to(ROOT.resolve() / 'Content'):
            raise RuntimeError(f'Content dependency leaves the project: {relative}')
        if not source.is_file():
            raise FileNotFoundError(f'Missing scene dependency: {relative}')
        closure.add(relative)
        if source.suffix in ['.rxscene', '.rxgraph', '.rxsprite', '.rxmat', '.json']:
            doc = json.loads(source.read_text(encoding='utf-8-sig'))
            for reference in strings(doc):
                if reference.startswith('Content/'):
                    queue.append(reference)
                elif reference in index:
                    queue.append(index[reference])
                elif GUID.fullmatch(reference):
                    raise RuntimeError(f'Unresolved asset GUID {reference} in {relative}')
    files = list(closure)
    for relative in files:
        if (ROOT / (relative + '.meta')).is_file():
            closure.add(relative + '.meta')
    runtime = pathlib.Path(runtime) if runtime is not None else ROOT / 'dist/CodeSentinels-V3-Windows/bin/engine-host.exe'
    if not runtime.is_file():
        raise FileNotFoundError(f'The retained portable runtime is missing: {runtime}')
    cache = ROOT / '.forge/cache/rxdll'
    native = []
    for relative in sorted(closure):
        module = ROOT / relative
        if module.suffix not in ['.rs', '.rx']:
            continue
        binaries = list(cache.glob(module.stem + '-*.dll'))
        if not binaries:
            raise FileNotFoundError(f'Native module has not been compiled: {relative}')
        for binary in binaries:
            native.append(binary)
            manifest = binary.with_suffix('.native.json')
            if manifest.is_file():
                native.append(manifest)
    output.mkdir(parents=True)
    for relative in sorted(closure):
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / relative, destination)
    for file in native:
        destination = output / '.forge/cache/rxdll' / file.name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(file, destination)
    (output / 'bin').mkdir(exist_ok=True)
    shutil.copy2(runtime, output / 'bin/engine-host.exe')
    shutil.copy2(ROOT / 'forge.toml', output / 'forge.toml')
    scene_relative = scene.relative_to(ROOT).as_posix()
    (output / 'pack-run.ps1').write_text(f'''$ErrorActionPreference = 'Stop'
$gameRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$env:FORGE_PROJECT_ROOT = $gameRoot
& "$gameRoot\\bin\\engine-host.exe" --port 17890 --game "{scene_relative}"
''', encoding='utf8')
    return {'outDir': str(output), 'scene': str(scene), 'contentFiles': len(closure), 'nativeFiles': len(native), 'warnings': [],
            'packingMethod': 'file-only scene dependency closure', 'runtimeSource': str(runtime),
            'developmentService': 'Windows application control blocked forge-agentd.exe startup; no control settings changed',
            'gameStarted': False, 'testsRun': False}
