#!/usr/bin/env node
/**
 * F8 wave.4 浏览器真实任务矩阵(G-F8-4):纯浏览器(playwright-core + 系统 Edge/Chrome,
 * 不下载浏览器二进制)经 host(3080 静态托管 + /api/forge/* 代理)→ agentd(8103)→
 * MCP 真 engine 链,八任务逐独立布尔断言 + 截图 evidence。
 *
 * 任务矩阵(契约 wave.4):
 *   T1 建会话→选 Mock 模型→发消息→SSE 驱动 UI:用户卡+助手卡+终态点 DOM 断言
 *   T2 deepseek live 发消息:design-snapshot availability=available 则实测断言 completed;
 *      否则该腿如实标 annotated-mock(不充绿,契约允许)
 *   T3 编辑器视图浏览器出帧:canvas readback 轮询腿帧计数递增+设备名真实;
 *      场景有实体后(T4 后复测)断言 nonZeroPixels>0
 *   T4 chat 指令创建实体经 agent 工具循环真 engine 链:openai-compat 渠道指向本脚本
 *      内置确定性 mock OpenAI 服务器(仅 LLM 决策桩,agent 工具循环/MCP/engine 链全真),
 *      断言 entity_list 出现新实体 + 视口 nonZeroPixels>0
 *   T5 multitask 碰撞体 swarm:断言工具段「集群执行」+ completed
 *   T6 提案批准流:workbench 提案 tab pending 行「批准」→ approved
 *   T7 设置主题切换:dark/light 切,断言 data-theme 与 --accent computed 值变化
 *   T8 会话管理:fork(标题「分支 · 」)/重命名/置顶/删除,逐项 UI+后端断言
 *
 * 编排纪律:构建检查(cargo 七二进制 + client/host dist)→ 端口预检(占用者按命令行
 * 确认本仓 dev 实例后停掉留痕)→ agentd(FORGE_AGENTD_DATA_DIR/FORGE_GEN_DATA_DIR
 * 双隔离,FORGE_LLM_API_KEY 继承父进程环境,密钥永不打印)→ mock OpenAI 服务器
 * (T4 用,仅 LLM 桩)→ host → 八任务 → finally 清理(taskkill /T + 启动时间/命令行
 * 过滤防误杀,零孤儿校验)。exit code:任何任务 fail → 1;annotated-mock → 0 但如实记录。
 *
 * 用法:
 *   node tools/e2e/f8-w4-browser-matrix.mjs
 *   或薄包装:powershell -NoProfile -ExecutionPolicy Bypass -File scripts/f8-w4-browser-matrix-smoke.ps1
 * 产物:
 *   evidence/f8-w4-T{n}-*.png(每任务截图)
 *   evidence/f8-w4-matrix-<UTC>.json(逐任务 verdict+断言+数字)
 *   evidence/f8-w4-browser-matrix-<UTC>.log(全程日志)
 */
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

// ---------- 基本盘 ----------
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const EVIDENCE = path.join(ROOT, 'evidence');
const TS = new Date().toISOString().replace(/[:.]/g, '-').replace('T', 'T').slice(0, 19) + 'Z';
const STARTED = Date.now();
const AGENTD_PORT = 8103;
const HOST_PORT = 3080;
const HOST_ORIGIN = `http://127.0.0.1:${HOST_PORT}`;
const AGENTD_ORIGIN = `http://127.0.0.1:${AGENTD_PORT}`;
const ENT_NAME = `E2E-Cube-${TS.replace(/[^0-9A-Za-z]/g, '').slice(-8)}`;
const OAI_MODEL = 'e2e-deterministic-llm';
// 假 key:仅写入隔离数据目录 keystore(FORGE_GEN_DATA_DIR 临时目录),非任何真实机密;
// 刻意不带 sk- 前缀,防日志/截图红线误伤。
const OAI_DUMMY_KEY = 'e2e-local-dummy-key-not-a-secret';

fs.mkdirSync(EVIDENCE, { recursive: true });
const LOG_FILE = path.join(EVIDENCE, `f8-w4-browser-matrix-${TS}.log`);
const MATRIX_FILE = path.join(EVIDENCE, `f8-w4-matrix-${TS}.json`);
const TMP = fs.mkdtempSync(path.join(os.tmpdir(), 'f8-w4-e2e-'));
const AGENT_DATA = path.join(TMP, 'agent-data');
const GEN_DATA = path.join(TMP, 'gen-data');

function log(m) {
  const line = `[${new Date().toISOString().slice(11, 19)}] ${m}`;
  console.log(line);
  fs.appendFileSync(LOG_FILE, line + '\n');
}

// ---------- 小工具 ----------
function sleep(ms) { return new Promise((r) => setTimeout(r, ms)); }

/** 跑命令捕获输出(shell 形态,Windows pnpm/cargo 通吃);非零 exit 抛错(带末段输出)。 */
function run(cmd, opts = {}) {
  const { cwd = ROOT, timeoutMs = 20 * 60_000, allowFail = false, quiet = false } = opts;
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, { cwd, shell: true, env: process.env });
    let out = '';
    const timer = setTimeout(() => {
      try { execFileSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); } catch {}
      reject(new Error(`命令超时(${timeoutMs}ms): ${cmd}`));
    }, timeoutMs);
    child.stdout?.on('data', (d) => { out += d.toString(); });
    child.stderr?.on('data', (d) => { out += d.toString(); });
    child.on('error', (e) => { clearTimeout(timer); reject(e); });
    child.on('exit', (code) => {
      clearTimeout(timer);
      if (!quiet) {
        const tail = out.trim().split(/\r?\n/).slice(-4).join(' | ');
        if (tail) log(`  $ ${cmd.split(' ').slice(0, 3).join(' ')} … exit=${code} :: ${tail.slice(0, 300)}`);
      }
      if (code !== 0 && !allowFail) reject(new Error(`命令失败(exit=${code}): ${cmd}\n${out.slice(-2000)}`));
      else resolve({ code: code ?? -1, out });
    });
  });
}

async function portFree(port) {
  return new Promise((resolve) => {
    const srv = net.createServer();
    srv.once('error', () => resolve(false));
    srv.once('listening', () => srv.close(() => resolve(true)));
    srv.listen(port, '127.0.0.1');
  });
}

/** 端口监听 pid 清单(netstat 解析;仅 LISTENING)。 */
function listenerPids(port) {
  try {
    const out = execFileSync('netstat', ['-ano', '-p', 'tcp'], { encoding: 'utf8' });
    const pids = new Set();
    for (const line of out.split(/\r?\n/)) {
      if (!line.includes('LISTENING')) continue;
      const cols = line.trim().split(/\s+/);
      if (cols.length >= 5 && cols[1].endsWith(`:${port}`)) pids.add(Number(cols[4]));
    }
    return [...pids].filter((n) => Number.isInteger(n) && n > 0);
  } catch { return []; }
}

function pidCommandLine(pid) {
  try {
    const out = execFileSync('powershell', [
      '-NoProfile', '-Command',
      `(Get-CimInstance Win32_Process -Filter "ProcessId=${pid}").CommandLine`,
    ], { encoding: 'utf8' });
    return out.trim();
  } catch { return ''; }
}

function killTree(pid) {
  try { execFileSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' }); return true; }
  catch { return false; }
}

/** 端口占用处置:本仓 dev 实例(命令行含特征路径)→ 停掉留痕;否则拒绝启动。 */
function reclaimPort(port, signature) {
  const pids = listenerPids(port);
  for (const pid of pids) {
    const cmd = pidCommandLine(pid);
    if (signature.test(cmd)) {
      log(`  端口 ${port} 被本仓 dev 实例占用(pid=${pid}),停掉: ${cmd.slice(0, 160)}`);
      killTree(pid);
    } else {
      throw new Error(`端口 ${port} 被未知进程占用(pid=${pid}, cmd=${cmd.slice(0, 120)}),不越权清理,中止`);
    }
  }
}

async function waitHttp(url, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const r = await fetch(url, { signal: AbortSignal.timeout(2000) });
      if (r.ok) return true;
    } catch {}
    if (Date.now() > deadline) throw new Error(`${label} 就绪超时: ${url}`);
    await sleep(300);
  }
}

/** host 代理面 REST(八任务全部经 3080,顺带实测代理)。 */
async function hapi(method, p, body) {
  const res = await fetch(`${HOST_ORIGIN}${p}`, {
    method,
    headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch {}
  return { status: res.status, json, text };
}

/** MCP 工具调用(经 host 代理;拆信封 content[0].text 二次解析)。 */
async function mcp(tool, args = {}) {
  const r = await hapi('POST', '/api/forge/mcp/call', { tool, arguments: args });
  if (r.status !== 200) throw new Error(`mcp ${tool} HTTP ${r.status}: ${r.text.slice(0, 300)}`);
  const env = r.json;
  const txt = env?.content?.[0]?.text;
  if (typeof txt === 'string') { try { return JSON.parse(txt); } catch { return txt; } }
  return env?.structuredContent ?? env;
}

// ---------- 结果记录 ----------
const matrix = {
  contract: 'F8 wave.4 / G-F8-4',
  startedUtc: new Date(STARTED).toISOString(),
  host: { origin: HOST_ORIGIN, agentdOrigin: AGENTD_ORIGIN },
  env: {},
  prep: [],
  tasks: [],
  hygiene: { pageErrors: [], consoleErrors: [] },
  orphanCheck: { before: [], after: [] },
};
const results = [];

function check(t, name, pass, detail = '') {
  t.assertions.push({ name, pass: !!pass, detail: String(detail) });
  if (!pass) log(`  [FAIL][${t.id}] ${name}: ${detail}`);
  else log(`  [ok][${t.id}] ${name}${detail ? `: ${String(detail).slice(0, 160)}` : ''}`);
}

async function shot(page, t, name) {
  const file = path.join(EVIDENCE, `f8-w4-${t.id}-${name}.png`);
  try {
    await page.screenshot({ path: file, fullPage: false });
    t.screenshots.push(path.relative(ROOT, file));
    log(`  [shot] ${path.basename(file)}`);
  } catch (e) { log(`  [shot-fail] ${name}: ${e.message}`); }
}

async function task(id, title, fn) {
  const t = { id, title, verdict: 'fail', assertions: [], screenshots: [], notes: [], durationMs: 0 };
  results.push(t);
  const started = Date.now();
  log(`== ${id} ${title} ==`);
  try {
    await fn(t);
    if (t.verdict !== 'annotated-mock') {
      t.verdict = t.assertions.length > 0 && t.assertions.every((a) => a.pass) ? 'pass' : 'fail';
    }
  } catch (e) {
    t.assertions.push({ name: 'unhandled-exception', pass: false, detail: String(e?.message ?? e) });
    t.verdict = 'fail';
    log(`  [EXC][${id}] ${e?.stack ?? e}`);
  }
  t.durationMs = Date.now() - started;
  log(`-- ${id} verdict=${t.verdict} (${t.durationMs}ms)`);
}

// ---------- 编排:进程句柄 ----------
let agentdChild = null;
let hostChild = null;
let mockServer = null;
let browser = null;

function spawnLogged(name, cmd, args, env) {
  const child = spawn(cmd, args, { cwd: ROOT, env: { ...process.env, ...env }, windowsHide: true });
  child.stdout?.on('data', (d) => fs.appendFileSync(LOG_FILE, `[${name}] ${d.toString()}`));
  child.stderr?.on('data', (d) => fs.appendFileSync(LOG_FILE, `[${name}!] ${d.toString()}`));
  return child;
}

/** 孤儿扫描:按进程名+启动时间+命令行特征三条件(防误杀/防漏杀留痕)。 */
function orphanScan() {
  const names = ['forge-agentd', 'engine-scene-mcp', 'engine-host', 'code-forge-mcp',
    'asset-pipeline-mcp', 'gen-image-mcp', 'gen-model-mcp'];
  const since = new Date(STARTED - 5000).toISOString();
  const ps = `
$t0=[DateTime]::Parse('${since}');
$hit=@();
Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
  ($_.CreationDate -ge $t0) -and (
    (${names.map((n) => `$_.Name -eq '${n}.exe'`).join(' -or ')}) -or
    ($_.Name -eq 'node.exe' -and $_.CommandLine -match 'packages[\\\\/]host[\\\\/]dist[\\\\/]index\\.js') -or
    ($_.Name -match '^(msedge|chrome)\\.exe$' -and $_.CommandLine -match 'playwright')
  )
} | ForEach-Object { "$($_.ProcessId)|$($_.Name)" }`;
  try {
    const out = execFileSync('powershell', ['-NoProfile', '-Command', ps], { encoding: 'utf8' });
    return out.trim().split(/\r?\n/).filter(Boolean);
  } catch { return ['(scan-error)']; }
}

async function cleanup() {
  log('== finally 清理(browser → host → agentd → mock-server → 孤儿扫荡) ==');
  try { await browser?.close(); } catch {}
  browser = null;
  for (const [label, child] of [['host', hostChild], ['agentd', agentdChild]]) {
    if (child?.pid) {
      const ok = killTree(child.pid);
      log(`  taskkill ${label} pid=${child.pid} tree → ${ok ? 'done' : 'already-exit'}`);
    }
  }
  agentdChild = null;
  hostChild = null;
  try { mockServer?.close(); } catch {}
  mockServer = null;
  await sleep(800);
  // 兜底:名字+启动时间过滤的连带清杀(同 f6/f7 冒烟纪律)
  const left = orphanScan();
  for (const row of left) {
    const pid = Number(row.split('|')[0]);
    if (Number.isInteger(pid) && pid > 0 && pid !== process.pid) {
      log(`  兜底清杀 ${row}`);
      killTree(pid);
    }
  }
  await sleep(600);
  matrix.orphanCheck.after = orphanScan();
  try { fs.rmSync(TMP, { recursive: true, force: true }); } catch {}
}

// ---------- 浏览器断言辅助 ----------
async function forgeReady(page) {
  await page.waitForFunction(() => !!window.__forgeShell, null, { timeout: 20000 });
}
async function activeSessionId(page) {
  return page.evaluate(() => window.__forgeShell.stores.sessions.getState().activeSessionId);
}
async function chatState(page) {
  return page.evaluate(() => {
    const c = window.__forgeShell.stores.chat.getState();
    return {
      activeRunId: c.activeRunId,
      models: c.models,
      selectedModelId: c.selectedModelId,
      messages: c.messages.map((m) => ({
        id: m.id, role: m.role, text: m.text, status: m.status, model: m.model,
        provider: m.provider, error: m.error ?? null,
        texts: m.blocks.filter((b) => b.kind === 'text').map((b) => b.text),
        tools: m.blocks.filter((b) => b.kind === 'tool').map((b) => ({ name: b.name, ok: b.ok ?? null, error: b.error ?? null })),
      })),
    };
  });
}
/** 等「第 from 条之后的新 assistant 到终态」;返回该 assistant 摘要。 */
async function waitNewTerminal(page, fromCount, timeoutMs) {
  await page.waitForFunction((from) => {
    const c = window.__forgeShell?.stores?.chat?.getState?.();
    if (!c || c.activeRunId !== null) return false;
    const assistants = c.messages.filter((m) => m.role === 'assistant');
    if (assistants.length <= from) return false;
    const last = assistants[assistants.length - 1];
    return ['completed', 'failed', 'cancelled'].includes(last.status);
  }, fromCount, { timeout: timeoutMs });
  const st = await chatState(page);
  const assistants = st.messages.filter((m) => m.role === 'assistant');
  return assistants[assistants.length - 1];
}
async function newSessionViaUi(page) {
  await page.getByTestId('sidebar-new-agent').click();
  await page.waitForFunction(() => {
    const s = window.__forgeShell.stores.sessions.getState();
    const c = window.__forgeShell.stores.chat.getState();
    return s.activeSessionId !== null && c.models.length > 0 && !c.hydrating;
  }, null, { timeout: 20000 });
  return activeSessionId(page);
}
async function pickModel(page, id) {
  await page.getByTestId('composer-model').click();
  await page.getByTestId('composer-model-menu').waitFor({ timeout: 5000 });
  await page.getByTestId(`model-item-${id}`).click();
  await page.waitForFunction(
    (mid) => window.__forgeShell.stores.chat.getState().selectedModelId === mid,
    id, { timeout: 10000 },
  );
}
async function sendChat(page, text) {
  const before = (await chatState(page)).messages.filter((m) => m.role === 'assistant').length;
  await page.getByTestId('composer-input').fill(text);
  await page.getByTestId('composer-send').click();
  return before;
}
/** 视口帧统计行(text 形态 `{device} · draws N · frames N · px N`)。 */
async function viewportStat(page) {
  const loc = page.locator('text=/·\\s*draws\\s+\\d+\\s*·\\s*frames\\s+\\d+\\s*·\\s*px\\s+\\d+/').first();
  const txt = await loc.textContent({ timeout: 3000 }).catch(() => null);
  if (!txt) return null;
  const m = txt.match(/^(.*?)\s*·\s*draws\s+(\d+)\s*·\s*frames\s+(\d+)\s*·\s*px\s+(\d+)/);
  if (!m) return null;
  return { deviceName: m[1].trim(), draws: Number(m[2]), frames: Number(m[3]), px: Number(m[4]) };
}
async function waitViewportStat(page, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const st = await viewportStat(page);
    if (st) return st;
    if (Date.now() > deadline) return null;
    await sleep(400);
  }
}

// ---------- 主流程 ----------
let t3BaselinePx = -1;
async function main() {
  matrix.orphanCheck.before = orphanScan();
  log('== F8 wave.4 浏览器真实任务矩阵(G-F8-4)==');
  log(`evidence: ${path.relative(ROOT, MATRIX_FILE)};TMP=${TMP};ENT=${ENT_NAME}`);

  // ── 0. 构建检查(编排器可 FORGE_E2E_SKIP_BUILD=1 跳过编,仍校验产物)──
  if (process.env.FORGE_E2E_SKIP_BUILD === '1') {
    log('== 0. 跳过构建(FORGE_E2E_SKIP_BUILD=1),仅校验产物==');
  } else {
    log('== 0. 构建检查(cargo 七二进制 + client/host dist)==');
    await run('cargo build -p forge-agentd -p engine-scene-mcp -p engine-host -p asset-pipeline-mcp -p code-forge-mcp -p gen-image-mcp -p gen-model-mcp');
    await run('pnpm --filter @forge/client build');
    await run('pnpm --filter @forge/host build');
  }
  for (const rel of [
    'target/debug/forge-agentd.exe', 'target/debug/engine-scene-mcp.exe', 'target/debug/engine-host.exe',
    'target/debug/asset-pipeline-mcp.exe', 'target/debug/code-forge-mcp.exe',
    'target/debug/gen-image-mcp.exe', 'target/debug/gen-model-mcp.exe',
    'packages/host/dist/index.js', 'packages/client/dist/index.html',
  ]) {
    if (!fs.existsSync(path.join(ROOT, rel))) throw new Error(`构建产物缺失: ${rel}`);
  }
  matrix.prep.push('构建产物齐(7 二进制 + client dist + host dist)');

  // ── 1. 端口预检/回收 ──
  log('== 1. 端口预检(8103 agentd / 3080 host)==');
  reclaimPort(AGENTD_PORT, /target[\\/]debug[\\/]forge-agentd\.exe/i);
  reclaimPort(HOST_PORT, /packages[\\/]host[\\/]dist[\\/]index\.js/i);
  if (!(await portFree(AGENTD_PORT)) || !(await portFree(HOST_PORT))) throw new Error('端口回收后仍被占用');
  matrix.prep.push('端口 8103/3080 已就绪(占用处置见上日志)');

  // ── 2. mock OpenAI 服务器(T4 LLM 桩;确定性,仅 LLM 决策为桩) ──
  const oaiRequests = [];
  mockServer = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => { raw += c.toString(); });
    req.on('end', () => {
      let body = {};
      try { body = JSON.parse(raw); } catch {}
      const messages = Array.isArray(body.messages) ? body.messages : [];
      const hasToolRole = messages.some((m) => m?.role === 'tool');
      oaiRequests.push({
        at: new Date().toISOString(),
        path: req.url,
        model: body.model ?? null,
        toolsCount: Array.isArray(body.tools) ? body.tools.length : 0,
        hasAuthorization: typeof req.headers.authorization === 'string' && req.headers.authorization.length > 0,
        hasToolRole,
      });
      const reply = hasToolRole
        ? { choices: [{ message: { role: 'assistant', content: `已创建 1 个实体(${ENT_NAME}),agent 工具循环闭环` } }], usage: { prompt_tokens: 11, completion_tokens: 7, total_tokens: 18 } }
        : { choices: [{ message: { role: 'assistant', tool_calls: [{ id: 'call_e2e_1', type: 'function', function: { name: 'mcp__engine-scene__entity_create', arguments: JSON.stringify({ name: ENT_NAME }) } }] } }], usage: { prompt_tokens: 5, completion_tokens: 3, total_tokens: 8 } };
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(reply));
    });
  });
  await new Promise((r) => mockServer.listen(0, '127.0.0.1', r));
  const oaiPort = mockServer.address().port;
  const oaiBaseUrl = `http://127.0.0.1:${oaiPort}`;
  log(`== 2. mock OpenAI 服务器就绪 ${oaiBaseUrl}(仅 LLM 决策桩;entity=${ENT_NAME})==`);

  // ── 3. agentd(双数据目录隔离;FORGE_LLM_API_KEY 继承父 env,永不打印) ──
  log('== 3. 启动 agentd(8103;FORGE_AGENTD_DATA_DIR/FORGE_GEN_DATA_DIR 隔离)==');
  agentdChild = spawnLogged('agentd', path.join(ROOT, 'target/debug/forge-agentd.exe'), [], {
    FORGE_AGENTD_DATA_DIR: AGENT_DATA,
    FORGE_GEN_DATA_DIR: GEN_DATA,
  });
  await waitHttp(`${AGENTD_ORIGIN}/health`, 30000, 'agentd');
  matrix.prep.push(`agentd pid=${agentdChild.pid} /health ok`);

  // ── 4. host ──
  log('== 4. 启动 host(3080 静态托管 + 代理)==');
  hostChild = spawnLogged('host', process.execPath, ['packages/host/dist/index.js'], {
    FORGE_AGENTD_ORIGIN: AGENTD_ORIGIN,
  });
  await waitHttp(`${HOST_ORIGIN}/api/forge/health`, 30000, 'host');
  matrix.prep.push(`host pid=${hostChild.pid} /api/forge/health ok`);

  // 场景归一:scene_new 空场景起步(编辑器打开时 ensureDefaultScene 会在空场景自动加载
  // maze 默认场景——产品既有行为,T3 基线实体数/px 如实记录,不伪造"空场景"叙事)
  const sceneNew = await mcp('mcp__engine-scene__scene_new', { name: 'f8-w4-e2e' });
  log(`  scene_new: ${JSON.stringify(sceneNew).slice(0, 120)}`);
  matrix.prep.push('scene_new(f8-w4-e2e) 已执行(编辑器打开时 ensureDefaultScene 将自动加载 maze 默认场景)');

  // deepseek availability(服务端同源判定;T2 腿裁决)
  const snap0 = await hapi('GET', '/api/forge/design-snapshot');
  const dsAvail = snap0.json?.models?.models?.find((m) => m.id === 'deepseek-chat')?.availability ?? 'unknown';
  matrix.env.deepseekAvailability = dsAvail;
  log(`  deepseek availability=${dsAvail}(design-snapshot 同源)`);

  // ── 5. 浏览器(msedge 优先,chrome 兜底;无下载) ──
  log('== 5. 启动浏览器(playwright-core + 系统浏览器)==');
  let channelUsed = null;
  for (const ch of ['msedge', 'chrome']) {
    try {
      browser = await chromium.launch({ channel: ch, headless: true });
      channelUsed = ch;
      break;
    } catch (e) { log(`  channel ${ch} 不可用: ${String(e.message).split('\n')[0]}`); }
  }
  if (!browser) throw new Error('系统无可用 Edge/Chrome(channel 启动失败)');
  matrix.env.browserChannel = channelUsed;
  const ctx = await browser.newContext({ viewport: { width: 1600, height: 900 }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => matrix.hygiene.pageErrors.push(String(e.message ?? e)));
  page.on('console', (msg) => { if (msg.type() === 'error') matrix.hygiene.consoleErrors.push(msg.text()); });
  page.setDefaultTimeout(15000);
  log(`  browser channel=${channelUsed}`);

  await page.goto(`${HOST_ORIGIN}/`, { waitUntil: 'domcontentloaded' });
  await page.getByTestId('shell').waitFor({ timeout: 20000 });
  await forgeReady(page);
  matrix.prep.push('shell 渲染 + __forgeShell seam 就绪');

  // ================= T1 =================
  await task('T1', '建会话→Mock 模型→发消息→SSE 驱动 UI 断言', async (t) => {
    const sid = await newSessionViaUi(page);
    check(t, 'session-created-ui', !!sid, `sid=${sid}`);
    await pickModel(page, 'mock');
    check(t, 'model-mock-selected', true, 'selectedModelId=mock');

    const before = await sendChat(page, '说一句你好');
    const term = await waitNewTerminal(page, before, 30000);

    // DOM:用户卡
    const userTexts = await page.getByTestId('user-message-text').allTextContents();
    check(t, 'user-card-dom', userTexts.some((x) => x.includes('说一句你好')), `userTexts=${JSON.stringify(userTexts)}`);
    // DOM:助手卡 + 模型 label + mock 回文
    const assistantCount = await page.getByTestId('assistant-message').count();
    check(t, 'assistant-card-dom', assistantCount >= 1, `count=${assistantCount}`);
    const modelLabel = (await page.getByTestId('assistant-model').last().textContent())?.trim();
    check(t, 'assistant-model-mock', modelLabel === 'mock', `label=${modelLabel}`);
    check(t, 'assistant-text-mock-echo', term.texts.some((x) => x.includes('mock:已收到「说一句你好」')), term.texts.join(' | ').slice(0, 160));
    check(t, 'assistant-status-completed', term.status === 'completed', `status=${term.status}`);
    // DOM:终态(无 stream-caret;终态点 = --dot-done computed)
    check(t, 'no-stream-caret', (await page.getByTestId('stream-caret').count()) === 0);
    const dotDone = await page.evaluate(() => {
      const msgs = document.querySelectorAll('[data-testid="assistant-message"]');
      const last = msgs[msgs.length - 1];
      const dot = last?.querySelector('.flex.items-center span.rounded-full');
      if (!dot) return { found: false, same: false };
      const probe = document.createElement('span');
      probe.style.background = 'var(--dot-done)';
      document.body.appendChild(probe);
      const want = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return { found: true, same: getComputedStyle(dot).backgroundColor === want };
    });
    check(t, 'terminal-dot-done', dotDone.found && dotDone.same, JSON.stringify(dotDone));
    // 后端:事件序列 + run completed(SSE 驱动的事实源)
    const snap = await hapi('GET', `/api/forge/design-snapshot?sessionId=${encodeURIComponent(sid)}`);
    const types = (snap.json?.events ?? []).map((e) => e.type);
    const wantSeq = ['session.created', 'composer.user.message', 'agent.started', 'agent.message', 'agent.completed'];
    check(t, 'backend-event-sequence', wantSeq.every((w) => types.includes(w)), types.join(','));
    const completedEvt = (snap.json?.events ?? []).find((e) => e.type === 'agent.completed');
    check(t, 'backend-run-completed', !!completedEvt, `latestSeq=${snap.json?.latestSeq}`);
    await shot(page, t, 'mock-chat');
  });

  // ================= T2 =================
  await task('T2', 'deepseek live 发消息(available 实测 / 否则 annotated-mock)', async (t) => {
    if (matrix.env.deepseekAvailability === 'available') {
      await pickModel(page, 'deepseek-chat');
      const before = await sendChat(page, '用一句话回答:1+1 等于几?');
      const term = await waitNewTerminal(page, before, 150000);
      check(t, 'live-status-completed', term.status === 'completed', `status=${term.status};err=${term.error ?? ''}`);
      check(t, 'live-model-label', term.model === 'deepseek-chat', `model=${term.model}`);
      check(t, 'live-provider-deepseek', term.provider === 'deepseek', `provider=${term.provider}`);
      check(t, 'live-text-nonempty', term.texts.join('').trim().length > 0, term.texts.join(' | ').slice(0, 120));
      t.notes.push('deepseek live 实测(availability=available)');
      await shot(page, t, 'deepseek-live');
    } else {
      // annotated-mock:菜单禁用态如实留证,不充绿
      await page.getByTestId('composer-model').click();
      await page.getByTestId('composer-model-menu').waitFor({ timeout: 5000 });
      const item = page.getByTestId('model-item-deepseek-chat');
      const disabled = await item.isDisabled();
      const title = await item.getAttribute('title');
      check(t, 'deepseek-needs-key-menu-disabled', disabled && title === '未配置 Key', `disabled=${disabled};title=${title}`);
      t.notes.push(`availability=${matrix.env.deepseekAvailability}(design-snapshot);未配置 Key 诚实禁用态,deepseek live 腿 annotated-mock 不充绿`);
      t.verdict = 'annotated-mock';
      await shot(page, t, 'deepseek-needs-key');
      await page.keyboard.press('Escape');
      await page.getByTestId('composer-model').click();
    }
  });

  // ================= T3 =================
  await task('T3', '编辑器视图浏览器出帧(canvas readback 轮询腿)', async (t) => {
    await page.evaluate(() => window.__forgeShell.openEditor());
    await page.getByTestId('workbench-tab-editor').waitFor({ timeout: 10000 });
    const degraded = await page.locator('text=Viewport 帧通道降级').count();
    check(t, 'viewport-not-degraded', degraded === 0, `degradedPanels=${degraded}`);
    const s1 = await waitViewportStat(page, 20000);
    check(t, 'frame-stat-visible', !!s1, JSON.stringify(s1));
    if (s1) {
      check(t, 'device-name-real', s1.deviceName.length > 0 && !/待首帧|降级/.test(s1.deviceName), s1.deviceName);
      await sleep(900);
      const s2 = await viewportStat(page);
      check(t, 'frames-increasing', !!s2 && s2.frames > s1.frames, `frames ${s1.frames} → ${s2?.frames}`);
      matrix.env.viewportDevice = s1.deviceName;
      t3BaselinePx = s1.px;
      // 场景实体数如实记录(ensureDefaultScene 自动加载 maze 后的真实数量)
      const list0 = await mcp('mcp__engine-scene__entity_list').catch(() => null);
      const entCount0 = Array.isArray(list0?.entities) ? list0.entities.length : -1;
      t.assertions.push({ name: 'baseline-recorded', pass: true, detail: `px=${s1.px};实体数=${entCount0}` });
      t.notes.push(`基线:实体数=${entCount0}(编辑器 ensureDefaultScene 自动加载 maze),px=${s1.px};T4 后复测腿断言 px>0`);
      // canvas 像素抽样:浏览器 canvas 真实显示引擎帧(非清屏色即出帧;清屏色 rgb(23,24,29) 容差 3)
      const canvasPx = await page.evaluate(() => {
        const c = document.querySelector('.bg-ink canvas');
        if (!c || c.width === 0 || c.height === 0) return { ok: false, reason: 'canvas-missing-or-zero-size' };
        const g = c.getContext('2d');
        const step = 16;
        let nonBg = 0, total = 0;
        for (let y = 0; y < c.height; y += step) {
          const row = g.getImageData(0, y, c.width, 1).data;
          for (let x = 0; x < c.width; x += step) {
            const i = x * 4;
            total++;
            if (Math.abs(row[i] - 23) > 3 || Math.abs(row[i + 1] - 24) > 3 || Math.abs(row[i + 2] - 29) > 3) nonBg++;
          }
        }
        return { ok: true, nonBg, total, w: c.width, h: c.height };
      });
      check(t, 'canvas-pixels-nonzero', canvasPx.ok === true && canvasPx.nonBg > 0, JSON.stringify(canvasPx));
    }
    await shot(page, t, 'editor-frame');
  });

  // ================= T4 =================
  await task('T4', 'chat 指令创建实体(agent 工具循环真 engine 链)', async (t) => {
    // 实体工具名先查(契约:GET /api/forge/mcp/tools)
    const tools = await hapi('GET', '/api/forge/mcp/tools');
    const entityTool = (tools.json?.tools ?? []).find((x) => x.includes('entity_create'));
    check(t, 'entity-tool-declared', entityTool === 'mcp__engine-scene__entity_create', String(entityTool));

    // openai-compat 渠道指向内置 mock OpenAI 服务器(隔离数据目录,零外部副作用)
    const cfg = await hapi('POST', '/api/forge/llm/openai-compat/config', {
      baseUrl: oaiBaseUrl, model: OAI_MODEL, key: OAI_DUMMY_KEY,
    });
    check(t, 'openai-compat-configured', cfg.status === 200 && cfg.json?.configured === true, `HTTP ${cfg.status}`);
    const st = await hapi('GET', '/api/forge/llm/openai-compat/status');
    check(t, 'openai-compat-status', st.json?.configured === true && st.json?.baseUrl === oaiBaseUrl, JSON.stringify({ configured: st.json?.configured }));

    // UI:新会话 → 选 openai-compat → 发创建指令
    const sid = await newSessionViaUi(page);
    await pickModel(page, 'openai-compat');
    const before = await sendChat(page, `请创建一个名为 ${ENT_NAME} 的立方体实体`);
    const term = await waitNewTerminal(page, before, 90000);
    check(t, 'run-completed', term.status === 'completed', `status=${term.status};err=${term.error ?? ''}`);
    check(t, 'tool-loop-entity-create', term.tools.some((x) => x.name === 'mcp__engine-scene__entity_create' && x.ok === true), JSON.stringify(term.tools));
    check(t, 'model-label-oai', term.model === OAI_MODEL, `model=${term.model}`);

    // DOM:工具段「创建实体」
    const segCount = await page.getByTestId('activity-segment').count();
    check(t, 'activity-segment-dom', segCount >= 1, `segments=${segCount}`);
    if (segCount >= 1) {
      await page.getByTestId('activity-segment').last().click();
      const segText = await page.locator('[data-testid^="tool-line-"]').last().textContent().catch(() => '');
      check(t, 'tool-line-create-entity', String(segText).includes('创建实体'), String(segText).slice(0, 120));
    }
    await shot(page, t, 'entity-created-chat');

    // 后端:entity_list 出现新实体(真 engine 链)
    const list = await mcp('mcp__engine-scene__entity_list');
    const names = (list?.entities ?? []).map((e) => e.name);
    check(t, 'entity-list-contains-new', names.includes(ENT_NAME), names.join(','));

    // 视口复测(T3 后测腿):px>0
    await page.getByTestId('workbench-tab-editor').click();
    let pxNow = 0;
    const deadline = Date.now() + 15000;
    for (;;) {
      const s = await viewportStat(page);
      if (s) pxNow = s.px;
      if (pxNow > 0 || Date.now() > deadline) break;
      await sleep(500);
    }
    check(t, 'viewport-nonzero-pixels', pxNow > 0, `px=${pxNow}(T3 基线 ${t3BaselinePx})`);
    // mock OpenAI 服务器请求面留痕(不含 Authorization 值)
    check(t, 'oai-server-saw-requests', oaiRequests.length >= 2 && oaiRequests.every((r) => r.hasAuthorization) && oaiRequests.every((r) => r.model === OAI_MODEL), JSON.stringify(oaiRequests));
    t.notes.push(`openai-compat → 内置确定性 mock OpenAI(仅 LLM 决策桩);agent 工具循环/MCP/engine 链全真;请求面=${oaiRequests.length} 发`);
    t.notes.push(`px:T3 基线 ${t3BaselinePx}(maze 自动加载)→ 实体创建后 ${pxNow}(场景含 maze 实体 + 新建 ${ENT_NAME})`);
    await shot(page, t, 'entity-viewport-pixels');
  });

  // ================= T5 =================
  await task('T5', 'multitask 碰撞体 swarm(工具段「集群执行」+ completed)', async (t) => {
    // 回 chat 视(T4 后停在 editor tab;对话列恒在,无需切 tab)
    await page.getByTestId('composer-add').click();
    await page.getByTestId('composer-add-menu').waitFor({ timeout: 5000 });
    await page.getByTestId('mode-item-multitask').click();
    const chip = await page.getByTestId('composer-mode-chip').textContent().catch(() => '');
    check(t, 'mode-chip-multitask', String(chip).includes('Multitask'), String(chip));

    const before = await sendChat(page, '给场景加碰撞体 collider');
    const term = await waitNewTerminal(page, before, 90000);
    check(t, 'run-completed', term.status === 'completed', `status=${term.status};err=${term.error ?? ''}`);
    check(t, 'swarm-tool-ok', term.tools.some((x) => x.name === 'swarm.execute' && x.ok === true), JSON.stringify(term.tools));
    check(t, 'summary-text', term.texts.some((x) => x.includes('swarm 分片聚合') && x.includes('失败 0')), term.texts.join(' | ').slice(0, 160));

    // DOM:工具段「集群执行」
    const segCount = await page.getByTestId('activity-segment').count();
    check(t, 'activity-segment-dom', segCount >= 1, `segments=${segCount}`);
    if (segCount >= 1) {
      await page.getByTestId('activity-segment').last().click();
      const line = await page.locator('[data-testid^="tool-line-"]').last().textContent().catch(() => '');
      check(t, 'tool-line-swarm-verb', String(line).includes('集群执行'), String(line).slice(0, 120));
    }
    // 后端:swarm state 分片全 done
    const sw = await hapi('GET', '/api/forge/swarm/state');
    const shards = sw.json?.shards ?? [];
    check(t, 'swarm-shards-done', shards.length >= 1 && shards.every((s) => s.status === 'done'), `shards=${shards.length}`);
    await shot(page, t, 'multitask-swarm');
  });

  // ================= T6 =================
  await task('T6', '提案批准流(pending → 批准 → approved)', async (t) => {
    let proposals = (await hapi('GET', '/api/forge/proposals')).json?.proposals ?? [];
    let target = proposals.find((p) => p.status === 'pending');
    if (!target) {
      const created = await hapi('POST', '/api/forge/proposals', {
        kind: 'asset.cleanup', summary: 'F8-W4 E2E 验收提案(矩阵自造 pending)', impact: { assets: ['Meshes/e2e-probe.gltf'] },
      });
      check(t, 'proposal-created', created.status === 200 && created.json?.status === 'pending', `HTTP ${created.status}`);
      target = created.json;
    } else {
      t.notes.push('复用既有 pending 提案');
    }
    const pid = target.id;
    await page.evaluate(() => window.__forgeShell.openTab('proposals'));
    await page.getByTestId('proposals-tab').waitFor({ timeout: 10000 });
    await page.getByTestId('proposals-refresh').click();
    await page.getByTestId(`proposal-row-${pid}`).waitFor({ timeout: 10000 });
    const before = (await page.getByTestId(`proposal-status-${pid}`).textContent())?.trim();
    check(t, 'status-pending-before', before === 'pending', `status=${before}`);
    await page.getByTestId(`proposal-approve-${pid}`).click();
    await page.waitForFunction(
      (id) => document.querySelector(`[data-testid="proposal-status-${id}"]`)?.textContent?.trim() === 'approved',
      pid, { timeout: 10000 },
    );
    const after = (await page.getByTestId(`proposal-status-${pid}`).textContent())?.trim();
    check(t, 'status-approved-ui', after === 'approved', `status=${after}`);
    const back = (await hapi('GET', '/api/forge/proposals')).json?.proposals?.find((p) => p.id === pid);
    check(t, 'status-approved-backend', back?.status === 'approved', `backend=${back?.status}`);
    await shot(page, t, 'proposal-approved');
  });

  // ================= T7 =================
  await task('T7', '设置主题切换(data-theme + --accent computed)', async (t) => {
    const readTheme = () => page.evaluate(() => ({
      theme: document.documentElement.dataset.theme,
      accent: getComputedStyle(document.documentElement).getPropertyValue('--accent').trim(),
    }));
    const before = await readTheme();
    await page.getByTestId('sidebar-settings').click();
    await page.getByTestId('settings-overlay').waitFor({ timeout: 10000 });
    await page.getByTestId('settings-nav-appearance').click();
    await page.getByTestId('settings-page-appearance').waitFor({ timeout: 10000 });

    await page.getByTestId('theme-mode-dark').click();
    const dark = await readTheme();
    check(t, 'dark-data-theme', dark.theme === 'dark', JSON.stringify(dark));
    check(t, 'dark-accent-changed', dark.accent !== before.accent && dark.accent !== '', `${before.accent} → ${dark.accent}`);
    await shot(page, t, 'theme-dark');

    await page.getByTestId('theme-mode-light').click();
    const light = await readTheme();
    check(t, 'light-data-theme', light.theme === 'light', JSON.stringify(light));
    check(t, 'light-accent-changed', light.accent !== dark.accent && light.accent !== '', `${dark.accent} → ${light.accent}`);
    await shot(page, t, 'theme-light');

    // 复原初始 mode(不留设置副作用)
    const restore = before.theme === 'dark' ? 'dark' : before.theme === 'light' ? 'light' : 'auto';
    await page.getByTestId(`theme-mode-${restore}`).click();
    await page.keyboard.press('Escape');
    t.notes.push(`主题已复原 mode=${restore}`);
  });

  // ================= T8 =================
  await task('T8', '会话管理(fork/重命名/置顶/删除)', async (t) => {
    const sid = await newSessionViaUi(page);
    check(t, 'session-created', !!sid, `sid=${sid}`);

    // fork
    await page.getByTestId('chat-fork').click();
    await page.waitForFunction((src) => {
      const st = window.__forgeShell.stores.sessions.getState();
      return st.activeSessionId !== src && st.sessions.some((s) => s.title.startsWith('分支 · '));
    }, sid, { timeout: 10000 });
    const forkId = await activeSessionId(page);
    check(t, 'fork-ui-branch-title', !!forkId && forkId !== sid, `forkId=${forkId}`);
    const forkBack = await hapi('GET', `/api/forge/sessions/${forkId}`);
    check(t, 'fork-backend-title', String(forkBack.json?.session?.title ?? '').startsWith('分支 · '), forkBack.json?.session?.title);
    await shot(page, t, 'forked');

    // 重命名(侧栏双击内联)
    await page.getByTestId(`session-row-${forkId}`).dblclick();
    await page.getByTestId(`session-rename-${forkId}`).fill('F8W4 改名会话');
    await page.keyboard.press('Enter');
    await page.waitForFunction((id) => {
      const s = window.__forgeShell.stores.sessions.getState().sessions.find((x) => x.id === id);
      return s?.title === 'F8W4 改名会话';
    }, forkId, { timeout: 10000 });
    const renBack = await hapi('GET', `/api/forge/sessions/${forkId}`);
    check(t, 'rename-backend', renBack.json?.session?.title === 'F8W4 改名会话' && renBack.json?.session?.titleManuallySet === true, `title=${renBack.json?.session?.title};manual=${renBack.json?.session?.titleManuallySet}`);

    // 置顶
    await page.getByTestId(`session-row-${forkId}`).hover();
    await page.getByTestId(`session-row-${forkId}`).getByLabel('置顶').click();
    await page.waitForFunction((id) => {
      const s = window.__forgeShell.stores.sessions.getState().sessions.find((x) => x.id === id);
      return s?.pinned === true;
    }, forkId, { timeout: 10000 });
    const pinBack = await hapi('GET', `/api/forge/sessions/${forkId}`);
    check(t, 'pin-backend', pinBack.json?.session?.pinned === true, `pinned=${pinBack.json?.session?.pinned}`);
    check(t, 'pin-ui-section', (await page.locator('text=PINNED').count()) >= 1);
    await shot(page, t, 'pinned');

    // 删除
    await page.getByTestId(`session-row-${forkId}`).hover();
    await page.getByTestId(`session-row-${forkId}`).getByLabel('删除会话').click();
    await page.waitForFunction((id) => {
      return !window.__forgeShell.stores.sessions.getState().sessions.some((x) => x.id === id);
    }, forkId, { timeout: 10000 });
    const delBack = await hapi('GET', `/api/forge/sessions/${forkId}`);
    check(t, 'delete-backend-404', delBack.status === 404, `HTTP ${delBack.status}`);
    await shot(page, t, 'deleted');
  });

  await ctx.close();
}

// ---------- 入口 ----------
let exitCode = 0;
try {
  await main();
} catch (e) {
  log(`[FATAL] ${e?.stack ?? e}`);
  exitCode = 1;
} finally {
  await cleanup();
}

matrix.finishedUtc = new Date().toISOString();
matrix.durationMs = Date.now() - STARTED;
matrix.tasks = results;
matrix.verdicts = Object.fromEntries(results.map((t) => [t.id, t.verdict]));
matrix.allGreen = results.length === 8 && results.every((t) => t.verdict === 'pass');
matrix.gateGreen = results.every((t) => t.verdict === 'pass' || t.verdict === 'annotated-mock');
matrix.orphanFree = matrix.orphanCheck.after.length === 0;
fs.writeFileSync(MATRIX_FILE, JSON.stringify(matrix, null, 2));

log('== 汇总 ==');
for (const t of results) log(`  ${t.id}: ${t.verdict} (${t.durationMs}ms, 断言 ${t.assertions.filter((a) => a.pass).length}/${t.assertions.length})`);
log(`  hygiene: pageErrors=${matrix.hygiene.pageErrors.length} consoleErrors=${matrix.hygiene.consoleErrors.length}`);
log(`  orphanCheck.after=${JSON.stringify(matrix.orphanCheck.after)}(空=零孤儿)`);
log(`  allGreen=${matrix.allGreen} gateGreen=${matrix.gateGreen}`);
if (results.some((t) => t.verdict === 'fail')) exitCode = 1;
if (matrix.orphanCheck.after.length > 0) {
  log('  [WARN] 清理后仍有疑似孤儿进程(见上),exit 置 1');
  exitCode = 1;
}
log(`exit=${exitCode}`);
process.exit(exitCode);
