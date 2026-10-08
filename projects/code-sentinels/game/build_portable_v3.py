"""V3 UI-only portable builder. Run only after the root confirms final dist."""
import argparse,hashlib,json,pathlib,re,shutil,urllib.request
from engine_client import ROOT,REPO
from v3_baseline import verify,digest,EVIDENCE

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--confirmed-client',required=True,help='Exact final Vite JS bundle filename explicitly confirmed by the root task')
    args=parser.parse_args()
    if not re.fullmatch(r'index-[A-Za-z0-9_-]+\.js',args.confirmed_client):raise ValueError('Expected an exact index-<hash>.js filename')
    baseline=verify()
    dist=REPO/'packages/client/dist';index=(dist/'index.html').read_text(encoding='utf8')
    if f'/assets/{args.confirmed_client}'not in index:raise RuntimeError('Confirmed final bundle is not the current dist; do not package an older UI')
    out=ROOT/'dist/CodeSentinels-V3-Windows'
    if out.exists():raise RuntimeError(f'Output already exists; preserving it: {out}')
    request=urllib.request.Request('http://127.0.0.1:8103/api/forge/project/pack',data=json.dumps({'sceneRef':str(ROOT/'Content/Scenes/Main.rxscene'),'outDir':str(out)}).encode(),headers={'content-type':'application/json'})
    with urllib.request.urlopen(request,timeout=90)as response:report=json.load(response)
    if report.get('warnings'):raise RuntimeError(report['warnings'])
    assert (out/'Content/Scripts/sentinels_v2.rs').is_file()
    assert (out/'Content/Graphs/V2BatchController.rxgraph').is_file()
    source_hash=digest(out/'Content/Scripts/sentinels_v2.rs')
    matches=[]
    for p in(out/'.forge/cache/rxdll').glob('*.native.json'):
        meta=json.loads(p.read_text(encoding='utf8'))
        if meta.get('sourceSha256')==source_hash:
            assert digest(p.parent/meta['dll'])==meta['dllSha256'];matches.append(meta['dll'])
    assert matches,'Matching native source/DLL manifest was not packed'
    assert digest(out/'bin/engine-host.exe')==baseline['runtimeAndProtocolFiles']['target/debug/engine-host.exe']
    shutil.copytree(dist,out/'Web')
    node=pathlib.Path.home()/'.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin/node.exe'
    shutil.copy2(node,out/'bin/node.exe')
    # The V2 game transport/command whitelist stays intact; V3 adds UI MIME types.
    bridge=(ROOT/'game/portable-bridge-v2.mjs').read_text(encoding='utf8').replace('Code Sentinels V2 is ready:','Code Sentinels V3 is ready:')
    bridge=bridge.replace("'.svg': 'image/svg+xml'", "'.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.avif': 'image/avif', '.gif': 'image/gif', '.woff': 'font/woff', '.ttf': 'font/ttf', '.svg': 'image/svg+xml'")
    (out/'bridge.mjs').write_text(bridge,encoding='utf8')
    shutil.copy2(ROOT/'game/vendor/Node-LICENSE.txt',out/'Node-LICENSE.txt');shutil.copy2(REPO/'LICENSE',out/'Forge-LICENSE.txt')
    for name,flags,label in [('Start-Game.cmd','','Code Sentinels V3'),('Start-Game-GPU-Particles.cmd',' --gpu-particles','Code Sentinels V3 - GPU Particles')]:
        (out/name).write_bytes(f'@echo off\r\ntitle {label}\r\n"%~dp0bin\\node.exe" "%~dp0bridge.mjs"{flags}\r\nif errorlevel 1 pause\r\n'.encode('ascii'))
    (out/'README.txt').write_text('''CODE SENTINELS V3 / 编译防线：卡牌界面版

双击 Start-Game.cmd 启动常规版（实验GPU粒子明确关闭）。
双击 Start-Game-GPU-Particles.cmd 启动实验版（实验GPU粒子明确开启）。
两个入口都保留真实图生视频角色/技能动画。战斗仍由V2原生引擎运行；V3重制卡牌与界面，没有改变显卡经济、寻路、技能或三关战役。

先在基地购买显卡，再部署防御单元。只有显卡生产算力，普攻和技能消耗算力；建设经費用于购买和升级。
卡牌使用实际原生状态；购买、升级、施法是否成功以游戏反馈为准。
具体操作见游戏内手册。显卡成本和产量是游戏平衡值，不是实际硬件价格或基准测试。

无需安装Node、Python、Rust或RurixForge，不需账号或API密钥。仅监听本机127.0.0.1随机端口。
关闭启动窗口可停止对应游戏实例。日志在Logs，战役解锁存在.forge/save/code-sentinels-v2.txt。
存档保留战区解锁，不保存进行中的布置和波次；重开会建立新局。
来源与署名见游戏档案。本包不包含测试存档或开发会话。
''',encoding='utf8')
    assert not(out/'.forge/save').exists()
    # Byte equality protects the UI-only scope, including all native texture/scene assets.
    native_mismatches=[]
    for relative,expected in baseline['runtimeAndProtocolFiles'].items():
        prefix='projects/code-sentinels/Content/'
        if relative.startswith(prefix):
            packaged=out/'Content'/relative[len(prefix):]
            if digest(packaged)!=expected:native_mismatches.append(relative)
    assert not native_mismatches,native_mismatches
    verify()
    report.update({'version':3,'clientBundle':args.confirmed_client,'clientIndexSha256':digest(dist/'index.html'),'runtimeUnchanged':True,'matchingNativeModules':matches,'launchers':['Start-Game.cmd','Start-Game-GPU-Particles.cmd'],'finalBytes':sum(p.stat().st_size for p in out.rglob('*')if p.is_file()),'validationStatus':'pack built; runtime/UI verification pending'})
    (EVIDENCE/'portable-build.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf8')
    print(json.dumps({'outDir':str(out),'bytes':report['finalBytes'],'clientBundle':args.confirmed_client,'runtimeUnchanged':True,'warnings':report.get('warnings')},ensure_ascii=False))

if __name__=='__main__':main()
