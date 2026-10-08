import assert from 'node:assert/strict';
import test from 'node:test';
import http from 'node:http';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { evaluateAssertions, runProbe, validateRequest } from './web-demo-probe.mjs';

test('only scoped loopback URLs, bounded inputs and known hooks can run', () => {
  const valid = { url: 'http://127.0.0.1:4567/u/demo_123/', inputs: [], assertions: [] };
  assert.equal(validateRequest(valid).url, valid.url);
  for (const url of ['https://example.com/u/demo/', 'http://127.0.0.1:4567/api/', 'http://user@localhost:4567/u/demo/']) {
    assert.throws(() => validateRequest({ ...valid, url }));
  }
  assert.throws(() => validateRequest({ ...valid, script: [{ call: 'eval' }] }));
  assert.throws(() => validateRequest({ ...valid, inputs: [{ kind: 'wait', ms: 2001 }] }));
  assert.throws(() => validateRequest({ ...valid, assertions: [{ path: '__proto__.x' }] }));
});

test('assertion failures and missing properties remain red', () => {
  const results = evaluateAssertions({ player: { x: 12 }, phase: 'playing' }, [
    { path: 'player.x', op: 'gt', expected: 10 },
    { path: 'phase', expected: 'won' },
    { path: 'missing', op: 'neq', expected: 1 },
  ]);
  assert.deepEqual(results.map((row) => row.pass), [true, false, false]);
});

test('system browser drives real keyboard, captures evidence and rejects broken games', { timeout: 90000 }, async () => {
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'forge-web-probe-test-'));
  const html = `<!doctype html><meta charset="utf-8"><canvas width="960" height="540"></canvas><script>
  let x=0; const draw=()=>{const c=document.querySelector('canvas').getContext('2d');c.fillStyle='#162136';c.fillRect(0,0,960,540);c.fillStyle='#ed8';c.fillRect(20+x,30,50,50)};
  window.__demo={reset(){x=0;draw()},tick(){},getState(){return {player:{x},phase:'playing'}}};
  addEventListener('keydown',e=>{if(e.key==='ArrowRight'){x+=10;draw()}});draw();
  </script>`;
  const server = http.createServer((req, res) => {
    res.writeHead(200, { 'Content-Type': 'text/html' });
    res.end(html);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const request = { url: `http://127.0.0.1:${server.address().port}/u/probe_test/`,
      script: [{ call: 'reset' }], inputs: [{ kind: 'keyPress', key: 'ArrowRight' }],
      assertions: [{ path: 'player.x', op: 'eq', expected: 10 }], screenshotPath: path.join(temporary, 'frame.png') };
    const good = await runProbe(request);
    assert.equal(good.unavailable, false, JSON.stringify(good));
    assert.equal(good.ok, true, JSON.stringify(good));
    assert.equal(good.inputVerified, true);
    assert.ok((await fs.stat(good.screenshot)).size > 100);
    const wrong = await runProbe({ ...request, assertions: [{ path: 'player.x', expected: 99 }] });
    assert.equal(wrong.ok, false);
    assert.ok(wrong.errors.some((error) => error.startsWith('ASSERTION_FAILED')));
    const hooksOnly = await runProbe({ ...request, inputs: [] });
    assert.equal(hooksOnly.ok, false);
    assert.equal(hooksOnly.inputVerified, false);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    // Only this test's generated temporary directory; never recurse from an unchecked path.
    const resolved = await fs.realpath(temporary);
    const parent = await fs.realpath(os.tmpdir());
    assert.equal(path.dirname(resolved), parent);
    assert.ok(path.basename(resolved).startsWith('forge-web-probe-test-'));
    await fs.rm(resolved, { recursive: true });
  }
});
