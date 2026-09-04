/**
 * PvZ 网页实机试玩:真实 Edge 打开 IDE(http://localhost:5173)→ 切到 pvz 工作区 → Assets 面板
 * 双击 Level_1_1 装进视口 → Play → 鼠标点种子卡 / 点格子种植 / 点格子收阳光 → 打到胜负横幅。
 * 观测面走同一工作区作用域的 /api/forge/mcp/call(与视口连的是同一个 engine-host)。
 * 运行:node tools/web_playtest.mjs [level] [maxSeconds]   产物:evidence/pvz/web-playtest/W*.png + web_playtest.log
 */
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..', '..', '..');
const { chromium } = createRequire(path.join(ROOT, 'tools', 'e2e', 'package.json'))('playwright-core');

const OUT = path.join(ROOT, 'evidence', 'pvz', 'web-playtest');
fs.mkdirSync(OUT, { recursive: true });
const HOST = 'http://127.0.0.1:3080';
const LEVEL = process.argv[2] ?? '1-1';
const MAX_SEC = Number(process.argv[3] ?? 240);
const SCENE_ASSET = `Scenes/Levels/Level_${LEVEL.replace('-', '_')}.rxscene`;
const ORTHO = 6.2;
const CARD_X0 = -6.2, CARD_PITCH = 1.3, CARD_Y = 5.35;
const log = [];
const t0 = Date.now();
const say = (m) => {
  const line = `[t+${((Date.now() - t0) / 1000).toFixed(1).padStart(6)}s] ${m}`;
  console.log(line);
  log.push(line);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function workspaceId() {
  const r = await fetch(`${HOST}/api/forge/workspaces`);
  const j = await r.json();
  const ws = j.workspaces.find((w) => w.name === 'pvz' || /projects[\\/]+pvz/i.test(w.root));
  if (!ws) throw new Error('未注册 pvz 工作区');
  return ws.id;
}

let WS = null;
async function mcp(tool, args = {}) {
  const r = await fetch(`${HOST}/api/forge/mcp/call`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ tool: `mcp__engine-scene__${tool}`, arguments: args, workspaceId: WS }),
  });
  const env = await r.json();
  const txt = env?.content?.[0]?.text;
  if (typeof txt === 'string') {
    try { return JSON.parse(txt); } catch { return txt; }
  }
  return env;
}

async function entities(prefix) {
  const r = await mcp('entity_list');
  return (r.entities ?? [])
    .filter((e) => e.name.startsWith(prefix))
    .map((e) => ({ id: e.id, name: e.name, pos: e.transform.translation.map((v) => Math.round(v * 100) / 100) }));
}
const onscreen = async (prefix) => (await entities(prefix)).filter((e) => e.pos[1] > -20);

async function main() {
  WS = await workspaceId();
  say(`pvz workspace = ${WS}`);
  let browser = null;
  for (const ch of ['msedge', 'chrome']) {
    try {
      browser = await chromium.launch({ channel: ch, headless: false, args: ['--window-size=1700,1000'] });
      say(`browser: ${ch}`);
      break;
    } catch (e) {
      say(`launch ${ch} failed: ${e.message}`);
    }
  }
  if (!browser) throw new Error('no system browser');
  const ctx = await browser.newContext({ viewport: { width: 1700, height: 950 } });
  // 页面加载前就把当前工作区指到 pvz(workspaceStore 从 localStorage 镜像键初始化)。
  await ctx.addInitScript((id) => {
    localStorage.setItem('forge:activeWorkspace', id);
  }, WS);
  const page = await ctx.newPage();
  const shot = async (name) => {
    await page.screenshot({ path: path.join(OUT, name) });
    say(`[shot] ${name}`);
  };
  const vp = page.locator('div[role="application"][aria-label="Viewport 画布"]');
  const pie = () => page.locator('section[aria-label="Viewport"] span.font-mono').first().textContent().catch(() => null);
  const sceneLabel = () => page.locator('section[aria-label="Viewport"] span.truncate').last().textContent().catch(() => null);
  const toasts = () => page.locator('[role="status"], [role="alert"]').allTextContents().catch(() => []);

  async function clickWorld(wx, wy, label) {
    const box = await vp.boundingBox();
    const aspect = box.width / box.height;
    const nx = (wx / (ORTHO * aspect) + 1) / 2;
    const ny = (1 - wy / ORTHO) / 2;
    await page.mouse.click(box.x + nx * box.width, box.y + ny * box.height);
    say(`click ${label} world=(${wx.toFixed(2)},${wy.toFixed(2)}) canvas=(${(nx * box.width).toFixed(0)},${(ny * box.height).toFixed(0)})`);
  }
  const clickCard = (slot) => clickWorld(CARD_X0 + slot * CARD_PITCH, CARD_Y, `card#${slot}`);
  const clickCell = (row, col, rows = 5) =>
    clickWorld(-7.2 + (col - 0.5) * 1.6, (rows - row + 0.5) * 1.6 - rows * 0.8, `cell r${row}c${col}`);
  async function collectSunsOnScreen() {
    let n = 0;
    for (const s of await onscreen('SunPool_')) {
      const col = Math.floor((s.pos[0] + 7.2) / 1.6) + 1;
      const row = Math.floor((4.0 - s.pos[1]) / 1.6) + 1;
      if (row >= 1 && row <= 5 && col >= 1 && col <= 9) {
        await clickCell(row, col);
        n += 1;
        await sleep(250);
      }
    }
    return n;
  }

  try {
    if ((await mcp('play_state')).state !== 'edit') say(`reset: ${JSON.stringify(await mcp('play_exit'))}`);

    // 1) IDE → 编辑器;收起会话栏与右栏让视口变宽(游戏 HUD 按 16:9 布局)
    await page.goto('http://localhost:5173/', { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => !!window.__forgeShell, null, { timeout: 30000 });
    await page.evaluate(() => {
      window.__forgeShell.openEditor();
      const wb = window.__forgeShell.stores.workbench.getState();
      if (!wb.collapsed.sessions) wb.togglePane('sessions');
      if (!wb.collapsed.inspector) wb.togglePane('inspector');
    });
    await vp.waitFor({ timeout: 20000 });
    await sleep(2500);
    say(`editor opened: pie=${await pie()} scene=${await sceneLabel()}`);
    await shot('W01_ide_pvz_workspace.png');

    // 2) Assets 面板双击关卡场景 → 装进视口
    const item = page.locator(`[data-asset-path="${SCENE_ASSET}"]`).first();
    await item.waitFor({ timeout: 30000 });
    await item.scrollIntoViewIfNeeded();
    await item.dblclick();
    await page.waitForFunction(
      (name) => {
        const spans = document.querySelectorAll('section[aria-label="Viewport"] span.truncate');
        return [...spans].some((s) => s.textContent && s.textContent.includes(name));
      },
      `Level_${LEVEL.replace('-', '_')}`,
      { timeout: 30000 },
    );
    say(`scene opened via Assets dblclick: scene=${await sceneLabel()} toasts=${JSON.stringify(await toasts())}`);
    // 收起 Assets 底栏,让画面更大
    await page.locator('[data-testid="editor-toggle-assets"]').click();
    await sleep(2500);
    await shot('W02_level_loaded.png');

    // 3) Play
    await page.locator('button[title="Play"]').click();
    await page.waitForFunction(
      () => [...document.querySelectorAll('section[aria-label="Viewport"] span.font-mono')].some((s) => s.textContent === 'play_running'),
      null,
      { timeout: 15000 },
    );
    say(`play: pie=${await pie()} engine=${JSON.stringify(await mcp('play_state'))}`);
    await sleep(1200);
    await shot('W03_playing.png');

    // 4) 选卡 + 点格(50 阳光不够 → 不种)
    await clickCard(0);
    await sleep(300);
    await shot('W04_card_selected.png');
    await clickCell(3, 2);
    await sleep(600);
    say(`plants after first try (expect none, 50<100): ${JSON.stringify(await onscreen('PlantPool_'))}`);

    // 5) 收阳光直到能种:每 1.5s 扫一次屏上阳光并点其格子;凑齐两颗后选卡种到 r3c2
    let collected = 0;
    const deadline = Date.now() + 40000;
    while (collected < 2 && Date.now() < deadline) {
      collected += await collectSunsOnScreen();
      await sleep(1500);
    }
    say(`collected ${collected} suns`);
    await clickCard(0);
    await sleep(300);
    await clickCell(3, 2);
    await sleep(800);
    let plants = await onscreen('PlantPool_');
    say(`plants after planting: ${JSON.stringify(plants)}`);
    await shot('W05_planted.png');

    // 6) 战斗:持续收阳光,阳光够就补种 r3c4/r3c6;直到横幅出现
    const targets = [[3, 4], [3, 6], [3, 1]];
    let ti = 0;
    let extraCollected = 0;
    const battleStart = Date.now();
    let lastShot = 0;
    while ((Date.now() - t0) / 1000 < MAX_SEC) {
      extraCollected += await collectSunsOnScreen();
      if (ti < targets.length && extraCollected >= 4) {
        await clickCard(0);
        await sleep(300);
        await clickCell(targets[ti][0], targets[ti][1]);
        await sleep(600);
        const now = await onscreen('PlantPool_');
        if (now.length > plants.length) {
          plants = now;
          extraCollected -= 4;
          ti += 1;
          say(`planted #${plants.length}: ${JSON.stringify(now.map((p) => p.pos))}`);
        }
      }
      const zombies = await onscreen('ZombiePool_');
      const banners = await onscreen('Banner_');
      const secs = Math.round((Date.now() - battleStart) / 1000);
      if (secs - lastShot >= 30) {
        lastShot = secs;
        say(`battle +${secs}s zombies=${JSON.stringify(zombies.map((z) => z.pos))} plants=${plants.length} peas=${(await onscreen('PeaPool_')).length}`);
        await shot(`W06_battle_${String(secs).padStart(3, '0')}s.png`);
      }
      if (banners.length > 0) {
        say(`*** ${banners.map((b) => b.name).join(',')} at +${secs}s`);
        await sleep(800);
        await shot('W09_result.png');
        break;
      }
      await sleep(1500);
    }
    say(`final pie=${await pie()} toasts=${JSON.stringify(await toasts())}`);
  } catch (e) {
    say(`FATAL ${e.stack ?? e}`);
    await shot('W99_error.png').catch(() => {});
    process.exitCode = 1;
  } finally {
    fs.writeFileSync(path.join(OUT, 'web_playtest.log'), log.join('\n') + '\n', 'utf8');
    await browser.close();
  }
}

main();
