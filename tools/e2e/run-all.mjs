#!/usr/bin/env node
/**
 * 全方向全流程编排:静态门 → (stack 冒烟) → F8 浏览器矩阵 → F9 journey。
 * 用法:
 *   node tools/e2e/run-all.mjs
 *   node tools/e2e/run-all.mjs --profile core
 *   node tools/e2e/run-all.mjs --profile stack
 * 某层失败继续往后跑,硬门失败总 exit=1。汇总 evidence/full-auto-<UTC>.json
 */
import fs from 'node:fs';
import path from 'node:path';
import {
  ROOT, EVIDENCE, utcStamp, makeLog, run, ensureBuild, orphanScan,
} from './lib/harness.mjs';

const TS = utcStamp();
const STARTED = Date.now();
const profileArg = process.argv.includes('--profile')
  ? process.argv[process.argv.indexOf('--profile') + 1]
  : 'stack';
const PROFILE = profileArg === 'core' ? 'core' : 'stack';

fs.mkdirSync(EVIDENCE, { recursive: true });
const LOG_FILE = path.join(EVIDENCE, `full-auto-${TS}.log`);
const SUMMARY_FILE = path.join(EVIDENCE, `full-auto-${TS}.json`);
const log = makeLog(LOG_FILE);

const SKIPPED = [
  { id: 'f3-w4-settings', reason: '已退役,断言面不存在' },
  { id: 'desktop-electron', reason: 'f7-w3/w4/w5 f5-w3 f4-w4-nodegraph f6-w3 与 F8/F9 重叠,本波不编入' },
  { id: 'f6-w5-perf', reason: '本机 60fps 门,不作默认硬门' },
  { id: 'rd-f1-002', reason: '真 LLM live,无 key 也会 SKIP,不进默认硬门' },
];

const STACK_SMOKES = [
  'scripts/f4-w3-logic-smoke.ps1',
  'scripts/f5-w1-gen-backends-smoke.ps1',
  'scripts/f6-w1-playtest-smoke.ps1',
  'scripts/f6-w2-maze-matrix-smoke.ps1',
  'scripts/f6-w4-pack-smoke.ps1',
  'scripts/f7-w1-events-smoke.ps1',
  'scripts/f7-w2-turn-smoke.ps1',
  // F11(D-025):资产商店 registry 全链 + Skill 管理 CRUD/Proposal 两阶段
  'scripts/f11-w1-store-smoke.ps1',
  'scripts/f11-w2-skill-smoke.ps1',
];

function latestEvidence(prefix) {
  try {
    const files = fs.readdirSync(EVIDENCE)
      .filter((f) => f.startsWith(prefix) && (f.endsWith('.json')))
      .map((f) => ({ f, t: fs.statSync(path.join(EVIDENCE, f)).mtimeMs }))
      .sort((a, b) => b.t - a.t);
    return files[0] ? path.posix.join('evidence', files[0].f) : null;
  } catch {
    return null;
  }
}

function ps(file) {
  return `powershell -NoProfile -ExecutionPolicy Bypass -File ${file}`;
}

const layers = [];

async function layer(id, title, fn, { timeoutMs = 25 * 60_000 } = {}) {
  const rec = { id, title, verdict: 'fail', exit: null, durationMs: 0, report: null, detail: '' };
  layers.push(rec);
  const t0 = Date.now();
  log(`==== LAYER ${id} ${title} ====`);
  try {
    const r = await fn({ timeoutMs });
    rec.exit = r?.code ?? 0;
    rec.detail = (r?.out ?? '').trim().split(/\r?\n/).slice(-6).join(' | ').slice(0, 500);
    rec.report = r?.report ?? null;
    rec.verdict = rec.exit === 0 ? 'pass' : 'fail';
  } catch (e) {
    rec.exit = 1;
    rec.detail = String(e?.message ?? e).slice(0, 800);
    rec.verdict = 'fail';
    log(`  [LAYER FAIL] ${id}: ${rec.detail}`);
  }
  rec.durationMs = Date.now() - t0;
  log(`---- ${id} verdict=${rec.verdict} exit=${rec.exit} (${rec.durationMs}ms)`);
}

async function main() {
  log(`== full-auto profile=${PROFILE} ==`);
  await layer('build', '一次构建七二进制+client/host', async ({ timeoutMs }) => {
    await ensureBuild(log, { skip: false });
    return { code: 0 };
  }, { timeoutMs: 30 * 60_000 });

  await layer('typecheck', 'pnpm -r typecheck', ({ timeoutMs }) =>
    run('pnpm -r typecheck', { log, timeoutMs, allowFail: true }));

  await layer('vitest', 'pnpm -r test', ({ timeoutMs }) =>
    run('pnpm -r test', { log, timeoutMs, allowFail: true }));

  await layer('cargo', 'cargo test --workspace', ({ timeoutMs }) =>
    run('cargo test --workspace', { log, timeoutMs, allowFail: true }), { timeoutMs: 30 * 60_000 });

  await layer('go', 'go -C gateway-go test ./...', ({ timeoutMs }) =>
    run('go -C gateway-go test ./...', { log, timeoutMs, allowFail: true }));

  await layer('f0', 'f0-stack-smoke (gateway 8102 JWT)', ({ timeoutMs }) =>
    run(ps('scripts/f0-stack-smoke.ps1'), { log, timeoutMs, allowFail: true }));

  if (PROFILE === 'stack') {
    for (const rel of STACK_SMOKES) {
      const id = path.basename(rel, '.ps1');
      await layer(id, rel, ({ timeoutMs }) =>
        run(ps(rel), { log, timeoutMs, allowFail: true }), { timeoutMs: 15 * 60_000 });
    }
  }

  await layer('f8', 'F8 浏览器八任务矩阵', async ({ timeoutMs }) => {
    const r = await run('node tools/e2e/f8-w4-browser-matrix.mjs', {
      log, timeoutMs, allowFail: true,
      env: { ...process.env, FORGE_E2E_SKIP_BUILD: '1' },
    });
    return { ...r, report: latestEvidence('f8-w4-matrix-') };
  }, { timeoutMs: 25 * 60_000 });

  await layer('f9', 'F9 全项目 journey', async ({ timeoutMs }) => {
    const r = await run('node tools/e2e/f9-full-journey.mjs', {
      log, timeoutMs, allowFail: true,
      env: { ...process.env, FORGE_E2E_SKIP_BUILD: '1' },
    });
    return { ...r, report: latestEvidence('f9-e2e-summary-') };
  }, { timeoutMs: 25 * 60_000 });

  await layer('f11', 'F11 资产商店 / Skill 管理 journey', async ({ timeoutMs }) => {
    const r = await run('node tools/e2e/f11-store-journey.mjs', {
      log, timeoutMs, allowFail: true,
      env: { ...process.env, FORGE_E2E_SKIP_BUILD: '1' },
    });
    return { ...r, report: latestEvidence('f11-journey-') };
  }, { timeoutMs: 25 * 60_000 });
}

try {
  await main();
} catch (e) {
  log(`[FATAL] ${e?.stack ?? e}`);
}

const after = orphanScan(STARTED);
const summary = {
  spec: 'full-auto-test',
  profile: PROFILE,
  startedUtc: new Date(STARTED).toISOString(),
  finishedUtc: new Date().toISOString(),
  durationMs: Date.now() - STARTED,
  layers,
  skipped: SKIPPED,
  orphanCheck: { after },
  allGreen: layers.length > 0 && layers.every((l) => l.verdict === 'pass'),
  gateGreen: layers.length > 0 && layers.every((l) => l.verdict === 'pass'),
  orphanFree: after.length === 0,
};
fs.writeFileSync(SUMMARY_FILE, JSON.stringify(summary, null, 2));
log('== 总汇总 ==');
for (const l of layers) log(`  ${l.id}: ${l.verdict} exit=${l.exit} (${l.durationMs}ms) ${l.report ?? ''}`);
log(`  skipped=${SKIPPED.map((s) => s.id).join(',')}`);
log(`  allGreen=${summary.allGreen} orphanFree=${summary.orphanFree}`);
log(`  summary=${path.relative(ROOT, SUMMARY_FILE)}`);

const exitCode = summary.gateGreen && summary.orphanFree ? 0 : 1;
process.exit(exitCode);
