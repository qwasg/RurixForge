import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { bundleUltraPlanRuntime } from './ultraplan-runtime.mjs';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

const requiredArtifacts = [
  path.join(repoRoot, 'packages', 'host', 'dist', 'index.js'),
  path.join(repoRoot, 'packages', 'client', 'dist', 'index.html'),
];

const missing = requiredArtifacts.filter((p) => !fs.existsSync(p));

if (missing.length > 0) {
  console.error('[build] FAIL: 缺少上游构建产物:');
  for (const p of missing) {
    console.error(`  - ${p}`);
  }
  process.exit(1);
}

console.log('[build] OK: 上游产物齐备');
for (const p of requiredArtifacts) {
  console.log(`  - ${p}`);
}
const runtime = await bundleUltraPlanRuntime({ repoRoot,
  outputDir: path.join(repoRoot, 'apps/desktop/dist/ultraplan-runtime') });
console.log(`[build] UltraPlan runtime: ${runtime.outputDir}`);
process.exit(0);
