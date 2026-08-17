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
// 可见冒烟(FORGE_SMOKE_VISIBLE=1):真实显示窗口 + 非离屏,G-F1-9 桌面腿专用
// (presenter 子窗口嵌入 + OS 级截屏锚点比对);常规冒烟仍离屏不嵌原生层。
const isSmokeVisible = process.env.FORGE_SMOKE_VISIBLE === '1';
const useOffscreen = isSmoke && !isSmokeVisible;

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

// ───────────── viewport-presenter 集成(F1 wave.2 G-F1-9 桌面腿) ─────────────
// renderer 上报视口容器 bounds(CSS px + dpr)→ spawn presenter 子窗口嵌入本窗
// 客户区(WS_EX_TRANSPARENT 点击穿透,web 交互不受影响)→ 经 host MCP 打开 D3D12
// 共享纹理(NT handle 复制给 presenter pid)→ stdin bind/move/close 驱动。
// 帧源唯一(viewport.frame 同帧写共享纹理与 canvas 回退腿),尺寸变更重建共享纹理。

const PRESENTER_EXE_CANDIDATES = [
  path.join(repoRoot, 'target', 'debug', 'viewport-presenter.exe'),
  path.join(repoRoot, 'target', 'release', 'viewport-presenter.exe'),
];
const PRESENTER_RECT_EVIDENCE = path.join(evidenceDir, 'viewport-presenter-rect.json');
const OS_CAPTURE_DONE_FLAG = path.join(evidenceDir, 'os-capture-done.flag');

const presenter = {
  proc: null,
  ready: false,
  texW: 0,
  texH: 0,
  x: 0,
  y: 0,
  statTimer: null,
  evidenceWritten: false,
  presented: 0,
  rect: null,
  chain: Promise.resolve(),
  readyWaiters: [],
};

function presenterExe() {
  return PRESENTER_EXE_CANDIDATES.find((p) => fs.existsSync(p)) || null;
}

/** host(3080)MCP 调用(本机代理,无需 JWT);返回 content[0].text 二次解析结果。 */
function mcpCallHost(tool, args) {
  return new Promise((resolve, reject) => {
    const body = JSON.stringify({ tool, arguments: args });
    const req = http.request(
      `${HOST_ORIGIN}/api/forge/mcp/call`,
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(body),
        },
      },
      (res) => {
        let raw = '';
        res.on('data', (c) => {
          raw += c.toString('utf8');
        });
        res.on('end', () => {
          try {
            const outer = JSON.parse(raw);
            if (res.statusCode !== 200) {
              reject(new Error(`HTTP ${res.statusCode}: ${raw.slice(0, 200)}`));
              return;
            }
            const text = outer && outer.content && outer.content[0] && outer.content[0].text;
            resolve(typeof text === 'string' ? JSON.parse(text) : outer);
          } catch (e) {
            reject(new Error(`bad mcp response: ${e.message} raw=${raw.slice(0, 200)}`));
          }
        });
      }
    );
    req.on('error', reject);
    req.setTimeout(15000, () => req.destroy(new Error('mcp call timed out')));
    req.write(body);
    req.end();
  });
}

function presenterWrite(line) {
  try {
    if (presenter.proc && presenter.proc.exitCode === null) presenter.proc.stdin.write(`${line}\n`);
  } catch {
    // 进程已退,静默(canvas 腿兜底)
  }
}

/** 解析 presenter stdout 行:PRESENTER_READY / STAT presented=N rect=x,y,w,h */
function onPresenterLine(line) {
  if (line === 'PRESENTER_READY') {
    presenter.ready = true;
    presenter.readyWaiters.splice(0).forEach((r) => r());
    return;
  }
  const m = /^STAT presented=(\d+) rect=(.*)$/.exec(line);
  if (m) {
    presenter.presented = Number(m[1]);
    presenter.rect = m[2] === 'none' ? null : m[2].split(',').map(Number);
    void maybeWriteRectEvidence();
  }
}

/** 可见冒烟证据:presented>=3 后写 rect + 锚点(中心)像素 readback 值,供 OS 截屏比对 */
async function maybeWriteRectEvidence() {
  if (!isSmokeVisible || presenter.evidenceWritten) return;
  if (presenter.presented < 3 || !presenter.rect) return;
  presenter.evidenceWritten = true;
  try {
    const f = await mcpCallHost('mcp__engine-scene__viewport_frame', {
      width: presenter.texW,
      height: presenter.texH,
    });
    const bin = Buffer.from(f.pixelsB64, 'base64');
    const cx = Math.floor(f.width / 2);
    const cy = Math.floor(f.height / 2);
    const i = (cy * f.width + cx) * 4;
    const evidence = {
      rect: { x: presenter.rect[0], y: presenter.rect[1], w: presenter.rect[2], h: presenter.rect[3] },
      presented: presenter.presented,
      texW: presenter.texW,
      texH: presenter.texH,
      centerRgba: [bin[i], bin[i + 1], bin[i + 2], bin[i + 3]],
      framePath: f.framePath || '',
      deviceName: f.deviceName || '',
    };
    fs.mkdirSync(evidenceDir, { recursive: true });
    fs.writeFileSync(PRESENTER_RECT_EVIDENCE, JSON.stringify(evidence, null, 2));
    smokeLog(`presenter evidence: ${JSON.stringify(evidence)}`);
  } catch (err) {
    presenter.evidenceWritten = false; // 下轮 stat 重试
    smokeLog(`presenter evidence failed: ${err.message}`);
  }
}

async function syncPresenterInner(b) {
  if (quitting || !mainWindow || useOffscreen) return;
  if (!b || b.visible === false || !(b.w > 0) || !(b.h > 0)) {
    stopPresenter();
    return;
  }
  const exe = presenterExe();
  if (!exe) return; // 未构建:canvas 腿独立可用,不噪声

  const dpr = b.dpr > 0 ? b.dpr : 1;
  const x = Math.round(b.x * dpr);
  const y = Math.round(b.y * dpr);
  const w = Math.max(16, Math.round(b.w * dpr));
  const h = Math.max(16, Math.round(b.h * dpr));

  if (!presenter.proc) {
    const hwnd = mainWindow.getNativeWindowHandle().readBigUInt64LE(0).toString();
    const proc = spawn(exe, ['--hwnd', hwnd, String(x), String(y), String(w), String(h)], {
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });
    presenter.proc = proc;
    presenter.ready = false;
    let outBuf = '';
    proc.stdout.on('data', (c) => {
      outBuf += c.toString('utf8');
      let i = outBuf.indexOf('\n');
      while (i !== -1) {
        const line = outBuf.slice(0, i).replace(/\r$/, '').trim();
        outBuf = outBuf.slice(i + 1);
        if (line) onPresenterLine(line);
        i = outBuf.indexOf('\n');
      }
    });
    proc.stderr.on('data', (c) => smokeLog(`[presenter:err] ${c.toString('utf8').trim()}`));
    proc.on('exit', (code) => {
      smokeLog(`presenter exited code=${code}`);
      if (presenter.proc === proc) {
        presenter.proc = null;
        presenter.ready = false;
      }
    });
    // 等 PRESENTER_READY(有界 10s)
    const ok = await new Promise((resolve) => {
      const timer = setTimeout(() => resolve(false), 10_000);
      presenter.readyWaiters.push(() => {
        clearTimeout(timer);
        resolve(true);
      });
    });
    if (!ok || presenter.proc !== proc) {
      smokeLog('presenter 就绪超时,回退 canvas 腿');
      stopPresenter();
      return;
    }
    const share = await mcpCallHost('mcp__engine-scene__viewport_share_open', {
      pid: proc.pid,
      width: w,
      height: h,
    });
    presenterWrite(`bind ${share.handleKind === 'heap' ? 'heap' : 'tex'} ${share.texHandle} ${share.fenceHandle} ${w} ${h}`);
    presenter.texW = w;
    presenter.texH = h;
    presenter.x = x;
    presenter.y = y;
    smokeLog(`presenter embedded: pid=${proc.pid} ${w}x${h} tex=${share.texHandle} kind=${share.handleKind || '?'}`);
    // 可见冒烟:轮询 stat 直至 presented>=3 写证据
    if (isSmokeVisible && !presenter.statTimer) {
      presenter.statTimer = setInterval(() => presenterWrite('stat'), 500);
    }
    return;
  }

  if (!presenter.ready) return; // 正在启动,下轮 bounds 再同步
  if (w !== presenter.texW || h !== presenter.texH) {
    // 尺寸变化:重建共享纹理 + 重 bind(presenter 侧 swapchain 随 bind 重建)
    await mcpCallHost('mcp__engine-scene__viewport_share_close', {}).catch(() => {});
    const share = await mcpCallHost('mcp__engine-scene__viewport_share_open', {
      pid: presenter.proc.pid,
      width: w,
      height: h,
    });
    presenterWrite(`bind ${share.handleKind === 'heap' ? 'heap' : 'tex'} ${share.texHandle} ${share.fenceHandle} ${w} ${h}`);
    presenter.texW = w;
    presenter.texH = h;
  }
  if (x !== presenter.x || y !== presenter.y) {
    presenterWrite(`move ${x} ${y} ${presenter.texW} ${presenter.texH}`);
    presenter.x = x;
    presenter.y = y;
  }
}

function syncPresenter(b) {
  presenter.chain = presenter.chain
    .then(() => syncPresenterInner(b))
    .catch((err) => smokeLog(`syncPresenter: ${err.message}`));
}

function stopPresenter() {
  if (presenter.statTimer) {
    clearInterval(presenter.statTimer);
    presenter.statTimer = null;
  }
  const p = presenter.proc;
  presenter.proc = null;
  presenter.ready = false;
  presenter.texW = 0;
  presenter.texH = 0;
  if (p && p.exitCode === null && p.signalCode === null) {
    try {
      p.stdin.write('close\n');
    } catch {
      // 已退
    }
    setTimeout(() => {
      try {
        if (p.exitCode === null) p.kill();
      } catch {
        // 已退
      }
    }, 800);
  }
  // 共享纹理关闭幂等(host 不在则静默)
  mcpCallHost('mcp__engine-scene__viewport_share_close', {}).catch(() => {});
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

/**
 * 可见冒烟(G-F1-9 桌面腿):等外部脚本完成 OS 级截屏(os-capture-done.flag)再放行退出;
 * 非可见冒烟即时放行。flag 超时 90s 如实记日志后照常退出(不阻塞冒烟)。
 */
async function waitForOsCaptureFlag() {
  if (!isSmokeVisible) return;
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    if (fs.existsSync(OS_CAPTURE_DONE_FLAG)) {
      smokeLog('os-capture flag received');
      return;
    }
    if (!presenter.evidenceWritten) {
      // 证据 JSON 都还没写出来,先把窗口多留一会(stat 轮询仍在跑)
      smokeLog('waiting presenter evidence...');
      await new Promise((resolve) => setTimeout(resolve, 2000));
      continue;
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  smokeLog('os-capture flag timeout(90s),照常退出');
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
    show: !isSmoke || isSmokeVisible,
    backgroundColor: '#1e1e1e',
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      // 冒烟模式:离屏渲染确保证隐藏窗口也产生帧(对齐 apps/ide 旧 screenshot.cjs 已验证模式);
      // 可见冒烟(G-F1-9 桌面腿)必须非离屏,presenter 子窗口才能真实合成上屏
      offscreen: useOffscreen,
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
        .then(() => waitForOsCaptureFlag())
        .then(
          () => {
            stopPresenter();
            killHost();
            app.exit(0);
          },
          (captureErr) => {
            smokeLog(`capturePage failed: ${captureErr}`);
            stopPresenter();
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
  if (isSmoke) smokeLog(`flags: isSmoke=${isSmoke} visible=${isSmokeVisible} offscreen=${useOffscreen} presenterExe=${presenterExe() || 'none'}`);
  ipcMain.handle('forge:health', () => httpGetJson(HEALTH_URL));
  ipcMain.on('win:minimize', () => mainWindow && mainWindow.minimize());
  ipcMain.on('win:toggle-maximize', () => {
    if (!mainWindow) return;
    if (mainWindow.isMaximized()) mainWindow.unmaximize();
    else mainWindow.maximize();
  });
  ipcMain.on('win:close', () => mainWindow && mainWindow.close());
  ipcMain.on('viewport:bounds', (_e, b) => syncPresenter(b));

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
    stopPresenter();
    killHost();
    app.quit();
  });

  app.on('before-quit', () => {
    quitting = true;
    stopPresenter();
    killHost();
  });
}
