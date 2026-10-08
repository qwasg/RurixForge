"""Audit a finished V3 package; write ZIP only after final UI acceptance."""
import argparse,hashlib,json,pathlib,zipfile
from engine_client import ROOT,REPO
from v3_baseline import verify,digest,EVIDENCE

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--write-zip',action='store_true');args=parser.parse_args()
    baseline=verify();folder=ROOT/'dist/CodeSentinels-V3-Windows'
    if not folder.is_dir():raise RuntimeError('V3 package does not exist yet')
    build=json.loads((EVIDENCE/'portable-build.json').read_text(encoding='utf8'))
    assert build['clientBundle']in(folder/'Web/index.html').read_text(encoding='utf8')
    assert digest(folder/'bin/engine-host.exe')==baseline['runtimeAndProtocolFiles']['target/debug/engine-host.exe']
    checked=[]
    for relative,expected in baseline['runtimeAndProtocolFiles'].items():
        prefix='projects/code-sentinels/Content/'
        if relative.startswith(prefix):
            target=folder/'Content'/relative[len(prefix):]
            assert digest(target)==expected,relative;checked.append(str(target.relative_to(folder)).replace('\\','/'))
    native_source=digest(folder/'Content/Scripts/sentinels_v2.rs');matches=[]
    for p in(folder/'.forge/cache/rxdll').glob('*.native.json'):
        meta=json.loads(p.read_text(encoding='utf8'))
        if meta.get('sourceSha256')==native_source:
            assert digest(p.parent/meta['dll'])==meta['dllSha256'];matches.append(meta['dll'])
    assert matches
    files=[]
    for p in folder.rglob('*'):
        if not p.is_file():continue
        rel=p.relative_to(folder)
        if rel.parts[0]=='Logs'or rel.parts[:2]==('.forge','save'):continue
        files.append((p,rel))
    report={'version':3,'passed':True,'nativeContentFilesUnchanged':len(checked),'engineUnchanged':True,'oldArchivesUnchanged':True,'matchingNativeModules':matches,'members':len(files),'folderBytesExcludingRuntimeFiles':sum(p.stat().st_size for p,_ in files)}
    if args.write_zip:
        archive=folder.with_suffix('.zip')
        if archive.exists():raise RuntimeError('An existing V3 archive is preserved; no implicit replacement')
        with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED,compresslevel=6)as z:
            for p,rel in files:z.write(p,pathlib.Path(folder.name)/rel)
        with zipfile.ZipFile(archive)as z:
            assert z.testzip()is None
            for p,rel in files:
                name=str(pathlib.Path(folder.name)/rel).replace('\\','/')
                assert hashlib.sha256(z.read(name)).hexdigest()==digest(p),name
            assert not any('/Logs/'in name or '/.forge/save/'in name for name in z.namelist())
        report.update({'zip':str(archive),'zipBytes':archive.stat().st_size,'zipSha256':digest(archive),'allMembersMatchFolder':True,'runtimeLogsAndSavesIncluded':False})
    (EVIDENCE/('archive-integrity.json'if args.write_zip else'package-integrity.json')).write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf8')
    print(json.dumps(report,ensure_ascii=False))
if __name__=='__main__':main()
