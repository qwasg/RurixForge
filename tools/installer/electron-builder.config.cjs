'use strict';

// RurixForge Windows 安装包配置(electron-builder + NSIS)。由 scripts/package-windows.ps1 调用:
// Electron 应用目录(package.json + src/*.cjs)作为 --projectDir 传入,其余构建目录经环境变量传入:
//   FORGE_PKG_FORGE             随包运行根,装到 resources/forge(布局见 apps/desktop/src/backend.cjs)
//   FORGE_PKG_OUT               安装包输出目录
//   FORGE_PKG_ELECTRON_VERSION  Electron 版本(与 apps/desktop 锁定的一致)
//   FORGE_PKG_ELECTRON_DIST     可选:本机已解压的 Electron(有就不再下载)

const path = require('path');

function required(key) {
  const value = process.env[key];
  if (!value) throw new Error(`${key} 未设置;请用 scripts/package-windows.ps1 构建安装包`);
  return value;
}

const repoRoot = path.resolve(__dirname, '..', '..');
const icon = path.join(repoRoot, 'packages', 'client', 'public', 'favicon.ico');

module.exports = {
  appId: 'io.github.qwasg.rurixforge',
  productName: 'RurixForge',
  copyright: 'Copyright © 2026 RurixForge contributors · Apache-2.0',
  directories: {
    output: required('FORGE_PKG_OUT'),
    buildResources: __dirname,
  },
  electronVersion: required('FORGE_PKG_ELECTRON_VERSION'),
  ...(process.env.FORGE_PKG_ELECTRON_DIST ? { electronDist: process.env.FORGE_PKG_ELECTRON_DIST } : {}),
  files: ['package.json', 'src/**/*'],
  asar: true,
  // 桌面壳没有原生依赖,不需要重编 node 模块。
  npmRebuild: false,
  extraResources: [{ from: required('FORGE_PKG_FORGE'), to: 'forge' }],
  win: {
    target: [{ target: 'nsis', arch: ['x64'] }],
    icon,
    artifactName: 'RurixForge-Setup-${version}.${ext}',
  },
  nsis: {
    // 精简向导:可改安装目录 → 安装 → 完成(可勾选立即运行);按用户安装,无需管理员权限。
    oneClick: false,
    perMachine: false,
    allowElevation: false,
    allowToChangeInstallationDirectory: true,
    include: path.join(__dirname, 'installer.nsh'),
    createDesktopShortcut: 'always',
    createStartMenuShortcut: true,
    shortcutName: 'RurixForge',
    runAfterFinish: true,
    // 用户数据(%APPDATA%\RurixForge)在卸载与升级时保留。
    deleteAppDataOnUninstall: false,
    installerIcon: icon,
    uninstallerIcon: icon,
    installerLanguages: ['zh_CN'],
    language: '2052',
    unicode: true,
    differentialPackage: false,
  },
  // 没有自动更新服务,不生成也不上传更新元数据。
  publish: null,
};
