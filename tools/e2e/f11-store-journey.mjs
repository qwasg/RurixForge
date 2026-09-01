#!/usr/bin/env node
/**
 * F11 资产商店 / Skill 管理浏览器 journey(G-F11-6 的 UI 腿)。
 *
 * 纯浏览器(playwright-core + 系统 Edge/Chrome,不下载浏览器二进制)经 host
 * (静态托管 client/dist + /api/forge/* 代理)→ agentd → forge-store 真链,
 * 逐任务独立布尔断言 + 截图 evidence。**不打桩商店后端**:官方源 = 仓内 registry/
 * 的 file:// 驱动,安装真的走 assetd 构建链落 Content/。
 *
 * 任务矩阵:
 *   S1 Sidebar 两个入口 → 两个 workbench tab(D-025:不新增常驻面板)
 *   S2 发现页:搜索 / 类型过滤 / 卡片信息层级 / 坏源错误横幅如实上报(I-5)
 *   S3 详情页:版本列表 / 许可证 / 文件清单 sha256
 *   S4 安装:进度轮询 → completed → 已安装页出现 → 更新检查
 *   S5 卸载两阶段:确认条 → 409 建单 → 在此批准 → 完成 → 记录消失(I-6)
 *   S6 个人资产库:收藏 → 去重 → 装进项目 → 移出
 *   S7 Skill 管理:列表 / 新建 / 编辑保存 / 校验
 *   S8 Skill 删除两阶段 + Composer 技能菜单按 enabled 过滤
 *
 * 隔离与清理:agentd 用临时 data 根(源配置/个人库/keystore 全隔离)+ 预写
 * store-sources.json 锚定仓内 registry;finally 卸载残留资产、删临时技能、清进程。
 *
 * 端口:缺省 8103/3080;被开发者 dev 实例占用时可用 FORGE_E2E_AGENTD_PORT /
 * FORGE_E2E_HOST_PORT 覆盖并排跑(不越权杀别人的进程)。
 *
 * 用法: node tools/e2e/f11-store-journey.mjs
 * 产物: evidence/f11-journey-S{n}-*.png / evidence/f11-journey-<UTC>.json / .log
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  ROOT, EVIDENCE, utcStamp, makeLog, sleep, ensureBuild, waitHttp, makeHapi,
  spawnLogged, launchSystemBrowser, createTaskRunner, cleanupStack, portFree,
} from './lib/harness.mjs';

const TS = utcStamp();
const STARTED = Date.now();

/** 缺省端口被开发者 dev 实例占用时退备用,不 reclaim 杀别人的进程。 */
async function pickPort(preferred, fallback, envName) {
  const envVal = Number(process.env[envName]);
  if (Number.isInteger(envVal) && envVal > 0 && envVal < 65536) return envVal;
  if (await portFree(preferred)) return preferred;
  if (await portFree(fallback)) return fallback;
  throw new Error(`${preferred} 与备用 ${fallback} 均被占用,请先关掉一个实例或设 ${envName}`);
}

const AGENTD_PORT = await pickPort(8103, 8123, 'FORGE_E2E_AGENTD_PORT');
const HOST_PORT = await pickPort(3080, 3090, 'FORGE_E2E_HOST_PORT');
const HOST_ORIGIN = `http://127.0.0.1:${HOST_PORT}`;

fs.mkdirSync(EVIDENCE, { recursive: true });
const LOG_FILE = path.join(EVIDENCE, `f11-journey-${TS}.log`);
const SUMMARY_FILE = path.join(EVIDENCE, `f11-journey-${TS}.json`);
const log = makeLog(LOG_FILE);

const TMP = fs.mkdtempSync(path.join(os.tmpdir(), 'f11-journey-'));
const AGENT_DATA = path.join(TMP, 'agent-data');
const GEN_DATA = path.join(TMP, 'gen-data');
const DEMO = path.join(ROOT, 'projects', 'demo');
const SKILL_SCENE_AUDIT = path.join(ROOT, 'skills', 'scene-audit');
const TMP_SKILL = `zz-journey-${process.pid}`;
const TMP_SKILL_DIR = path.join(ROOT, 'skills', TMP_SKILL);
// 冒烟落地物(finally 精确清理;命名与 demo 既有资产错开)
const INSTALLED_FILES = [
  'Content/Meshes/starter_chair.gltf',
  'Content/Textures/starter_dot.png',
  'Content/Materials/starter_red.rxmat',
  'Content/Textures/pbr_wood_albedo.png',
  'Content/Textures/pbr_wood_normal.png',
  'Content/Textures/pbr_wood_roughness.png',
  'Content/Misc/f2w4_dot.png',
];

const results = [];
const { check, shot, task } = createTaskRunner(log, results);
const hapi = makeHapi(HOST_ORIGIN);

function seedSources() {
  fs.mkdirSync(GEN_DATA, { recursive: true });
  const registryUrl = `file:///${path.join(ROOT, 'registry').replace(/\\/g, '/')}`;
  fs.writeFileSync(
    path.join(GEN_DATA, 'store-sources.json'),
    JSON.stringify({
      sources: [
        { id: 'official', name: '官方源', baseUrl: registryUrl, enabled: true },
        // 故意坏源:验证 UI 如实展示不可达源,而非静默吞成「无结果」(I-5)
        { id: 'broken', name: '故意坏源', baseUrl: 'file:///Z:/no-such-registry-xyz', enabled: true },
      ],
    }, null, 2),
    'utf8',
  );
}

/** 等某个 testid 出现。 */
async function waitFor(page, testId, timeout = 20000) {
  await page.getByTestId(testId).waitFor({ timeout });
}

/** 发现页卡片(排除卡上按钮/价格的同前缀 testid)。 */
function storeCards(page) {
  return page.locator('[data-testid^="store-card-forge."]');
}

/**
 * CodeMirror 6 的宿主 div 不是 input,Playwright fill() 会拒。
 * 点 .cm-content 后 Ctrl+A + insertText,经 updateListener 回写 skillStore.draft。
 */
async function fillSkillEditor(page, text) {
  const cm = page.locator('[data-testid="skill-editor"] .cm-content');
  await cm.waitFor({ timeout: 15000 });
  await cm.click();
  await page.keyboard.press('Control+A');
  await page.keyboard.insertText(text);
}

async function openStoreTab(page) {
  await page.getByTestId('sidebar-asset-store').click();
  await waitFor(page, 'store-tab');
}

async function openSkillsTab(page) {
  await page.getByTestId('sidebar-skills').click();
  await waitFor(page, 'skills-tab');
}

let browser = null;
let agentdChild = null;
let hostChild = null;

async function main() {
  await ensureBuild(log);
  if (!fs.existsSync(path.join(ROOT, 'registry', 'index.json'))) {
    throw new Error('registry/ 种子缺失,先跑 scripts/f11-seed-registry.ps1');
  }
  if (fs.existsSync(SKILL_SCENE_AUDIT)) {
    throw new Error('skills/scene-audit 已存在(上次 journey 未清理?),请先删除');
  }

  log(`== 端口: agentd=${AGENTD_PORT} host=${HOST_PORT}${AGENTD_PORT !== 8103 || HOST_PORT !== 3080 ? ' (缺省被占,已退备用)' : ''} ==`);
  if (!(await portFree(AGENTD_PORT))) throw new Error(`agentd 端口 ${AGENTD_PORT} 仍被占用`);
  if (!(await portFree(HOST_PORT))) throw new Error(`host 端口 ${HOST_PORT} 仍被占用`);

  seedSources();
  log('== 启动 agentd(隔离 data 根) ==');
  agentdChild = spawnLogged('agentd', path.join(ROOT, 'target/debug/forge-agentd.exe'), [], {
    FORGE_AGENTD_ADDR: `127.0.0.1:${AGENTD_PORT}`,
    FORGE_AGENTD_DATA_DIR: AGENT_DATA,
    FORGE_GEN_DATA_DIR: GEN_DATA,
  }, LOG_FILE);
  await waitHttp(`http://127.0.0.1:${AGENTD_PORT}/health`, 30000, 'agentd');

  log('== 启动 host ==');
  hostChild = spawnLogged('host', process.execPath, [path.join(ROOT, 'packages/host/dist/index.js')], {
    FORGE_HOST_PORT: String(HOST_PORT),
    FORGE_AGENTD_ORIGIN: `http://127.0.0.1:${AGENTD_PORT}`,
  }, LOG_FILE);
  await waitHttp(`${HOST_ORIGIN}/api/forge/health`, 30000, 'host');

  const launched = await launchSystemBrowser();
  browser = launched.browser;
  log(`浏览器: ${launched.channel}`);
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const pageErrors = [];
  page.on('pageerror', (e) => pageErrors.push(String(e?.message ?? e)));
  page.on('console', (m) => { if (m.type() === 'error') pageErrors.push(`console: ${m.text()}`); });
  await page.goto(HOST_ORIGIN, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => !!window.__forgeShell, null, { timeout: 30000 });

  // ---------- S1 两个入口 ----------
  await task('S1', 'Sidebar 两个入口 → workbench tab', async (t) => {
    await page.getByTestId('sidebar-asset-store').waitFor({ timeout: 15000 });
    check(t, 'sidebar-asset-store 可见', await page.getByTestId('sidebar-asset-store').isVisible());
    check(t, 'sidebar-skills 可见', await page.getByTestId('sidebar-skills').isVisible());
    await openStoreTab(page);
    const tabs1 = await page.evaluate(() => window.__forgeShell.stores.workbench.getState().tabs.map((x) => x.id));
    check(t, '点资产商店开 store tab', tabs1.includes('store'), tabs1.join(','));
    await openSkillsTab(page);
    const st2 = await page.evaluate(() => {
      const w = window.__forgeShell.stores.workbench.getState();
      return { tabs: w.tabs.map((x) => x.id), active: w.activeTabId };
    });
    check(t, '点 Skill 管理开 skills tab', st2.tabs.includes('skills'), st2.tabs.join(','));
    check(t, 'skills tab 激活', st2.active === 'skills', String(st2.active));
    // 重复点击不重复开(单例)
    await page.getByTestId('sidebar-skills').click();
    const st3 = await page.evaluate(() => window.__forgeShell.stores.workbench.getState().tabs.filter((x) => x.id === 'skills').length);
    check(t, '重复点击不重复开 tab', st3 === 1, `count=${st3}`);
    await shot(page, t, 'entries', 'f11-journey');
  });

  // ---------- S2 发现页 ----------
  await task('S2', '发现页:搜索 / 过滤 / 坏源如实上报', async (t) => {
    await openStoreTab(page);
    await waitFor(page, 'store-grid', 30000);
    const cards = await storeCards(page).count();
    check(t, '三个种子包渲染为卡片', cards === 3, `cards=${cards}`);
    check(t, 'starter-props 卡片在', await page.getByTestId('store-card-forge.starter-props').isVisible());
    // 坏源:横幅如实展示,不静默吞
    const banner = page.getByTestId('store-source-errors');
    check(t, '不可达源有错误横幅', await banner.isVisible());
    await page.getByTestId('store-source-errors-toggle').click();
    const errText = await page.getByTestId('store-source-error-broken').textContent();
    check(t, '横幅列出坏源与错误码', /STORE_SOURCE_UNREACHABLE/.test(errText ?? ''), (errText ?? '').slice(0, 120));
    await shot(page, t, 'discover', 'f11-journey');
    // 类型过滤:只剩 skill 包
    await page.getByTestId('store-kind-skill').click();
    await page.waitForFunction(() => document.querySelectorAll('[data-testid^="store-card-forge."]').length === 1, null, { timeout: 15000 });
    const skillOnly = await storeCards(page).count();
    check(t, '类型过滤只剩 skill 包', skillOnly === 1, `count=${skillOnly}`);
    await page.getByTestId('store-kind-all').click();
    await page.waitForFunction(() => document.querySelectorAll('[data-testid^="store-card-forge."]').length === 3, null, { timeout: 15000 });
    // 中文搜索
    await page.getByTestId('store-search-input').fill('木质');
    await page.getByTestId('store-search-input').press('Enter');
    await page.getByTestId('store-card-forge.wood-pbr').waitFor({ timeout: 15000 });
    const woodHit = await page.getByTestId('store-card-forge.wood-pbr').isVisible();
    check(t, '中文关键词命中木质包', woodHit);
    await page.getByTestId('store-search-input').fill('');
    await page.getByTestId('store-search-input').press('Enter');
    await page.waitForFunction(() => document.querySelectorAll('[data-testid^="store-card-forge."]').length === 3, null, { timeout: 15000 });
  });

  // ---------- S3 详情页 ----------
  await task('S3', '详情页:版本 / 许可证 / 文件清单', async (t) => {
    await page.getByTestId('store-card-forge.starter-props').click();
    await waitFor(page, 'store-detail');
    const files = await page.locator('[data-testid^="store-detail-file-"]').count();
    check(t, '文件清单三条', files === 3, `files=${files}`);
    const lic = await page.getByTestId('store-detail-license').textContent();
    check(t, '许可证展示', /CC0-1\.0/.test(lic ?? ''), (lic ?? '').trim());
    const price = await page.getByTestId('store-detail-price').textContent();
    check(t, '免费包标「免费」', /免费/.test(price ?? ''), (price ?? '').trim());
    await shot(page, t, 'detail', 'f11-journey');
  });

  // ---------- S4 安装 ----------
  await task('S4', '安装:进度 → completed → 已安装 → 更新检查', async (t) => {
    await page.getByTestId('store-detail-install').click();
    // 进度条出现(轮询驱动)
    const bar = page.getByTestId('store-task-bar');
    await bar.waitFor({ timeout: 15000 }).catch(() => {});
    // 等后端落地(以 REST 为准,UI 只是呈现)
    let installed = [];
    for (let i = 0; i < 60; i += 1) {
      const r = await hapi('GET', '/api/forge/store/installed');
      installed = r.json?.installed ?? [];
      if (installed.some((x) => x.packageId === 'forge.starter-props')) break;
      await sleep(500);
    }
    const rec = installed.find((x) => x.packageId === 'forge.starter-props');
    check(t, '后端已装记录出现', !!rec, JSON.stringify(rec ?? {}).slice(0, 160));
    check(t, '落地三个资产', (rec?.assetPaths?.length ?? 0) === 3, (rec?.assetPaths ?? []).join(','));
    // provenance 契约(08 Errata E-08-001)
    const metaPath = path.join(DEMO, 'Content/Meshes/starter_chair.gltf.meta');
    const metaText = fs.existsSync(metaPath) ? fs.readFileSync(metaPath, 'utf8') : '';
    check(t, '.meta provenance.origin=store-install', /origin:\s*store-install/.test(metaText));
    check(t, '.meta 带 packageId 溯源', /forge\.starter-props/.test(metaText));
    // 已安装页
    await page.getByTestId('store-subtab-installed').click();
    await waitFor(page, 'store-installed-list', 20000);
    const rowVisible = await page.getByTestId('store-installed-row-official/forge.starter-props').isVisible().catch(() => false);
    check(t, '已安装页出现该包', rowVisible);
    await shot(page, t, 'installed', 'f11-journey');
    // 装一个旧版 wood 包,再查更新
    const w = await hapi('POST', '/api/forge/store/install', { sourceId: 'official', pkgId: 'forge.wood-pbr', version: '1.0.0' });
    for (let i = 0; i < 60; i += 1) {
      const s = await hapi('GET', `/api/forge/store/tasks/${w.json.taskId}`);
      if (s.json?.status !== 'running') break;
      await sleep(500);
    }
    await page.getByTestId('store-check-updates').click();
    await sleep(2000);
    const updateBtn = await page.getByTestId('store-update-official/forge.wood-pbr').isVisible().catch(() => false);
    check(t, '旧版包显示「更新到 1.1.0」', updateBtn);
    await shot(page, t, 'updates', 'f11-journey');
  });

  // ---------- S5 卸载两阶段 ----------
  await task('S5', '卸载 Proposal 两阶段(I-6)', async (t) => {
    await page.getByTestId('store-subtab-installed').click();
    await waitFor(page, 'store-installed-list');
    await page.getByTestId('store-uninstall-official/forge.starter-props').click();
    await waitFor(page, 'store-uninstall-bar');
    check(t, '出现内联确认条(非模态)', await page.getByTestId('store-uninstall-bar').isVisible());
    const dialogs = await page.locator('dialog').count();
    check(t, '未用 <dialog>(非模态纪律)', dialogs === 0, `dialog=${dialogs}`);
    await page.getByTestId('store-uninstall-confirm').click();
    await waitFor(page, 'store-uninstall-proposal', 20000);
    const propText = await page.getByTestId('store-uninstall-proposal').textContent();
    check(t, '被 409 拦下并显示提案号', /prop_/.test(propText ?? ''), (propText ?? '').slice(0, 120));
    const stillThere = fs.existsSync(path.join(DEMO, 'Content/Meshes/starter_chair.gltf'));
    check(t, '批准前文件未删', stillThere);
    await shot(page, t, 'uninstall-proposal', 'f11-journey');
    await page.getByTestId('store-uninstall-approve').click();
    let gone = false;
    for (let i = 0; i < 60; i += 1) {
      const r = await hapi('GET', '/api/forge/store/installed');
      gone = !(r.json?.installed ?? []).some((x) => x.packageId === 'forge.starter-props');
      if (gone) break;
      await sleep(500);
    }
    check(t, '批准后卸载完成(记录消失)', gone);
    check(t, '资产文件已删', !fs.existsSync(path.join(DEMO, 'Content/Meshes/starter_chair.gltf')));
    check(t, '.meta 已删', !fs.existsSync(path.join(DEMO, 'Content/Meshes/starter_chair.gltf.meta')));
    await shot(page, t, 'uninstall-done', 'f11-journey');
  });

  // ---------- S6 个人资产库 ----------
  await task('S6', '个人资产库:收藏 / 去重 / 装进项目 / 移出', async (t) => {
    await page.getByTestId('store-subtab-library').click();
    await waitFor(page, 'store-library');
    await page.getByTestId('store-library-path-input').fill('Textures/f2w4_dot.png');
    await page.getByTestId('store-library-add').click();
    await page.locator('[data-testid^="store-library-item-"]').first().waitFor({ timeout: 15000 });
    let items = (await hapi('GET', '/api/forge/store/library')).json?.items ?? [];
    check(t, '收藏成功', items.length === 1, `items=${items.length}`);
    // 同内容二次入库 → 去重
    await page.getByTestId('store-library-path-input').fill('Textures/f2w4_dot.png');
    await page.getByTestId('store-library-add').click();
    await sleep(800);
    items = (await hapi('GET', '/api/forge/store/library')).json?.items ?? [];
    check(t, '同内容不长第二条(内容寻址去重)', items.length === 1, `items=${items.length}`);
    await shot(page, t, 'library', 'f11-journey');
    const id = items[0]?.id;
    // 显式落到 Misc,避免默认 Textures 盖掉 demo 既有 f2w4_dot.png
    await page.getByTestId('store-library-dest').fill('Misc');
    await page.getByTestId(`store-library-install-${id}`).click();
    let landed = false;
    for (let i = 0; i < 40; i += 1) {
      if (fs.existsSync(path.join(DEMO, 'Content/Misc/f2w4_dot.png'))) { landed = true; break; }
      await sleep(250);
    }
    check(t, '装进项目(Misc/)', landed);
    await page.getByTestId(`store-library-remove-${id}`).click();
    await page.getByTestId('store-library-empty').waitFor({ timeout: 15000 }).catch(() => {});
    items = (await hapi('GET', '/api/forge/store/library')).json?.items ?? [];
    check(t, '移出个人库', items.length === 0, `items=${items.length}`);
  });

  // ---------- S7 Skill 管理 ----------
  await task('S7', 'Skill 管理:列表 / 新建 / 编辑 / 校验', async (t) => {
    await openSkillsTab(page);
    await sleep(1200);
    const rows = await page.locator('[data-testid^="skill-row-"]').count();
    check(t, '既有技能全部列出(≥13)', rows >= 13, `rows=${rows}`);
    await page.getByTestId('skill-new').click();
    await page.getByTestId('skill-new-name').fill(TMP_SKILL);
    await page.getByTestId('skill-new-submit').click();
    await sleep(2000);
    check(t, '新建落盘', fs.existsSync(path.join(TMP_SKILL_DIR, 'SKILL.md')));
    const listed = await page.getByTestId(`skill-row-${TMP_SKILL}`).isVisible().catch(() => false);
    check(t, '新建技能进列表', listed);
    await page.getByTestId(`skill-select-${TMP_SKILL}`).click().catch(async () => {
      await page.getByTestId(`skill-row-${TMP_SKILL}`).click();
    });
    await waitFor(page, 'skill-editor', 15000);
    // 缺省模板自身合法
    await page.getByTestId('skill-validate').click();
    await waitFor(page, 'skill-validation-ok', 15000);
    check(t, '缺省模板自身通过校验', await page.getByTestId('skill-validation-ok').isVisible());
    await shot(page, t, 'skills', 'f11-journey');
    // 编辑 → 保存(CodeMirror 不能对宿主 div fill)
    const marker = `journey-${TS}`;
    await fillSkillEditor(
      page,
      `---\nname: ${TMP_SKILL}\ndescription: 冒烟技能 ${marker}。当任务涉及「journey 冒烟」时使用。\n---\n\n# ${marker}\n\n## 目标\n验证编辑保存链路。\n\n## 执行流程\n1. 读现状。\n2. 改一处。\n\n## 输出约束\n一行结论。\n\n## 失败回退策略\n如实报告失败,不掩盖。\n`,
    );
    await page.getByTestId('skill-save').click();
    let disk = '';
    for (let i = 0; i < 40; i += 1) {
      if (fs.existsSync(path.join(TMP_SKILL_DIR, 'SKILL.md'))) {
        disk = fs.readFileSync(path.join(TMP_SKILL_DIR, 'SKILL.md'), 'utf8');
        if (disk.includes(marker)) break;
      }
      await sleep(250);
    }
    check(t, '编辑内容已落盘', disk.includes(marker));
  });

  // ---------- S8 Skill 删除两阶段 + Composer 过滤 ----------
  await task('S8', 'Skill 删除两阶段 + Composer 技能菜单按启停过滤', async (t) => {
    // 先把临时技能禁用,验证 Composer 菜单过滤
    await hapi('POST', '/api/forge/skills/config/write', { disabled: [TMP_SKILL] });
    await page.evaluate(() => {
      const w = window.__forgeShell.stores.workbench.getState();
      if (w.collapsed.chat) w.togglePane('chat');
      if (w.chatMini) w.setChatMini(false);
    });
    await page.getByTestId('sidebar-new-agent').click();
    await page.waitForFunction(() => window.__forgeShell.stores.sessions.getState().activeSessionId !== null, null, { timeout: 20000 });
    await page.getByTestId('composer-skills').click();
    await page.getByTestId('composer-skill-menu').waitFor({ timeout: 10000 });
    // 菜单先空后填(每次开都重拉 list),等启用项出现再断言,避免把「加载中」当成过滤失败。
    await page.getByTestId('skill-item-asset-cleanup').waitFor({ timeout: 15000 });
    const disabledCount = await page.getByTestId(`skill-item-${TMP_SKILL}`).count();
    check(t, '禁用技能不出现在 Composer 菜单(修既有缺陷)', disabledCount === 0, `count=${disabledCount}`);
    const enabledCount = await page.getByTestId('skill-item-asset-cleanup').count();
    check(t, '启用技能仍在菜单里', enabledCount === 1, `count=${enabledCount}`);
    await shot(page, t, 'composer-skills', 'f11-journey');
    await page.keyboard.press('Escape');
    await hapi('POST', '/api/forge/skills/config/write', { disabled: [] });

    // 删除两阶段
    await openSkillsTab(page);
    await sleep(1000);
    await page.getByTestId(`skill-select-${TMP_SKILL}`).click().catch(async () => {
      await page.getByTestId(`skill-row-${TMP_SKILL}`).click();
    });
    await waitFor(page, 'skill-editor', 15000);
    await page.getByTestId('skill-delete').click();
    await waitFor(page, 'skill-delete-bar');
    const dialogs = await page.locator('dialog').count();
    check(t, '删除确认为内联条(非模态)', dialogs === 0, `dialog=${dialogs}`);
    await page.getByTestId('skill-delete-continue').click();
    await waitFor(page, 'skill-delete-proposal', 20000);
    const ptext = await page.getByTestId('skill-delete-proposal').textContent();
    check(t, '被 409 拦下并显示提案号', /prop_/.test(ptext ?? ''), (ptext ?? '').slice(0, 120));
    check(t, '批准前技能目录还在', fs.existsSync(TMP_SKILL_DIR));
    await shot(page, t, 'skill-delete-proposal', 'f11-journey');
    await page.getByTestId('skill-delete-approve').click();
    let removed = false;
    for (let i = 0; i < 40; i += 1) {
      if (!fs.existsSync(TMP_SKILL_DIR)) { removed = true; break; }
      await sleep(400);
    }
    check(t, '批准后技能目录被删', removed);
    const r = await hapi('GET', '/api/forge/skills/list');
    const still = (r.json?.skills ?? []).some((s) => s.name === TMP_SKILL);
    check(t, '技能从清单消失', !still);
  });

  log(`页面错误数: ${pageErrors.length}`);
  return { pageErrors };
}

let hygiene = { pageErrors: [] };
try {
  hygiene = (await main()) ?? hygiene;
} catch (e) {
  log(`[FATAL] ${e?.stack ?? e}`);
  results.push({ id: 'FATAL', title: '编排异常', verdict: 'fail', assertions: [{ name: 'orchestration', pass: false, detail: String(e?.message ?? e) }], screenshots: [], notes: [], durationMs: 0 });
}

// ---------- 清理 ----------
// 商店装进 demo 的资产与临时技能一律清掉,不留改动进仓库。
for (const rel of INSTALLED_FILES) {
  const abs = path.join(DEMO, rel);
  try { fs.rmSync(abs, { force: true }); } catch { /* ignore */ }
  try { fs.rmSync(`${abs}.meta`, { force: true }); } catch { /* ignore */ }
}
for (const dir of [
  path.join(DEMO, '.forge', 'store'),
  path.join(DEMO, '.forge', 'tmp', 'store'),
  path.join(DEMO, 'Content', 'Misc'),
  TMP_SKILL_DIR,
  SKILL_SCENE_AUDIT,
]) {
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch { /* ignore */ }
}

const after = await cleanupStack({
  browser, hostChild, agentdChild, mockServer: null, tmp: TMP, startedMs: STARTED, log,
});

const pass = results.filter((r) => r.verdict === 'pass').length;
const failed = results.filter((r) => r.verdict === 'fail');
const summary = {
  at: new Date().toISOString(),
  wave: 'f11-store-journey',
  ports: { agentd: AGENTD_PORT, host: HOST_PORT },
  tasks: results,
  totals: { total: results.length, pass, fail: failed.length },
  hygiene: { pageErrors: hygiene.pageErrors.length, pageErrorSamples: hygiene.pageErrors.slice(0, 5) },
  orphans: after,
  orphanFree: after.length === 0,
  degraded: ['https 社区源(HttpSource)未做真实网络实测,官方源为仓内 file:// 驱动'],
  gateGreen: failed.length === 0 && after.length === 0,
};
fs.writeFileSync(SUMMARY_FILE, JSON.stringify(summary, null, 2), 'utf8');
log(`== 汇总:${pass}/${results.length} 任务 pass;孤儿=${after.length} ==`);
for (const r of results) log(`  ${r.id} ${r.verdict} — ${r.title}`);
log(`矩阵: ${path.relative(ROOT, SUMMARY_FILE)}`);
process.exit(summary.gateGreen ? 0 : 1);
