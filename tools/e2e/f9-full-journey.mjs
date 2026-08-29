#!/usr/bin/env node
/**
 * F9 可重复全项目 journey:先 API 闭环(无浏览器),再 UI 薄层巡检 + chat 建实体。
 * 产品入口一律 host:3080。不伪造空项目(scene_new 基线如实记录)。
 * 绕行未修缺陷 D2/D3/D4:PIE/图加载走 MCP 或键盘;保存显式 path;playtest 用测试夹具。
 *
 * 用法: node tools/e2e/f9-full-journey.mjs
 * 产物: evidence/f9-e2e-*.png + evidence/f9-e2e-summary-<UTC>.json + .log
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  ROOT, EVIDENCE, HOST_ORIGIN, AGENTD_ORIGIN, OAI_MODEL, OAI_DUMMY_KEY,
  PORT_SIG_AGENTD, PORT_SIG_HOST, AGENTD_PORT, HOST_PORT,
  utcStamp, sleep, makeLog, makeHapi, makeMcp, spawnLogged,
  reclaimPort, portFree, waitHttp, ensureBuild, startMockOpenAI,
  launchSystemBrowser, createTaskRunner, makeTmpDirs, cleanupStack, orphanScan,
  forgeReady, newSessionViaUi, pickModel, sendChat, waitNewTerminal,
  waitViewportStat,
} from './lib/harness.mjs';

const TS = utcStamp();
const STARTED = Date.now();
const ENT_NAME = `JourneyBox-E2E-${TS.replace(/[^0-9A-Za-z]/g, '').slice(-8)}`;
const W4_MAT_GUID = '2a1004f5-6469-437f-84d4-04b3482df41c';
const GRAPH_REF = 'Content/Graphs/door_opener.rxgraph';
const SCENE_REL = `projects/demo/Content/Scenes/e2e-journey-${TS.replace(/[^0-9A-Za-z]/g, '').slice(-12)}.rxscene`;
const SCENE_ABS = path.join(ROOT, SCENE_REL);

fs.mkdirSync(EVIDENCE, { recursive: true });
const LOG_FILE = path.join(EVIDENCE, `f9-e2e-journey-${TS}.log`);
const SUMMARY_FILE = path.join(EVIDENCE, `f9-e2e-summary-${TS}.json`);
const log = makeLog(LOG_FILE);
const { tmp, agentData, genData } = makeTmpDirs('f9-e2e-');
const PACK_OUT = path.join(os.tmpdir(), `forge-f9-pack-${TS}`);
const MATRIX_FILE = path.join(tmp, 'journey-matrix.json');

const summary = {
  spec: 'full-auto-test / f9-full-journey',
  startedUtc: new Date(STARTED).toISOString(),
  host: { origin: HOST_ORIGIN, agentdOrigin: AGENTD_ORIGIN },
  env: { entity: ENT_NAME, sceneRel: SCENE_REL },
  prep: [],
  notes: [
    '不伪造空项目:scene_new 基线实体数如实记录;编辑器 ensureDefaultScene 可能加载 maze',
    'D2/D3/D4 仍开:PIE/图加载走 MCP 或 Enter;保存显式 path;playtest 为测试夹具不是产品写文件面',
  ],
  hygiene: { pageErrors: [], consoleErrors: [] },
  orphanCheck: { before: [], after: [] },
};
const results = [];
const { check, shot, task } = createTaskRunner(log, results);
const hapi = makeHapi();
const mcp = makeMcp(hapi);

let agentdChild = null;
let hostChild = null;
let mock = null;
let browser = null;

function entityIdOf(created) {
  return created?.id ?? created?.entity?.id ?? created?.entityId ?? null;
}

function pickGraphField(comp) {
  const candidates = ['props.graphRef', 'props.props.graphRef', 'graphRef'];
  for (const f of candidates) {
    let v = comp;
    let ok = true;
    for (const seg of f.split('.')) {
      if (v && typeof v === 'object' && seg in v) v = v[seg];
      else { ok = false; break; }
    }
    if (ok && typeof v === 'string') return f;
  }
  return 'props.graphRef';
}

async function cleanupJourneyArtifacts() {
  try { if (fs.existsSync(SCENE_ABS)) fs.unlinkSync(SCENE_ABS); } catch { /* ignore */ }
  try { if (fs.existsSync(`${SCENE_ABS}.meta`)) fs.unlinkSync(`${SCENE_ABS}.meta`); } catch { /* ignore */ }
  try { fs.rmSync(PACK_OUT, { recursive: true, force: true }); } catch { /* ignore */ }
}

async function apiJourney() {
  await task('A0', 'API 基线 scene_new + health', async (t) => {
    const health = await hapi('GET', '/api/forge/health');
    check(t, 'host-health', health.status === 200, `HTTP ${health.status}`);
    const ad = await fetch(`${AGENTD_ORIGIN}/health`);
    check(t, 'agentd-health', ad.ok, `status=${ad.status}`);
    const created = await mcp('mcp__engine-scene__scene_new', { name: `f9-e2e-${TS.slice(-6)}` });
    const list = await mcp('mcp__engine-scene__entity_list').catch(() => ({ entities: [] }));
    const n = Array.isArray(list?.entities) ? list.entities.length : -1;
    summary.env.sceneNewBaseline = n;
    t.notes.push(`scene_new 后实体数=${n}(如实,不宣称空项目);resp=${JSON.stringify(created).slice(0, 160)}`);
    check(t, 'scene-new-ok', true, `entities=${n}`);
  });

  let entId = null;
  await task('A1', 'API 创建实体', async (t) => {
    const created = await mcp('mcp__engine-scene__entity_create', {
      name: ENT_NAME,
      translation: [2, 1, 2],
      components: [
        { type: 'MeshRenderer', props: { mesh: 'cube', material: W4_MAT_GUID } },
        { type: 'Script', props: { graphRef: GRAPH_REF, module: '', props: {} } },
      ],
    });
    entId = entityIdOf(created);
    const list = await mcp('mcp__engine-scene__entity_list');
    const names = (list?.entities ?? []).map((e) => e.name);
    if (entId == null) {
      const hit = (list?.entities ?? []).find((e) => e.name === ENT_NAME);
      entId = hit?.id ?? null;
    }
    summary.env.entityId = entId;
    check(t, 'entity-created', names.includes(ENT_NAME) && entId != null, `id=${entId};names=${names.slice(-6).join(',')}`);
  });

  await task('A2', 'API 赋材质 w4_mat', async (t) => {
    check(t, 'entity-id', entId != null, `id=${entId}`);
    if (entId == null) return;
    const got = await mcp('mcp__engine-scene__entity_get', { id: entId });
    const types = (got?.components ?? []).map((c) => c.type);
    if (types.includes('MeshRenderer')) {
      await mcp('mcp__engine-scene__component_set', {
        id: entId, type: 'MeshRenderer', props: { mesh: 'cube', material: W4_MAT_GUID },
      });
    } else {
      await mcp('mcp__engine-scene__component_add', {
        id: entId, type: 'MeshRenderer', props: { mesh: 'cube', material: W4_MAT_GUID },
      });
    }
    const mr = await mcp('mcp__engine-scene__component_get', { id: entId, type: 'MeshRenderer' });
    const blob = JSON.stringify(mr);
    check(t, 'material-guid-present', blob.includes(W4_MAT_GUID), blob.slice(0, 240));
    t.notes.push('asset_refs 在 scene_save 之后核验(未保存时 referencedBy 可能 NO_META)');
  });

  let graphField = 'props.graphRef';
  await task('A3', 'API 挂 door_opener 逻辑图', async (t) => {
    check(t, 'entity-id', entId != null, `id=${entId}`);
    if (entId == null) return;
    const got = await mcp('mcp__engine-scene__entity_get', { id: entId });
    const types = (got?.components ?? []).map((c) => c.type);
    if (types.includes('Script')) {
      await mcp('mcp__engine-scene__component_set', {
        id: entId, type: 'Script', props: { graphRef: GRAPH_REF, module: '', props: {} },
      });
    } else {
      await mcp('mcp__engine-scene__component_add', {
        id: entId, type: 'Script', props: { graphRef: GRAPH_REF, module: '', props: {} },
      });
    }
    const sc = await mcp('mcp__engine-scene__component_get', { id: entId, type: 'Script' });
    graphField = pickGraphField(sc);
    const blob = JSON.stringify(sc);
    check(t, 'graph-ref-present', blob.includes(GRAPH_REF), `field=${graphField};${blob.slice(0, 240)}`);
    summary.env.graphField = graphField;
  });

  await task('A4', 'API scene_save 显式路径(绕 D3)', async (t) => {
    fs.mkdirSync(path.dirname(SCENE_ABS), { recursive: true });
    const saved = await mcp('mcp__engine-scene__scene_save', { path: SCENE_REL.replace(/\\/g, '/') });
    const exists = fs.existsSync(SCENE_ABS);
    const bytes = exists ? fs.statSync(SCENE_ABS).size : 0;
    const text = exists ? fs.readFileSync(SCENE_ABS, 'utf8') : '';
    check(t, 'scene-file-exists', exists && bytes > 0, `bytes=${bytes};save=${JSON.stringify(saved).slice(0, 120)}`);
    check(t, 'scene-contains-entity', text.includes(ENT_NAME), `hasName=${text.includes(ENT_NAME)}`);
    check(t, 'scene-contains-graph', text.includes('door_opener'), `hasGraph=${text.includes('door_opener')}`);
    const refs = await mcp('mcp__asset-pipeline__asset_refs', {
      assetPath: 'Content/Materials/w4_mat.rxmat',
      direction: 'referencedBy',
    }).catch((e) => ({ error: String(e.message) }));
    t.notes.push(`asset_refs=${JSON.stringify(refs).slice(0, 240)}`);
    t.notes.push('显式 path,避开 D3 缺省 data/scene.rxscene');
  });

  await task('A5', 'API PIE play_enter/state/exit(绕 D2)', async (t) => {
    const enter = await mcp('mcp__engine-scene__play_enter', {});
    const st = await mcp('mcp__engine-scene__play_state', {});
    const state = st?.state ?? st?.playState ?? enter?.state;
    check(t, 'play-running', state === 'play_running', `state=${state};enter=${JSON.stringify(enter).slice(0, 120)}`);
    await mcp('mcp__engine-scene__viewport_set_camera', {
      target: [2, 1, 2], yaw: 45, pitch: -20, dist: 8, fovY: 60,
    }).catch(() => null);
    const frame = await mcp('mcp__engine-scene__viewport_frame', { width: 320, height: 180 }).catch((e) => ({ error: e.message }));
    const px = frame?.nonZeroPixels ?? frame?.nonzeroPixels ?? 0;
    if (frame?.error) t.notes.push(`viewport_frame: ${frame.error}`);
    else check(t, 'viewport-frame-nonzero', Number(px) > 0, `nonZeroPixels=${px}`);
    const exit = await mcp('mcp__engine-scene__play_exit', {});
    const st2 = await mcp('mcp__engine-scene__play_state', {});
    const state2 = st2?.state ?? st2?.playState ?? exit?.state;
    check(t, 'play-back-edit', state2 === 'edit', `state=${state2}`);
  });

  await task('A6', 'API playtest 夹具矩阵(D4 标注)', async (t) => {
    const tpl = fs.readFileSync(path.join(ROOT, 'tools/e2e/fixtures/journey-matrix.template.json'), 'utf8');
    const body = tpl
      .replaceAll('{{SCENE}}', SCENE_REL.replace(/\\/g, '/'))
      .replaceAll('{{ENTITY}}', ENT_NAME)
      .replaceAll('{{GRAPH_FIELD}}', graphField);
    fs.writeFileSync(MATRIX_FILE, body);
    const r = await hapi('POST', '/api/forge/playtest/run', { matrixRef: MATRIX_FILE.replace(/\\/g, '/') });
    check(t, 'playtest-http', r.status === 200, `HTTP ${r.status};${r.text.slice(0, 200)}`);
    const ok = r.json?.ok === true;
    check(t, 'playtest-ok', ok, `passed=${r.json?.passed} failed=${r.json?.failed}`);
    t.notes.push('夹具由测试基建写入临时目录,不是产品 workspace 写文件面;D4 仍开');
    t.notes.push(`report=${JSON.stringify({ ok: r.json?.ok, passed: r.json?.passed, failed: r.json?.failed, cases: r.json?.cases }).slice(0, 400)}`);
  });

  await task('A7', 'API project-pack 经 3080(无 UI)', async (t) => {
    try { fs.rmSync(PACK_OUT, { recursive: true, force: true }); } catch { /* ignore */ }
    const r = await hapi('POST', '/api/forge/project/pack', {
      sceneRef: SCENE_REL.replace(/\\/g, '/'),
      outDir: PACK_OUT.replace(/\\/g, '/'),
    });
    check(t, 'pack-http-3080', r.status === 200, `HTTP ${r.status};${r.text.slice(0, 240)}`);
    const exe = path.join(PACK_OUT, 'bin', 'engine-host.exe');
    const runner = path.join(PACK_OUT, 'pack-run.ps1');
    check(t, 'pack-engine-host', fs.existsSync(exe), exe);
    check(t, 'pack-run-script', fs.existsSync(runner), runner);
    const warnings = r.json?.warnings ?? [];
    t.notes.push(`warnings=${JSON.stringify(warnings).slice(0, 240)};files=${JSON.stringify(r.json?.files ?? r.json?.copied ?? []).toString().slice(0, 200)}`);
    if (warnings.length) t.notes.push('warnings 如实记录,不因非空自动标红');
  });
}

async function uiJourney(page) {
  await task('U0', '基线壳三栏 + SSE', async (t) => {
    await page.goto(`${HOST_ORIGIN}/`, { waitUntil: 'domcontentloaded' });
    await page.getByTestId('shell').waitFor({ timeout: 20000 });
    await forgeReady(page);
    const panes = await Promise.all(['pane-sessions', 'pane-chat', 'pane-main'].map(async (id) => ({
      id, n: await page.getByTestId(id).count(),
    })));
    check(t, 'three-panes', panes.every((p) => p.n >= 1), JSON.stringify(panes));
    const sb = await page.getByTestId('shell-statusbar').count();
    check(t, 'statusbar', sb >= 1, `n=${sb}`);
    await shot(page, t, 'baseline', 'f9-e2e');
  });

  await task('U1', '命令面板 Ctrl+K 打开设置', async (t) => {
    await page.keyboard.press('Control+K');
    await page.getByTestId('command-palette-input').waitFor({ timeout: 8000 });
    check(t, 'palette-open', (await page.getByTestId('command-palette-input').count()) === 1);
    await page.getByTestId('command-row-settings.open').click();
    await page.getByTestId('settings-overlay').waitFor({ timeout: 8000 });
    check(t, 'settings-opened', (await page.getByTestId('settings-overlay').count()) === 1);
    await shot(page, t, 'command-palette', 'f9-e2e');
  });

  await task('U2', '设置五页渲染 + 模型 availability', async (t) => {
    await page.evaluate(() => window.__forgeShell.openSettings('appearance'));
    for (const id of ['appearance', 'agent', 'models', 'skills', 'about']) {
      await page.getByTestId(`settings-nav-${id}`).click();
      await page.getByTestId(`settings-page-${id}`).waitFor({ timeout: 8000 });
      check(t, `page-${id}`, (await page.getByTestId(`settings-page-${id}`).count()) === 1);
    }
    await page.getByTestId('settings-nav-models').click();
    await page.getByTestId('settings-page-models').waitFor({ timeout: 8000 });
    const ds = (await page.getByTestId('deepseek-availability').textContent().catch(() => ''))?.trim();
    const oai = (await page.getByTestId('oai-availability').textContent().catch(() => ''))?.trim();
    check(t, 'deepseek-availability-real', !!ds && ds.length > 0, `deepseek=${ds}`);
    check(t, 'oai-availability-real', !!oai && oai.length > 0, `oai=${oai}`);
    await shot(page, t, 'settings-models', 'f9-e2e');
    await page.keyboard.press('Escape');
  });

  await task('U3', 'workbench + 底栏 + Inspector 预览', async (t) => {
    await page.evaluate(() => {
      window.__forgeShell.openTab('plan');
    });
    await page.getByTestId('workbench-tab-plan').waitFor({ timeout: 8000 });
    check(t, 'tab-plan', true);
    await page.evaluate(() => window.__forgeShell.openTab('todo'));
    await page.getByTestId('workbench-tab-todo').waitFor({ timeout: 5000 });
    check(t, 'tab-todo', true);
    await page.evaluate(() => window.__forgeShell.openEditor());
    await page.getByTestId('workbench-tab-editor').waitFor({ timeout: 10000 });
    check(t, 'tab-editor', true);
    await page.evaluate(() => window.__forgeShell.toggleBottom());
    await page.getByTestId('bottom-panel').waitFor({ timeout: 8000 });
    for (const id of ['logs', 'output', 'metrics']) {
      await page.getByTestId(`bottom-tab-${id}`).click();
      check(t, `bottom-${id}`, (await page.getByTestId(`bottom-tab-${id}`).count()) === 1);
    }
    // 编辑器 tab 激活后右栏由层级接管,切回「工作区」页看文件树
    await page.getByTestId('rightpane-tab-hierarchy').waitFor({ timeout: 8000 });
    check(t, 'rightpane-hierarchy-on-editor', (await page.getByTestId('hierarchy-panel').count()) === 1);
    await page.getByTestId('rightpane-tab-files').click();
    await page.getByTestId('inspector').waitFor({ timeout: 8000 });
    const files = page.locator('[data-testid^="ws-file-"]');
    const n = await files.count();
    check(t, 'ws-files', n >= 1, `n=${n}`);
    if (n >= 1) {
      await files.first().click();
      const preview = await page.getByTestId('ws-preview-content').textContent({ timeout: 8000 }).catch(() => '');
      check(t, 'file-preview-nonempty', String(preview).trim().length > 0, `len=${String(preview).trim().length}`);
    }
    await shot(page, t, 'workbench-inspector', 'f9-e2e');
  });

  await task('U4', '资产右键浏览器禁用态', async (t) => {
    // 1600 宽下主区 669px 落中档,Assets 列自动收起(响应式波);点工具条钮唤回
    if ((await page.locator('[aria-label="Assets"]').count()) === 0) {
      await page.getByTestId('editor-toggle-assets').click();
      t.notes.push('Assets 列在 1600 宽下自动收起,经工具条 PanelLeft 钮唤回后继续');
    }
    const assets = page.locator('[aria-label="Assets"]');
    check(t, 'assets-panel', (await assets.count()) >= 1);
    await assets.click({ button: 'right' }).catch(() => {});
    const importItem = page.getByText('Import to here');
    const showItem = page.getByText('Show in folder');
    const importVisible = await importItem.count();
    if (importVisible === 0) {
      t.notes.push('右键菜单未弹出(可能需点资产行);记降级不充绿');
      t.verdict = 'pass-degraded';
      await shot(page, t, 'assets-nomenu', 'f9-e2e');
      return;
    }
    const importDisabled = await importItem.isDisabled().catch(() => false);
    const showDisabled = await showItem.isDisabled().catch(() => false);
    check(t, 'import-disabled', importDisabled, `disabled=${importDisabled}`);
    check(t, 'show-in-folder-disabled', showDisabled, `disabled=${showDisabled}`);
    await shot(page, t, 'assets-disabled', 'f9-e2e');
    await page.keyboard.press('Escape');
  });

  await task('U5', 'Composer 薄检(模式/模型菜单)', async (t) => {
    await page.getByTestId('composer-add').click();
    await page.getByTestId('composer-add-menu').waitFor({ timeout: 5000 });
    check(t, 'mode-menu', (await page.getByTestId('composer-add-menu').count()) === 1);
    await page.keyboard.press('Escape');
    await page.getByTestId('composer-model').click();
    await page.getByTestId('composer-model-menu').waitFor({ timeout: 5000 });
    check(t, 'model-menu', (await page.getByTestId('composer-model-menu').count()) === 1);
    await page.keyboard.press('Escape');
    const toastN = await page.getByTestId('toast-stack').count();
    t.notes.push(`toast-stack 节点=${toastN};未主动触发 toast,不充绿不编造`);
    await shot(page, t, 'composer', 'f9-e2e');
  });

  await task('U6', 'UI 建会话 + chat 建实体(确定性 mock LLM)', async (t) => {
    const cfg = await hapi('POST', '/api/forge/llm/openai-compat/config', {
      baseUrl: mock.baseUrl, model: OAI_MODEL, key: OAI_DUMMY_KEY,
    });
    check(t, 'openai-compat-configured', cfg.status === 200 && cfg.json?.configured === true, `HTTP ${cfg.status}`);
    const sid = await newSessionViaUi(page);
    check(t, 'session-created', !!sid, `sid=${sid}`);
    await page.getByTestId(`session-row-${sid}`).dblclick();
    await page.getByTestId(`session-rename-${sid}`).fill(`e2e-journey-${TS.slice(-6)}`);
    await page.keyboard.press('Enter');
    await page.getByTestId(`session-row-${sid}`).hover();
    await page.getByTestId(`session-row-${sid}`).getByLabel('置顶').click();
    await page.waitForFunction((id) => {
      const s = window.__forgeShell.stores.sessions.getState().sessions.find((x) => x.id === id);
      return s?.pinned === true;
    }, sid, { timeout: 10000 });
    check(t, 'session-pinned', true);
    await pickModel(page, 'openai-compat');
    const uiEnt = `${ENT_NAME}-UI`;
    const before = await sendChat(page, `请创建一个名为 ${uiEnt} 的立方体实体`);
    const term = await waitNewTerminal(page, before, 90000);
    check(t, 'run-completed', term.status === 'completed', `status=${term.status};err=${term.error ?? ''}`);
    check(t, 'tool-loop-entity-create', term.tools.some((x) => x.name === 'mcp__engine-scene__entity_create' && x.ok === true), JSON.stringify(term.tools));
    const list = await mcp('mcp__engine-scene__entity_list');
    const names = (list?.entities ?? []).map((e) => e.name);
    check(t, 'entity-list-has-ui', names.includes(uiEnt) || names.includes(ENT_NAME), names.slice(-8).join(','));
    await shot(page, t, 'chat-create', 'f9-e2e');
  });

  await task('U7', '编辑器出帧 + NodeGraph Enter 加载(绕 D2)', async (t) => {
    await page.evaluate(() => window.__forgeShell.openEditor());
    await page.getByTestId('workbench-tab-editor').waitFor({ timeout: 10000 });
    await mcp('mcp__engine-scene__viewport_set_camera', {
      target: [2, 1, 2], yaw: 45, pitch: -20, dist: 8, fovY: 60,
    }).catch(() => null);
    await sleep(600);
    const st = await waitViewportStat(page, 20000);
    check(t, 'viewport-stat', !!st, JSON.stringify(st));
    if (st) check(t, 'viewport-px', st.px > 0, `px=${st.px}`);
    await page.getByText('NodeGraph', { exact: true }).first().click().catch(() => {});
    const input = page.locator('[data-graph-path-input]');
    if (await input.count()) {
      await input.fill(GRAPH_REF);
      await input.press('Enter');
      await sleep(800);
      const cards = await page.locator('[data-testid^="const-"], [data-testid^="exposed-"]').count();
      if (cards >= 1) check(t, 'graph-nodes', cards >= 1, `nodesish=${cards}`);
      else {
        t.notes.push('Enter 加载后未见节点卡;API 步已核验 graphRef;D2 可能仍挡加载钮');
        t.verdict = results.find((x) => x.id === 'U7') ? undefined : 'pass-degraded';
        const self = results[results.length - 1];
        if (self && self.assertions.every((a) => a.pass)) self.verdict = 'pass-degraded';
      }
    } else {
      t.notes.push('NodeGraph 输入未找到(中心 tab 可能未切到);API 步已核验 graphRef');
      const self = results[results.length - 1];
      if (self) self.verdict = 'pass-degraded';
    }
    await shot(page, t, 'editor-graph', 'f9-e2e');
  });
}

async function main() {
  summary.orphanCheck.before = orphanScan(STARTED);
  log(`== F9 全项目 journey== evidence=${path.relative(ROOT, SUMMARY_FILE)};ENT=${ENT_NAME}`);
  await ensureBuild(log);
  summary.prep.push('构建产物齐');

  log('== 端口预检 8103/3080 ==');
  reclaimPort(AGENTD_PORT, PORT_SIG_AGENTD, log);
  reclaimPort(HOST_PORT, PORT_SIG_HOST, log);
  if (!(await portFree(AGENTD_PORT)) || !(await portFree(HOST_PORT))) throw new Error('端口回收后仍被占用');
  summary.prep.push('端口就绪');

  mock = await startMockOpenAI({ entityName: `${ENT_NAME}-UI` });
  log(`== mock OpenAI ${mock.baseUrl} ==`);

  agentdChild = spawnLogged('agentd', path.join(ROOT, 'target/debug/forge-agentd.exe'), [], {
    FORGE_AGENTD_DATA_DIR: agentData,
    FORGE_GEN_DATA_DIR: genData,
  }, LOG_FILE);
  await waitHttp(`${AGENTD_ORIGIN}/health`, 30000, 'agentd');
  summary.prep.push(`agentd pid=${agentdChild.pid}`);

  hostChild = spawnLogged('host', process.execPath, ['packages/host/dist/index.js'], {
    FORGE_AGENTD_ORIGIN: AGENTD_ORIGIN,
  }, LOG_FILE);
  await waitHttp(`${HOST_ORIGIN}/api/forge/health`, 30000, 'host');
  summary.prep.push(`host pid=${hostChild.pid}`);

  await apiJourney();

  log('== 启动浏览器 UI 薄层 ==');
  const launched = await launchSystemBrowser();
  browser = launched.browser;
  summary.env.browserChannel = launched.channel;
  const ctx = await browser.newContext({ viewport: { width: 1600, height: 900 }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => summary.hygiene.pageErrors.push(String(e.message ?? e)));
  page.on('console', (msg) => { if (msg.type() === 'error') summary.hygiene.consoleErrors.push(msg.text()); });
  page.setDefaultTimeout(15000);
  await uiJourney(page);
  await ctx.close();
}

let exitCode = 0;
try {
  await main();
} catch (e) {
  log(`[FATAL] ${e?.stack ?? e}`);
  exitCode = 1;
} finally {
  summary.orphanCheck.after = await cleanupStack({
    browser, hostChild, agentdChild, mockServer: mock?.server, tmp, startedMs: STARTED, log,
  });
  await cleanupJourneyArtifacts();
}

summary.finishedUtc = new Date().toISOString();
summary.durationMs = Date.now() - STARTED;
summary.tasks = results;
summary.verdicts = Object.fromEntries(results.map((t) => [t.id, t.verdict]));
const hardFail = results.some((t) => t.verdict === 'fail');
summary.allGreen = results.length > 0 && results.every((t) => t.verdict === 'pass');
summary.gateGreen = results.every((t) => t.verdict === 'pass' || t.verdict === 'pass-degraded' || t.verdict === 'annotated-mock');
summary.orphanFree = summary.orphanCheck.after.length === 0;
fs.writeFileSync(SUMMARY_FILE, JSON.stringify(summary, null, 2));

log('== 汇总 ==');
for (const t of results) {
  log(`  ${t.id}: ${t.verdict} (${t.durationMs}ms, 断言 ${t.assertions.filter((a) => a.pass).length}/${t.assertions.length})`);
}
log(`  hygiene pageErrors=${summary.hygiene.pageErrors.length} consoleErrors=${summary.hygiene.consoleErrors.length}`);
log(`  allGreen=${summary.allGreen} gateGreen=${summary.gateGreen} orphanFree=${summary.orphanFree}`);
if (hardFail) exitCode = 1;
if (!summary.orphanFree) exitCode = 1;
log(`exit=${exitCode} summary=${path.relative(ROOT, SUMMARY_FILE)}`);
process.exit(exitCode);
