// Exercise the real presenter functions without starting Electron or a GPU process.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { EventEmitter } = require('node:events');

function harness(rpc) {
  const source = fs.readFileSync(path.join(__dirname, '../src/main.cjs'), 'utf8');
  const start = source.indexOf('const presenter = {');
  const end = source.indexOf('\nfunction smokeLog(msg)', start);
  assert.ok(start > 0 && end > start, 'presenter section must exist');
  const processes = [];
  const timers = new Set();
  const context = vm.createContext({
    Buffer, console, quitting: false, useOffscreen: false, isSmokeVisible: false,
    PRESENTER_EXE_CANDIDATES: ['presenter.exe'],
    fs: { existsSync: () => true },
    mainWindow: { getNativeWindowHandle: () => Buffer.alloc(8) },
    smokeLog: () => {}, __rpc: rpc,
    setTimeout(fn, ms) { const t = setTimeout(fn, ms); timers.add(t); return t; },
    clearTimeout(t) { clearTimeout(t); timers.delete(t); },
    clearInterval, setInterval,
    spawn() {
      const proc = new EventEmitter();
      Object.assign(proc, { pid: 100 + processes.length, exitCode: null, signalCode: null,
        stdout: new EventEmitter(), stderr: new EventEmitter(), commands: [] });
      proc.stdin = { write(line) {
        proc.commands.push(line);
        if (line === 'close\n') proc.crash();
      } };
      proc.crash = () => { proc.exitCode = 1; proc.emit('exit', 1); };
      proc.kill = proc.crash;
      processes.push(proc);
      queueMicrotask(() => proc.stdout.emit('data', Buffer.from('PRESENTER_READY\n')));
      return proc;
    },
  });
  vm.runInContext(source.slice(start, end) + '\nmcpCallHost = __rpc; globalThis.api = { presenter, syncPresenter, stopPresenter };', context);
  return { ...context.api, processes, dispose() { for (const t of timers) clearTimeout(t); } };
}
const bounds = (workspaceId) => ({ x: 0, y: 0, w: 320, h: 180, dpr: 1.25, streamW: 320, streamH: 180, visible: true, workspaceId });
const share = { handleKind: 'buffer', texHandle: '11', fenceHandle: '12', width: 320, height: 180, rowPitch: 1280 };
const tick = () => new Promise((resolve) => setImmediate(resolve));

test('presenter crash cleanup finishes before opening another share in the same workspace', async (t) => {
  const calls = [];
  let finishClose;
  const h = harness(async (tool, args, workspace) => {
    calls.push([tool, workspace]);
    if (tool.endsWith('share_close')) await new Promise((resolve) => { finishClose = resolve; });
    return share;
  });
  t.after(h.dispose);
  h.syncPresenter(bounds('A'));
  await h.presenter.chain;
  h.processes[0].crash();
  h.syncPresenter(bounds('A'));
  await tick();
  assert.deepEqual(calls.map(([tool]) => tool), ['mcp__engine-scene__viewport_share_open', 'mcp__engine-scene__viewport_share_close']);
  assert.equal(h.processes.length, 1, 'replacement must wait for the old close');
  finishClose();
  await h.presenter.chain;
  assert.equal(h.processes.length, 2);
  assert.equal(calls[2][0], 'mcp__engine-scene__viewport_share_open');
  assert.equal(h.presenter.shareOpen, true);
});

test('workspace switch closes the original workspace before opening the new one', async (t) => {
  const calls = [];
  const h = harness(async (tool, args, workspace) => { calls.push([tool, workspace]); return share; });
  t.after(h.dispose);
  h.syncPresenter(bounds('A'));
  await h.presenter.chain;
  h.syncPresenter(bounds('B'));
  await h.presenter.chain;
  assert.deepEqual(calls, [
    ['mcp__engine-scene__viewport_share_open', 'A'],
    ['mcp__engine-scene__viewport_share_close', 'A'],
    ['mcp__engine-scene__viewport_share_open', 'B'],
  ]);
  assert.equal(h.presenter.workspaceId, 'B');
  assert.match(h.processes[1].commands[0], /^bind buf 11 12 320 180 1280/);
});

test('share-open failure releases the presenter so web rendering can continue', async (t) => {
  const h = harness(async () => { throw new Error('sharing unavailable'); });
  t.after(h.dispose);
  h.syncPresenter(bounds('A'));
  await h.presenter.chain;
  assert.equal(h.presenter.proc, null);
  assert.equal(h.presenter.shareOpen, false);
  assert.ok(h.processes[0].commands.includes('close\n'));
});
