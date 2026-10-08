// Relocatable, offline runtime: copy this directory beside forge-agentd or into
// the application's resources directory. No source checkout or global Node is required.
import fs from 'node:fs/promises';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

export async function bundleUltraPlanRuntime({ repoRoot, outputDir, nodeExecutable = process.execPath }) {
  const require = createRequire(path.join(repoRoot, 'tools/e2e/package.json'));
  const packageFile = require.resolve('playwright-core/package.json');
  const packageRoot = await fs.realpath(path.dirname(packageFile));
  const playwright = JSON.parse(await fs.readFile(packageFile, 'utf8'));
  if (Object.keys(playwright.dependencies ?? {}).length) {
    throw new Error('playwright-core has external dependencies; update runtime bundling before release');
  }
  const runner = path.join(repoRoot, 'tools/e2e/web-demo-probe.mjs');
  const runnerBytes = await fs.readFile(runner);
  // Refuse to write through reparse points, including an existing output directory.
  outputDir = path.resolve(outputDir);
  for (let current = outputDir; ; current = path.dirname(current)) {
    try {
      if ((await fs.lstat(current)).isSymbolicLink()) throw new Error(`Runtime output follows a link: ${current}`);
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (path.dirname(current) === current) break;
  }
  await fs.mkdir(path.join(outputDir, 'node_modules'), { recursive: true });
  const nodeName = process.platform === 'win32' ? 'node.exe' : 'node';
  await fs.copyFile(nodeExecutable, path.join(outputDir, nodeName));
  if (process.platform !== 'win32') await fs.chmod(path.join(outputDir, nodeName), 0o755);
  await fs.writeFile(path.join(outputDir, 'web-demo-probe.mjs'), runnerBytes);
  // pnpm's workspace link must be dereferenced so the release can move elsewhere.
  await fs.cp(packageRoot, path.join(outputDir, 'node_modules/playwright-core'), { recursive: true, dereference: true });
  const manifest = {
    schemaVersion: 1, node: process.version, platform: process.platform, arch: process.arch,
    playwright: playwright.version, runnerSha256: createHash('sha256').update(runnerBytes).digest('hex'),
    browser: 'System Microsoft Edge or Google Chrome (missing browser is a failed probe)',
  };
  await fs.writeFile(path.join(outputDir, 'runtime-manifest.json'), JSON.stringify(manifest, null, 2));
  await fs.writeFile(path.join(outputDir, 'README.txt'),
    'RurixForge UltraPlan browser verification runtime\n' +
    'Keep this directory intact beside forge-agentd, or as resources/ultraplan-runtime.\n' +
    'FORGE_ULTRAPLAN_RUNTIME_DIR can override its location.\n' +
    'Requires installed Microsoft Edge or Google Chrome. No browser is downloaded.\n' +
    `Node ${process.version}: https://github.com/nodejs/node/blob/${process.version}/LICENSE\n` +
    'Playwright notices and license: node_modules/playwright-core/NOTICE and LICENSE\n');
  return { outputDir, ...manifest };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
  const result = await bundleUltraPlanRuntime({ repoRoot,
    outputDir: path.join(repoRoot, 'apps/desktop/dist/ultraplan-runtime') });
  console.log(`[build] UltraPlan runtime: ${result.outputDir} (Node ${result.node}, Playwright ${result.playwright})`);
}
