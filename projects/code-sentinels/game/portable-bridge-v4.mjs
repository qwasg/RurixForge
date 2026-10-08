/** Portable local UI bridge. Combat, animation and rendering stay in engine-host.
 * Uses only Node built-ins; no npm, Python, compiler, credentials or Forge services.
 */
import http from 'node:http';
import { createMultiplayerService } from './multiplayer-v4.mjs';
import net from 'node:net';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

const root = path.dirname(fileURLToPath(import.meta.url));
const web = path.join(root, 'Web');
const lobby = createMultiplayerService({ journalDir: path.join(root, '.forge', 'multiplayer') });
const gpuParticles = process.argv.includes('--gpu-particles') ? 'on' : 'off';
const logs = path.join(root, 'Logs', gpuParticles === 'on' ? 'gpu-particles' : 'normal');
fs.mkdirSync(logs, { recursive: true });
const engineLog = fs.openSync(path.join(logs, 'engine.log'), 'a');
const child = spawn(path.join(root, 'bin', 'engine-host.exe'), ['--port', '0'], {
  cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', engineLog],
  env: { ...process.env, FORGE_PROJECT_ROOT: root, FORGE_GPU_PARTICLES: gpuParticles,
    RURIX_PREFER_DISCRETE_GPU: '1',
    FORGE_GAME_SAVE_DIR: path.join(root, '.forge', 'save'),
    // A portable game proves it can load its verified prebuilt native script.
    FORGE_RUSTC: path.join(root, 'compiler-not-required.exe') },
});
let enginePort, origin = '', server, socket, stopped = false, nextId = 1, buffer = Buffer.alloc(0);
const pending = new Map();
const rpcLog = fs.createWriteStream(path.join(logs, 'bridge.jsonl'), { flags: 'a' });
const record = (event, detail = {}) => rpcLog.write(JSON.stringify({ at: new Date().toISOString(), event, ...detail }) + '\n');
function stop(code = 0) {
  if (stopped) return; stopped = true;
  for (const request of pending.values()) { clearTimeout(request.timer); request.reject(new Error('Game closed')); }
  pending.clear(); lobby.dispose(); server?.close(); socket?.destroy(); child.kill(); rpcLog.end();
  setTimeout(() => process.exit(code), 100).unref();
}
process.on('SIGINT', () => stop()); process.on('SIGTERM', () => stop());
child.on('error', (error) => { console.error(error.message); stop(1); });
child.on('exit', (code) => { if (!stopped) { console.error(`Native engine exited (${code}). See Logs/engine.log.`); stop(code || 1); } });
function rpc(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    const budget = ['viewport.frame','asset.reload','play.enter'].includes(method) ? 45000 : 20000;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Native engine timed out: ${method}`)); }, budget);
    pending.set(id, { resolve, reject, timer });
    const data = Buffer.from(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    const head = Buffer.alloc(4); head.writeUInt32LE(data.length);
    socket.write(Buffer.concat([head, data]));
  });
}
const METHODS = {
  host_ping: 'host.ping', scene_summary: 'scene.summary', asset_reload: 'asset.reload',
  scene_load: 'scene.load', viewport_set_camera: 'viewport.setCamera', viewport_stream_info: 'viewport.streamInfo',
  play_enter: 'play.enter', play_exit: 'play.exit', play_pause: 'play.pause', play_resume: 'play.resume',
  play_step: 'play.step', play_state: 'play.state', entity_list: 'entity.list', component_get: 'component.get',
  logic_inject_input: 'logic.inject_input', host_events_drain: 'events.drain', viewport_frame: 'viewport.frame',
};
const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.avif': 'image/avif', '.gif': 'image/gif', '.woff': 'font/woff', '.ttf': 'font/ttf', '.svg': 'image/svg+xml', '.png': 'image/png', '.webp': 'image/webp', '.ico': 'image/x-icon', '.woff2': 'font/woff2', '.json': 'application/json' };
function reply(res, status, value) { res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' }); res.end(JSON.stringify(value)); }
async function body(req) {
  let data = ''; for await (const chunk of req) { data += chunk; if (data.length > 65536) throw new Error('Request too large'); }
  return JSON.parse(data || '{}');
}
async function handle(req, res) {
  try {
    if (req.headers.host !== new URL(origin).host) return reply(res, 403, { error: { message: 'Local game host required' } });
    if (req.headers.origin && req.headers.origin !== origin) return reply(res, 403, { error: { message: 'Same-origin game requests required' } });
    const url = new URL(req.url, origin);
    if (await lobby.handle(req, res, url)) return;
    if (req.method === 'GET' && url.pathname === '/api/sentinels/campaign-progress') {
      // Read the same bounded unlock value as the native core. This is not a
      // browser-created save and contains no current battle state.
      const saved = await fs.promises.readFile(path.join(root, '.forge', 'save', 'code-sentinels-v4.txt'), 'utf8').catch(() => '1');
      const parsed = /^\d+$/.test(saved.trim()) ? Number(saved.trim()) : 1;
      const unlocked = Number.isInteger(parsed) && parsed <= 4294967295 ? Math.max(1, Math.min(3, parsed)) : 1;
      return reply(res, 200, { unlocked });
    }
    if (req.method === 'GET' && url.pathname === '/api/forge/workspaces') return reply(res, 200, {
      workspaces: [{ id: 'portable', name: 'Code Sentinels', root, createdAt: '2026-09-06', updatedAt: '2026-09-06' }],
    });
    if (req.method === 'POST' && url.pathname === '/api/forge/mcp/call') {
      const input = await body(req); const tool = String(input.tool || '').replace(/^mcp__engine-scene__/, '');
      if (!METHODS[tool]) return reply(res, 400, { error: { message: 'This portable game exposes only its native play controls' } });
      const args = input.arguments || {};
      if (tool === 'scene_load' && args.path !== 'Content/Scenes/Command.rxscene') return reply(res, 400, { error: { message: 'Only the packaged game scene can be loaded' } });
      if (tool === 'logic_inject_input' && args.action !== 'cs4') return reply(res, 400, { error: { message: 'Unknown game input' } });
      const result = await rpc(METHODS[tool], args);
      if (!['entity_list','viewport_frame'].includes(tool)) record('rpc', { tool });
      return reply(res, 200, { content: [{ type: 'text', text: JSON.stringify(result) }] });
    }
    if (req.method === 'GET' && url.pathname === '/health') return reply(res, 200, { ok: true, enginePid: child.pid, enginePort, root, gpuParticles, compilerDisabled: true });
    if (req.method !== 'GET' && req.method !== 'HEAD') return reply(res, 405, { error: { message: 'Method not allowed' } });
    if (url.pathname === '/' && url.searchParams.get('play') !== 'code-sentinels') {
      res.writeHead(302, { location: '/?play=code-sentinels&workspace=portable&standalone=1' }); return res.end();
    }
    const pathname = decodeURIComponent(url.pathname);
    const file = path.resolve(web, '.' + (pathname === '/' ? '/index.html' : pathname));
    if (!file.startsWith(web + path.sep)) return reply(res, 403, { error: { message: 'Path outside game UI' } });
    const stat = await fs.promises.stat(file).catch(() => null);
    if (!stat?.isFile()) return reply(res, 404, { error: { message: 'Game file not found' } });
    res.writeHead(200, { 'content-type': MIME[path.extname(file)] || 'application/octet-stream', 'content-length': stat.size, 'cache-control': 'no-cache' });
    if (req.method === 'HEAD') res.end(); else fs.createReadStream(file).pipe(res);
  } catch (error) { reply(res, 500, { error: { message: error.message } }); }
}
let lines = '';
child.stdout.on('data', (data) => {
  lines += data.toString(); let n;
  while ((n = lines.indexOf('\n')) !== -1) {
    const line = lines.slice(0, n).trim(); lines = lines.slice(n + 1); record('native_stdout', { line });
    const match = /FORGE_HOST_LISTENING port=(\d+)/.exec(line);
    if (!match || socket) continue;
    enginePort = Number(match[1]); socket = net.createConnection({ host: '127.0.0.1', port: enginePort });
    socket.on('data', (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      while (buffer.length >= 4) {
        const length = buffer.readUInt32LE(0); if (length > 8 * 1024 * 1024) return stop(1);
        if (buffer.length < length + 4) break;
        const response = JSON.parse(buffer.subarray(4, length + 4).toString()); buffer = buffer.subarray(length + 4);
        const request = pending.get(response.id); if (!request) continue;
        pending.delete(response.id); clearTimeout(request.timer);
        if (response.error) request.reject(new Error(response.error.message)); else request.resolve(response.result);
      }
    });
    socket.on('error', (error) => { console.error(error.message); stop(1); });
    socket.once('connect', () => {
      server = http.createServer(handle); server.listen(0, '127.0.0.1', () => {
        origin = `http://127.0.0.1:${server.address().port}`;
        const url = `${origin}/?play=code-sentinels&workspace=portable&standalone=1`;
        fs.writeFileSync(path.join(logs, 'last-launch.json'), JSON.stringify({ url, enginePid: child.pid, bridgePid: process.pid, enginePort, gpuParticles }, null, 2));
        console.log(`Code Sentinels V4 is ready: ${url}\nClose this window to stop the native game.`); record('ready', { url, enginePid: child.pid, gpuParticles });
        if (!process.argv.includes('--no-open')) {
          const opener = spawn('powershell.exe', ['-NoProfile','-Command',`Start-Process '${url}' -WindowStyle Hidden`], { windowsHide: true, stdio: 'ignore' });
          opener.on('error', () => console.log(`Open this address in your browser: ${url}`));
        }
      });
    });
  }
});
