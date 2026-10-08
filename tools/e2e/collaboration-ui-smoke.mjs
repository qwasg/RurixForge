#!/usr/bin/env node
/**
 * Browser-only collaboration acceptance against a running Vite client.
 * Uses the existing system browser and fixture HTTP responses; no agent/model runs.
 * Start: pnpm --filter @forge/client exec vite --host 127.0.0.1 --port 5174
 * Run: node tools/e2e/collaboration-ui-smoke.mjs [http://127.0.0.1:5174]
 */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const origin = process.argv[2] ?? 'http://127.0.0.1:5174';
const executablePath = process.env.FORGE_BROWSER_PATH ?? [
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
].find((candidate) => fs.existsSync(candidate));
assert.ok(executablePath, 'Set FORGE_BROWSER_PATH to an existing browser; this script never downloads one');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const evidence = path.join(root, 'evidence');
fs.mkdirSync(evidence, { recursive: true });
const now = new Date().toISOString();
const sid = 'collaboration-browser-fixture';
const participants = [
  { id: 'leader', sessionId: sid, name: '主 Agent', role: 'root', status: 'running', engine: 'local', activeRunId: 'root-run' },
  { id: 'builder', sessionId: sid, parentAgentId: 'leader', teamId: 'team-browser', name: '实现成员', role: 'member', status: 'running', engine: 'local', activeRunId: 'builder-run' },
  { id: 'reviewer', sessionId: sid, parentAgentId: 'leader', teamId: 'team-browser', name: '评审成员', role: 'member', status: 'idle', engine: 'local', activeRunId: null },
];
let team = {
  id: 'team-browser', sessionId: sid, name: '协作功能验收', status: 'active', leaderAgentId: 'leader', memberAgentIds: ['builder', 'reviewer'],
  revision: 1, maxParallel: 2, maxFixRounds: 2, fixRounds: 0, createdAt: now, updatedAt: now,
  tasks: [
    { id: 'implement', teamId: 'team-browser', title: '完成实现', prompt: '实现目标', ownerAgentId: 'builder', status: 'running', stage: '实施', deps: [], attempts: 1, createdAt: now, updatedAt: now },
    { id: 'review', teamId: 'team-browser', title: '验证实现', prompt: '验收目标', ownerAgentId: 'reviewer', status: 'blocked', stage: '验证', deps: ['implement'], result: '等待实现任务完成', attempts: 0, createdAt: now, updatedAt: now },
  ],
};
const messages = [
  { id: 'recovery-message', sessionId: sid, fromAgentId: 'reviewer', toAgentId: 'builder', source: 'agent', text: '先验证消息去重', status: 'recoveryRequired', createdAt: now, error: '需要明确重发确认' },
];
const session = { id: sid, title: '协作 UI 固定数据验收', status: 'active', agentKind: 'coding', agentEngine: 'local', selectedModelId: null, thinkingEnabled: false, reasoningEffort: null, contextOptionId: null, webSearchEnabled: false, activeRunId: 'root-run', createdAt: now, updatedAt: now, pinned: false, titleManuallySet: true };
const requests = [];
const errors = [];
const checks = [];
const browser = await chromium.launch({ executablePath, headless: true });
const page = await browser.newPage({ viewport: { width: 1366, height: 950 } });
page.on('pageerror', (error) => errors.push(error.message));
await page.route('**/api/forge/**', async (route) => {
  const request = route.request();
  const url = new URL(request.url());
  const method = request.method();
  const body = request.postDataJSON();
  requests.push({ method, path: url.pathname, body });
  let result = {};
  if (url.pathname.endsWith('/messages')) {
    const actor = url.pathname.split('/').at(-2);
    if (method === 'POST') {
      const existing = messages.find((message) => message.clientMessageId === body.clientMessageId);
      const message = existing ?? { id: `message-${messages.length}`, sessionId: sid, toAgentId: actor, source: 'user', text: body.text, clientMessageId: body.clientMessageId, status: 'queued', createdAt: new Date().toISOString() };
      if (!existing) messages.push(message);
      result = { message };
    } else result = { messages: messages.filter((message) => message.toAgentId === actor || message.fromAgentId === actor) };
  } else if (url.pathname.endsWith('/agents')) result = { agents: participants };
  else if (url.pathname.endsWith('/team') || url.pathname.includes('/teams/')) {
    if (method === 'PATCH') team = { ...team, revision: team.revision + 1, status: { pause: 'paused', resume: 'active', stop: 'stopped' }[body.action] };
    result = { team };
  } else if (url.pathname.endsWith('/sessions')) result = { sessions: [] };
  else if (url.pathname.endsWith('/chat-folders')) result = { folders: [] };
  else if (url.pathname.endsWith('/workspaces')) result = { workspaces: [] };
  else if (url.pathname.endsWith('/account/status')) result = { loggedIn: false, byoConfigured: true, devMock: true };
  else if (url.pathname.endsWith('/design-snapshot')) result = { models: { models: [], defaultModelId: null }, events: [], todos: [] };
  else if (url.pathname.endsWith('/health')) result = { service: 'forge-host', status: 'ok' };
  else if (url.pathname.endsWith('/skills')) result = { skills: [] };
  await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(result) });
});

try {
  await page.goto(origin, { waitUntil: 'networkidle' });
  await page.waitForFunction(() => !!window.__forgeShell);
  await page.evaluate(async ({ sid, session, participants, team, now }) => {
    const { useCollaborationStore } = await import('/src/lib/collaborationStore.ts');
    const { stores } = window.__forgeShell;
    stores.chat.setState({ currentSessionId: sid, activeRunId: 'root-run', hydrating: false, messages: [
      { id: 'initial-user', role: 'user', text: '按约束计划完成任务', runId: 'root-run', ts: now, time: now.slice(11, 16), blocks: [], status: 'completed' },
    ] });
    stores.sessions.setState({ sessions: [session], activeSessionId: sid, loading: false, offline: false });
    stores.account.setState({ dismissed: true, authOpen: false });
    useCollaborationStore.getState().reset(sid);
    useCollaborationStore.setState({ agents: participants, team, supported: true });
  }, { sid, session, participants, team, now });

  await page.getByTestId('team-board').waitFor();
  await page.getByTestId('composer-input').fill('优先修复消息隔离');
  assert.equal(await page.getByTestId('composer-abort').isVisible(), true);
  await page.getByTestId('composer-send').click();
  await page.waitForFunction(() => window.__forgeShell.stores.chat.getState().messages.some((message) => message.text === '优先修复消息隔离'));
  await page.getByTestId('composer-input').fill('保留每条引导的顺序');
  await page.getByTestId('composer-send').click();
  await page.waitForFunction(() => window.__forgeShell.stores.chat.getState().messages.filter((message) => message.messageId).length === 2);
  const leaderPosts = requests.filter((request) => request.method === 'POST' && request.path.endsWith('/leader/messages'));
  assert.equal(leaderPosts.length, 2);
  assert.equal(leaderPosts[0].body.expectedRunId, 'root-run');
  assert.notEqual(leaderPosts[0].body.clientMessageId, leaderPosts[1].body.clientMessageId);
  assert.equal(await page.evaluate(() => window.__forgeShell.stores.chat.getState().activeRunId), 'root-run');
  checks.push('Running composer sends two independent guarded messages while Stop remains visible');

  await page.getByTestId('team-toggle').click();
  assert.match(await page.getByTestId('team-board').innerText(), /依赖：完成实现/);
  assert.match(await page.getByTestId('team-board').innerText(), /等待实现任务完成/);
  await page.getByTestId('team-pause-resume').click();
  await page.getByTestId('team-pause-resume').filter({ hasText: '恢复' }).waitFor();
  assert.match(await page.getByTestId('team-board').innerText(), /暂停派工，当前任务可完成/);
  await page.getByTestId('team-pause-resume').click();
  await page.getByTestId('team-pause-resume').filter({ hasText: '暂停' }).waitFor();
  checks.push('Team board shows owner/dependency/blocked reason and applies pause/resume HTTP state');

  await page.getByRole('button', { name: '实现成员 · 进行中', exact: true }).click();
  const overlay = page.getByTestId('subagent-overlay');
  await overlay.waitFor();
  await overlay.getByText('待恢复确认', { exact: true }).waitFor();
  assert.match(await overlay.innerText(), /评审成员 → 实现成员/);
  await overlay.getByTestId('agent-message-input').fill('请补充边界测试');
  await overlay.getByTestId('agent-message-send').click();
  await overlay.getByText('请补充边界测试', { exact: true }).waitFor();
  const childPost = requests.find((request) => request.method === 'POST' && request.path.endsWith('/builder/messages'));
  assert.equal(childPost.body.expectedRunId, 'builder-run');
  assert.equal('fromAgentId' in childPost.body, false);
  checks.push('Persistent member overlay displays peer history/recovery and sends to actual child run');
  await page.screenshot({ path: path.join(evidence, `collaboration-ui-member-${stamp}.png`) });
  await overlay.getByTestId('subagent-overlay-close').click();

  const rootMessage = { ...messages.find((message) => message.toAgentId === 'leader'), status: 'injected', runId: 'root-run', injectedAt: now };
  await page.evaluate(({ sid, rootMessage }) => {
    const event = { id: 'delivery-fixture', sessionId: sid, seq: 20, type: 'agent.message.injected', payload: rootMessage };
    const chat = window.__forgeShell.stores.chat;
    chat.getState().applyEvent(event);
    chat.getState().applyEvent(event);
    chat.getState().applyEvent({ id: 'child-done-fixture', sessionId: sid, seq: 21, type: 'agent.completed', payload: { runId: 'root-run', agentId: 'builder', parentAgentId: 'leader', agentRunId: 'builder-run' } });
  }, { sid, rootMessage });
  const state = await page.evaluate(() => {
    const state = window.__forgeShell.stores.chat.getState();
    return { activeRunId: state.activeRunId, steering: state.messages.filter((message) => message.messageId).map((message) => ({ text: message.text, status: message.messageStatus })) };
  });
  assert.equal(state.activeRunId, 'root-run');
  assert.equal(state.steering.length, 2);
  assert.equal(state.steering[0].status, 'injected');
  checks.push('Replayed delivery deduplicates and member completion leaves root run active');
  await page.screenshot({ path: path.join(evidence, `collaboration-ui-team-${stamp}.png`) });
  assert.deepEqual(errors, []);
  const report = { mode: 'browser-ui-http-fixture-no-model', verdict: 'passed', checks, pageErrors: errors, requests: requests.filter((request) => request.method !== 'GET') };
  fs.writeFileSync(path.join(evidence, `collaboration-ui-${stamp}.json`), `${JSON.stringify(report, null, 2)}\n`);
  console.log(JSON.stringify(report, null, 2));
} catch (error) {
  await page.screenshot({ path: path.join(evidence, `collaboration-ui-failed-${stamp}.png`) });
  console.error(JSON.stringify({ verdict: 'failed', checks, pageErrors: errors, error: String(error) }, null, 2));
  throw error;
} finally {
  await browser.close();
}
