"""Isolated V6 candidate assembly and evidence-gated archives; old outputs are preserved."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import uuid
import zipfile
from release_gate import ReleaseError, require, digest, sha, read_json, validate_release

ROOT = Path(__file__).resolve().parents[2]
REPO = ROOT.parents[1]
GAME = ROOT / 'game'
MARKER = 'v6-candidate.json'
CRT = ('concrt140.dll','msvcp140.dll','msvcp140_1.dll','msvcp140_2.dll','msvcp140_atomic_wait.dll','msvcp140_codecvt_ids.dll','vccorlib140.dll','vcruntime140.dll','vcruntime140_1.dll','vcruntime140_threads.dll')
LEGACY_SOURCES = ('video-generation-prompts.json','video-frame-inventory.json','vfx-reference-prompts.json','sources.md','sources-gpu.md','reference-manifest.json','PROMPTS.md','legacy-v4-art-prompts.json','legacy-v3-art-prompts.json','hub-art-prompts.json','building-reference-prompts.json')
LEGACY_LICENSES = ('README.md','Microsoft-VC-Runtime.md','Microsoft-VC-Redist.txt','JetBrains-Mono-OFL-1.1.txt','Inter-OFL-1.1.txt')
MANAGED_TOP = {'Content','Web','bin','Licenses','Sources','QA','v6',MARKER,'forge.toml','bridge.mjs','multiplayer-v6.mjs','README.md','Node-LICENSE.txt','Forge-LICENSE.txt','Start-Game.cmd','Start-Game-GPU-Particles.cmd'}
PRIVATE_PARTS = {'.forge','.git','.codex','__pycache__','node_modules','logs','log','saves','save','tokens','credentials','.cache'}
PRIVATE_NAMES = {'token.json','tokens.json','auth.json','credentials.json','session-token.json'}
SECRET_KEYS = {'apikey','api_key','accesstoken','access_token','refreshtoken','refresh_token','authorization','client_secret','password','cookie','ownertoken','guesttoken','sessiontoken','session_token','roomcode','joincode','token'}


def canonical_digest(records):
    return hashlib.sha256(json.dumps(records,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode('utf-8')).hexdigest()


def relative_name(value):
    text=str(value).replace('\\','/')
    require(text and not text.startswith('/') and not re.match(r'^[A-Za-z]:',text) and not any(ord(c)<32 for c in text),'Unsafe package-relative path')
    parts=text.split('/')
    require(all(p not in ('','.','..') for p in parts),'Package traversal/empty component')
    require(not any(p.casefold() in PRIVATE_PARTS for p in parts) and parts[-1].casefold() not in PRIVATE_NAMES,'Runtime tokens/logs/saves/private directories cannot be packaged')
    require(not any(p.casefold().startswith('.env') for p in parts) and not text.lower().endswith(('.log','.pyc','.pdb','.dmp','.bak')),'Environment/debug/log files cannot be packaged')
    return '/'.join(parts)


def under(root,relative):
    root=Path(root).resolve();target=(root/relative_name(relative)).resolve()
    require(target.is_relative_to(root),'Resolved package path escapes target root')
    return target


def check_data_privacy(path):
    if path.suffix.lower() not in ('.json','.jsonl'):return
    text=path.read_text(encoding='utf-8-sig')
    values=[json.loads(line) for line in text.splitlines() if line.strip()] if path.suffix.lower()=='.jsonl' else [json.loads(text)]
    def visit(value):
        if isinstance(value,dict):
            for key,child in value.items():
                if key.casefold() in SECRET_KEYS and child not in (None,'',False,[],{}):
                    raise ReleaseError(f'Sensitive key in distributable {path.name}: {key}; publish a new safe summary and preserve raw evidence')
                visit(child)
        elif isinstance(value,list):
            for child in value:visit(child)
        elif isinstance(value,str):
            require(not re.search(r'(?i)[?&](?:access_token|api_key|auth_token|refresh_token)=([^&\s]+)',value),f'Credential-bearing URL in {path.name}')
    for value in values:visit(value)


class CopyPlan:
    def __init__(self):self.files={};self.case_names={}
    def add(self,source,relative,trusted_root=None,expected_sha=None,replace=False):
        name=relative_name(relative);source=Path(source).resolve()
        require(source.is_file(),f'Missing package input: {source}')
        if trusted_root is not None:require(source.is_relative_to(Path(trusted_root).resolve()),'Source symlink/path escapes explicit input root')
        require(name.casefold() not in self.case_names or self.case_names[name.casefold()]==name,'Case-insensitive duplicate filename')
        require(name not in self.files or replace,f'Duplicate package destination: {name}')
        check_data_privacy(source);actual=sha(source)
        if expected_sha is not None:require(actual==digest(expected_sha,name),f'Evidence changed before copy: {name}')
        self.files[name]={'source':source,'sha256':actual,'bytes':source.stat().st_size};self.case_names[name.casefold()]=name
    def data(self,raw,relative):
        name=relative_name(relative);require(name.casefold() not in self.case_names,'Duplicate generated package path')
        self.files[name]={'data':raw,'sha256':hashlib.sha256(raw).hexdigest(),'bytes':len(raw)};self.case_names[name.casefold()]=name
    def tree(self,source,relative,replace=False):
        source=Path(source).resolve();require(source.is_dir(),f'Missing package directory: {source}')
        for file in sorted(source.rglob('*')):
            if file.is_file():self.add(file,Path(relative)/file.relative_to(source),source,replace=replace)
    def records(self,predicate=lambda _:True):
        return [{'path':name,'bytes':item['bytes'],'sha256':item['sha256']} for name,item in sorted(self.files.items()) if predicate(name)]


def runtime_path(name):
    return name.startswith(('bin/','Content/','Web/','v6/')) or name in {'bridge.mjs','multiplayer-v6.mjs','forge.toml','Start-Game.cmd','Start-Game-GPU-Particles.cmd'}


def media_checks(project,media):
    require(set(media.get('characters',{}))=={'deepseek','gpt','claude','gemini','kimi','minimax','glm'},'Seven reviewed V6 operators required')
    for name,item in media['characters'].items():
        require(item.get('ready') is True,f'Unfinished operator asset: {name}')
        atlas=under(project,item.get('nativeAtlas',''));metadata=under(project,item.get('nativeMetadata',''));doc=read_json(metadata)
        require(sha(atlas)==item.get('atlasSha256'),f'Atlas hash mismatch: {name}')
        require(doc.get('frameCount')==512 and len(doc.get('boxes',[]))==512 and doc.get('preservePixelDensity') is True,f'Incomplete frame/density contract: {name}')
        require(all(len(doc.get('clips',{}).get(a,{}))==8 for a in ('idle','walk','attack','cast','hit','death')),f'Missing action directions: {name}')
    basic=read_json(project/'Content/UI/v6/qa/media-verification.json');native=read_json(project/'Content/UI/v6/qa/native-lifecycle-verification.json')
    require(basic.get('assetVerificationPass') is True and basic.get('issues')==[],'Full offline media/source verification required')
    require(native.get('pass') is True and native.get('characters',{}).get('actualAtlasFramesCovered')==3584 and native.get('effects',{}).get('uniqueActualSourceFramesCovered')==656 and native.get('foregroundOcclusion',{}).get('pass') is True,'Latest complete native media lifecycle evidence required')
    for root,entries in [('characters',native.get('characterAssets',[])),('effects',native.get('effectAssets',[]))]:
        require(len(entries)==(7 if root=='characters' else 15),'Native lifecycle source inventory incomplete')
        for item in entries:
            for ext,key in [('png','atlasSha256'),('json','metadataSha256')]:require(sha(project/f'Content/Animations/v6/{root}/{item["id"]}.{ext}')==item.get(key),'Media changed since native lifecycle QA')
    return native


def find_build_receipt(engine_hash,explicit=None):
    matched=[]
    for file in ([Path(explicit)] if explicit else sorted((GAME/'v6').glob('*.json'))):
        try:doc=read_json(file)
        except (ValueError,OSError):continue
        if not isinstance(doc,dict):continue
        artifact=doc.get('artifact');source=doc.get('source')
        if isinstance(artifact,dict) and isinstance(source,dict) and str(artifact.get('sha256','')).lower()==engine_hash and source.get('unchangedDuringBuild') is True:matched.append((file,doc))
    require(matched,'Exact-engine stable native build receipt required; use --build-receipt')
    require(len({(d.get('rulesVersion'),d.get('rulesFingerprint')) for _,d in matched})==1,'Conflicting identities for the same engine')
    return matched[-1]


def make_plan(engine,web,baseline,build_receipt):
    plan=CopyPlan();media_path=ROOT/'Content/UI/v6/resource-manifest.json';media=read_json(media_path);native_media=media_checks(ROOT,media)
    plan.tree(baseline/'Content','Content');roots=media.get('assetRoots');require(isinstance(roots,list) and roots,'Explicit native asset roots required')
    for relative in roots:
        source=under(ROOT,relative);require(source.is_relative_to((ROOT/'Content').resolve()),'Asset root outside Content');plan.tree(source,relative,replace=True)
    require((web/'index.html').is_file(),'Compiled standalone Web/index.html required');plan.tree(web,'Web')
    for name,item in media['characters'].items():
        for public,native in [('atlas','nativeAtlas'),('metadata','nativeMetadata')]:require(plan.files.get('Web/'+item[public].lstrip('/'),{}).get('sha256')==sha(ROOT/item[native]),f'Stale Web operator asset: {name}')
    require(plan.files.get('Web/games/code-sentinels/ui-v6/resource-manifest.json',{}).get('sha256')==sha(media_path),'Web media manifest is stale')
    qa_root=ROOT/'Content/UI/v6/qa'
    for file in qa_root.rglob('*'):
        if file.is_file():require(plan.files.get('Web/games/code-sentinels/ui-v6/qa/'+file.relative_to(qa_root).as_posix(),{}).get('sha256')==sha(file),'Web build predates current complete media QA')
    plan.add(engine,'bin/engine-host.exe',engine.parent);crt=engine.parent if (engine.parent/CRT[0]).is_file() else GAME/'v5/runtime-bin'
    for name in CRT:plan.add(crt/name,'bin/'+name,crt)
    for source,name in [(baseline/'bin/node.exe','bin/node.exe'),(baseline/'forge.toml','forge.toml')]:plan.add(source,name,baseline)
    for source,name in [('portable-bridge-v6.mjs','bridge.mjs'),('multiplayer-v6.mjs','multiplayer-v6.mjs'),('v6/native-rpc.mjs','v6/native-rpc.mjs'),('v6/session-controller.mjs','v6/session-controller.mjs')]:plan.add(GAME/source,name,GAME)
    for name in ('Node-LICENSE.txt','Forge-LICENSE.txt'):plan.add(baseline/name,name,baseline)
    for name in LEGACY_LICENSES:plan.add(baseline/'Licenses'/name,'Licenses/'+name,baseline)
    for name in LEGACY_SOURCES:plan.add(baseline/'Sources'/name,'Sources/'+name,baseline)
    plan.add(media_path,'Sources/v6-resource-manifest.json',ROOT)
    refs_root=ROOT/'references/v6';refs=read_json(refs_root/'sources.json')
    require(isinstance(refs,list) and len(refs)==5 and {r.get('id') for r in refs}=={'claude','glm','minimax','kimi','gemini'},'Five reviewed original-reference records required')
    plan.add(refs_root/'sources.json','Sources/references/v6/sources.json',refs_root)
    for ref in refs:
        name=ref.get('file','');require(Path(name).name==name and Path(name).suffix.lower()=='.png' and ref.get('source') and ref.get('page') and ref.get('repositoryCommitObserved'),'Original author/page/version provenance required')
        plan.add(refs_root/name,'Sources/references/v6/'+name,refs_root,expected_sha=ref.get('sha256'))
    plan.tree(qa_root,'QA/media');plan.add(qa_root/'media-verification.json','QA/media-verification.json',ROOT);plan.add(build_receipt,'QA/native-build-receipt.json',ROOT)
    for name in ('protocol-tests.json','controller-tests.json','collection-delta-tests.json','reconnect-bootstrap-tests.json','save-identity-tests.json'):
        if (GAME/'v6'/name).is_file():plan.add(GAME/'v6'/name,'QA/development/'+name,ROOT)
    if (ROOT/'qa/v6/client').is_dir():
        for file in sorted((ROOT/'qa/v6/client').rglob('*')):
            if file.is_file() and file.suffix.lower() in ('.json','.md','.png'):plan.add(file,Path('QA/client')/file.relative_to(ROOT/'qa/v6/client'),ROOT)
    plan.add(GAME/'v6/README.md','README.md',GAME);plan.add(ROOT/'native-v6/CONTRACT.md','Sources/V6-CONTRACT.md',ROOT)
    for file in ('RELEASE-ACCEPTANCE-SCHEMA.md','strategy-plan.json'):plan.add(GAME/'v6'/file,'Sources/'+file,GAME)
    for name,flags,title in [('Start-Game.cmd','','Code Sentinels V6'),('Start-Game-GPU-Particles.cmd',' --gpu-particles','Code Sentinels V6 GPU Particles')]:plan.data((f'@echo off\r\ntitle {title}\r\n"%~dp0bin\\node.exe" "%~dp0bridge.mjs"{flags}\r\nif errorlevel 1 pause\r\n').encode('ascii'),name)
    return plan,media,native_media


def verify_tree(root,marker):
    records=marker.get('files',[])
    require(marker.get('included')==sorted(set(marker.get('included',[]))),'Included manifest is not unique/sorted')
    require(marker['included']==sorted([r['path'] for r in records]+[MARKER]),'Final QA files/marker missing from included manifest')
    require(marker.get('manifestSha256')==canonical_digest(records),'Content manifest digest mismatch')
    for record in records:
        file=under(root,record['path']);require(file.is_file() and file.stat().st_size==record['bytes'] and sha(file)==record['sha256'],'Packaged file differs: '+record['path'])
    managed_files=set()
    for name in MANAGED_TOP:
        entry=Path(root)/name
        if entry.is_file():managed_files.add(name)
        elif entry.is_dir():managed_files.update(f.relative_to(root).as_posix() for f in entry.rglob('*') if f.is_file())
    require(managed_files==set(marker['included']),'Unexpected stale/unlisted file in managed package contents')
    require(read_json(under(root,MARKER))==marker,'On-disk final/candidate marker mismatch')


def install_stage(stage,out,refresh):
    parent=out.parent.resolve();require(stage.resolve().is_relative_to(parent) and out.resolve().is_relative_to(parent),'Invalid stage/output parent')
    old=[]
    if out.exists():
        require(refresh and (out/MARKER).is_file(),'Existing output preserved; explicit candidate refresh required');m=read_json(out/MARKER)
        require(m.get('version')==6 and m.get('candidate') is True,'Released/unknown output preserved; choose a new release parent')
        old=[n for n in MANAGED_TOP if (out/n).exists()]
    out.mkdir(parents=True,exist_ok=True);backup=parent/'.v6-candidate-history'/(datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')+'-'+uuid.uuid4().hex[:8])/out.name;old_moved=[];installed=[]
    try:
        for name in sorted(old):
            source=(out/name).resolve();destination=(backup/name).resolve();require(source.is_relative_to(out) and destination.is_relative_to(parent),'Unsafe history move')
            destination.parent.mkdir(parents=True,exist_ok=True);shutil.move(str(source),str(destination));old_moved.append(name)
        for source in sorted(stage.iterdir()):
            require(source.name in MANAGED_TOP and source.resolve().is_relative_to(stage.resolve()),'Unexpected staged top-level path');destination=out/source.name
            require(not destination.exists() and destination.resolve().is_relative_to(out),'Managed output collision');shutil.move(str(source),str(destination));installed.append(source.name)
    except Exception:
        for name in reversed(installed):
            source=out/name;destination=stage/name
            if source.exists() and not destination.exists():shutil.move(str(source),str(destination))
        for name in reversed(old_moved):
            source=backup/name;destination=out/name
            if source.exists() and not destination.exists():shutil.move(str(source),str(destination))
        raise
    return str(backup) if old_moved else None


def verify_archive(archive,root_name,marker):
    with zipfile.ZipFile(archive) as package:
        names=package.namelist();expected={root_name+'/'+name for name in marker['included']}
        require(len(names)==len(set(names)) and set(names)==expected,'ZIP is not the exact frozen allowlist');require(package.testzip() is None,'ZIP CRC failed')
        packed=json.loads(package.read(root_name+'/'+MARKER));require(packed==marker and packed['candidate'] is False,'ZIP marker is stale or not final')
        for record in marker['files']:
            data=package.read(root_name+'/'+record['path']);require(len(data)==record['bytes'] and hashlib.sha256(data).hexdigest()==record['sha256'],'ZIP content hash mismatch: '+record['path'])


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine',type=Path,default=GAME/'v6/runtime-bin/engine-host.exe');parser.add_argument('--web',type=Path,required=True)
    parser.add_argument('--out',type=Path,default=ROOT/'dist/CodeSentinels-V6-Windows');parser.add_argument('--baseline',type=Path,default=ROOT/'dist/CodeSentinels-V5-Windows')
    parser.add_argument('--build-receipt',type=Path);parser.add_argument('--no-zip',action='store_true');parser.add_argument('--refresh-candidate',action='store_true')
    parser.add_argument('--acceptance',type=Path,default=GAME/'v6/acceptance.json');parser.add_argument('--evidence-root',type=Path,default=ROOT)
    args=parser.parse_args();out=args.out.resolve();baseline=args.baseline.resolve();engine=args.engine.resolve();web=args.web.resolve()
    require(out.name=='CodeSentinels-V6-Windows' and not out.is_relative_to(baseline) and not baseline.is_relative_to(out),'V6 output must be separate from V5')
    require(not engine.is_relative_to(out) and not web.is_relative_to(out),'Candidate cannot be its own engine/Web input')
    if out.exists():require(args.refresh_candidate and (out/MARKER).is_file() and read_json(out/MARKER).get('candidate') is True,'Existing/final output preserved; refresh a marked candidate or choose a fresh parent')
    require(engine.is_file(),'Native engine input missing');engine_hash=sha(engine);build_path,build=find_build_receipt(engine_hash,args.build_receipt)
    plan,media,native_media=make_plan(engine,web,baseline,build_path)
    target={'engineSha256':engine_hash,'rulesVersion':build.get('rulesVersion'),'rulesFingerprint':str(build.get('rulesFingerprint','')).lower(),'payloadSha256':canonical_digest(plan.records(runtime_path)),'webManifestSha256':canonical_digest(plan.records(lambda p:p.startswith('Web/'))),'mediaManifestSha256':sha(ROOT/'Content/UI/v6/resource-manifest.json')}
    target['rulesFingerprint']=digest(target['rulesFingerprint'],'candidate rules fingerprint')
    require(isinstance(target['rulesVersion'],str) and target['rulesVersion'].strip(),'Candidate compiled rulesVersion required')
    gate=None;acceptance_hash=None
    if not args.no_zip:
        require(args.acceptance.is_file(),'No final acceptance evidence: use --no-zip for a candidate');acceptance_hash=sha(args.acceptance);gate=validate_release(args.acceptance,target,args.evidence_root,REPO,out)
        plan.add(args.acceptance,'QA/acceptance.json',args.evidence_root,expected_sha=acceptance_hash)
        evidence_index=[]
        for label,ref in sorted(gate.store.references.items()):
            packed=None
            if ref['publish'] and ref['path'].suffix.lower() in ('.json','.jsonl','.md','.txt','.png'):
                packed='QA/final-evidence/'+label+ref['path'].suffix.lower();plan.add(ref['path'],packed,args.evidence_root,expected_sha=ref['sha256'])
            evidence_index.append({'label':label,'originalPath':str(ref['path']),'sha256':ref['sha256'],'packagedPath':packed,'omissionReason':None if packed else 'Execution logs/runner binaries remain in developer receipts; not part of distributable runtime'})
        plan.data(json.dumps({'schemaVersion':1,'scope':'Original evidence bytes retained; this index resolves their original reference paths inside the distribution.', 'evidence':evidence_index},ensure_ascii=False,indent=2).encode('utf-8'),'QA/evidence-index.json')
        plan.data(json.dumps({'scope':'Derived facts from verified source evidence, not a generated acceptance decision',**gate.facts},ensure_ascii=False,indent=2).encode('utf-8'),'QA/release-verification.json')
    records=plan.records();marker={'schemaVersion':2,'version':6,'createdAt':datetime.now(timezone.utc).isoformat(),'candidate':args.no_zip,'packageName':out.name,'target':target,**target,'engineBytes':engine.stat().st_size,'mediaReady':True,'mediaLifecycleTestedEngineSha256':native_media['engineSha256'],'files':records,'included':sorted(list(plan.files)+[MARKER]),'manifestSha256':canonical_digest(records),'manifestOwnHashExcluded':[MARKER],'webFiles':sum(p.startswith('Web/') for p in plan.files),'acceptanceStatus':'candidate assembly only; no final release claim' if args.no_zip else 'independent final evidence verified','acceptanceSha256':acceptance_hash}
    stage=out.parent/('.v6-staging-'+uuid.uuid4().hex);require(stage.resolve().is_relative_to(out.parent.resolve()),'Stage escaped parent');stage.mkdir(parents=True)
    for name,item in plan.files.items():
        destination=under(stage,name);destination.parent.mkdir(parents=True,exist_ok=True)
        if 'source' in item:shutil.copy2(item['source'],destination)
        else:destination.write_bytes(item['data'])
        require(destination.stat().st_size==item['bytes'] and sha(destination)==item['sha256'],'Source changed during staging: '+name)
    (stage/MARKER).write_text(json.dumps(marker,ensure_ascii=False,indent=2),encoding='utf-8');verify_tree(stage,marker)
    if gate:
        require(sha(args.acceptance)==acceptance_hash,'Acceptance decision changed during staging')
        validate_release(args.acceptance,target,args.evidence_root,REPO,out)
    archive=out.with_suffix('.zip')
    if gate:require(not archive.exists(),'Existing archive preserved; choose a new release parent')
    history=install_stage(stage,out,args.refresh_candidate);verify_tree(out,marker)
    result={**marker,'outDir':str(out),'previousCandidateHistory':history,'stagePreservedAt':str(stage)}
    if gate:
        temporary=out.parent/('.v6-archive-'+uuid.uuid4().hex+'.pending.zip')
        with zipfile.ZipFile(temporary,'x',zipfile.ZIP_DEFLATED,compresslevel=6) as package:
            for name in marker['included']:package.write(under(out,name),out.name+'/'+name)
        verify_archive(temporary,out.name,marker);require(not archive.exists(),'Archive appeared concurrently; verified pending archive preserved')
        if os.name=='nt':os.rename(temporary,archive)
        else:os.link(temporary,archive);temporary.unlink()
        result.update(zip=str(archive),zipBytes=archive.stat().st_size,zipSha256=sha(archive),archiveFiles=len(marker['included']),archiveCrcPassed=True)
    receipt=out.parent/(out.name+'-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')+'-'+uuid.uuid4().hex[:8]+'-build.json');receipt.write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
    (GAME/'v6/portable-build.json').write_text(json.dumps({**result,'immutableBuildReceipt':str(receipt)},ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps({'candidate':args.no_zip,'outDir':str(out),'files':len(marker['included']),'payloadSha256':target['payloadSha256'],'buildReceipt':str(receipt),'zip':result.get('zip')},ensure_ascii=False))
