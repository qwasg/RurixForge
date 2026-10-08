"""Build only the approved direct-range candidate after explicit native freeze."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'game/v6/direct-range-probe80-20260913'
OLD=ROOT/'game/v6/escort-two-pilot-20260912'
read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
now=lambda:datetime.datetime.now(datetime.timezone.utc).isoformat()
ref=lambda p:dict(path=p.relative_to(ROOT).as_posix(),sha256=sha(p),bytes=p.stat().st_size)


def write(p,v):
    with p.open('x',encoding='utf-8',newline='\n')as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')


def sources():
    files=[]
    for folder in [ROOT/'native-v6',ROOT/'game/v6/balance-runner']:
        files.extend(folder.joinpath('src').rglob('*.rs'))
        files.extend(folder.joinpath('tests').rglob('*.rs'))
        files.extend(p for p in [folder/'Cargo.toml',folder/'Cargo.lock',folder/'build.rs']if p.exists())
    return sorted([ref(p)for p in files],key=lambda x:x['path'])


def fingerprint():
    base=ROOT/'native-v6';paths=[*base.joinpath('src').rglob('*.rs'),base/'Cargo.toml',base/'Cargo.lock',base/'build.rs']
    digest=hashlib.sha256(b'code-sentinels-native-rules-v1\0')
    for p in sorted(paths,key=lambda p:p.relative_to(base).as_posix()):
        name=p.relative_to(base).as_posix().encode();data=p.read_bytes()
        digest.update(len(name).to_bytes(8,'little'));digest.update(name);digest.update(len(data).to_bytes(8,'little'));digest.update(data)
    return digest.hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expected-fingerprint',required=True)
    parser.add_argument('--native-freeze-evidence',required=True)
    args=parser.parse_args()
    assert fingerprint()==args.expected_fingerprint
    old=read(OLD/'candidate-manifest.json');changed=[]
    old_native={item['path'] for item in old['files']if item['path'].startswith('native-v6/src/')}
    current_native={p.relative_to(ROOT).as_posix()for p in (ROOT/'native-v6/src').rglob('*.rs')}
    assert current_native==old_native,'Unexpected production source inventory change.'
    for item in old['files']:
        path=item['path']
        if path.startswith('native-v6/src/')or path in ['native-v6/Cargo.toml','native-v6/Cargo.lock','native-v6/build.rs']or path.startswith('game/v6/balance-runner/'):
            assert sha(OLD/'source'/path)==item['sha256']
            if sha(ROOT/path)!=item['sha256']:changed.append(path)
    assert changed==['native-v6/src/ballistics.rs'],changed
    before=sources();write(OUT/'source-before-build.json',before)
    target=ROOT/'game/v6/direct-range-probe80-target-20260913'
    command=['cargo','build','--release','--manifest-path',str(ROOT/'game/v6/balance-runner/Cargo.toml'),'--target-dir',str(target),'-j','4']
    invocation=dict(command=command,cwd=str(ROOT),startedAtUtc=now(),rulesFingerprint=args.expected_fingerprint,nativeFreezeEvidence=args.native_freeze_evidence,approvedGameSourceChanges=changed,preRegistration=ref(OUT/'pre-registration.json'),sourceBefore=ref(OUT/'source-before-build.json'))
    write(OUT/'build-invocation.json',invocation)
    with(OUT/'runner-build.stdout.log').open('x',encoding='utf-8')as stdout,(OUT/'runner-build.stderr.log').open('x',encoding='utf-8')as stderr:
        result=subprocess.run(command,cwd=ROOT,stdout=stdout,stderr=stderr,creationflags=subprocess.CREATE_NO_WINDOW)
    after=sources();write(OUT/'source-after-build.json',after)
    receipt=dict(**invocation,endedAtUtc=now(),exitCode=result.returncode,sourceAfter=ref(OUT/'source-after-build.json'),sourceUnchanged=before==after,postBuildRulesFingerprint=fingerprint())
    if result.returncode==0:
        built=target/'release/sentinels-v6-balance-runner.exe';frozen=OUT/'artifact/sentinels-v6-balance-runner.exe';assert not frozen.exists();shutil.copy2(built,frozen);assert sha(built)==sha(frozen);receipt['artifact']=ref(frozen)
    write(OUT/'runner-build-receipt.json',receipt)
    assert result.returncode==0 and before==after and fingerprint()==args.expected_fingerprint
    print(json.dumps(dict(built=True,rulesFingerprint=args.expected_fingerprint,artifact=receipt['artifact'],approvedGameSourceChanges=changed)),flush=True)


if __name__=='__main__':main()
