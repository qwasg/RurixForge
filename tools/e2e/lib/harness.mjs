/**
 * F8/F9/编排器共享基建:进程、端口、HTTP/MCP、浏览器、断言、构建校验。
 * 从 tools/e2e/f8-w4-browser-matrix.mjs 抽出,语义保持一致。
 */
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
export const EVIDENCE = path.join(ROOT, 'evidence');
export const AGENTD_PORT = 8103;
export const HOST_PORT = 3080;
export const HOST_ORIGIN = `http://127.0.0.1:${HOST_PORT}`;
export const AGENTD_ORIGIN = `http://127.0.0.1:${AGENTD_PORT}`;
export const OAI_MODEL = 'e2e-deterministic-llm';
export const OAI_DUMMY_KEY = 'e2e-local-dummy-key-not-a-secret';

export const BUILD_ARTIFACTS = [
  'target/debug/forge-agentd.exe',
  'target/debug/engine-scene-mcp.exe',
  'target/debug/engine-host.exe',
  'target/debug/asset-pipeline-mcp.exe',
  'target/debug/code-forge-mcp.exe',
  'target/debug/gen-image-mcp.exe',
  'target/debug/gen-model-mcp.exe',
  'target/debug/store-mcp.exe',
  'packages/host/dist/index.js',
  'packages/client/dist/index.html',
];

export const PORT_SIG_AGENTD = /target[\\/]debug[\\/]forge-agentd\.exe/i;
export const PORT_SIG_HOST = /packages[\\/]host[\\/]dist[\\/]index\.js/i;

export function utcStamp() {
  return new Date().toISOString().replace(/[:.]/g, '-').replace('T', 'T').slice(0, 19) + 'Z';
}

export function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

export function makeLog(logFile) {
  fs.mkdirSync(path.dirname(logFile), { recursive: true });
  return function log(m) {
    const line = `[${new Date().toISOString().slice(11, 19)}] ${m}`;
    console.log(line);
    fs.appendFileSync(logFile, `${line}\n`);
  };
}

export function run(cmd, opts = {}) {
  const {
    cwd = ROOT,
    timeoutMs = 20 * 60_000,
    allowFail = false,
    quiet = false,
    log = () => {},
    env = process.env,
  } = opts;
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, { cwd, shell: true, env });
    let out = '';
    const timer = setTimeout(() => {
      try { execFileSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); } catch { /* ignore */ }
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

export function portFree(port) {
  return new Promise((resolve) => {
    const srv = net.createServer();
    srv.once('error', () => resolve(false));
    srv.once('listening', () => srv.close(() => resolve(true)));
    srv.listen(port, '127.0.0.1');
  });
}

export function listenerPids(port) {
  try {
    const out = execFileSync('netstat', ['-ano', '-p', 'tcp'], { encoding: 'utf8' });
    const pids = new Set();
    for (const line of out.split(/\r?\n/)) {
      if (!line.includes('LISTENING')) continue;
      const cols = line.trim().split(/\s+/);
      if (cols.length >= 5 && cols[1].endsWith(`:${port}`)) pids.add(Number(cols[4]));
    }
    return [...pids].filter((n) => Number.isInteger(n) && n > 0);
  } catch {
    return [];
  }
}

export function pidCommandLine(pid) {
  try {
    const out = execFileSync('powershell', [
      '-NoProfile', '-Command',
      `(Get-CimInstance Win32_Process -Filter "ProcessId=${pid}").CommandLine`,
    ], { encoding: 'utf8' });
    return out.trim();
  } catch {
    return '';
  }
}

export function killTree(pid) {
  try {
    execFileSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

export function reclaimPort(port, signature, log = () => {}) {
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

export async function waitHttp(url, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const r = await fetch(url, { signal: AbortSignal.timeout(2000) });
      if (r.ok) return true;
    } catch { /* retry */ }
    if (Date.now() > deadline) throw new Error(`${label} 就绪超时: ${url}`);
    await sleep(300);
  }
}

export function makeHapi(origin = HOST_ORIGIN) {
  return async function hapi(method, p, body) {
    const res = await fetch(`${origin}${p}`, {
      method,
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    let json = null;
    try { json = JSON.parse(text); } catch { /* raw */ }
    return { status: res.status, json, text };
  };
}

export function makeMcp(hapi) {
  return async function mcp(tool, args = {}) {
    const r = await hapi('POST', '/api/forge/mcp/call', { tool, arguments: args });
    if (r.status !== 200) throw new Error(`mcp ${tool} HTTP ${r.status}: ${r.text.slice(0, 300)}`);
    const env = r.json;
    if (env?.isError === true) {
      const txt = env?.content?.[0]?.text ?? JSON.stringify(env).slice(0, 300);
      throw new Error(`mcp ${tool} isError: ${txt}`);
    }
    const txt = env?.content?.[0]?.text;
    if (typeof txt === 'string') {
      try { return JSON.parse(txt); } catch { return txt; }
    }
    return env?.structuredContent ?? env;
  };
}

export function spawnLogged(name, cmd, args, env, logFile) {
  const child = spawn(cmd, args, { cwd: ROOT, env: { ...process.env, ...env }, windowsHide: true });
  child.stdout?.on('data', (d) => fs.appendFileSync(logFile, `[${name}] ${d.toString()}`));
  child.stderr?.on('data', (d) => fs.appendFileSync(logFile, `[${name}!] ${d.toString()}`));
  return child;
}

export function orphanScan(startedMs) {
  const names = ['forge-agentd', 'engine-scene-mcp', 'engine-host', 'code-forge-mcp',
    'asset-pipeline-mcp', 'gen-image-mcp', 'gen-model-mcp', 'store-mcp', 'context-mcp'];
  const since = new Date(startedMs - 5000).toISOString();
  const ps = `
$t0=[DateTime]::Parse('${since}');
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
  } catch {
    return ['(scan-error)'];
  }
}

export function assertBuildArtifacts() {
  for (const rel of BUILD_ARTIFACTS) {
    if (!fs.existsSync(path.join(ROOT, rel))) throw new Error(`构建产物缺失: ${rel}`);
  }
}

export async function ensureBuild(log, { skip = process.env.FORGE_E2E_SKIP_BUILD === '1' } = {}) {
  if (skip) {
    log('== 构建跳过(FORGE_E2E_SKIP_BUILD=1),仅校验产物 ==');
  } else {
    log('== 构建检查(cargo 七二进制 + client/host dist) ==');
    const rust = await run('cargo build -p forge-agentd -p engine-scene-mcp -p engine-host -p asset-pipeline-mcp -p code-forge-mcp -p gen-image-mcp -p gen-model-mcp -p store-mcp', {
      log, timeoutMs: 25 * 60_000, allowFail: true,
    });
    const client = await run('pnpm --filter @forge/client build', { log, timeoutMs: 10 * 60_000, allowFail: true });
    const host = await run('pnpm --filter @forge/host build', { log, timeoutMs: 5 * 60_000, allowFail: true });
    if (rust.code !== 0 || client.code !== 0 || host.code !== 0) {
      throw new Error(`构建未全绿 rust=${rust.code} client=${client.code} host=${host.code}(仍尝试校验已有产物)`);
    }
  }
  assertBuildArtifacts();
}

export function startMockOpenAI({ entityName, extraReplies } = {}) {
  const requests = [];
  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => { raw += c.toString(); });
    req.on('end', () => {
      let body = {};
      try { body = JSON.parse(raw); } catch { /* ignore */ }
      const messages = Array.isArray(body.messages) ? body.messages : [];
      const hasToolRole = messages.some((m) => m?.role === 'tool');
      requests.push({
        at: new Date().toISOString(),
        path: req.url,
        model: body.model ?? null,
        toolsCount: Array.isArray(body.tools) ? body.tools.length : 0,
        hasAuthorization: typeof req.headers.authorization === 'string' && req.headers.authorization.length > 0,
        hasToolRole,
      });
      const custom = extraReplies ? extraReplies({ hasToolRole, messages, body }) : null;
      const reply = custom ?? (hasToolRole
        ? { choices: [{ message: { role: 'assistant', content: `已创建 1 个实体(${entityName}),agent 工具循环闭环` } }], usage: { prompt_tokens: 11, completion_tokens: 7, total_tokens: 18 } }
        : { choices: [{ message: { role: 'assistant', tool_calls: [{ id: 'call_e2e_1', type: 'function', function: { name: 'mcp__engine-scene__entity_create', arguments: JSON.stringify({ name: entityName }) } }] } }], usage: { prompt_tokens: 5, completion_tokens: 3, total_tokens: 8 } });
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(reply));
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const port = server.address().port;
      resolve({
        server,
        port,
        baseUrl: `http://127.0.0.1:${port}`,
        requests,
      });
    });
  });
}

export async function launchSystemBrowser() {
  let lastErr = null;
  for (const ch of ['msedge', 'chrome']) {
    try {
      const browser = await chromium.launch({ channel: ch, headless: true });
      return { browser, channel: ch };
    } catch (e) {
      lastErr = e;
    }
  }
  throw new Error(`系统无可用 Edge/Chrome: ${lastErr?.message ?? lastErr}`);
}

export function createTaskRunner(log, results) {
  function check(t, name, pass, detail = '') {
    t.assertions.push({ name, pass: !!pass, detail: String(detail) });
    if (!pass) log(`  [FAIL][${t.id}] ${name}: ${detail}`);
    else log(`  [ok][${t.id}] ${name}${detail ? `: ${String(detail).slice(0, 160)}` : ''}`);
  }

  async function shot(page, t, name, prefix) {
    const file = path.join(EVIDENCE, `${prefix}-${t.id}-${name}.png`);
    try {
      await page.screenshot({ path: file, fullPage: false });
      t.screenshots.push(path.relative(ROOT, file));
      log(`  [shot] ${path.basename(file)}`);
    } catch (e) {
      log(`  [shot-fail] ${name}: ${e.message}`);
    }
  }

  async function task(id, title, fn) {
    const t = { id, title, verdict: 'fail', assertions: [], screenshots: [], notes: [], durationMs: 0 };
    results.push(t);
    const started = Date.now();
    log(`== ${id} ${title} ==`);
    try {
      await fn(t);
      if (t.verdict !== 'annotated-mock' && t.verdict !== 'pass-degraded') {
        t.verdict = t.assertions.length > 0 && t.assertions.every((a) => a.pass) ? 'pass' : 'fail';
      }
    } catch (e) {
      t.assertions.push({ name: 'unhandled-exception', pass: false, detail: String(e?.message ?? e) });
      t.verdict = 'fail';
      log(`  [EXC][${id}] ${e?.stack ?? e}`);
    }
    t.durationMs = Date.now() - started;
    log(`-- ${id} verdict=${t.verdict} (${t.durationMs}ms)`);
    return t;
  }

  return { check, shot, task };
}

export async function forgeReady(page) {
  await page.waitForFunction(() => !!window.__forgeShell, null, { timeout: 20000 });
}

export async function activeSessionId(page) {
  return page.evaluate(() => window.__forgeShell.stores.sessions.getState().activeSessionId);
}

export async function chatState(page) {
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

export async function waitNewTerminal(page, fromCount, timeoutMs) {
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

export async function newSessionViaUi(page) {
  await page.getByTestId('sidebar-new-agent').click();
  await page.waitForFunction(() => {
    const s = window.__forgeShell.stores.sessions.getState();
    const c = window.__forgeShell.stores.chat.getState();
    return s.activeSessionId !== null && c.models.length > 0 && !c.hydrating;
  }, null, { timeout: 20000 });
  return activeSessionId(page);
}

export async function pickModel(page, id) {
  await page.getByTestId('composer-model').click();
  await page.getByTestId('composer-model-menu').waitFor({ timeout: 5000 });
  await page.getByTestId(`model-item-${id}`).click();
  await page.waitForFunction(
    (mid) => window.__forgeShell.stores.chat.getState().selectedModelId === mid,
    id, { timeout: 10000 },
  );
}

export async function sendChat(page, text) {
  const before = (await chatState(page)).messages.filter((m) => m.role === 'assistant').length;
  await page.getByTestId('composer-input').fill(text);
  await page.getByTestId('composer-send').click();
  return before;
}

export async function viewportStat(page) {
  const loc = page.locator('text=/·\\s*draws\\s+\\d+\\s*·\\s*frames\\s+\\d+\\s*·\\s*px\\s+\\d+/').first();
  const txt = await loc.textContent({ timeout: 3000 }).catch(() => null);
  if (!txt) return null;
  const m = txt.match(/^(.*?)\s*·\s*draws\s+(\d+)\s*·\s*frames\s+(\d+)\s*·\s*px\s+(\d+)/);
  if (!m) return null;
  return { deviceName: m[1].trim(), draws: Number(m[2]), frames: Number(m[3]), px: Number(m[4]) };
}

export async function waitViewportStat(page, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const st = await viewportStat(page);
    if (st) return st;
    if (Date.now() > deadline) return null;
    await sleep(400);
  }
}

export function makeTmpDirs(prefix) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  return {
    tmp,
    agentData: path.join(tmp, 'agent-data'),
    genData: path.join(tmp, 'gen-data'),
  };
}

export async function cleanupStack({ browser, hostChild, agentdChild, mockServer, tmp, startedMs, log }) {
  log('== finally 清理(browser → host → agentd → mock-server → 孤儿扫荡) ==');
  try { await browser?.close(); } catch { /* ignore */ }
  for (const [label, child] of [['host', hostChild], ['agentd', agentdChild]]) {
    if (child?.pid) {
      const ok = killTree(child.pid);
      log(`  taskkill ${label} pid=${child.pid} tree → ${ok ? 'done' : 'already-exit'}`);
    }
  }
  try { mockServer?.close(); } catch { /* ignore */ }
  await sleep(800);
  const left = orphanScan(startedMs);
  for (const row of left) {
    const pid = Number(row.split('|')[0]);
    if (Number.isInteger(pid) && pid > 0 && pid !== process.pid) {
      log(`  兜底清杀 ${row}`);
      killTree(pid);
    }
  }
  await sleep(600);
  const after = orphanScan(startedMs);
  if (tmp) {
    try { fs.rmSync(tmp, { recursive: true, force: true }); } catch { /* ignore */ }
  }
  return after;
}
