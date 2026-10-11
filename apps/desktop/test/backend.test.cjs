// 本地服务编排(backend.cjs)的纯逻辑与端口/健康检查,不启动 Electron。
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const net = require('node:net');
const os = require('node:os');
const path = require('node:path');
const backend = require('../src/backend.cjs');

function tempDir(tag) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `forge-backend-${tag}-`));
}

function listen(server, port = 0) {
  return new Promise((resolve) => server.listen(port, '127.0.0.1', () => resolve(server.address().port)));
}

test('source checkout resolves the repo root and target/{debug,release} binaries', () => {
  const sourceDir = path.join('R:', 'repo', 'apps', 'desktop', 'src');
  const layout = backend.resolveLayout({ packaged: false, resourcesPath: 'ignored', sourceDir });
  const root = path.join('R:', 'repo');
  assert.equal(layout.root, root);
  assert.equal(layout.hostEntry, path.join(root, 'packages', 'host', 'dist', 'index.js'));
  assert.equal(path.dirname(layout.agentdCandidates[0]), path.join(root, 'target', 'debug'));
  assert.equal(path.dirname(layout.agentdCandidates[1]), path.join(root, 'target', 'release'));
  assert.match(layout.icon, /packages[\\/]client[\\/]public[\\/]/);
});

test('installed copy resolves resources/forge with every binary in bin/', () => {
  const resourcesPath = path.join('C:', 'Apps', 'RurixForge', 'resources');
  const layout = backend.resolveLayout({ packaged: true, resourcesPath, sourceDir: 'ignored' });
  const root = path.join(resourcesPath, 'forge');
  assert.equal(layout.root, root);
  assert.equal(layout.hostEntry, path.join(root, 'packages', 'host', 'dist', 'index.js'));
  assert.deepEqual(layout.agentdCandidates.map(path.dirname), [path.join(root, 'bin')]);
  assert.deepEqual(layout.presenterCandidates.map(path.dirname), [path.join(root, 'bin')]);
  assert.match(layout.icon, /packages[\\/]client[\\/]dist[\\/]/);
});

test('installed paths keep user data under userData and pin install-describing variables', () => {
  const layout = backend.resolveLayout({ packaged: true, resourcesPath: path.join('C:', 'App', 'resources'), sourceDir: 'x' });
  const userData = path.join('C:', 'Users', 'u', 'AppData', 'Roaming', 'RurixForge');
  const stale = {
    FORGE_ROOT: 'D:\\RurixForge',
    FORGE_GODOT_RUNTIME_DIR: 'D:\\RurixForge\\target\\godot-runtime',
    FORGE_AGENTD_DATA_DIR: 'D:\\RurixForge\\data',
  };
  const paths = backend.packagedPaths({ layout, userData, env: stale });
  const dataDir = path.join(userData, 'data');
  assert.equal(paths.dataDir, dataDir);
  assert.equal(paths.workspace, path.join(dataDir, 'workspaces', 'default'));
  assert.equal(paths.logsDir, path.join(userData, 'logs'));
  assert.equal(paths.env.FORGE_LOG_DIR, paths.logsDir);
  assert.equal(paths.env.FORGE_ROOT, layout.root);
  assert.equal(paths.env.FORGE_GODOT_RUNTIME_DIR, path.join(layout.root, 'godot-runtime'));
  assert.equal(paths.env.FORGE_ULTRAPLAN_RUNTIME_DIR, path.join(layout.root, 'ultraplan-runtime'));
  for (const key of ['FORGE_DATA_DIR', 'FORGE_AGENTD_DATA_DIR', 'FORGE_GEN_DATA_DIR']) {
    assert.equal(paths.env[key], dataDir, key);
  }
});

test('FORGE_DATA_DIR and FORGE_AGENTD_WORKSPACE_ROOT relocate user data on request', () => {
  const layout = backend.resolveLayout({ packaged: true, resourcesPath: 'res', sourceDir: 'x' });
  const paths = backend.packagedPaths({
    layout,
    userData: 'ud',
    env: { FORGE_DATA_DIR: path.join('E:', 'forge-data'), FORGE_AGENTD_WORKSPACE_ROOT: path.join('E:', 'games', 'main') },
  });
  assert.equal(paths.dataDir, path.join('E:', 'forge-data'));
  assert.equal(paths.env.FORGE_AGENTD_DATA_DIR, path.join('E:', 'forge-data'));
  assert.equal(paths.workspace, path.join('E:', 'games', 'main'));
});

test('the default workspace is seeded once and never overwritten', () => {
  const dir = tempDir('seed');
  try {
    const template = path.join(dir, 'template');
    fs.mkdirSync(path.join(template, 'Content', 'Scenes'), { recursive: true });
    fs.writeFileSync(path.join(template, 'forge.toml'), '[project]\nname = "t"\n');
    fs.writeFileSync(path.join(template, 'Content', 'Scenes', 'Main.rxscene'), '{}');
    const target = path.join(dir, 'data', 'workspaces', 'default');
    assert.equal(backend.seedWorkspace(template, target), true);
    assert.equal(fs.readFileSync(path.join(target, 'Content', 'Scenes', 'Main.rxscene'), 'utf8'), '{}');
    fs.writeFileSync(path.join(target, 'forge.toml'), 'edited');
    assert.equal(backend.seedWorkspace(template, target), false);
    assert.equal(fs.readFileSync(path.join(target, 'forge.toml'), 'utf8'), 'edited');
    // 模板缺失(残缺安装)时仍给出一个空目录,而不是让 agentd 落回安装目录。
    const bare = path.join(dir, 'bare');
    assert.equal(backend.seedWorkspace(path.join(dir, 'missing'), bare), true);
    assert.equal(fs.statSync(bare).isDirectory(), true);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('a busy preferred port falls through to the next candidate or a free port', async () => {
  const busy = net.createServer();
  const busyPort = await listen(busy);
  try {
    const fallback = await backend.choosePort([busyPort]);
    assert.notEqual(fallback, busyPort);
    assert.ok(fallback > 0);
    const avoided = await backend.choosePort([], [busyPort]);
    assert.notEqual(avoided, busyPort);
  } finally {
    busy.close();
  }
});

test('installed ports are remembered so the page origin (and its local storage) stays stable', async () => {
  const dir = tempDir('ports');
  const busy = net.createServer();
  const busyPort = await listen(busy);
  try {
    const stateFile = path.join(dir, 'launcher.json');
    fs.writeFileSync(stateFile, JSON.stringify({ hostPort: busyPort, agentdPort: busyPort }));
    const first = await backend.choosePackagedPorts({ stateFile, env: {} });
    assert.notEqual(first.hostPort, busyPort);
    assert.notEqual(first.agentdPort, busyPort);
    assert.notEqual(first.hostPort, first.agentdPort);
    assert.deepEqual(JSON.parse(fs.readFileSync(stateFile, 'utf8')), first);
    const second = await backend.choosePackagedPorts({ stateFile, env: {} });
    assert.deepEqual(second, first);
    const explicit = await backend.choosePackagedPorts({
      stateFile,
      env: { FORGE_HOST_PORT: '4567', FORGE_AGENTD_ADDR: '127.0.0.1:4568' },
    });
    assert.deepEqual(explicit, { hostPort: 4567, agentdPort: 4568 });
    assert.deepEqual(JSON.parse(fs.readFileSync(stateFile, 'utf8')), first, 'explicit ports are not persisted');
  } finally {
    busy.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('source checkout keeps the fixed dev ports unless overridden', () => {
  assert.deepEqual(backend.devPorts({}), { hostPort: 3080, agentdPort: 8103 });
  assert.deepEqual(backend.devPorts({ FORGE_HOST_PORT: '3999', FORGE_AGENTD_ADDR: '127.0.0.1:8999' }), {
    hostPort: 3999,
    agentdPort: 8999,
  });
  assert.deepEqual(backend.devPorts({ FORGE_HOST_PORT: 'abc' }), { hostPort: 3080, agentdPort: 8103 });
});

test('a running dev agentd is reused; a missing build is reported instead of thrown', async () => {
  const server = http.createServer((req, res) => {
    res.writeHead(req.url === '/health' ? 200 : 404);
    res.end('{}');
  });
  const port = await listen(server);
  try {
    const dir = tempDir('agentd');
    const layout = backend.resolveLayout({ packaged: false, resourcesPath: 'x', sourceDir: path.join(dir, 'a', 'b', 'c') });
    const reused = await backend.ensureAgentd({ layout, env: {}, port });
    assert.equal(reused.reused, true);
    assert.equal(reused.child, null);
    server.close();
    const missing = await backend.ensureAgentd({ layout, env: {}, port });
    assert.equal(missing.child, null);
    assert.equal(missing.missing, layout.agentdCandidates[0]);
    fs.rmSync(dir, { recursive: true, force: true });
  } finally {
    server.close();
  }
});

test('an agentd that exits before becoming healthy fails fast with its log file', async () => {
  const dir = tempDir('crash');
  try {
    // 一个无参即退出、也不监听端口的程序,模拟启动即崩溃的 agentd。
    const fake = process.platform === 'win32'
      ? path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'hostname.exe')
      : '/bin/true';
    const layout = { packaged: true, root: dir, agentdCandidates: [fake] };
    const port = await backend.choosePort([]);
    const started = Date.now();
    await assert.rejects(
      backend.ensureAgentd({ layout, env: process.env, port, logsDir: path.join(dir, 'logs'), timeoutMs: 20_000 }),
      (error) => {
        assert.match(error.message, /forge-agentd 启动失败:进程已退出/);
        assert.equal(error.logFile, path.join(dir, 'logs', 'agentd.log'));
        return true;
      }
    );
    assert.ok(Date.now() - started < 10_000, 'exit should be noticed long before the timeout');
    // 输出确实写进了日志文件(hostname 打印了机器名)。
    assert.ok(fs.readFileSync(path.join(dir, 'logs', 'agentd.log'), 'utf8').trim().length > 0);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
