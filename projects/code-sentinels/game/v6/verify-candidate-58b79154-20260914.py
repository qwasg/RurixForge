"""File-only audit of the new workshop candidate. Never launches native code."""
from pathlib import Path
from datetime import datetime, timezone
import json
import struct

import release_gate as gate
import portable_release as portable

PROJECT=Path(__file__).resolve().parents[2]
CANDIDATE=PROJECT/'dist/final-candidate-58b79154-20260914/CodeSentinels-V6-Windows'
RUN=PROJECT/'game/v6/candidate-58b79154-20260914-a'
OUTPUT=RUN/'exact-candidate-verification.json'
BUILD=PROJECT/'game/v6/native-workshop-58b79154-20260914-a/native-build-receipt.json'
IDENTITY=PROJECT/'Logs/v6/native-workshop-58b79154-20260914-a-host-identity/host-identity.json'
WEB_RECEIPT=PROJECT/'game/v6/workshop-stock-ui-20260914-b/build-receipt.json'

def ref(path):
    return {'path':path.relative_to(PROJECT).as_posix(),'sha256':gate.sha(path)}

marker=gate.read_json(CANDIDATE/'v6-candidate.json')
target=marker['target']
assert marker['candidate'] is True and marker['acceptanceSha256'] is None
portable.verify_tree(CANDIDATE,marker)
actual_names={p.relative_to(CANDIDATE).as_posix() for p in CANDIDATE.rglob('*') if p.is_file()}
assert actual_names==set(marker['included'])
assert portable.canonical_digest([r for r in marker['files'] if portable.runtime_path(r['path'])])==target['payloadSha256']
assert portable.canonical_digest([r for r in marker['files'] if r['path'].startswith('Web/')])==target['webManifestSha256']
assert gate.sha(CANDIDATE/'Content/UI/v6/resource-manifest.json')==target['mediaManifestSha256']
web=gate.read_json(WEB_RECEIPT)
assert web['webManifestSha256']==target['webManifestSha256']
assert gate.sha(Path(web['entryScript']['path']))==web['entryScript']['sha256']
assert [r for r in marker['files'] if r['path'].startswith('Web/')]==gate.read_json(Path(web['fileManifest']['path']))
source_mirror=gate.read_json(RUN/'source-mirror-verification.json')
assert source_mirror['fileVerificationPassed'] is True and source_mirror['issues']==[]
store=gate.EvidenceStore(PROJECT)
gate.validate_host(gate.read_json(IDENTITY),target,store)
gate.validate_build(gate.read_json(BUILD),target,store,PROJECT.parents[1])
assert gate.sha(CANDIDATE/'bin/engine-host.exe')==target['engineSha256']

records={r['path']:r for r in marker['files']}
index_name='Content/Animations/v6/runtime-frames/index.json'
index=gate.read_json(CANDIDATE/index_name)
assert gate.sha(CANDIDATE/index_name)==gate.sha(PROJECT/index_name)
pages=set(); counts={'characters':0,'effects':0}
for atlas,entry in index['atlases'].items():
    assert records[atlas]['sha256']==entry['sourceAtlasSha256']
    assert records[entry['metadata']]['sha256']==entry['sourceMetadataSha256']
    assert records[atlas]['bytes']==entry['sourceAtlasBytes']
    assert records[entry['metadata']]['bytes']==entry['sourceMetadataBytes']
    meta=gate.read_json(CANDIDATE/entry['metadata'])
    assert len(entry['frames'])==entry['frameCount']==meta['frameCount']
    for ordinal,frame in enumerate(entry['frames']):
        assert ordinal==frame['index'] and frame['bbox']==meta['boxes'][ordinal]
        name=frame['path'];assert name not in pages;pages.add(name)
        assert records[name]['sha256']==frame['sha256']
        assert frame['tileSize'] in (256,512)
        with (CANDIDATE/name).open('rb') as stream:
            header=stream.read(24)
        assert header[:8]==b'\x89PNG\r\n\x1a\n'
        assert struct.unpack('>II',header[16:24])==(frame['tileSize'],frame['tileSize'])
        counts['characters' if '/characters/' in atlas else 'effects']+=1
assert len(index['atlases'])==23 and len(pages)==4288
assert counts=={'characters':3584,'effects':704}

source_mappings={f'Sources/{name}':PROJECT/'dist/CodeSentinels-V5-Windows/Sources'/name for name in portable.LEGACY_SOURCES}
source_mappings.update({'Sources/V6-CONTRACT.md':PROJECT/'native-v6/CONTRACT.md',
    'Sources/RELEASE-ACCEPTANCE-SCHEMA.md':PROJECT/'game/v6/RELEASE-ACCEPTANCE-SCHEMA.md',
    'Sources/strategy-plan.json':PROJECT/'game/v6/strategy-plan.json',
    'Sources/v6-resource-manifest.json':PROJECT/'Content/UI/v6/resource-manifest.json'})
for file in (CANDIDATE/'Sources/references/v6').iterdir():
    source_mappings[file.relative_to(CANDIDATE).as_posix()]=PROJECT/'references/v6'/file.name
assert set(source_mappings)=={p for p in actual_names if p.startswith('Sources/')}
for name,source in source_mappings.items():assert records[name]['sha256']==gate.sha(source)
prior=gate.read_json(PROJECT/'game/v6/native-workshop-58b79154-20260914-a/build-preflight.json')
assert gate.sha(PROJECT/'game/v6/runtime-bin/engine-host.exe')==prior['genericRuntimeSha256']
assert gate.sha(PROJECT/'game/v6/native-room-d33999c4-20260914-a/runtime/engine-host.exe')==prior['oldNativeSha256']
assert gate.sha(PROJECT/'dist/final-candidate-d33999c4-20260914/CodeSentinels-V6-Windows/v6-candidate.json')==prior['oldCandidateMarkerSha256']
assert not CANDIDATE.with_suffix('.zip').exists()
report={
    'kind':'candidate-exact-file-runtime-page-source-verification',
    'generatedAtUtc':datetime.now(timezone.utc).isoformat(),'passed':True,'finalEligible':False,**target,
    'candidate':str(CANDIDATE),'manifestFiles':len(marker['included']),
    'individuallyHashedFiles':len(marker['files']),'exactWholeDirectoryAllowlist':True,
    'runtimeFileCount':sum(portable.runtime_path(r['path']) for r in marker['files']),
    'runtimePayloadRehashed':True,'sourceMirrorComparisons':len(source_mirror['hashComparisons']),
    'sourcesProvenanceFiles':len(source_mappings),'sourcesAllMatchCurrentInputs':True,
    'runtimePages':{'registeredAtlases':23,'characterFrames':3584,'effectFrames':704,'pageCount':4288,
                    'allHashesSourceReferencesAndPngDimensionsMatch':True,'index':ref(CANDIDATE/index_name),
                    'pixelDecodeScope':'Existing registered page bytes exactly match the unchanged source index and its SHA records. No new RGBA equivalence/render run is claimed by this file-only audit.'},
    'compiledSourceFiles':175,'buildAndActualCatalogueVerified':True,
    'latestWorkshopContractSha256':records['Sources/V6-CONTRACT.md']['sha256'],
    'webEntryScript':'Web/assets/'+Path(web['entryScript']['path']).name,
    'frontendReceipt':ref(WEB_RECEIPT),'genericRuntimePreservedSha256':prior['genericRuntimeSha256'],
    'previousHostAndCandidatePreserved':True,'candidateNativeExecutionPerformed':False,
    'coldStartPerformed':False,'nativeUiPerformed':False,'pressurePerformed':False,'lanPerformed':False,
    'completeFunctionalityAccepted':False,'balanceAccepted':False,'finalApprovalCreated':False,'zipPublished':False,
    'evidence':[ref(CANDIDATE/'v6-candidate.json'),ref(BUILD),ref(IDENTITY),ref(RUN/'source-mirror-verification.json'),
                ref(RUN/'assembly-process.json'),ref(WEB_RECEIPT),ref(Path(__file__))],
    'scope':'New H24fd/R58b candidate file assembly and verification only. No native, browser, GPU, cold-start, pressure or LAN process was launched for this candidate. Original V5/older candidates/runtime-bin remain preserved; full functionality, balance and final release are separate unfinished decisions.'
}
with OUTPUT.open('x',encoding='utf-8') as stream:
    json.dump(report,stream,indent=2,ensure_ascii=False);stream.write('\n')
print(json.dumps({'path':str(OUTPUT),'sha256':gate.sha(OUTPUT),'passed':True,'target':target,
                  'files':len(marker['included']),'sourceMirrors':len(source_mirror['hashComparisons']),'compiledSources':175,'pages':4288}))
