"""Read-only gameplay/runtime baseline for the V3 UI-only release."""
import hashlib,json,pathlib,datetime
from engine_client import ROOT,REPO

EVIDENCE=ROOT/'game/v3'
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def capture():
    EVIDENCE.mkdir(parents=True,exist_ok=True)
    output=EVIDENCE/'baseline.json'
    if output.exists():raise RuntimeError('V3 baseline already exists; refusing to overwrite historical evidence')
    native=ROOT/'dist/CodeSentinels-V2-Windows'
    protected=[p for p in (native/'Content').rglob('*')if p.is_file()]
    current_files={}
    v2_comparison=[]
    for previous in protected:
        relative=previous.relative_to(native)
        current=ROOT/relative
        if not current.exists():raise FileNotFoundError(current)
        checksum=digest(current);current_files[str(current.relative_to(REPO)).replace('\\','/')]=checksum
        v2_comparison.append({'path':str(relative).replace('\\','/'),'matchesV2':checksum==digest(previous)})
    for folder in ['crates/engine-host/src','crates/forge-logic/src']:
        for p in (REPO/folder).glob('*.rs'):current_files[str(p.relative_to(REPO)).replace('\\','/')]=digest(p)
    for filename in ['target/debug/engine-host.exe','projects/code-sentinels/forge.toml','packages/client/src/lib/sentinelsV2.ts','packages/client/src/lib/viewportStream.ts']:
        p=REPO/filename;current_files[filename]=digest(p)
    archives={}
    for version in ['CodeSentinels-Windows.zip','CodeSentinels-V2-Windows.zip']:
        p=ROOT/'dist'/version
        if p.exists():archives[version]={'sha256':digest(p),'bytes':p.stat().st_size}
    report={'version':3,'capturedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'UI-only baseline; no running services or user saves inspected/modified','runtimeAndProtocolFiles':current_files,'currentNativeContentMatchesV2':all(x['matchesV2']for x in v2_comparison),'nativeContentComparison':v2_comparison,'currentEngineMatchesV2':digest(REPO/'target/debug/engine-host.exe')==digest(native/'bin/engine-host.exe'),'preservedArchives':archives}
    output.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf8')
    print(json.dumps({'baseline':str(output),'files':len(current_files),'nativeContentMatchesV2':report['currentNativeContentMatchesV2'],'engineMatchesV2':report['currentEngineMatchesV2']},ensure_ascii=False))
    return report
def verify():
    original=json.loads((EVIDENCE/'baseline.json').read_text(encoding='utf8'))
    changes=[relative for relative,expected in original['runtimeAndProtocolFiles'].items()if not(REPO/relative).exists()or digest(REPO/relative)!=expected]
    archive_changes=[name for name,expected in original['preservedArchives'].items()if digest(ROOT/'dist'/name)!=expected['sha256']]
    if changes or archive_changes:raise RuntimeError(f'UI-only baseline changed: runtime={changes}, oldArchives={archive_changes}')
    return original
if __name__=='__main__':capture()
