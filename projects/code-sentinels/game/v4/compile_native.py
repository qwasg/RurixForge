"""Compile the V4 module and emit its required portable cache metadata.
This build step does not load the DLL, run the game, or execute tests.
"""
import hashlib,json,pathlib,subprocess

ROOT=pathlib.Path(__file__).resolve().parents[2]
MODULE='Content/Scripts/sentinels_v4.rs'
source=(ROOT/MODULE).read_bytes()
version=subprocess.run(['rustc','--version','--verbose'],check=True,stdout=subprocess.PIPE).stdout
cache=ROOT/'.forge/cache/rxdll';cache.mkdir(parents=True,exist_ok=True)
key=hashlib.sha256(source+b'|rust-cdylib-v1|edition2021|opt2|panic-abort|'+version).hexdigest()[:16]
dll=cache/f'sentinels_v4-{key}.dll'
subprocess.run(['rustc',str(ROOT/MODULE),'--crate-type','cdylib','--edition','2021','-C','opt-level=2','-C','panic=abort','-o',str(dll)],check=True)
manifest={'backend':'rust-cdylib-v1','module':MODULE,'sourceSha256':hashlib.sha256(source).hexdigest(),'dllSha256':hashlib.sha256(dll.read_bytes()).hexdigest(),'dll':dll.name}
dll.with_suffix('.native.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2),encoding='utf8')
build={'operation':'native compilation only','module':MODULE,'dll':str(dll),'manifest':str(dll.with_suffix('.native.json')),'bytes':dll.stat().st_size,'testsRun':False,'gameStarted':False}
(ROOT/'game/v4/native-build.json').write_text(json.dumps(build,ensure_ascii=False,indent=2),encoding='utf8')
print(json.dumps(build,ensure_ascii=False))
