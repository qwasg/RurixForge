"""Use the real Forge pack endpoint, then add its portable native-game UI."""
import pathlib,json,urllib.request,shutil,sys
from engine_client import ROOT,REPO
OUT=ROOT/'dist'/(sys.argv[1] if len(sys.argv)>1 else 'CodeSentinels-Windows')
req=urllib.request.Request('http://127.0.0.1:8103/api/forge/project/pack',data=json.dumps({'sceneRef':str(ROOT/'Content/Scenes/Main.rxscene'),'outDir':str(OUT)}).encode(),headers={'content-type':'application/json'})
with urllib.request.urlopen(req,timeout=90) as response:report=json.load(response)
assert not report.get('warnings'),report
shutil.copytree(REPO/'packages/client/dist',OUT/'Web')
shutil.copy2(pathlib.Path.home()/'.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin/node.exe',OUT/'bin/node.exe')
shutil.copy2(ROOT/'game/portable-bridge.mjs',OUT/'bridge.mjs')
shutil.copy2(ROOT/'game/vendor/Node-LICENSE.txt',OUT/'Node-LICENSE.txt')
shutil.copy2(REPO/'LICENSE',OUT/'Forge-LICENSE.txt')
(OUT/'Start-Game.cmd').write_bytes(b'@echo off\r\ntitle Code Sentinels\r\n"%~dp0bin\\node.exe" "%~dp0bridge.mjs"\r\nif errorlevel 1 pause\r\n')
(OUT/'README.txt').write_text('CODE SENTINELS / 编译防线\n\n双击 Start-Game.cmd 开始。游戏由本地原生引擎运行，浏览器显示实时游戏画面。无需安装 Node、Python、Rust 或 RurixForge，也无需账号/API密钥。\n\n关闭启动窗口可停止游戏。日志在 Logs/。仅使用 127.0.0.1 本机端口。角色来源可在游戏「角色档案」查看。\n\n玩法：先部署，再点下一波。1–4选角色，点击空格部署；点击已有单元后，用右侧升级按钮升级。Q释放所选通路热修复，空格暂停。守住8波获胜。\n',encoding='utf8')
report['portableUI']=True;report['launcher']='Start-Game.cmd';report['nodeBytes']=(OUT/'bin/node.exe').stat().st_size
report['finalBytes']=sum(p.stat().st_size for p in OUT.rglob('*') if p.is_file())
(ROOT/'game/native/portable-build.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf8')
print(json.dumps({'outDir':str(OUT),'bytes':report['finalBytes'],'warnings':report.get('warnings')},ensure_ascii=False))
