"""Build V2 through the real Forge project/pack API; V1 stays untouched."""
import pathlib,json,urllib.request,shutil,hashlib
from engine_client import ROOT,REPO

OUT=ROOT/'dist/CodeSentinels-V2-Windows'
req=urllib.request.Request('http://127.0.0.1:8103/api/forge/project/pack',
    data=json.dumps({'sceneRef':str(ROOT/'Content/Scenes/Main.rxscene'),'outDir':str(OUT)}).encode(),
    headers={'content-type':'application/json'})
with urllib.request.urlopen(req,timeout=90) as response:report=json.load(response)
assert not report.get('warnings'),report
assert (OUT/'Content/Scripts/sentinels_v2.rs').is_file(), 'Native batch module was not packed'
assert (OUT/'Content/Graphs/V2BatchController.rxgraph').is_file()
source_hash=hashlib.sha256((OUT/'Content/Scripts/sentinels_v2.rs').read_bytes()).hexdigest()
assert any(json.loads(p.read_text(encoding='utf8')).get('sourceSha256')==source_hash for p in (OUT/'.forge/cache/rxdll').glob('*.native.json'))
shutil.copytree(REPO/'packages/client/dist',OUT/'Web')
shutil.copy2(pathlib.Path.home()/'.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin/node.exe',OUT/'bin/node.exe')
shutil.copy2(ROOT/'game/portable-bridge-v2.mjs',OUT/'bridge.mjs')
shutil.copy2(ROOT/'game/vendor/Node-LICENSE.txt',OUT/'Node-LICENSE.txt')
shutil.copy2(REPO/'LICENSE',OUT/'Forge-LICENSE.txt')
for filename,args,title in [('Start-Game.cmd','','Code Sentinels V2'),('Start-Game-GPU-Particles.cmd',' --gpu-particles','Code Sentinels V2 - GPU Particles')]:
    (OUT/filename).write_bytes(f'@echo off\r\ntitle {title}\r\n"%~dp0bin\\node.exe" "%~dp0bridge.mjs"{args}\r\nif errorlevel 1 pause\r\n'.encode('ascii'))
(OUT/'README.txt').write_text('''CODE SENTINELS V2 / 编译防线：开放战线

双击 Start-Game.cmd 启动常规版（显式关闭实验GPU粒子）。
双击 Start-Game-GPU-Particles.cmd 启动GPU粒子实验版（显式开启）。
两个版本都包含真实图生视频截帧的角色动画与技能效果；实验版额外运行GPU compute粒子。
游戏在本地原生引擎中运行，浏览器显示真实GPU帧。无需安装Node、Python、Rust、RurixForge，不需账号或API密钥。

第一步在左侧基地选择GPU插槽，并购买真实型号显卡。初始560经费、0算力、空GPU。
只有GPU生产算力；每次自动攻击和角色技能都会消耗算力。击杀奖励经费，不产算力。
显卡图片来自已核实厂商产品图；成本、产量、容量和升级均为游戏平衡值，不代表真实硬件性能或价格。

点击空闲地面部署单位。选中已有单位后使用升级、回收或技能按钮。
技能进入选点模式后点击目标地面释放。N开启下一波，空格暂停/继续。
三关各四波；水、岩石阻路，道路加速，高地增加射程。不要封死所有入口到基地的路径。
Boss半血及死亡会改变真实地图通行结构并触发重新寻路。

关闭启动窗口可停止该游戏实例。只监听127.0.0.1本机随机端口。
日志在Logs/normal与Logs/gpu-particles；战役解锁存在.forge/save，初始包不含测试存档。
角色、真实硬件和BUG资料来源在游戏档案中。
''',encoding='utf8')
assert not (OUT/'.forge/save').exists(),'Pack must not include test saves'
report.update({'portableUI':True,'version':2,'launchers':['Start-Game.cmd','Start-Game-GPU-Particles.cmd'],'nodeBytes':(OUT/'bin/node.exe').stat().st_size,'sourceSha256':source_hash,'finalBytes':sum(p.stat().st_size for p in OUT.rglob('*')if p.is_file())})
(ROOT/'game/v2/portable-build.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf8')
print(json.dumps({'outDir':str(OUT),'bytes':report['finalBytes'],'warnings':report.get('warnings'),'client':(OUT/'Web/index.html').read_text(encoding='utf8')},ensure_ascii=False))
