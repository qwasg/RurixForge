"""Assemble the complete V5 release. No gameplay is started by this builder."""
import json
import argparse
import pathlib
import shutil
import zipfile
from pack_v4_files import build_pack

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
OUT = ROOT / 'dist/CodeSentinels-V5-Windows'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--no-zip', action='store_true', help='Build the candidate directory for runtime/browser acceptance before archiving')
    args = parser.parse_args()
    if OUT.exists() or OUT.with_suffix('.zip').exists():
        raise RuntimeError('Preserving an existing V5 release; use a new version for later revisions')
    media = json.loads((ROOT / 'game/v5/media-inventory.json').read_text(encoding='utf8'))
    if media.get('newVideoFrames') != 1856 or len(media.get('items', [])) != 20:
        raise RuntimeError('Complete genuine building and impact media must be imported before release')
    scene = ROOT / 'Content/Scenes/CommandV5.rxscene'
    engine = ROOT / 'game/v5/runtime-bin/engine-host.exe'
    if not engine.is_file():
        raise FileNotFoundError('Build the isolated V5 engine with game/v5/build_engine.ps1 before packaging')
    report = build_pack(scene, OUT, runtime=engine)
    for name in ['concrt140.dll', 'msvcp140.dll', 'msvcp140_1.dll', 'msvcp140_2.dll',
                 'msvcp140_atomic_wait.dll', 'msvcp140_codecvt_ids.dll', 'vccorlib140.dll',
                 'vcruntime140.dll', 'vcruntime140_1.dll', 'vcruntime140_threads.dll']:
        shutil.copy2(engine.parent / name, OUT / 'bin' / name)
    # The packaged entry and bridge are V5-specific; the editor's V4 entry is
    # kept available throughout production and old portable folders are intact.
    config = (OUT / 'forge.toml').read_text(encoding='utf8').replace('Content/Scenes/Command.rxscene', 'Content/Scenes/CommandV5.rxscene')
    (OUT / 'forge.toml').write_text(config, encoding='utf8')
    shutil.copytree(REPO / 'packages/client/dist', OUT / 'Web')
    runtime = pathlib.Path.home() / '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin/node.exe'
    shutil.copy2(runtime, OUT / 'bin/node.exe')
    bridge = (ROOT / 'game/portable-bridge-v4.mjs').read_text(encoding='utf8')
    bridge = bridge.replace('Content/Scenes/Command.rxscene', 'Content/Scenes/CommandV5.rxscene')
    bridge = bridge.replace("args.action !== 'cs4'", "args.action !== 'cs5'")
    bridge = bridge.replace('Code Sentinels V4 is ready:', 'Code Sentinels V5 is ready:')
    bridge = bridge.replace('workspace=portable&standalone=1', 'workspace=portable&standalone=1&version=5')
    (OUT / 'bridge.mjs').write_text(bridge, encoding='utf8')
    for name in ['multiplayer-v4.mjs', 'pvp-lobby-server-v4.mjs']:
        shutil.copy2(ROOT / 'game' / name, OUT / name)
    for filename, script, flags, title in [
        ('Start-Game.cmd', 'bridge.mjs', '', 'Code Sentinels V5 - Living Frontier'),
        ('Start-Game-GPU-Particles.cmd', 'bridge.mjs', ' --gpu-particles', 'Code Sentinels V5 - GPU Particles'),
        ('Start-PVP-Lobby.cmd', 'pvp-lobby-server-v4.mjs', '', 'Code Sentinels V5 - PVP Preparation')]:
        (OUT / filename).write_bytes(f'@echo off\r\ntitle {title}\r\n"%~dp0bin\\node.exe" "%~dp0{script}"{flags}\r\nif errorlevel 1 pause\r\n'.encode('ascii'))
    shutil.copy2(ROOT / 'game/vendor/Node-LICENSE.txt', OUT / 'Node-LICENSE.txt')
    shutil.copy2(REPO / 'LICENSE', OUT / 'Forge-LICENSE.txt')
    shutil.copy2(ROOT / 'game/V5-README.md', OUT / 'README.md')
    shutil.copy2(ROOT / 'game/v4/MULTIPLAYER.md', OUT / 'MULTIPLAYER.md')
    sources = OUT / 'Sources'; sources.mkdir()
    for name in ['sources.md', 'sources-gpu.md']:
        shutil.copy2(ROOT / 'references' / name, sources / name)
    shutil.copy2(ROOT / 'game/v5/media-inventory.json', sources / 'video-frame-inventory.json')
    for name in ['vfx-reference-prompts.json', 'building-reference-prompts.json', 'reference-manifest.json']:
        shutil.copy2(ROOT / 'Content/UI/v5' / name, sources / name)
    shutil.copy2(ROOT / 'Content/UI/v5/hub/art-prompts.json', sources / 'hub-art-prompts.json')
    (OUT / 'README.txt').write_text('''CODE SENTINELS V5 / 编译防线 · 动态战线

完整解压后运行 Start-Game.cmd，在浏览器点击“开始单人行动”。
本版重做12类建筑/炮台的落地、工作、摧毁动画及8类打击支援特效，均来自真实图生视频截帧。
发电、数据中心装卡、算力网络、移动AI、科技和cudad护城河的经营玩法延续V4。
不需安装Codex、开发环境、编译器或配置API密钥。已经生成的媒体随包提供，游玩不调用生成服务。

Start-Game-GPU-Particles.cmd额外开启实验GPU粒子；两个游戏入口都保留真实视频帧动画。
Start-PVP-Lobby.cmd只启动局域网准备室；双玩家战斗同步尚未接入。

H查看手册。B/U/I切牌组，L/C布线，W建墙，A或右键移动，Q技能，N入侵，空格暂停。
日志在Logs，战役解锁保存在.forge/save；进行中的战局由当前引擎进程保存。
本版按用户最新要求完成素材制作、原生逻辑、界面与独立包测试；测试范围和结果见QA说明。
''', encoding='utf8')
    # Reviewed source references and redistribution notices accompany every
    # rebuild, not just the manually inspected first release directory.
    shutil.copytree(ROOT / 'game/v5/release-support', OUT, dirs_exist_ok=True)
    if args.no_zip:
        report.update({'version': 5, 'outDir': str(OUT), 'rendererFeature': 'spriteVariants-v1', 'candidate': True,
            'buildingClips': 36, 'impactClips': 8, 'newVideoFrames': 1856, 'acceptanceStatus': 'candidate assembled; final runtime/browser acceptance pending'})
        (ROOT / 'game/v5/portable-build.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf8')
        print(json.dumps({'outDir': str(OUT), 'candidate': True}, ensure_ascii=False))
        return
    archive = OUT.with_suffix('.zip')
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as package:
        for file in OUT.rglob('*'):
            if not file.is_file(): continue
            relative = file.relative_to(OUT)
            if relative.parts[0] == 'Logs' or relative.parts[:2] in [('.forge', 'save'), ('.forge', 'multiplayer')]: continue
            package.write(file, OUT.name + '/' + relative.as_posix())
    report.update({'version': 5, 'outDir': str(OUT), 'zip': str(archive), 'zipBytes': archive.stat().st_size,
        'rendererFeature': 'spriteVariants-v1', 'buildingClips': 36, 'impactClips': 8, 'newVideoFrames': 1856,
        'acceptanceStatus': 'see game/v5 acceptance reports; packaging alone is not a gameplay pass',
        'developmentService': 'not used; native scene dependencies assembled directly'})
    (ROOT / 'game/v5/portable-build.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf8')
    print(json.dumps({'outDir': str(OUT), 'zip': str(archive), 'zipBytes': report['zipBytes']}, ensure_ascii=False))


if __name__ == '__main__': main()
