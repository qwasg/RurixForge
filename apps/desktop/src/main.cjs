'use strict';

const { app, BrowserWindow, dialog, ipcMain } = require('electron');
const { spawn } = require('child_process');
const fs = require('fs');
const http = require('http');
const path = require('path');

const HOST_ORIGIN = 'http://127.0.0.1:3080';
const HEALTH_URL = `${HOST_ORIGIN}/api/forge/health`;
const HEALTH_INTERVAL_MS = 250;
const HEALTH_TIMEOUT_MS = 30_000;

const repoRoot = path.resolve(__dirname, '..', '..', '..');
const hostEntry = path.join(repoRoot, 'packages', 'host', 'dist', 'index.js');
const evidenceDir = path.join(__dirname, '..', 'evidence');
const isSmoke = process.env.FORGE_SMOKE === '1';
// 冒烟场景:home(默认)| editor(进编辑器视图再截图)
const smokeScenario = process.env.FORGE_SMOKE_SCENARIO || 'home';

let mainWindow = null;
let hostProcess = null;
let hostReady = false;
let quitting = false;

function forwardHostStream(stream, tag) {
  let buf = '';
  stream.on('data', (chunk) => {
    buf += chunk.toString('utf8');
    let idx = buf.indexOf('\n');
    while (idx !== -1) {
      const line = buf.slice(0, idx).replace(/\r$/, '');
      buf = buf.slice(idx + 1);
      console.log(`[host] ${line}`);
      if (isSmoke && tag === 'stderr') smokeLog(`[host:err] ${line}`);
      idx = buf.indexOf('\n');
    }
  });
  stream.on('end', () => {
    if (buf.length > 0) console.log(`[host] ${buf}`);
  });
}

function spawnHost() {
  if (!fs.existsSync(hostEntry)) {
    smokeLog(`host entry missing: ${hostEntry}`);
    if (isSmoke) {
      app.exit(1);
      return false;
    }
    dialog.showErrorBox(
      'Forge Desktop 启动失败',
      `未找到 @forge/host 构建产物:\n${hostEntry}\n\n请先构建 packages/host。`
    );
    app.exit(1);
    return false;
  }

  console.log(`[desktop] spawning host: node ${hostEntry}`);
  hostProcess = spawn('node', [hostEntry], {
    cwd: repoRoot,
    env: process.env,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  forwardHostStream(hostProcess.stdout, 'stdout');
  forwardHostStream(hostProcess.stderr, 'stderr');

  hostProcess.on('error', (err) => {
    if (isSmoke) smokeLog(`host spawn error: ${err.message}`);
    dialog.showErrorBox('Forge Desktop 启动失败', `无法启动 host 子进程:\n${err.message}`);
    app.exit(1);
  });

  hostProcess.on('exit', (code, signal) => {
    console.log(`[desktop] host exited (code=${code}, signal=${signal})`);
    if (isSmoke) smokeLog(`host exited early (code=${code}, signal=${signal})`);
    if (!hostReady && !quitting) {
      if (isSmoke) {
        // 冒烟模式无弹窗(模态框在无人值守下永久阻塞),直接失败退出
        killHost();
        app.exit(1);
        return;
      }
      dialog.showErrorBox(
        'Forge Desktop 启动失败',
        `host 子进程在健康检查通过前退出 (code=${code}, signal=${signal})。`
      );
      app.exit(1);
    }
  });

  return true;
}

function probeHealthOnce() {
  return new Promise((resolve) => {
    const req = http.get(HEALTH_URL, (res) => {
      res.resume();
      resolve(res.statusCode === 200);
    });
    req.on('error', () => resolve(false));
    req.setTimeout(2000, () => {
      req.destroy();
      resolve(false);
    });
  });
}

async function waitForHostHealthy() {
  const deadline = Date.now() + HEALTH_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (await probeHealthOnce()) return true;
    await new Promise((resolve) => setTimeout(resolve, HEALTH_INTERVAL_MS));
  }
  return false;
}

function httpGetJson(url) {
  return new Promise((resolve, reject) => {
    const req = http.get(url, (res) => {
      let body = '';
      res.on('data', (chunk) => {
        body += chunk.toString('utf8');
      });
      res.on('end', () => {
        let parsed = body;
        try {
          parsed = JSON.parse(body);
        } catch {
          // 非 JSON 响应透传原文
        }
        resolve({ status: res.statusCode, body: parsed });
      });
    });
    req.on('error', reject);
    req.setTimeout(5000, () => req.destroy(new Error('request timed out')));
  });
}

function killHost() {
  if (!hostProcess) return;
  if (hostProcess.exitCode !== null || hostProcess.signalCode !== null) return;

  const pid = hostProcess.pid;
  try {
    hostProcess.kill();
  } catch (err) {
    console.error('[desktop] host kill failed:', err);
  }

  if (process.platform === 'win32' && pid) {
    try {
      spawn('taskkill', ['/pid', String(pid), '/T', '/F'], {
        stdio: 'ignore',
        windowsHide: true,
      });
    } catch (err) {
      console.error('[desktop] taskkill fallback failed:', err);
    }
  }
}

function smokeLog(msg) {
  // GUI 子系统下 stdout 不可达,冒烟日志落盘
  try {
    fs.mkdirSync(evidenceDir, { recursive: true });
    fs.appendFileSync(path.join(evidenceDir, 'smoke.log'), `[${new Date().toISOString()}] ${msg}\n`);
  } catch {
    // 日志失败不阻塞主流程
  }
}

async function captureSmokeEvidence() {
  // 等首帧稳定后再截图,避免抓到纯背景
  await new Promise((resolve) => setTimeout(resolve, 1500));
  const image = await mainWindow.webContents.capturePage();
  fs.mkdirSync(evidenceDir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const file = path.join(evidenceDir, `desktop-smoke-${smokeScenario}-${stamp}.png`);
  fs.writeFileSync(file, image.toPNG());
  smokeLog(`screenshot saved: ${file} (${image.toPNG().length} bytes)`);
}

/** 冒烟场景脚本:editor = 点击侧栏「编辑器」入口,等编辑器真实拉数后再截 */
async function runSmokeScenario() {
  if (smokeScenario !== 'editor') return;
  const clicked = await mainWindow.webContents.executeJavaScript(
    "(() => { const b=[...document.querySelectorAll('button')].find((x)=>x.textContent.trim()==='编辑器'); if(b){b.click();return true;} return false; })()"
  );
  smokeLog(`scenario=editor nav click: ${clicked}`);
  if (!clicked) throw new Error('sidebar 未找到「编辑器」入口按钮');
  // 编辑器挂载后有多轮 MCP 往返(实体/摘要/PIE/事件),留足窗口
  await new Promise((resolve) => setTimeout(resolve, 4000));
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1440,
    height: 900,
    show: !isSmoke,
    backgroundColor: '#1e1e1e',
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      // 冒烟模式:离屏渲染确保证隐藏窗口也产生帧(对齐 apps/ide 旧 screenshot.cjs 已验证模式)
      offscreen: isSmoke,
    },
  });

  // 窗口控制 IPC(对齐旧 apps/ide preload 契约 win.*)
  mainWindow.on('maximize', () => mainWindow.webContents.send('win:maximized-changed', true));
  mainWindow.on('unmaximize', () => mainWindow.webContents.send('win:maximized-changed', false));

  if (isSmoke) {
    let settled = false;
    const settle = (ok, err) => {
      if (settled) return;
      settled = true;
      if (!ok) {
        smokeLog(`load failed: ${err}`);
        killHost();
        app.exit(1);
        return;
      }
      runSmokeScenario()
        .then(() => captureSmokeEvidence())
        .then(
          () => {
            killHost();
            app.exit(0);
          },
          (captureErr) => {
            smokeLog(`capturePage failed: ${captureErr}`);
            killHost();
            app.exit(1);
          }
        );
    };
    mainWindow.webContents.once('did-finish-load', () => settle(true));
    mainWindow.webContents.once('did-fail-load', (_e, code, desc) =>
      settle(false, `did-fail-load code=${code} ${desc}`)
    );
  } else {
    mainWindow.once('ready-to-show', () => mainWindow.show());
  }

  const targetUrl = process.env.FORGE_DEV_URL || HOST_ORIGIN;
  console.log(`[desktop] loading ${targetUrl}`);
  if (isSmoke) smokeLog(`loading ${targetUrl}`);
  mainWindow.loadURL(targetUrl);
}

async function main() {
  if (isSmoke) smokeLog('main() enter');
  ipcMain.handle('forge:health', () => httpGetJson(HEALTH_URL));
  ipcMain.on('win:minimize', () => mainWindow && mainWindow.minimize());
  ipcMain.on('win:toggle-maximize', () => {
    if (!mainWindow) return;
    if (mainWindow.isMaximized()) mainWindow.unmaximize();
    else mainWindow.maximize();
  });
  ipcMain.on('win:close', () => mainWindow && mainWindow.close());

  if (!spawnHost()) return;
  if (isSmoke) smokeLog(`host spawned pid=${hostProcess && hostProcess.pid}`);

  hostReady = await waitForHostHealthy();
  if (isSmoke) smokeLog(`hostReady=${hostReady}`);
  if (!hostReady) {
    smokeLog(`health timeout: ${HEALTH_URL}`);
    if (isSmoke) {
      killHost();
      app.exit(1);
      return;
    }
    dialog.showErrorBox(
      'Forge Desktop 启动失败',
      `等待 host 健康检查超时 (${HEALTH_TIMEOUT_MS / 1000}s):\n${HEALTH_URL}`
    );
    killHost();
    app.exit(1);
    return;
  }

  createWindow();
}

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  quitting = true;
  app.quit();
} else {
  app.on('second-instance', () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    }
  });

  app.whenReady().then(main).catch((err) => {
    smokeLog(`main() threw: ${err && err.stack ? err.stack : err}`);
    if (isSmoke) {
      killHost();
      app.exit(1);
      return;
    }
    dialog.showErrorBox('Forge Desktop 启动失败', String(err && err.stack ? err.stack : err));
    killHost();
    app.exit(1);
  });

  app.on('window-all-closed', () => {
    quitting = true;
    killHost();
    app.quit();
  });

  app.on('before-quit', () => {
    quitting = true;
    killHost();
  });
}
