import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const electronBin = require('electron');

const appDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const evidenceDir = path.join(appDir, 'evidence');
fs.mkdirSync(evidenceDir, { recursive: true });

const startedAt = Date.now();
const SMOKE_TIMEOUT_MS = 90_000;
const MIN_SHOT_BYTES = 10 * 1024;

function findLatestScreenshot() {
  const candidates = fs
    .readdirSync(evidenceDir)
    .filter((name) => /^desktop-smoke-.*\.png$/.test(name))
    .map((name) => {
      const full = path.join(evidenceDir, name);
      return { full, mtimeMs: fs.statSync(full).mtimeMs };
    })
    .filter((entry) => entry.mtimeMs >= startedAt - 1000)
    .sort((a, b) => b.mtimeMs - a.mtimeMs);
  return candidates.length > 0 ? candidates[0].full : null;
}

const scenario = process.env.FORGE_SMOKE_SCENARIO || 'shell'; // F7 wave.3:home 退役,默认 shell
console.log(`[smoke] launching electron (FORGE_SMOKE=1, scenario=${scenario}) ...`);
const childLog = fs.createWriteStream(path.join(evidenceDir, 'smoke-child.log'));
const child = spawn(electronBin, ['.'], {
  cwd: appDir,
  env: { ...process.env, FORGE_SMOKE: '1' },
  stdio: ['ignore', 'pipe', 'pipe'],
});
child.stdout.pipe(childLog);
child.stderr.pipe(childLog);

const killer = setTimeout(() => {
  console.error(`[smoke] FAIL: electron 进程 ${SMOKE_TIMEOUT_MS / 1000}s 内未退出,强制终止`);
  child.kill('SIGKILL');
}, SMOKE_TIMEOUT_MS);

child.on('error', (err) => {
  clearTimeout(killer);
  console.error(`[smoke] FAIL: 无法启动 electron: ${err.message}`);
  process.exit(1);
});

child.on('exit', (code) => {
  clearTimeout(killer);

  const shot = findLatestScreenshot();
  if (!shot) {
    console.error(`[smoke] FAIL: ${evidenceDir} 中未找到本次运行的截图`);
    process.exit(1);
  }

  const size = fs.statSync(shot).size;
  if (size <= MIN_SHOT_BYTES) {
    console.error(`[smoke] FAIL: 截图过小 (${size} bytes <= ${MIN_SHOT_BYTES}): ${shot}`);
    process.exit(1);
  }

  if (code !== 0) {
    console.error(`[smoke] FAIL: electron 退出码异常 (code=${code})`);
    process.exit(1);
  }

  console.log(`[smoke] PASS: ${shot} (${size} bytes)`);
  process.exit(0);
});
