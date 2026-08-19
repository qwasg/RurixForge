'use strict';

const { app, BrowserWindow, dialog, ipcMain, shell } = require('electron');
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
// 冒烟场景:shell(默认,F7 wave.3 新壳)| chat(F7 wave.4 聊天)| settings(F7 wave.5 设置体系)
// | workbench(F7 wave.5 workbench)| editor | assets | nodegraph | gen | console-metrics
// F7 wave.3 留痕:home 场景退役(旧 Home 视图随 D-F7-D 下线);
// wave.5 留痕:settings 场景重建(全屏设置体系,旧 SettingsView 的退役占位由新体系取代)。
const smokeScenario = process.env.FORGE_SMOKE_SCENARIO || 'shell';
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

/** F7 wave.3:打开编辑器 tab 的统一 seam(client App.tsx 挂 window.__forgeShell)。
 * 旧侧栏「编辑器」按钮随 D-F7-D 下线;游戏原生场景(editor/assets/nodegraph/gen/
 * console-metrics)导航路径改为开编辑器 tab,改动最小化。 */
const OPEN_EDITOR_JS =
  "(() => { const s=window.__forgeShell; if(s && typeof s.openEditor==='function'){ s.openEditor(); return true; } return false; })()";

/** 冒烟场景脚本:shell = F7 wave.3 新壳断言;editor = 开编辑器 tab 再截 */
async function runSmokeScenario() {
  // F7 wave.3:home 场景退役(视图已删,见文件头留痕);wave.5:settings 场景重建(见下)。
  if (smokeScenario === 'home') {
    throw new Error(`场景已退役(F7 wave.3 D-F7-D): ${smokeScenario}`);
  }
  // F7 wave.5 G-F7-5:settings 场景 = 开设置 → 外观页切预设/明暗 → CSS 变量实测变化
  // → 技能页禁用写回实测(GET 复核 + 复原) → 关设置。
  if (smokeScenario === 'settings') {
    const evalJs = (code) => mainWindow.webContents.executeJavaScript(code);
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    await sleep(2500); // React 首挂 + loadAll 往返
    // 钉亮表(断言确定性)
    await evalJs("window.__forgeShell.setThemeMode('light')");
    // 开设置(外观页;setState 后 React 重渲染异步,先等再断言)
    await evalJs("window.__forgeShell.openSettings('appearance')");
    await sleep(400);
    const opened = await evalJs(
      "!!document.querySelector('[data-testid=\"settings-overlay\"]')"
    );
    smokeLog(`scenario=settings open: ${opened}`);
    if (!opened) throw new Error('设置 overlay 未开');
    const page = await evalJs(
      "(() => ({ overlay: !!document.querySelector('[data-testid=\"settings-overlay\"]'), appearance: !!document.querySelector('[data-testid=\"settings-page-appearance\"]'), nav: [...document.querySelectorAll('[data-testid^=\"settings-nav-\"]')].length }))()"
    );
    smokeLog(`scenario=settings page: ${JSON.stringify(page)}`);
    if (!page.appearance || page.nav !== 5) throw new Error(`设置页结构缺失: ${JSON.stringify(page)}`);
    // 外观页切预设 github(触发器与菜单项分两次 evaluate,等 React 渲染)
    const trigger = await evalJs(
      "(() => { const t=document.querySelector('[data-testid=\"preset-select-trigger\"]'); if(!t) return false; t.click(); return true; })()"
    );
    await sleep(300);
    const picked = trigger && await evalJs(
      "(() => { const it=document.querySelector('[data-testid=\"preset-select-item-github\"]'); if(!it) return false; it.click(); return true; })()"
    );
    smokeLog(`scenario=settings preset github: ${picked}`);
    if (!picked) throw new Error('预设 github 不可选');
    await sleep(200);
    // CSS 变量实测:github 亮表 accent #0969DA
    const light = await evalJs(
      "(() => { const cs=getComputedStyle(document.documentElement); const probe=(v)=>{ const el=document.createElement('span'); el.style.color=v; document.body.appendChild(el); const c=getComputedStyle(el).color; el.remove(); return c; }; return { theme: document.documentElement.dataset.theme, accent: probe(cs.getPropertyValue('--accent').trim()) }; })()"
    );
    smokeLog(`scenario=settings github light tokens: ${JSON.stringify(light)}`);
    if (light.accent !== 'rgb(9, 105, 218)') throw new Error(`预设未生效: ${light.accent} (期望 #0969DA)`);
    // 切深色(模式三卡)→ github 暗表 accent #4493F8
    await evalJs("(() => { document.querySelector('[data-testid=\"theme-mode-dark\"]').click(); return true; })()");
    await sleep(200);
    const dark = await evalJs(
      "(() => { const cs=getComputedStyle(document.documentElement); const probe=(v)=>{ const el=document.createElement('span'); el.style.color=v; document.body.appendChild(el); const c=getComputedStyle(el).color; el.remove(); return c; }; return { theme: document.documentElement.dataset.theme, accent: probe(cs.getPropertyValue('--accent').trim()) }; })()"
    );
    smokeLog(`scenario=settings github dark tokens: ${JSON.stringify(dark)}`);
    if (dark.theme !== 'dark') throw new Error(`data-theme 非 dark: ${dark.theme}`);
    if (dark.accent !== 'rgb(68, 147, 248)') throw new Error(`暗表预设未生效: ${dark.accent} (期望 #4493F8)`);
    // 回亮 + 回 moonlit(不污染后续场景)
    await evalJs("window.__forgeShell.setThemeMode('light')");
    await evalJs("(() => { const t=document.querySelector('[data-testid=\"preset-select-trigger\"]'); if(t) t.click(); return true; })()");
    await sleep(300);
    await evalJs("(() => { const it=document.querySelector('[data-testid=\"preset-select-item-moonlit\"]'); if(it) it.click(); return true; })()");
    // 技能页:禁用 asset-cleanup → GET 复核 enabled=false → 复原
    await evalJs("window.__forgeShell.openSettings('skills')");
    await sleep(600); // skills/list 往返
    const skillOff = await evalJs(
      "(async () => { const t=document.querySelector('[data-testid=\"skill-toggle-asset-cleanup\"]'); if(!t) return { found:false }; t.click(); await new Promise((r)=>setTimeout(r,500)); const r=await fetch('/api/forge/skills/list'); const v=await r.json(); const s=(v.skills||[]).find((x)=>x.name==='asset-cleanup'); return { found:true, enabled: s ? s.enabled : null }; })()"
    );
    smokeLog(`scenario=settings skill disable: ${JSON.stringify(skillOff)}`);
    if (!skillOff.found) throw new Error('技能行未见 asset-cleanup');
    if (skillOff.enabled !== false) throw new Error('技能禁用未写回(复核 enabled!=false)');
    const skillOn = await evalJs(
      "(async () => { const t=document.querySelector('[data-testid=\"skill-toggle-asset-cleanup\"]'); if(!t) return { found:false }; t.click(); await new Promise((r)=>setTimeout(r,500)); const r=await fetch('/api/forge/skills/list'); const v=await r.json(); const s=(v.skills||[]).find((x)=>x.name==='asset-cleanup'); return { found:true, enabled: s ? s.enabled : null }; })()"
    );
    smokeLog(`scenario=settings skill restore: ${JSON.stringify(skillOn)}`);
    if (skillOn.enabled !== true) throw new Error('技能复原未生效');
    // 回外观页截屏(设置 overlay 开态为终态)
    await evalJs("window.__forgeShell.openSettings('appearance')");
    await sleep(200);
    smokeLog('scenario=settings 全腿绿(开/预设/明暗实测/技能写回复核)');
    return;
  }
  // F7 wave.5 G-F7-5:workbench 场景 = todo tab(真实待办分列)/ 提案 tab(pending 批准接线)/
  // 底部面板 Agent Logs+Output(真实事件渲染)/ Inspector 树(仓根目录懒加载实测)。
  if (smokeScenario === 'workbench') {
    const evalJs = (code) => mainWindow.webContents.executeJavaScript(code);
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    await sleep(2500);
    // 状态复位(F8 wave.5 回归修复):共享 electron profile 的 localStorage forge:bottomPanel 跨运行持久化,
    //   前次终态 bottomOpen=true 会让本次 toggleBottom() 翻成「关」致底部面板断言失败。
    //   先写 open:false 归零再 reload,使 loadBottom() 读到关闭态 → toggleBottom() 恒为「关→开」,任意前态确定性通过。
    await evalJs("try { localStorage.setItem('forge:bottomPanel', JSON.stringify({ open: false, tab: 'logs' })); true } catch (e) { false }");
    mainWindow.webContents.reload();
    // reload 后等 __forgeShell 就绪(React 首挂 + App.tsx 挂 window.__forgeShell;轮询防时序抖动)。
    for (let i = 0; i < 40; i++) {
      const ready = await evalJs("!!(window.__forgeShell && typeof window.__forgeShell.createSession === 'function')").catch(() => false);
      if (ready) break;
      await sleep(250);
    }
    await evalJs("window.__forgeShell.setThemeMode('light')");
    // 建会话(触发 selectSession 快照回放 + SSE 订阅)
    const sid = await evalJs(
      "(async () => { const s=await window.__forgeShell.createSession('w5 workbench 冒烟'); return s ? s.id : null; })()"
    );
    smokeLog(`scenario=workbench createSession: ${sid}`);
    if (!sid) throw new Error('建会话失败');
    await sleep(800);
    // 造真实待办(REST POST → SSE todo.created → chatStore.todos)
    const todosMade = await evalJs(
      `(async () => { const mk=(t,d)=>fetch('/api/forge/todos',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({sessionId:'${sid}',title:t,description:d})}).then((r)=>r.ok); const a=await mk('w5 排队待办','看板卡描述两行'); const b=await mk('w5 完成待办'); return a && b; })()`
    );
    smokeLog(`scenario=workbench todos made: ${todosMade}`);
    if (!todosMade) throw new Error('待办创建失败');
    // 完成第二条(PATCH → todo.updated)
    await evalJs(
      "(async () => { const r=await fetch('/api/forge/sessions/" + sid + "/todos'); const v=await r.json(); const t=(v.todos||[]).find((x)=>x.title==='w5 完成待办'); if(!t) return false; const p=await fetch('/api/forge/todos/'+t.id,{method:'PATCH',headers:{'Content-Type':'application/json'},body:JSON.stringify({status:'completed'})}); return p.ok; })()"
    );
    await sleep(800); // SSE 事件到达窗口
    // todo tab:四列 + 卡
    await evalJs("window.__forgeShell.openTab('todo')");
    await sleep(300);
    const board = await evalJs(
      "(() => { const col=(l)=>{ const el=document.querySelector('[data-testid=\"todo-col-'+l+'\"]'); return el ? el.textContent : null; }; return { backlog: col('Backlog'), done: col('Done'), tab: !!document.querySelector('[data-testid=\"todo-tab\"]') }; })()"
    );
    smokeLog(`scenario=workbench todo board: ${JSON.stringify(board)}`);
    if (!board.tab) throw new Error('todo tab 未渲染');
    if (!board.backlog || !board.backlog.includes('w5 排队待办')) throw new Error('Backlog 列未见排队待办');
    if (!board.done || !board.done.includes('w5 完成待办')) throw new Error('Done 列未见完成待办');
    // 提案 tab:造 pending → 列表 → 批准 → approved
    const propId = await evalJs(
      "(async () => { const r=await fetch('/api/forge/proposals',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({kind:'asset.cleanup',summary:'w5 冒烟提案',impact:{assets:['Textures/a.png']}})}); const v=await r.json(); return v.id || null; })()"
    );
    smokeLog(`scenario=workbench proposal created: ${propId}`);
    if (!propId) throw new Error('提案创建失败');
    await evalJs("window.__forgeShell.openTab('proposals')");
    await sleep(500);
    const propRow = await evalJs(
      `(() => { const row=document.querySelector('[data-testid="proposal-row-${propId}"]'); const st=document.querySelector('[data-testid="proposal-status-${propId}"]'); return { row: !!row, status: st ? st.textContent : null }; })()`
    );
    smokeLog(`scenario=workbench proposal row: ${JSON.stringify(propRow)}`);
    if (!propRow.row || propRow.status !== 'pending') throw new Error(`提案行未见/非 pending: ${JSON.stringify(propRow)}`);
    const approved = await evalJs(
      `(async () => { const b=document.querySelector('[data-testid="proposal-approve-${propId}"]'); if(!b) return false; b.click(); await new Promise((r)=>setTimeout(r,600)); const st=document.querySelector('[data-testid="proposal-status-${propId}"]'); return st ? st.textContent : null; })()`
    );
    smokeLog(`scenario=workbench proposal approved: ${approved}`);
    if (approved !== 'approved') throw new Error(`批准未生效: ${approved}`);
    // 底部面板:Agent Logs(真实事件行) + Output(派生行) + Metrics 卡
    await evalJs("window.__forgeShell.openTab('plan')"); // plan tab 空态(尚无计划)如实
    await evalJs("window.__forgeShell.toggleBottom()");
    await sleep(300);
    const bottom = await evalJs(
      "(() => { const panel=document.querySelector('[data-testid=\"bottom-panel\"]'); const logs=document.querySelectorAll('[data-testid^=\"log-row-\"]').length; const planEmpty=document.querySelector('[data-testid=\"plan-body\"]')?.textContent.includes('尚无计划') ?? false; return { panel: !!panel, logs, planEmpty }; })()"
    );
    smokeLog(`scenario=workbench bottom logs: ${JSON.stringify(bottom)}`);
    if (!bottom.panel) throw new Error('底部面板未开');
    if (bottom.logs < 2) throw new Error(`Agent Logs 事件行不足: ${bottom.logs}`);
    if (!bottom.planEmpty) throw new Error('plan tab 空态未如实呈现');
    const output = await evalJs(
      "(() => { const t=document.querySelector('[data-testid=\"bottom-tab-output\"]'); if(!t) return null; t.click(); return new Promise((r)=>setTimeout(()=>{ const lines=document.querySelector('[data-testid=\"output-lines\"]'); r({ lines: lines ? lines.children.length : 0, text: lines ? lines.textContent.slice(0,120) : '' }); },300)); })()"
    );
    smokeLog(`scenario=workbench output: ${JSON.stringify(output)}`);
    if (!output || output.lines < 2) throw new Error('Output 派生行不足');
    const metrics = await evalJs(
      "(() => { const t=document.querySelector('[data-testid=\"bottom-tab-metrics\"]'); if(!t) return null; t.click(); return new Promise((r)=>setTimeout(()=>{ const tok=document.querySelector('[data-testid=\"metric-tokens\"]'); const sess=document.querySelector('[data-testid=\"metric-sessions\"]'); r({ tokens: tok ? tok.textContent : null, sessions: sess ? sess.textContent : null }); },300)); })()"
    );
    smokeLog(`scenario=workbench metrics: ${JSON.stringify(metrics)}`);
    if (!metrics || metrics.sessions === null || !metrics.sessions.includes('1/2')) throw new Error(`Metrics Sessions 派生异常: ${JSON.stringify(metrics)}`);
    // Inspector 树:仓根懒加载 → crates 目录 → 展开见 forge-agentd
    const tree = await evalJs(
      "(async () => { const w=async ()=>new Promise((r)=>setTimeout(r,300)); const root=await new Promise((r)=>{ const f=()=>{ const el=document.querySelector('[data-testid=\"ws-dir-crates\"]'); if(el) r(true); else setTimeout(f,200); }; f(); setTimeout(()=>r(false),5000); }); if(!root) return { root:false }; document.querySelector('[data-testid=\"ws-dir-crates\"]').click(); await w(); return { root:true, agentd: !!document.querySelector('[data-testid=\"ws-dir-crates/forge-agentd\"]') }; })()"
    );
    smokeLog(`scenario=workbench inspector tree: ${JSON.stringify(tree)}`);
    if (!tree.root) throw new Error('Inspector 仓根未见 crates(工作区树未加载)');
    if (!tree.agentd) throw new Error('crates 展开未见 forge-agentd(懒加载失败)');
    // 终态:todo tab + 底部面板 logs 开(截屏证据)
    await evalJs("window.__forgeShell.openTab('todo')");
    await evalJs("(() => { const t=document.querySelector('[data-testid=\"bottom-tab-logs\"]'); if(t) t.click(); return true; })()");
    await sleep(200);
    smokeLog('scenario=workbench 全腿绿(todo 看板/提案批准/底部面板/Inspector 树)');
    return;
  }
  // F7 wave.3 G-F7-3:shell 场景 = 三栏/titlebar/statusbar DOM + 主题 CSS 变量实测
  // + 明暗切换 + 编辑器 tab 嵌入存活 + New Agent 经真后端建行。
  if (smokeScenario === 'shell') {
    // 等 React 首挂 + loadAll 往返
    await new Promise((resolve) => setTimeout(resolve, 2500));
    const layout = await mainWindow.webContents.executeJavaScript(
      "(() => { const ids=['shell-titlebar','pane-sessions','pane-chat','pane-main','pane-inspector','shell-statusbar','pane-divider-sessions','pane-divider-chat','pane-divider-inspector']; const missing=ids.filter((id)=>!document.querySelector('[data-testid=\"'+id+'\"]')); return { ok: missing.length===0, missing }; })()"
    );
    smokeLog(`scenario=shell layout: ${JSON.stringify(layout)}`);
    if (!layout.ok) throw new Error(`壳 DOM 缺失: ${layout.missing.join(',')}`);
    // 主题 CSS 变量:先显式 setMode('light') 钉死确定性(mode=auto 随 OS,不可断言)
    const light = await mainWindow.webContents.executeJavaScript(
      "(() => { window.__forgeShell.setThemeMode('light'); const cs=getComputedStyle(document.documentElement); return { theme: document.documentElement.dataset.theme, accent: cs.getPropertyValue('--accent').trim(), bg: cs.getPropertyValue('--bg').trim(), sunk: cs.getPropertyValue('--bg-sunk').trim() }; })()"
    );
    smokeLog(`scenario=shell light tokens: ${JSON.stringify(light)}`);
    if (light.theme !== 'light') throw new Error(`data-theme 非 light: ${light.theme}`);
    // getComputedStyle 对 #RRGGBB 返回值可能是 rgb() 形态;统一解析后比对
    const norm = await mainWindow.webContents.executeJavaScript(
      "(() => { const cs=getComputedStyle(document.documentElement); const probe=(v)=>{ const el=document.createElement('span'); el.style.color=v; document.body.appendChild(el); const c=getComputedStyle(el).color; el.remove(); return c; }; return { accent: probe(cs.getPropertyValue('--accent').trim()), bg: probe(cs.getPropertyValue('--bg').trim()) }; })()"
    );
    smokeLog(`scenario=shell light tokens(computed): ${JSON.stringify(norm)}`);
    if (norm.accent !== 'rgb(201, 100, 66)') throw new Error(`亮表 --accent 实测不符: ${norm.accent} (期望 #C96442)`);
    if (norm.bg !== 'rgb(250, 249, 245)') throw new Error(`亮表 --bg 实测不符: ${norm.bg} (期望 #FAF9F5)`);
    // 暗表切换(store action,等价 View 菜单「切换主题」)
    const dark = await mainWindow.webContents.executeJavaScript(
      "(() => { window.__forgeShell.setThemeMode('dark'); const cs=getComputedStyle(document.documentElement); const probe=(v)=>{ const el=document.createElement('span'); el.style.color=v; document.body.appendChild(el); const c=getComputedStyle(el).color; el.remove(); return c; }; return { theme: document.documentElement.dataset.theme, accent: probe(cs.getPropertyValue('--accent').trim()), bg: probe(cs.getPropertyValue('--bg').trim()) }; })()"
    );
    smokeLog(`scenario=shell dark tokens(computed): ${JSON.stringify(dark)}`);
    if (dark.theme !== 'dark') throw new Error(`data-theme 非 dark: ${dark.theme}`);
    if (dark.accent !== 'rgb(226, 136, 106)') throw new Error(`暗表 --accent 实测不符: ${dark.accent} (期望 #E2886A)`);
    if (dark.bg !== 'rgb(28, 27, 24)') throw new Error(`暗表 --bg 实测不符: ${dark.bg} (期望 #1C1B18)`);
    // 回亮色截图(亮表为基准)
    await mainWindow.webContents.executeJavaScript("window.__forgeShell.setThemeMode('light')");
    // 开编辑器 tab → EditorView 骨架 + ViewportCanvas 状态(出帧或如实降级文本)
    const opened = await mainWindow.webContents.executeJavaScript(OPEN_EDITOR_JS);
    smokeLog(`scenario=shell openEditor seam: ${opened}`);
    if (!opened) throw new Error('__forgeShell.openEditor seam 不存在');
    // 编辑器多轮 MCP 往返(实体/摘要/PIE/事件/帧),留足窗口
    await new Promise((resolve) => setTimeout(resolve, 5000));
    const editor = await mainWindow.webContents.executeJavaScript(
      "(() => { const tab=document.querySelector('[data-testid=\"workbench-tab-editor\"]'); const texts=[...document.querySelectorAll('span')].map((x)=>x.textContent); const hierarchy=texts.includes('Hierarchy'); const canvas=document.querySelector('section[aria-label=\"Viewport\"] canvas'); const degrade=[...document.querySelectorAll('span')].map((x)=>x.textContent).find((t)=>t && t.includes('DEV_ENV_DEGRADE')) || null; return { tab: !!tab, hierarchy, canvas: !!canvas, degrade }; })()"
    );
    smokeLog(`scenario=shell editor embed: ${JSON.stringify(editor)}`);
    if (!editor.tab) throw new Error('编辑器 tab 未出现');
    if (!editor.hierarchy) throw new Error('EditorView 骨架未挂载(Hierarchy 缺失)');
    if (editor.degrade) smokeLog(`scenario=shell viewport 如实降级: ${editor.degrade}`);
    else if (!editor.canvas) throw new Error('ViewportCanvas 既无帧画布也无降级文本');
    // New Agent 经真后端(agentd 前置启动;POST /sessions 经 host 3080)
    const created = await mainWindow.webContents.executeJavaScript(
      "(async () => { const s=await window.__forgeShell.createSession('w3 冒烟会话'); return s ? s.id : null; })()"
    );
    smokeLog(`scenario=shell createSession: ${created}`);
    if (!created) throw new Error('New Agent 建会话失败(后端不可达?)');
    await new Promise((resolve) => setTimeout(resolve, 600));
    const row = await mainWindow.webContents.executeJavaScript(
      `(() => { const el=document.querySelector('[data-testid="session-row-${created}"]'); const sidebar=document.querySelector('[data-testid="sidebar"]'); return { row: !!el, inSidebar: sidebar ? sidebar.textContent.includes('w3 冒烟会话') : false }; })()`
    );
    smokeLog(`scenario=shell session row: ${JSON.stringify(row)}`);
    if (!row.row || !row.inSidebar) throw new Error('侧栏未见新会话行');
    return;
  }
  // F7 wave.4 G-F7-4:chat 场景 = mock 发消息 SSE 驱动 UI(用户卡+助手卡+铸方块+终态点)
  // + multitask 工具段(真 engine 链 swarm.execute「集群执行」)+ 编辑重发链(revert 生效)。
  if (smokeScenario === 'chat') {
    const evalJs = (code) => mainWindow.webContents.executeJavaScript(code);
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    const pollUntil = async (code, timeoutMs, label) => {
      const deadline = Date.now() + timeoutMs;
      let last = null;
      while (Date.now() < deadline) {
        last = await evalJs(code);
        if (last) return last;
        await sleep(300);
      }
      // 超时诊断转储(chatStore 状态 + toast 面),如实进日志再抛
      try {
        const diag = await evalJs(
          "(() => { const c=window.__forgeShell.stores.chat.getState(); return { msgs: c.messages.map((m)=>({id:m.id,role:m.role,status:m.status,blocks:m.blocks.map((b)=>b.kind),texts:m.blocks.filter((b)=>b.kind==='text').map((b)=>b.text.slice(0,60))})), activeRunId: c.activeRunId, latestSeq: c.latestSeq, hydrating: c.hydrating, domText: (document.querySelector('[data-testid=\"assistant-message\"]')||{}).textContent?.slice(0,120) || null }; })()"
        );
        smokeLog(`scenario=chat diag(${label}): ${JSON.stringify(diag)}`);
      } catch (e) {
        smokeLog(`scenario=chat diag 失败: ${e.message}`);
      }
      throw new Error(`轮询超时(${label})`);
    };
    // 等 React 首挂 + loadAll 往返
    await sleep(2500);
    // 主题钉亮(状态点色值断言确定性)
    await evalJs("window.__forgeShell.setThemeMode('light')");
    // multitask 腿场景准备:scene_new + 3 实体(真 engine 链,主进程侧经 host 3080)
    const seeded = await (async () => {
      try {
        await mcpCallHost('mcp__engine-scene__scene_new', { name: 'f7w4-chat' });
        for (const n of ['w4_a', 'w4_b', 'w4_c']) {
          await mcpCallHost('mcp__engine-scene__entity_create', { name: n });
        }
        return true;
      } catch (e) {
        smokeLog(`scenario=chat seed 失败(如实): ${e.message}`);
        return false;
      }
    })();
    smokeLog(`scenario=chat engine seed: ${seeded}`);
    if (!seeded) throw new Error('engine 场景准备失败(scene_new/entity_create)');

    // ── 腿 1:New Agent → composer 填发「说一句你好」→ mock 完成 ──
    const sid = await evalJs(
      "(async () => { const s=await window.__forgeShell.createSession(''); return s ? s.id : null; })()"
    );
    smokeLog(`scenario=chat createSession: ${sid}`);
    if (!sid) throw new Error('建会话失败');
    await sleep(800); // selectSession 快照 + SSE 订阅窗口
    // 强制 Mock provider(本机配了 deepseek key——经模型菜单选 Mock provider,
    // PATCH selectedModelId,agentd 侧 provider_for_session 强制 Mock,wave.4 后端小补)
    const modelMenuOpen = await evalJs(
      "(() => { const chip=document.querySelector('[data-testid=\"composer-model\"]'); if(!chip) return false; chip.click(); return true; })()"
    );
    await sleep(300); // React 渲染菜单后再取项(click 与查询分两次 evaluate)
    const modelPicked = modelMenuOpen && await evalJs(
      "(() => { const it=document.querySelector('[data-testid=\"model-item-mock\"]'); if(!it || it.disabled) return false; it.click(); return true; })()"
    );
    smokeLog(`scenario=chat pick mock model: ${modelPicked}`);
    if (!modelPicked) throw new Error('模型菜单选 Mock provider 失败');
    await sleep(400); // PATCH 往返窗口
    const fill = await evalJs(
      "(() => { const ta=document.querySelector('[data-testid=\"composer-input\"]'); if(!ta) return false; const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set; s.call(ta,'说一句你好'); ta.dispatchEvent(new Event('input',{bubbles:true})); return ta.value; })()"
    );
    smokeLog(`scenario=chat leg1 fill: ${fill}`);
    if (fill !== '说一句你好') throw new Error('composer 填充失败');
    await sleep(300); // React 消化 input 事件后再点发送
    const sent = await evalJs(
      "(() => { const b=document.querySelector('[data-testid=\"composer-send\"]'); if(b && !b.disabled){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=chat leg1 send click: ${sent}`);
    if (!sent) throw new Error('发送钮不可点');
    await pollUntil(
      "(() => { const m=[...document.querySelectorAll('[data-testid=\"assistant-message\"]')]; const a=m[m.length-1]; return a && a.textContent.includes('mock:已收到「说一句你好」') && !a.querySelector('[data-testid=\"stream-caret\"]'); })()",
      20000,
      'mock 完成'
    );
    const leg1 = await evalJs(
      "(() => { const users=[...document.querySelectorAll('[data-testid=\"user-message-card\"]')]; const assts=[...document.querySelectorAll('[data-testid=\"assistant-message\"]')]; const a=assts[assts.length-1]; return { users: users.length, asst: assts.length, avatar: !!a.querySelector('[data-testid=\"assistant-avatar\"]'), avatarText: a.querySelector('[data-testid=\"assistant-avatar\"]')?.textContent || '', model: a.querySelector('[data-testid=\"assistant-model\"]')?.textContent || '', userText: users[0]?.textContent.includes('说一句你好') }; })()"
    );
    smokeLog(`scenario=chat leg1 assert: ${JSON.stringify(leg1)}`);
    if (leg1.users !== 1 || leg1.asst !== 1) throw new Error(`消息数不符: ${JSON.stringify(leg1)}`);
    if (!leg1.avatar || leg1.avatarText !== '铸') throw new Error('助手卡「铸」方块缺失');
    if (leg1.model !== 'mock') throw new Error(`模型 label 非 mock: ${leg1.model}`);
    if (!leg1.userText) throw new Error('用户卡文本不含输入');

    // ── 腿 2:multitask「给场景加碰撞体 collider」→ swarm.execute 工具段 + completed ──
    const addOpened = await evalJs(
      "(() => { const add=document.querySelector('[data-testid=\"composer-add\"]'); if(!add) return false; add.click(); return true; })()"
    );
    await sleep(300); // React 渲染菜单后再取项
    const modeSet = addOpened && await evalJs(
      "(() => { const it=document.querySelector('[data-testid=\"mode-item-multitask\"]'); if(!it) return false; it.click(); return true; })()"
    );
    smokeLog(`scenario=chat leg2 mode multitask: ${modeSet}`);
    if (!modeSet) throw new Error('模式菜单 multitask 不可选');
    const fill2 = await evalJs(
      "(() => { const ta=document.querySelector('[data-testid=\"composer-input\"]'); if(!ta) return false; const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set; s.call(ta,'给场景加碰撞体 collider'); ta.dispatchEvent(new Event('input',{bubbles:true})); return ta.value; })()"
    );
    if (fill2 !== '给场景加碰撞体 collider') throw new Error('multitask 填充失败');
    await sleep(300);
    const sent2 = await evalJs(
      "(() => { const b=document.querySelector('[data-testid=\"composer-send\"]'); if(b && !b.disabled){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=chat leg2 send click: ${sent2}`);
    if (!sent2) throw new Error('multitask 发送钮不可点');
    // 等 swarm.execute 完成(SSE 驱动;真 engine 链给足窗口)
    await pollUntil(
      "(() => { const m=[...document.querySelectorAll('[data-testid=\"assistant-message\"]')]; const a=m[m.length-1]; return a && !a.querySelector('[data-testid=\"stream-caret\"]') && a.querySelector('[data-testid=\"activity-segment\"]'); })()",
      30000,
      'multitask 工具段终态'
    );
    // 展开工具段断言动词「集群执行」(展开与读取分两次 evaluate,等 React 渲染)
    const segClicked = await evalJs(
      "(() => { const m=[...document.querySelectorAll('[data-testid=\"assistant-message\"]')]; const a=m[m.length-1]; const seg=a.querySelector('[data-testid=\"activity-segment\"]'); if(!seg) return false; seg.click(); return true; })()"
    );
    if (!segClicked) throw new Error('工具段行不可点');
    await sleep(300);
    const leg2 = await evalJs(
      "(() => { const m=[...document.querySelectorAll('[data-testid=\"assistant-message\"]')]; const a=m[m.length-1]; const seg=a.querySelector('[data-testid=\"activity-segment\"]'); const lines=[...a.querySelectorAll('[data-testid^=\"tool-line-\"]')].map((x)=>x.textContent); const err=a.querySelector('[data-testid=\"assistant-error\"]'); return { segText: seg.textContent, lines, err: err ? err.textContent : null }; })()"
    );
    smokeLog(`scenario=chat leg2 assert: ${JSON.stringify(leg2)}`);
    if (leg2.err) throw new Error(`multitask 腿失败: ${leg2.err}`);
    if (!leg2.lines.some((l) => l.includes('集群执行'))) throw new Error(`未见「集群执行」工具行: ${JSON.stringify(leg2.lines)}`);

    // ── 腿 3:编辑重发链(点腿 1 用户卡 → 改文本 → 重发 → revert 生效) ──
    const before = await evalJs(
      "document.querySelectorAll('[data-testid=\"user-message-card\"]').length + '|' + document.querySelectorAll('[data-testid=\"assistant-message\"]').length"
    );
    smokeLog(`scenario=chat leg3 before: ${before}`);
    const editClicked = await evalJs(
      "(() => { const t=document.querySelector('[data-testid=\"user-message-text\"]'); if(!t) return false; t.click(); return true; })()"
    );
    if (!editClicked) throw new Error('用户卡正文不可点');
    await sleep(300); // 等内联编辑渲染
    const editOpened = await evalJs(
      "(() => { const i=document.querySelector('[data-testid=\"user-edit-input\"]'); if(!i) return false; const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set; s.call(i,'说一句你好呀'); i.dispatchEvent(new Event('input',{bubbles:true})); return i.value; })()"
    );
    smokeLog(`scenario=chat leg3 edit fill: ${editOpened}`);
    if (editOpened !== '说一句你好呀') throw new Error('内联编辑填充失败');
    await sleep(300);
    const resent = await evalJs(
      "(() => { const b=document.querySelector('[data-testid=\"user-edit-resend\"]'); if(!b) return false; b.click(); return true; })()"
    );
    if (!resent) throw new Error('重发钮缺失');
    // revert 生效:消息数回退(4 → 截断至少仅剩新乐观卡)后重增至 2 且终态
    await pollUntil(
      "(() => { const u=document.querySelectorAll('[data-testid=\"user-message-card\"]').length; const a=document.querySelectorAll('[data-testid=\"assistant-message\"]').length; return u===1 && a===1; })()",
      20000,
      'revert 后消息数回退重增'
    );
    const leg3 = await evalJs(
      "(() => { const m=[...document.querySelectorAll('[data-testid=\"assistant-message\"]')]; const a=m[m.length-1]; const done=a && a.textContent.includes('mock:已收到「说一句你好呀」') && !a.querySelector('[data-testid=\"stream-caret\"]'); const stale=document.querySelector('[data-testid=\"message-list\"]').textContent.includes('给场景加碰撞体'); return { done, stale }; })()"
    );
    smokeLog(`scenario=chat leg3 assert: ${JSON.stringify(leg3)}`);
    if (!leg3.done) throw new Error('重发后 mock 回文未达终态');
    if (leg3.stale) throw new Error('revert 未生效:腿 2 消息仍残留');
    smokeLog('scenario=chat 三腿全绿(用户卡/助手卡/工具段/终态/编辑重发 revert)');
    return;
  }

  // (F7 wave.3:旧 settings 场景体已随分支前退役 throw 移除;f3-w4 脚本待 wave.5 设置体系重建后换新场景)
  // F2 wave.3 G-F2-3:assets 场景 = editor 导航 + 资产条目计数断言。
  if (smokeScenario === 'assets') {
    const clicked = await mainWindow.webContents.executeJavaScript(
      OPEN_EDITOR_JS
    );
    smokeLog(`scenario=assets nav click: ${clicked}`);
    if (!clicked) throw new Error('__forgeShell.openEditor seam 不存在');
    // 等 Assets 面板 load() 完成(asset_list + asset_build_status 两往返)。
    await new Promise((resolve) => setTimeout(resolve, 4000));
    const count = await mainWindow.webContents.executeJavaScript(
      "document.querySelectorAll('[data-asset-guid]').length"
    );
    smokeLog(`scenario=assets asset items: ${count}`);
    if (count < 1) throw new Error(`Assets 面板无资产条目(count=${count})`);
    return;
  }
  // F4 wave.4 G-F4-3:nodegraph 场景 = editor 导航 → 切 NodeGraph 页签 → 图路径加载
  // door_opener.rxgraph(graph_get 经 host→agentd→code-forge-mcp)→ 节点卡片计数断言。
  if (smokeScenario === 'nodegraph') {
    const nav = await mainWindow.webContents.executeJavaScript(
      OPEN_EDITOR_JS
    );
    smokeLog(`scenario=nodegraph nav click: ${nav}`);
    if (!nav) throw new Error('__forgeShell.openEditor seam 不存在');
    // 编辑器挂载后有多轮 MCP 往返(实体/摘要/PIE/事件),留足窗口
    await new Promise((resolve) => setTimeout(resolve, 4000));
    // 切 NodeGraph 页签(无选中实体 → 空态,路径输入框在顶栏常驻)
    const tab = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=[...document.querySelectorAll('button')].find((x)=>x.textContent.trim()==='NodeGraph'); if(b){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=nodegraph tab click: ${tab}`);
    if (!tab) throw new Error('未找到 NodeGraph 页签按钮');
    await new Promise((resolve) => setTimeout(resolve, 500));
    // 填图路径(React 受控 input 须走原生 setter + input 事件)
    const filled = await mainWindow.webContents.executeJavaScript(
      "(() => { const i=document.querySelector('[data-graph-path-input]'); if(!i) return false; const s=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set; s.call(i,'Content/Graphs/door_opener.rxgraph'); i.dispatchEvent(new Event('input',{bubbles:true})); return i.value; })()"
    );
    smokeLog(`scenario=nodegraph path filled: ${filled}`);
    if (!filled) throw new Error('未找到图路径输入框(data-graph-path-input)');
    // 分两步点「加载」:让 React 先消化 input 事件再读 pathDraft
    await new Promise((resolve) => setTimeout(resolve, 400));
    const loaded = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=document.querySelector('[data-graph-load]'); if(b && !b.disabled){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=nodegraph load click: ${loaded}`);
    if (!loaded) throw new Error('「加载」按钮不可点');
    // 等 graph_get 往返 + 节点渲染
    await new Promise((resolve) => setTimeout(resolve, 2000));
    const count = await mainWindow.webContents.executeJavaScript(
      "document.querySelectorAll('[data-graph-node]').length"
    );
    smokeLog(`scenario=nodegraph graph nodes: ${count}`);
    if (count < 4) throw new Error(`NodeGraph 节点卡片不足(count=${count} < 4)`);
    return;
  }
  // F5 wave.3 G-F5-3:gen 场景 = editor 导航 → Assets 右键「Generate...」→ 对话框填
  // prompt「wood 木纹」→ 生成候选 → 断言候选卡 >=4 → Accept 第一张 → 断言 Assets 列表
  // 出现新资产(wood-<seed>)。前置:agentd 侧 gen-backends.json 已配 local-mock(脚本写)。
  if (smokeScenario === 'gen') {
    const nav = await mainWindow.webContents.executeJavaScript(
      OPEN_EDITOR_JS
    );
    smokeLog(`scenario=gen nav click: ${nav}`);
    if (!nav) throw new Error('__forgeShell.openEditor seam 不存在');
    // 等 Assets 面板 load() 完成(asset_list + asset_build_status 两往返)。
    await new Promise((resolve) => setTimeout(resolve, 4000));
    const assetCount = await mainWindow.webContents.executeJavaScript(
      "document.querySelectorAll('[data-asset-guid]').length"
    );
    smokeLog(`scenario=gen asset items: ${assetCount}`);
    if (assetCount < 1) throw new Error(`Assets 面板无资产条目(count=${assetCount})`);
    // 右键首个资产(分两次 executeJavaScript:先开菜单,React 消化后再点菜单项)。
    const ctx = await mainWindow.webContents.executeJavaScript(
      "(() => { const el=document.querySelector('[data-asset-guid]'); if(!el) return false; el.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:120,clientY:120})); return true; })()"
    );
    smokeLog(`scenario=gen contextmenu dispatch: ${ctx}`);
    if (!ctx) throw new Error('Assets 面板无可右键资产条目');
    await new Promise((resolve) => setTimeout(resolve, 400));
    const menuOpened = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=[...document.querySelectorAll('button')].find((x)=>x.textContent.trim()==='Generate...'); if(b){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=gen Generate menu click: ${menuOpened}`);
    if (!menuOpened) throw new Error('右键菜单「Generate...」不可点');
    await new Promise((resolve) => setTimeout(resolve, 800));
    // 填 prompt(React 受控 textarea 须走原生 setter + input 事件)。
    const filled = await mainWindow.webContents.executeJavaScript(
      "(() => { const i=document.querySelector('[data-gen-prompt]'); if(!i) return false; const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set; s.call(i,'wood 木纹'); i.dispatchEvent(new Event('input',{bubbles:true})); return i.value; })()"
    );
    smokeLog(`scenario=gen prompt filled: ${filled}`);
    if (!filled) throw new Error('未找到 prompt 输入框(data-gen-prompt)');
    // 等后端清单加载(GET /api/forge/gen/backends 经 host 代理)+ React 消化 input。
    await new Promise((resolve) => setTimeout(resolve, 1200));
    const submitted = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=document.querySelector('[data-gen-submit]'); if(b && !b.disabled){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=gen submit click: ${submitted}`);
    if (!submitted) throw new Error('「生成候选」按钮不可点(后端未配置或 prompt 空)');
    // 等 gen_image 往返 + 候选渲染。
    await new Promise((resolve) => setTimeout(resolve, 3000));
    const candCount = await mainWindow.webContents.executeJavaScript(
      "document.querySelectorAll('[data-gen-candidate]').length"
    );
    smokeLog(`scenario=gen candidates: ${candCount}`);
    if (candCount < 4) throw new Error(`生成候选不足(count=${candCount} < 4)`);
    // Accept 第一张 → gen_accept 入管线 → 资产刷新。
    const accepted = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=document.querySelector('[data-gen-accept]'); if(b && !b.disabled){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=gen accept click: ${accepted}`);
    if (!accepted) throw new Error('首张候选 Accept 不可点');
    await new Promise((resolve) => setTimeout(resolve, 2500));
    const newAssets = await mainWindow.webContents.executeJavaScript(
      "document.querySelectorAll('[data-asset-path*=\"wood-\"]').length"
    );
    smokeLog(`scenario=gen accepted assets: ${newAssets}`);
    if (newAssets < 1) throw new Error('Accept 后 Assets 列表未见新资产(wood-<seed>)');
    return;
  }
  // F6 wave.3 G-F6-3:console-metrics 场景 = metrics tab 采样实测变化(PIE 运行中)
  // + Console playtest 报告注入 / 类型过滤 / 清空。
  if (smokeScenario === 'console-metrics') {
    const navEd = await mainWindow.webContents.executeJavaScript(
      OPEN_EDITOR_JS
    );
    smokeLog(`scenario=console-metrics nav click: ${navEd}`);
    if (!navEd) throw new Error('__forgeShell.openEditor seam 不存在');
    await new Promise((resolve) => setTimeout(resolve, 4000));
    const mt = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=[...document.querySelectorAll('button')].find((x)=>x.textContent.trim()==='metrics'); if(b){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=console-metrics metrics tab: ${mt}`);
    if (!mt) throw new Error('未找到 metrics tab');
    await mainWindow.webContents.executeJavaScript(
      "fetch('/api/forge/mcp/call',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({tool:'mcp__engine-scene__play_enter',arguments:{}})}).then(r=>r.json())"
    );
    await new Promise((resolve) => setTimeout(resolve, 4500));
    const framesText = await mainWindow.webContents.executeJavaScript(
      "(() => { const rows=[...document.querySelectorAll('div')].filter((d)=>d.firstElementChild&&d.firstElementChild.textContent==='frames'); if(!rows.length) return ''; return rows[0].lastElementChild.textContent; })()"
    );
    smokeLog(`scenario=console-metrics frames recent: ${framesText}`);
    const vals = String(framesText).trim().split(/\s+/).map(Number).filter((n) => !Number.isNaN(n));
    if (vals.length < 2) throw new Error(`metrics 采样不足(<2): ${framesText}`);
    if (new Set(vals).size < 2) throw new Error(`PIE 运行中采样未变化: ${framesText}`);
    // metrics 腿 play_enter 后单例 host 处于 play_running:先 play_exit 再跑矩阵(scene.load 禁 play 态)。
    const pexit = await mainWindow.webContents.executeJavaScript(
      "fetch('/api/forge/mcp/call',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({tool:'mcp__engine-scene__play_exit',arguments:{}})}).then(r=>r.json())"
    );
    smokeLog(`scenario=console-metrics play_exit: ${JSON.stringify(pexit).slice(0, 80)}`);
    const ct = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=[...document.querySelectorAll('button')].find((x)=>x.textContent.trim()==='console'); if(b){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=console-metrics console tab: ${ct}`);
    const rp = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=[...document.querySelectorAll('button[title]')].find((x)=>x.title.startsWith('Run maze playtest')); if(b){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=console-metrics run playtest: ${rp}`);
    if (!rp) throw new Error('未找到 Run playtest 按钮');
    await new Promise((resolve) => setTimeout(resolve, 12000));
    const reportCount = await mainWindow.webContents.executeJavaScript(
      "[...document.querySelectorAll('p')].filter((p)=>p.textContent.includes('playtest.report')).length"
    );
    smokeLog(`scenario=console-metrics playtest.report rows: ${reportCount}`);
    if (reportCount < 1) throw new Error('Console 未见 playtest.report 注入行');
    const chip = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=[...document.querySelectorAll('button')].find((x)=>x.textContent.trim().startsWith('playtest.case')); if(b){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=console-metrics filter chip playtest.case: ${chip}`);
    if (!chip) throw new Error('未找到 playtest.case 过滤 chip');
    await new Promise((resolve) => setTimeout(resolve, 500));
    const caseVisible = await mainWindow.webContents.executeJavaScript(
      "[...document.querySelectorAll('p')].filter((p)=>p.textContent.includes('playtest.case')).length"
    );
    smokeLog(`scenario=console-metrics filtered playtest.case rows: ${caseVisible}`);
    if (caseVisible !== 0) throw new Error(`过滤后仍见 playtest.case 行: ${caseVisible}`);
    const cleared = await mainWindow.webContents.executeJavaScript(
      "(() => { const b=[...document.querySelectorAll('button')].find((x)=>x.textContent.trim()==='清空'); if(b){b.click();return true;} return false; })()"
    );
    smokeLog(`scenario=console-metrics clear click: ${cleared}`);
    await new Promise((resolve) => setTimeout(resolve, 500));
    const left = await mainWindow.webContents.executeJavaScript(
      "[...document.querySelectorAll('p')].filter((p)=>p.textContent.includes('playtest.report')).length"
    );
    smokeLog(`scenario=console-metrics rows after clear: ${left}`);
    if (left !== 0) throw new Error(`清空后仍见报告行: ${left}`);
    return;
  }
  if (smokeScenario !== 'editor') return;
  const clicked = await mainWindow.webContents.executeJavaScript(OPEN_EDITOR_JS);
  smokeLog(`scenario=editor openEditor seam: ${clicked}`);
  if (!clicked) throw new Error('__forgeShell.openEditor seam 不存在');
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

  // 冒烟期渲染进程 console 透传(定位 UI 黑盒故障;仅 isSmoke)
  if (isSmoke) {
    mainWindow.webContents.on('console-message', (_e, level, message) => {
      if (level >= 2) smokeLog(`[renderer:${level}] ${String(message).slice(0, 300)}`);
    });
  }

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

  // F2 wave.3:Assets 面板右键菜单桌面能力(仅桌面端可用;web 端菜单项如实禁用)。
  // 导入到此处:系统文件对话框选源文件(多选),返回绝对路径列表,renderer 再走 asset_import。
  ipcMain.handle('assets:pick-import', async () => {
    if (!mainWindow) return [];
    const r = await dialog.showOpenDialog(mainWindow, {
      title: '导入资产到 Content',
      properties: ['openFile', 'multiSelections'],
      filters: [
        { name: 'Assets', extensions: ['gltf', 'glb', 'png', 'jpg', 'jpeg'] },
        { name: 'All Files', extensions: ['*'] },
      ],
    });
    return r.canceled ? [] : r.filePaths;
  });
  // 在文件夹中显示:rel = Content 相对路径,主进程拼项目根(demo)后 showItemInFolder。
  ipcMain.on('assets:show-in-folder', (_e, rel) => {
    if (typeof rel !== 'string' || rel.includes('..') || rel.includes(':')) return;
    const abs = path.join(repoRoot, 'projects', 'demo', 'Content', rel);
    if (fs.existsSync(abs)) shell.showItemInFolder(abs);
  });

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
