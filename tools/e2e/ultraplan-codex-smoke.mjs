// Read-only compatibility probe against the installed Codex app-server.
// --live permits one tiny model turn that only calls an echo dynamic tool.
// Never reads auth files, prints effective config, or writes the user's config.
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';
import readline from 'node:readline';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const live = process.argv.includes('--live');
const skillsIsolation = !process.argv.includes('--legacy-isolation');
const binArg = process.argv.indexOf('--codex');
const executable = binArg >= 0 ? process.argv[binArg + 1] : process.env.FORGE_CODEX_BIN;
if (!executable) throw new Error('Supply --codex <installed codex.exe> or FORGE_CODEX_BIN');
const evidence = path.join(repo, 'evidence/ultraplan-codex', new Date().toISOString().replace(/[:.]/g, '-'));
const cwd = path.join(evidence, 'workspace');
await fs.mkdir(cwd, { recursive: true });
const scrub = value => String(value).replace(/Bearer\s+\S+/gi, 'Bearer [REDACTED]')
  .replace(/\b(?:sk-|sk_|eyJ)[A-Za-z0-9_.-]{12,}/g, '[REDACTED]')
  .replace(/([?&](?:key|token|secret|access_token|api_key)=)[^&\s]+/gi, '$1[REDACTED]').slice(0, 1200);
const report = { scope: 'Installed app-server compatibility; not an UltraPlan game-generation or human-approval test',
  liveRequested: live, startedAt: new Date().toISOString(), checks: {}, errors: [], notificationCounts: {},
  nativeRequests: [], toolCalls: [], warnings: [], configWritten: false };
report.binary = { path: executable, version: execFileSync(executable, ['--version'], { windowsHide: true, encoding: 'utf8' }).trim(),
  sha256: createHash('sha256').update(await fs.readFile(executable)).digest('hex') };
const child = spawn(executable, ['app-server'], { cwd, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
let nextId = 0, threadId, turnId, stderrBytes = 0, terminal, dynamicCompleted = false;
let notifyWake;
const pending = new Map();
child.stderr.on('data', data => { stderrBytes += data.length; }); // Drain; never persist potentially sensitive logs.
const send = message => child.stdin.write(JSON.stringify(message) + '\n');
const request = (method, params, timeout = 25000) => new Promise((resolve, reject) => {
  const id = ++nextId, timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, timeout);
  pending.set(id, { resolve, reject, timer }); send({ id, method, params });
});
const wake = () => { if (notifyWake) { notifyWake(); notifyWake = null; } };
readline.createInterface({ input: child.stdout }).on('line', line => {
  let message;
  try { message = JSON.parse(line); } catch { return; }
  if (!message.method) {
    const p = pending.get(message.id);
    if (p) { clearTimeout(p.timer); pending.delete(message.id); message.error ? p.reject(new Error(scrub(JSON.stringify(message.error)))) : p.resolve(message.result); }
    return;
  }
  const { method, params = {} } = message;
  if (message.id !== undefined) {
    const tool = params.tool ?? params.name;
    const owned = threadId && params.threadId === threadId;
    if (owned && ['item/tool/call', 'tool/call'].includes(method) && tool === 'forge__compat_echo') {
      const args = params.arguments ?? params.args;
      const valid = args?.marker === 'ultraplan-readonly-probe';
      report.toolCalls.push({ method, tool, markerMatches: valid, threadMatches: owned });
      send({ id: message.id, result: { success: valid, contentItems: [{ type: 'inputText', text: valid ? 'ultraplan-readonly-probe' : 'Unexpected marker' }] } });
    } else {
      report.nativeRequests.push({ method });
      send({ id: message.id, error: { code: -32601, message: 'Probe only permits its declared echo tool' } });
    }
    wake(); return;
  }
  report.notificationCounts[method] = (report.notificationCounts[method] ?? 0) + 1;
  if (['configWarning', 'deprecationNotice', 'warning'].includes(method)) {
    const details = Object.fromEntries(['title', 'summary', 'message', 'details'].filter(key => typeof params[key] === 'string').map(key => [key, scrub(params[key])]));
    report.warnings.push({ method, ...details });
  }
  if (params.threadId === threadId) {
    if (method === 'turn/started') turnId ??= params.turn?.id;
    if (method === 'turn/completed') {
      terminal = { id: params.turn?.id, status: params.turn?.status,
        error: params.turn?.error ? scrub(params.turn.error.message ?? JSON.stringify(params.turn.error)) : null };
      report.turnTerminal = terminal;
    }
    if (method === 'item/completed' && params.item?.type === 'dynamicToolCall' && params.item.tool === 'forge__compat_echo') {
      report.dynamicToolTerminal = { status: params.item.status, success: params.item.success };
      dynamicCompleted = params.item.status === 'completed' && params.item.success !== false;
    }
    if (method === 'error') report.errors.push({ stage: 'notification', message: scrub(params.error?.message ?? params.message ?? 'Codex error'), willRetry: params.willRetry });
  }
  wake();
});
child.on('error', error => { for (const p of pending.values()) { clearTimeout(p.timer); p.reject(error); } pending.clear(); wake(); });
child.on('exit', code => { report.processExitCode = code; for (const p of pending.values()) { clearTimeout(p.timer); p.reject(new Error(`app-server exited ${code}`)); } pending.clear(); wake(); });
async function waitUntil(predicate, timeout) {
  const deadline = Date.now() + timeout;
  while (!predicate() && Date.now() < deadline && child.exitCode === null && child.signalCode === null) {
    let timer;
    await new Promise(resolve => { notifyWake = resolve; timer = setTimeout(resolve, Math.min(1000, deadline - Date.now())); });
    clearTimeout(timer); notifyWake = null;
  }
  return predicate();
}
async function verifySkillOverrides(overrides) {
  const toml = '[' + overrides.map(entry => `{path=${JSON.stringify(entry.path)},enabled=false}`).join(',') + ']';
  const isolated = spawn(executable, ['app-server', '-c', `skills.config=${toml}`], { cwd, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
  const inflight = new Map(); let id = 0;
  const call = (method, params) => new Promise((resolve, reject) => {
    const current = ++id, timer = setTimeout(() => { inflight.delete(current); reject(new Error(`skill override ${method} timeout`)); }, 25000);
    inflight.set(current, { resolve, reject, timer });
    isolated.stdin.write(JSON.stringify({ id: current, method, params }) + '\n');
  });
  readline.createInterface({ input: isolated.stdout }).on('line', line => {
    let message; try { message = JSON.parse(line); } catch { return; }
    const handler = inflight.get(message.id);
    if (handler) { clearTimeout(handler.timer); inflight.delete(message.id); message.error ? handler.reject(new Error(scrub(message.error.message))) : handler.resolve(message.result); }
  });
  try {
    await call('initialize', { clientInfo: { name: 'rurix_forge', version: '0.1.0' }, capabilities: { experimentalApi: true } });
    isolated.stdin.write(JSON.stringify({ method: 'initialized', params: {} }) + '\n');
    const listed = await call('skills/list', { cwds: [cwd], forceReload: true });
    const skills = listed.data?.flatMap(entry => entry.skills ?? []) ?? [];
    const errors = listed.data?.flatMap(entry => entry.errors ?? []) ?? [];
    const matching = skills.filter(skill => overrides.some(entry => entry.path === skill.path));
    return { discovered: skills.length, matched: matching.length, disabled: matching.filter(skill => skill.enabled === false).length,
      stillEnabled: matching.filter(skill => skill.enabled !== false).length, errors: errors.length };
  } finally {
    const exit = new Promise(resolve => isolated.once('exit', resolve));
    isolated.stdin.end();
    let timer; await Promise.race([exit, new Promise(resolve => { timer = setTimeout(resolve, 5000); })]); clearTimeout(timer);
    if (isolated.exitCode === null && isolated.signalCode === null) isolated.kill();
  }
}
let stage = 'initialize';
try {
  const initialized = await request('initialize', {
    clientInfo: { name: 'rurix_forge', title: 'RurixForge compatibility probe', version: '0.1.0' },
    capabilities: { experimentalApi: true, mcpServerOpenaiFormElicitation: true },
  });
  report.initialize = { userAgent: initialized.userAgent, platformFamily: initialized.platformFamily, platformOs: initialized.platformOs };
  report.checks.initialize = true;
  send({ method: 'initialized', params: {} });
  stage = 'model/list';
  const models = await request('model/list', {});
  const catalog = models.data ?? models.models ?? models.items ?? [];
  report.modelCatalog = catalog.map(model => ({ id: model.id, model: model.model, isDefault: model.isDefault, hidden: model.hidden }));
  report.checks.modelList = Array.isArray(catalog) && catalog.length > 0;
  stage = 'config/read';
  const effective = (await request('config/read', { includeLayers: false, cwd })).config;
  assert.ok(effective && typeof effective === 'object');
  const config = { agents: { enabled: false },
    features: { multi_agent: false, shell_tool: false, unified_exec: false, hooks: false, codex_hooks: false, apps: false, skills: false },
    web_search: 'disabled', apps: { _default: { enabled: false } }, mcp_servers: {}, plugins: {} };
  for (const key of ['mcp_servers', 'plugins', 'apps']) {
    for (const name of Object.keys(effective[key] ?? {})) config[key][name] = { enabled: false };
  }
  if (skillsIsolation) {
    stage = 'skills/list';
    const listed = await request('skills/list', { cwds: [cwd], forceReload: true });
    const matching = listed.data?.filter(entry => path.resolve(entry.cwd).toLowerCase() === path.resolve(cwd).toLowerCase()) ?? [];
    assert.equal(matching.length, 1, 'Skill discovery must identify exactly the requested cwd');
    const entries = matching[0].skills ?? [];
    const errors = matching[0].errors ?? [];
    assert.equal(errors.length, 0, 'Discovery errors must not silently weaken skill isolation');
    const paths = [...new Set([...entries.map(skill => skill.path), ...(effective.skills?.config ?? []).map(entry => entry.path)])];
    assert.ok(paths.every(value => typeof value === 'string' && path.isAbsolute(value)), 'Skill paths must be explicit absolute paths');
    config.skills = { config: paths.map(path => ({ path, enabled: false })) };
    delete config.features.skills; delete config.features.codex_hooks;
    report.skills = { discovered: entries.length, configuredOverrides: paths.length,
      skillFilePaths: paths.filter(value => /[\\/]SKILL\.md$/i.test(value)).length };
    report.skills.processOverrideVerification = await verifySkillOverrides(config.skills.config);
    assert.equal(report.skills.processOverrideVerification.stillEnabled, 0);
    assert.equal(report.skills.processOverrideVerification.errors, 0);
    assert.ok(report.skills.processOverrideVerification.matched > 0, 'Must verify real discovered paths, not an empty list');
  }
  report.isolation = { sandbox: 'read-only', approvalPolicy: 'never', features: config.features, web_search: 'disabled',
    disabledInheritedCounts: Object.fromEntries(['mcp_servers', 'plugins', 'apps'].map(key => [key, Object.keys(config[key]).length])) };
  stage = 'thread/start';
  const thread = await request('thread/start', { cwd, ephemeral: true, approvalPolicy: 'never', sandbox: 'read-only',
    developerInstructions: 'You are executing a Forge managed compatibility test. Use only the declared forge__compat_echo dynamic tool. Never run native shell, file edits, MCP, agents, goals, web search or user questions. Do not read any files. The only permitted action is to echo the exact supplied test marker.',
    config, dynamicTools: [{ name: 'forge__compat_echo', description: 'Echo a fixed harmless compatibility test marker.',
      inputSchema: { type: 'object', properties: { marker: { type: 'string', enum: ['ultraplan-readonly-probe'] } }, required: ['marker'], additionalProperties: false } }],
  }, 45000);
  threadId = thread.thread?.id;
  assert.ok(threadId, 'thread/start must return a thread id');
  report.thread = { id: threadId, ephemeral: thread.thread.ephemeral, model: thread.model, modelProvider: thread.modelProvider,
    sandbox: thread.sandbox, approvalPolicy: thread.approvalPolicy };
  report.checks.threadStartWithIsolation = true;
  if (skillsIsolation) assert.ok(!report.warnings.some(warning => warning.method === 'configWarning'), 'Ignored isolation settings are not allowed');
  if (live) {
    stage = 'turn/start';
    const params = { threadId, cwd, approvalPolicy: 'never', sandboxPolicy: { type: 'readOnly' },
      input: [{ type: 'text', text: 'Call forge__compat_echo exactly once with marker "ultraplan-readonly-probe". No other tools or actions. After the tool result, reply only OK.' }] };
    const selected = catalog.find(model => (model.model ?? model.id) === thread.model);
    if (selected?.supportedReasoningEfforts?.some(entry => (entry.reasoningEffort ?? entry) === 'low')) params.effort = 'low';
    const started = await request('turn/start', params, 45000);
    turnId ??= started.turn?.id;
    assert.ok(turnId, 'turn/start must return turn.id');
    report.checks.turnStart = true;
    stage = 'dynamic tool round-trip';
    await waitUntil(() => dynamicCompleted || !!terminal || report.nativeRequests.length > 0, 45000);
    report.checks.liveDynamicToolRoundTrip = dynamicCompleted && report.toolCalls.some(call => call.markerMatches);
    if (!terminal) {
      stage = 'turn/interrupt';
      await request('turn/interrupt', { threadId, turnId }, 10000);
      report.interruptAcknowledged = true;
      await waitUntil(() => !!terminal, 15000);
      report.checks.interruptTerminal = terminal?.status === 'interrupted';
    } else report.interruptNotNeeded = 'Turn already reached a terminal state';
    assert.equal(report.checks.liveDynamicToolRoundTrip, true, 'No successful real dynamic-tool round-trip');
    assert.ok(terminal && ['completed', 'interrupted'].includes(terminal.status), 'A real terminal notification is required');
    assert.deepEqual(report.nativeRequests, [], 'No native/foreign reverse requests permitted');
  }
  report.ok = true;
} catch (error) {
  report.ok = false; report.errors.push({ stage, message: scrub(error.message) });
} finally {
  if (threadId && turnId && !terminal && child.exitCode === null) {
    try { await request('turn/interrupt', { threadId, turnId }, 10000); report.interruptAcknowledged = true; await waitUntil(() => !!terminal, 10000); }
    catch (error) { report.errors.push({ stage: 'shutdown interrupt', message: scrub(error.message) }); }
  }
  if (child.exitCode === null && child.signalCode === null) {
    child.stdin.end();
    await waitUntil(() => child.exitCode !== null || child.signalCode !== null, 5000);
    if (child.exitCode === null && child.signalCode === null) { child.kill(); report.forcedProcessStop = true; }
  }
  report.stderrBytesDiscarded = stderrBytes;
  report.cwdFiles = await fs.readdir(cwd);
  report.finishedAt = new Date().toISOString();
  await fs.writeFile(path.join(evidence, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ ok: report.ok, checks: report.checks, errors: report.errors, evidence }));
}
process.exitCode = report.ok ? 0 : 1;
