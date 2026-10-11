'use strict';

// 本地服务编排:源码检出 / 安装版两种布局、数据目录与默认工作区、端口选择、agentd 拉起与回收。
// 不依赖 electron 模块(由 main.cjs 传入 app 路径),便于 node --test 直接测。

const { spawn, spawnSync } = require('child_process');
const fs = require('fs');
const http = require('http');
const net = require('net');
const path = require('path');

const DEFAULT_HOST_PORT = 3080;
const DEFAULT_AGENTD_PORT = 8103;
const EXE = process.platform === 'win32' ? '.exe' : '';

/**
 * 运行布局。源码检出:apps/desktop/src 上三级 = 仓根,二进制在 target/{debug,release};
 * 安装版:resources/forge 按仓库结构收纳运行所需的部分(scripts/package-windows.ps1 生成),二进制都在 bin/。
 */
function resolveLayout({ packaged, resourcesPath, sourceDir }) {
  const root = packaged ? path.join(resourcesPath, 'forge') : path.resolve(sourceDir, '..', '..', '..');
  const binDirs = packaged
    ? [path.join(root, 'bin')]
    : [path.join(root, 'target', 'debug'), path.join(root, 'target', 'release')];
  return {
    packaged,
    root,
    hostEntry: path.join(root, 'packages', 'host', 'dist', 'index.js'),
    agentdCandidates: binDirs.map((dir) => path.join(dir, `forge-agentd${EXE}`)),
    presenterCandidates: binDirs.map((dir) => path.join(dir, `viewport-presenter${EXE}`)),
    // vite 把 public/ 原样拷进 dist/,安装版只带 dist。
    icon: path.join(root, 'packages', 'client', packaged ? 'dist' : 'public',
      process.platform === 'win32' ? 'favicon.ico' : 'logo.png'),
  };
}

/**
 * 安装版的目录约定:随包资源只读看待,用户数据全部落在 userData 下,卸载/升级不动它。
 * FORGE_DATA_DIR / FORGE_AGENTD_WORKSPACE_ROOT 可由用户环境改放;描述安装本身的变量一律以安装目录为准。
 */
function packagedPaths({ layout, userData, env }) {
  const dataDir = env.FORGE_DATA_DIR || path.join(userData, 'data');
  const workspace = env.FORGE_AGENTD_WORKSPACE_ROOT || path.join(dataDir, 'workspaces', 'default');
  const logsDir = path.join(userData, 'logs');
  return {
    dataDir,
    workspace,
    logsDir,
    stateFile: path.join(userData, 'launcher.json'),
    env: {
      FORGE_ROOT: layout.root,
      FORGE_DATA_DIR: dataDir,
      FORGE_LOG_DIR: logsDir,
      // 各服务自己的数据目录变量同指一处,开发机上残留的全局设置不会把数据写回源码仓。
      FORGE_AGENTD_DATA_DIR: dataDir,
      FORGE_GEN_DATA_DIR: dataDir,
      FORGE_AGENTD_WORKSPACE_ROOT: workspace,
      FORGE_GODOT_RUNTIME_DIR: path.join(layout.root, 'godot-runtime'),
      FORGE_ULTRAPLAN_RUNTIME_DIR: path.join(layout.root, 'ultraplan-runtime'),
    },
  };
}

/** 首次启动把随包模板复制成默认工作区;目录已存在(含用户改过的)一律不碰。 */
function seedWorkspace(templateDir, target) {
  if (fs.existsSync(target)) return false;
  fs.mkdirSync(path.dirname(target), { recursive: true });
  if (fs.existsSync(templateDir)) fs.cpSync(templateDir, target, { recursive: true });
  else fs.mkdirSync(target, { recursive: true });
  return true;
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return {};
  }
}

function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
}

function validPort(value) {
  const n = Number(value);
  return Number.isInteger(n) && n > 0 && n < 65536 ? n : null;
}

function listenOnce(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once('error', () => resolve(null));
    server.listen({ port, host: '127.0.0.1' }, () => {
      const bound = server.address().port;
      server.close(() => resolve(bound));
    });
  });
}

/** 依次试候选端口(上次用过的、缺省值),都被占就让系统给一个空闲端口;avoid 里的端口不用。 */
async function choosePort(candidates, avoid = []) {
  for (const candidate of candidates) {
    const port = validPort(candidate);
    if (port && !avoid.includes(port) && (await listenOnce(port)) === port) return port;
  }
  for (let attempt = 0; attempt < 5; attempt++) {
    const port = await listenOnce(0);
    if (port && !avoid.includes(port)) return port;
  }
  throw new Error('找不到可用的本机端口');
}

/**
 * 安装版端口:显式环境变量优先;否则沿用上次端口(页面的本地存储按源区分,端口稳定设置才不丢),
 * 被别的程序占了再退缺省值、随机端口,并记下来供下次使用。
 */
async function choosePackagedPorts({ stateFile, env }) {
  const state = readJson(stateFile);
  const explicitHost = validPort(env.FORGE_HOST_PORT);
  const explicitAgentd = validPort((env.FORGE_AGENTD_ADDR || '').split(':').pop());
  const hostPort = explicitHost || (await choosePort([state.hostPort, DEFAULT_HOST_PORT], [explicitAgentd]));
  const agentdPort = explicitAgentd || (await choosePort([state.agentdPort, DEFAULT_AGENTD_PORT], [hostPort]));
  if (!explicitHost && !explicitAgentd && (state.hostPort !== hostPort || state.agentdPort !== agentdPort)) {
    writeJson(stateFile, { ...state, hostPort, agentdPort });
  }
  return { hostPort, agentdPort };
}

/** 源码检出沿用固定端口(FORGE_HOST_PORT / FORGE_AGENTD_ADDR 可改),与 pnpm dev:host / dev:client 对得上。 */
function devPorts(env) {
  return {
    hostPort: validPort(env.FORGE_HOST_PORT) || DEFAULT_HOST_PORT,
    agentdPort: validPort((env.FORGE_AGENTD_ADDR || '').split(':').pop()) || DEFAULT_AGENTD_PORT,
  };
}

function probe(url, timeoutMs = 2000) {
  return new Promise((resolve) => {
    const req = http.get(url, (res) => {
      res.resume();
      resolve(res.statusCode === 200);
    });
    req.on('error', () => resolve(false));
    req.setTimeout(timeoutMs, () => {
      req.destroy();
      resolve(false);
    });
  });
}

/** 轮询健康地址直到通过;stillRunning() 返回 false(子进程已退)时提前放弃。 */
async function waitHealthy(url, { timeoutMs, intervalMs = 250, stillRunning = () => true }) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await probe(url)) return true;
    if (!stillRunning()) return false;
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
  return false;
}

/** 日志只留本次与上一次,免得长期运行把磁盘写满。 */
function openLog(logsDir, name) {
  fs.mkdirSync(logsDir, { recursive: true });
  const file = path.join(logsDir, `${name}.log`);
  try {
    if (fs.existsSync(file)) fs.renameSync(file, path.join(logsDir, `${name}.prev.log`));
  } catch {
    // 上一个实例还占着文件时就接着往后写
  }
  return { file, fd: fs.openSync(file, 'a') };
}

/**
 * 确保 agentd 在 agentdPort 上可用。源码检出时已有人手动启动的就直接复用;
 * 否则拉起本布局里的 forge-agentd:stdin 留一根管道(本进程退出或崩溃时 agentd 读到 EOF 自行退出),
 * 输出写日志文件——传文件句柄而不是管道,Windows 上会得到一个隐藏的控制台,
 * 它再拉起的 MCP 服务器、引擎等控制台程序都继承这个控制台,不会各自弹出窗口。
 */
async function ensureAgentd({ layout, env, port, logsDir, timeoutMs = 60_000 }) {
  const origin = `http://127.0.0.1:${port}`;
  const health = `${origin}/health`;
  if (!layout.packaged && (await probe(health))) return { origin, child: null, reused: true };
  const exe = layout.agentdCandidates.find((p) => fs.existsSync(p));
  if (!exe) {
    return { origin, child: null, missing: layout.agentdCandidates[0] };
  }
  const log = logsDir ? openLog(logsDir, 'agentd') : null;
  const child = spawn(exe, [], {
    cwd: layout.root,
    env: { ...env, FORGE_AGENTD_ADDR: `127.0.0.1:${port}`, FORGE_EXIT_ON_STDIN_EOF: '1' },
    stdio: ['pipe', log ? log.fd : 'inherit', log ? log.fd : 'inherit'],
    windowsHide: true,
  });
  if (log) fs.closeSync(log.fd);
  let exited = null;
  child.on('exit', (code, signal) => {
    exited = { code, signal };
  });
  const spawnError = new Promise((resolve) => child.once('error', resolve));
  const ready = await Promise.race([
    waitHealthy(health, { timeoutMs, stillRunning: () => exited === null }),
    spawnError.then((err) => {
      exited = { error: err.message };
      return false;
    }),
  ]);
  if (!ready) {
    killTree(child);
    const why = exited
      ? exited.error || `进程已退出 (code=${exited.code}, signal=${exited.signal})`
      : `${timeoutMs / 1000}s 内未通过健康检查`;
    const error = new Error(`AI 后端 forge-agentd 启动失败:${why}`);
    error.logFile = log ? log.file : null;
    throw error;
  }
  return { origin, child, exe, logFile: log ? log.file : null };
}

/** 结束整棵进程树(agentd 下挂着 MCP 服务器、引擎、Codex 等)。退出路径上同步执行,赶在 stdin 关闭前。 */
function killTree(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === 'win32' && child.pid) {
    spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
  } else {
    child.kill();
  }
}

module.exports = {
  DEFAULT_AGENTD_PORT,
  DEFAULT_HOST_PORT,
  choosePackagedPorts,
  choosePort,
  devPorts,
  ensureAgentd,
  killTree,
  openLog,
  packagedPaths,
  probe,
  resolveLayout,
  seedWorkspace,
  waitHealthy,
};
