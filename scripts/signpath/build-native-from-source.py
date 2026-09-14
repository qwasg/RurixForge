"""Build one own-project executable from a clean, reviewed GitHub checkout.

No local executable, prebuilt Web, V5 package or artwork is accepted as an input.
This command is intentionally unavailable outside the approved hosted CI context.
It never launches engine-host, a test executable, a balance match or a GPU scene.
"""
from pathlib import Path
from datetime import datetime, timezone
from urllib.parse import urlparse
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
sys.dont_write_bytecode = True
from pe_signing_content import unsigned_record


def require(condition, message):
    if not condition:
        raise ValueError(message)


def utc():
    return datetime.now(timezone.utc).isoformat()


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def record(path, root):
    return dict(path=path.relative_to(root).as_posix(), sha256=sha(path), bytes=path.stat().st_size)


def write_new(path, value):
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write('\n')


def capture(args, cwd):
    completed = subprocess.run(args, cwd=cwd, check=True, capture_output=True)
    return completed.stdout.decode('utf-8').strip()


def inside(root, relative):
    require(isinstance(relative, str) and relative and not re.match(r'^[A-Za-z]:', relative), 'Machine-specific path input is forbidden.')
    path = (root / relative).resolve()
    require(path.is_relative_to(root), 'Source input leaves the checked-out repository.')
    return path


def metadata_version(version):
    require(isinstance(version, str) and re.fullmatch(r'\d+\.\d+\.\d+', version), 'A reviewed numeric Cargo product version is required.')
    require(all(int(part) <= 65535 for part in version.split('.')), 'Windows version components must fit16 bits.')
    return version + '.0'


def confirmed_company(value):
    company = value.strip()
    require(company and len(company) <= 128 and not any(character in company for character in '\r\n\0"\\')
            and not re.search(r'(?i)(TO_BE_FILLED|PENDING|CHANGEME|REQUIRED_|\bTODO\b|\bTBD\b)', company),
            'Confirmed maintainer/company display name is required; placeholders are forbidden.')
    return company


def native_prerequisites(names, locator=shutil.which):
    found = {name: locator(name) for name in names}
    require(all(found.values()), 'Required normal source-build tools are missing from PATH: ' + ', '.join(name for name, path in found.items() if not path))
    return found


def validate_dependency(value, manifest, root):
    if not isinstance(value, dict):
        return
    if 'path' in value:
        relative = value['path']
        require(isinstance(relative, str) and not re.match(r'^(?:[A-Za-z]:|/|\\)', relative), f'Absolute Cargo dependency in {manifest.relative_to(root)}')
        target = (manifest.parent / relative).resolve()
        require(target.is_relative_to(root) and (target / 'Cargo.toml').is_file(), f'Cargo path dependency escapes/misses repository content in {manifest.relative_to(root)}')
    if 'git' in value:
        parsed = urlparse(value['git'])
        # Do not print malformed URLs: they could contain credentials.
        require(parsed.scheme == 'https' and parsed.hostname == 'github.com' and not parsed.username and not parsed.password
                and not parsed.query and not parsed.fragment, f'Nonpublic/unreviewed Cargo Git source in {manifest.relative_to(root)}')
        require(re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/?', parsed.path.lstrip('/')) is not None,
                f'Unexpected Cargo Git repository path in {manifest.relative_to(root)}')
        require(re.fullmatch(r'[0-9a-f]{40}', value.get('rev', '')) is not None and not any(key in value for key in ['branch','tag']),
                f'Cargo Git dependency is not pinned to a full commit in {manifest.relative_to(root)}')


def inspect_dependencies(document, manifest, root):
    if isinstance(document, dict):
        for key, value in document.items():
            if key in ['dependencies', 'dev-dependencies', 'build-dependencies'] and isinstance(value, dict):
                for dependency in value.values():
                    validate_dependency(dependency, manifest, root)
            elif key == 'patch' and isinstance(value, dict):
                for overrides in value.values():
                    for dependency in overrides.values():
                        validate_dependency(dependency, manifest, root)
            inspect_dependencies(value, manifest, root)
    elif isinstance(document, list):
        for value in document:
            inspect_dependencies(value, manifest, root)


def source_inventory(root, config):
    tracked = set(capture(['git', 'ls-files', '-z'], root).split('\0'))
    tracked.discard('')
    for name in config['requiredFiles']:
        require(name in tracked and inside(root, name).is_file(), f'Required reviewed file is missing/untracked: {name}')
    for name in config['requiredTrackedInputDirectories']:
        require(inside(root, name).is_dir() and any(file.startswith(name + '/') for file in tracked), f'Required vendored build-input directory is missing/untracked: {name}')
    for name in ['.cargo/config', '.cargo/config.toml']:
        require(not (root / name).exists(), 'Additional Cargo configuration must receive an explicit source-policy review before using this recipe.')
    relevant = sorted(name for name in tracked if name in ['Cargo.toml','Cargo.lock','RURIX_PIN.json']
                      or any(name.startswith(prefix + '/') for prefix in config['sourceRoots']))
    # git status alone does not report ignored inputs. Nothing under a source
    # root may silently supply untracked Rust, metadata, shader or build content.
    for prefix in config['sourceRoots']:
        source_root = inside(root, prefix)
        require(source_root.is_dir(), f'Missing source directory: {prefix}')
        for actual in source_root.rglob('*'):
            require(not actual.is_symlink(), f'Source symlink requires explicit review: {actual.relative_to(root)}')
            if actual.is_file():
                name = actual.relative_to(root).as_posix()
                require(name in tracked, f'Ignored/untracked build input is forbidden: {name}')
    for name in relevant:
        path = inside(root, name)
        require(path.is_file() and not (root / name).is_symlink(), f'Source input is not an ordinary tracked file: {name}')
        if path.name == 'Cargo.toml':
            inspect_dependencies(tomllib.loads(path.read_text(encoding='utf-8')), path, root)
    workspace = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))
    for dependency in workspace['workspace']['dependencies'].values():
        if isinstance(dependency, dict) and dependency.get('git') == 'https://github.com/qwasg/Rurix':
            require(dependency.get('rev') == config['expectedRurixRevision'], 'Rurix source pin differs from reviewed configuration.')
    require(workspace['patch']['https://github.com/qwasg/Rurix']['rurix-rt']['path'] == 'vendor/rurix/src/rurix-rt', 'Reviewed repository-local renderer patch is required.')
    vendor = json.loads((root / 'vendor/rurix/FORGE_BLEND_PATCH.json').read_text(encoding='utf-8'))
    require(vendor['upstreamRevision'] == config['expectedRurixRevision'], 'Vendored source provenance and Cargo pin disagree.')
    return [record(inside(root, name), root) for name in relevant]


def rules_fingerprint(root):
    native = root / 'projects/code-sentinels/native-v6'
    digest = hashlib.sha256(b'code-sentinels-native-rules-v1\0')
    paths = [*native.joinpath('src').rglob('*.rs'), native / 'Cargo.toml', native / 'Cargo.lock', native / 'build.rs']
    for path in sorted(paths, key=lambda p: p.relative_to(native).as_posix()):
        name, data = path.relative_to(native).as_posix().encode(), path.read_bytes()
        digest.update(len(name).to_bytes(8, 'little')); digest.update(name)
        digest.update(len(data).to_bytes(8, 'little')); digest.update(data)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo-root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root, output = args.repo_root.resolve(), args.output.resolve()
    require(os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('V6_RUNNER_ENVIRONMENT') == 'github-hosted', 'Only the reviewed GitHub-hosted workflow may build these signing inputs.')
    require(os.environ.get('GITHUB_EVENT_NAME') == 'workflow_dispatch', 'This preparation permits only an explicit manual workflow dispatch.')
    config_path = root / '.signpath/v6-native-inputs.json'
    config = json.loads(config_path.read_text(encoding='utf-8'))
    require(config['sourceReleaseApproved'] is True and os.environ.get('V6_SOURCE_RELEASE_READY') == 'true', 'Public source/rights review is not marked complete.')
    require(os.environ.get('GITHUB_REPOSITORY') == config['expectedRepository'], 'Workflow repository differs from the reviewed project.')
    commit = capture(['git','rev-parse','HEAD'], root)
    require(commit == os.environ.get('GITHUB_SHA') and re.fullmatch(r'[0-9a-f]{40}', commit), 'Checkout does not match the GitHub run commit.')
    require(not capture(['git','status','--porcelain','--untracked-files=all'], root), 'Build must start from a clean checkout without local inputs.')
    company = confirmed_company(os.environ.get('V6_RELEASE_COMPANY_NAME', ''))
    workspace = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))
    version = workspace['workspace']['package']['version']; file_version = metadata_version(version)
    temp = Path(os.environ['RUNNER_TEMP']).resolve()
    require(output.is_relative_to(temp) and output != temp and not output.is_relative_to(root) and not output.exists(), 'Output must be a new child of RUNNER_TEMP, outside the checkout.')
    before = source_inventory(root, config)
    fp = rules_fingerprint(root)
    prerequisites = native_prerequisites(config['nativePrerequisites'])
    output.mkdir()
    unsigned = output / 'unsigned'; (unsigned / 'bin').mkdir(parents=True)
    write_new(unsigned / 'source-manifest.json', before)
    # MSBuild/CMake can still hit MAX_PATH in nested targets. Use a short hosted
    # temp prefix, not the source-candidate/QA directory. This is not a profile or
    # operating-system policy workaround and does not guarantee every path fits.
    target = output / 't'
    command = ['cargo', '+' + config['rustToolchain'], 'build', '--locked', '--release', '-p', config['cargoPackage'], '--bin', config['cargoBinary'],
               '--target', config['targetTriple'], '--target-dir', str(target)]
    environment = os.environ.copy()
    environment.update(FORGE_RELEASE_METADATA='1', FORGE_RELEASE_COMPANY_NAME=company)
    require(not environment.get('RUSTFLAGS') and not environment.get('CARGO_ENCODED_RUSTFLAGS'), 'Unreviewed compiler flags are forbidden.')
    invocation = dict(startedAtUtc=utc(), command=command, commit=commit, repository=config['expectedRepository'],
                      workflowRunId=os.environ.get('GITHUB_RUN_ID'), workflowRunAttempt=os.environ.get('GITHUB_RUN_ATTEMPT'),
                      config=record(config_path, root), rulesFingerprintFromSource=fp,
                      rustVersion=capture(['rustc', '+' + config['rustToolchain'], '--version'], root),
                      cargoVersion=capture(['cargo', '+' + config['rustToolchain'], '--version'], root),
                      hostedImage=dict(imageOS=os.environ.get('ImageOS'), imageVersion=os.environ.get('ImageVersion'),
                                       vcToolsVersion=os.environ.get('VCToolsVersion'), windowsSdkVersion=os.environ.get('WindowsSDKVersion')),
                      nativePrerequisites=prerequisites, cmakeVersion=capture([prerequisites['cmake'], '--version'], root).splitlines()[0],
                      packageVersion=version, fileVersion=file_version, productName=config['productName'],
                      gameRuntimeExecuted=False, externalBinaryInputs=[], artworkInputs=[])
    write_new(output / 'build-invocation.json', invocation)
    with (output / 'cargo.stdout.log').open('x', encoding='utf-8') as stdout, (output / 'cargo.stderr.log').open('x', encoding='utf-8') as stderr:
        process = subprocess.run(command, cwd=root, env=environment, stdout=stdout, stderr=stderr)
    after = source_inventory(root, config)
    write_new(output / 'build-exit.json', dict(endedAtUtc=utc(), exitCode=process.returncode, sourceUnchanged=before == after))
    require(process.returncode == 0 and before == after and not capture(['git','status','--porcelain','--untracked-files=all'], root), 'Normal Cargo build failed or changed source/lock inputs.')
    observed_fingerprints = set()
    for path in target.rglob('output'):
        if path.parent.name.startswith('sentinels-v6-'):
            observed_fingerprints.update(re.findall(r'cargo:rustc-env=SENTINELS_V6_RULES_FINGERPRINT=([0-9a-f]{64})', path.read_text(encoding='utf-8')))
    require(observed_fingerprints == {fp}, 'Cargo native build output does not bind the expected rules source fingerprint.')
    built = target / config['targetTriple'] / 'release/engine-host.exe'
    require(built.is_file(), 'Fresh source-built engine-host artifact is absent.')
    copied = unsigned / 'bin/engine-host.exe'
    shutil.copy2(built, copied)
    require(sha(built) == sha(copied), 'Fresh artifact copy differs.')
    metadata_command = ['pwsh','-NoProfile','-File',str(root / 'scripts/signpath/verify-pe-metadata.ps1'),
                        '-Path',str(copied),'-ProductName',config['productName'],'-ProductVersion',version,
                        '-FileVersion',file_version,'-CompanyName',company]
    subprocess.run(metadata_command, cwd=root, check=True)
    provenance = dict(**invocation, endedAtUtc=utc(), buildExitCode=0, sourceUnchanged=True,
                      sourceManifestSha256=sha(unsigned / 'source-manifest.json'), unsignedEngineSha256=sha(copied),
                      signingContent=unsigned_record(copied.read_bytes()),
                      compiledFingerprintFromCargoOutput=fp, nativeRuntimeIdentityObserved=False,
                      fullGameValidationClaimed=False, signPathApprovalClaimed=False,
                      scope='GitHub source-build provenance for this engine-host only; not a playable package, functional acceptance or local test executable endorsement.')
    write_new(unsigned / 'build-provenance.json', provenance)
    with Path(os.environ['GITHUB_OUTPUT']).open('a', encoding='utf-8') as stream:
        for key, value in dict(product_version=version, file_version=file_version, unsigned_sha256=sha(copied),
                               provenance_sha256=sha(unsigned / 'build-provenance.json')).items():
            stream.write(f'{key}={value}\n')
    print(json.dumps(dict(sourceBuildCompleted=True, artifactDirectory=str(unsigned), rulesFingerprint=fp, gameRuntimeExecuted=False)), flush=True)


if __name__ == '__main__':
    main()
