import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { bundleUltraPlanRuntime } from './ultraplan-runtime.mjs';

test('packaged probe works without checkout cwd, global Node or node_modules links', { timeout: 90000 }, async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'forge-runtime-test-'));
  const runtime = path.join(root, 'ultraplan-runtime');
  const server = http.createServer((_request, response) => {
    response.setHeader('Content-Type', 'text/html');
    response.end(`<!doctype html><body style="background:#123;color:#ffd">Demo<span id="count">0</span><script>
      let count=0; window.__demo={reset(){count=0},tick(){},getState(){return {count}}};
      addEventListener('keydown',e=>{if(e.key==='ArrowRight'){count++;document.querySelector('#count').textContent=count}});
      </script>`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
    await bundleUltraPlanRuntime({ repoRoot, outputDir: runtime });
    assert.equal((await fs.lstat(path.join(runtime, 'node_modules/playwright-core'))).isSymbolicLink(), false);
    const node = path.join(runtime, process.platform === 'win32' ? 'node.exe' : 'node');
    const output = await new Promise((resolve, reject) => {
      const child = spawn(node, [path.join(runtime, 'web-demo-probe.mjs')], {
        cwd: root, env: { ...process.env, PATH: '', NODE_PATH: '', NODE_OPTIONS: '' }, windowsHide: true,
        stdio: ['pipe', 'pipe', 'pipe'],
      });
      let stdout = '', stderr = '';
      child.stdout.on('data', data => { stdout += data; });
      child.stderr.on('data', data => { stderr += data; });
      child.on('error', reject);
      child.on('close', code => code === 0 ? resolve(stdout) : reject(new Error(`${code}: ${stderr}\n${stdout}`)));
      child.stdin.end(JSON.stringify({ url: `http://127.0.0.1:${server.address().port}/u/bundle/`,
        script: [{ call: 'reset' }], inputs: [{ kind: 'keyPress', key: 'ArrowRight' }],
        assertions: [{ path: 'count', expected: 1 }], screenshotPath: path.join(root, 'frame.png') }));
    });
    const report = JSON.parse(output);
    assert.equal(report.ok, true, JSON.stringify(report));
    assert.equal(report.inputVerified, true);
    assert.ok((await fs.stat(report.screenshot)).size > 100);
  } finally {
    await new Promise(resolve => server.close(resolve));
    const resolved = await fs.realpath(root);
    assert.equal(path.dirname(resolved), await fs.realpath(os.tmpdir()));
    assert.ok(path.basename(resolved).startsWith('forge-runtime-test-'));
    await fs.rm(resolved, { recursive: true });
  }
});
