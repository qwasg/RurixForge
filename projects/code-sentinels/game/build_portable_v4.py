"""Assemble the V4 release through Forge's real dependency packer.

This builds distribution files only. It does not launch the game, run a test,
compare historical hashes or claim gameplay acceptance.
"""
import argparse
import json
import pathlib
import shutil
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
OUT = ROOT / 'dist/CodeSentinels-V4-Windows'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--zip', action='store_true', help='Compress the newly assembled release')
    parser.add_argument('--file-pack', action='store_true', help='Assemble dependencies without starting the development pack service')
    args = parser.parse_args()
    if OUT.exists():
        raise RuntimeError(f'Preserving existing package; choose a new release directory: {OUT}')
    if args.file_pack:
        from pack_v4_files import build_pack
        report = build_pack(ROOT / 'Content/Scenes/Command.rxscene', OUT)
    else:
        request = urllib.request.Request('http://127.0.0.1:8103/api/forge/project/pack',
            data=json.dumps({'sceneRef': str(ROOT / 'Content/Scenes/Command.rxscene'), 'outDir': str(OUT)}).encode(),
            headers={'content-type': 'application/json'})
        with urllib.request.urlopen(request, timeout=180) as response:
            report = json.load(response)
    if report.get('warnings'):
        raise RuntimeError(f'Native dependencies need completion before packaging: {report["warnings"]}')
    shutil.copytree(REPO / 'packages/client/dist', OUT / 'Web')
    runtime = pathlib.Path.home() / '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin/node.exe'
    shutil.copy2(runtime, OUT / 'bin/node.exe')
    for source, destination in [('portable-bridge-v4.mjs', 'bridge.mjs'),
                                 ('multiplayer-v4.mjs', 'multiplayer-v4.mjs'),
                                 ('pvp-lobby-server-v4.mjs', 'pvp-lobby-server-v4.mjs')]:
        shutil.copy2(ROOT / 'game' / source, OUT / destination)
    shutil.copy2(ROOT / 'game/vendor/Node-LICENSE.txt', OUT / 'Node-LICENSE.txt')
    shutil.copy2(REPO / 'LICENSE', OUT / 'Forge-LICENSE.txt')
    for name, script, flags, title in [
        ('Start-Game.cmd', 'bridge.mjs', '', 'Code Sentinels V4 - Command Frontier'),
        ('Start-Game-GPU-Particles.cmd', 'bridge.mjs', ' --gpu-particles', 'Code Sentinels V4 - GPU Particles'),
        ('Start-PVP-Lobby.cmd', 'pvp-lobby-server-v4.mjs', '', 'Code Sentinels V4 - LAN PVP Preparation'),
    ]:
        (OUT / name).write_bytes(f'@echo off\r\ntitle {title}\r\n"%~dp0bin\\node.exe" "%~dp0{script}"{flags}\r\nif errorlevel 1 pause\r\n'.encode('ascii'))
    shutil.copy2(ROOT / 'game/V4-README.md', OUT / 'README.md')
    shutil.copy2(ROOT / 'game/v4/MULTIPLAYER.md', OUT / 'MULTIPLAYER.md')
    (OUT / 'v4').mkdir(exist_ok=True)
    shutil.copy2(ROOT / 'game/v4/MULTIPLAYER.md', OUT / 'v4/MULTIPLAYER.md')
    sources = OUT / 'Sources'; sources.mkdir(exist_ok=True)
    for name in ['sources.md', 'sources.json', 'sources-gpu.md', 'sources-gpu.json', 'ui-v3-reference.md']:
        source = ROOT / 'references' / name
        if source.is_file():
            shutil.copy2(source, sources / name)
    art_sources = ROOT / 'Content/UI/v4'
    for source in art_sources.glob('*.json'):
        shutil.copy2(source, sources / ('v4-' + source.name))
    for source in art_sources.glob('*.md'):
        shutil.copy2(source, sources / ('v4-' + source.name))
    (sources / 'README.md').write_text('''# 素材来源

人物与真实显卡的来源、作者及厂商链接保留在本目录。原项目清单中的相对文件路径指制作项目，游戏实际采用的图片已随 Web 和 Content 打包。

DeepSeek / GPT 的角色和技能动作沿用此前真实图生视频素材。V4 新建筑为内置 imagegen 生成的原创工业 RTS 美术；本目录同时保留其提示词与制作元数据。图像生成的候选文件不代表全部都被游戏采用，以最终素材清单为准。

游戏数值用于玩法平衡，不是厂商价格、功耗或基准测试。
''', encoding='utf8')
    (OUT / 'README.txt').write_text('''CODE SENTINELS V4 / 编译防线 · 算力边疆

双击 Start-Game.cmd 启动单人经营战场。
Start-Game-GPU-Particles.cmd 额外开启实验GPU粒子；真实图生视频角色和技能帧在两个入口均保留。

建设发电站和数据中心，拉电力线，点击数据中心安装GPU；再建基站或铺算力线路支持角色和固定炮台。
移动AI使用随身算力电池，覆盖内补充。研究院升级科技，城墙闭合后使用算力形成护盾。
地图交互、快捷键和各建筑条件见游戏内作战手册及 README.md。

Start-PVP-Lobby.cmd 启动独立局域网准备室。双方打开同一个服务器地址再用房间代码加入。
本版多人只包含实际准备室与配套协议基础设施，双玩家战斗同步尚未接入；不代表可以进行PVP战斗。
局域网准备室不启动原生引擎，也不暴露本机游戏控制接口。关闭窗口结束对应服务。

无需安装Node、Python、Rust或RurixForge，不需要账户、API密钥或编译器。
V4包为新版本；旧版本和正在运行的旧游戏不受影响。
显卡照片与网络人物沿用原核实来源。建设成本、功率、产量和科技等级均为游戏数值，不是硬件实际测评。

按用户要求，本次仅完成制作、编译与打包，没有追加自动化测试或游戏试玩。V3的验收不等同于V4验收。
''', encoding='utf8')
    if args.zip:
        archive = OUT.with_suffix('.zip')
        if archive.exists():
            raise RuntimeError(f'Preserving existing ZIP: {archive}')
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as package:
            for file in OUT.rglob('*'):
                if not file.is_file():
                    continue
                relative = file.relative_to(OUT)
                if relative.parts[0] == 'Logs' or relative.parts[:2] in [('.forge', 'save'), ('.forge', 'multiplayer')]:
                    continue
                package.write(file, OUT.name + '/' + relative.as_posix())
        report['zip'] = str(archive)
        report['zipBytes'] = archive.stat().st_size
    report.update({'version': 4, 'outDir': str(OUT), 'additionalTestsRun': False, 'gameplayAcceptance': 'not requested; user asked to skip further testing',
                   'multiplayer': 'PVP preparation rooms and authority boundary; multiplayer combat adapter not installed'})
    evidence = ROOT / 'game/v4'; evidence.mkdir(parents=True, exist_ok=True)
    (evidence / 'portable-build.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf8')
    print(json.dumps({'outDir': str(OUT), 'zip': report.get('zip'), 'zipBytes': report.get('zipBytes')}, ensure_ascii=False))


if __name__ == '__main__':
    main()
