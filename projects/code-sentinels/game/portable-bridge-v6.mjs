/** V6 portable desktop bridge: local UI/renderer plus a separate narrow LAN game port. */
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { launchNative } from './v6/native-rpc.mjs';
import { createSessionController } from './v6/session-controller.mjs';
import { readJson, jsonReply, fault } from './multiplayer-v6.mjs';

const args = process.argv.slice(2);
const argument = (name, fallback) => { const i = args.indexOf(name); return i < 0 ? fallback : args[i + 1]; };
const root = path.resolve(argument('--root', path.dirname(fileURLToPath(import.meta.url))));
const web = path.resolve(argument('--web', path.join(root, 'Web')));
const logs = path.join(root, 'Logs', 'v6'); fs.mkdirSync(logs, { recursive: true });
const records = fs.createWriteStream(path.join(logs, 'bridge.jsonl'), { flags: 'a' });
const record = (event, fields = {}) => records.write(JSON.stringify({ at: new Date().toISOString(), event, ...fields }) + '\n');
const native = await launchNative({ root, logs, executable: argument('--engine', path.join(root, 'bin', 'engine-host.exe')), gpuParticles: args.includes('--gpu-particles') });
const controller = createSessionController({ native, root, record, allowSuggestions: args.includes('--test-advice') });
let origin = '', stopped = false, mutations = Promise.resolve();
const mime = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json',
  '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.avif': 'image/avif', '.gif': 'image/gif',
  '.woff2': 'font/woff2', '.woff': 'font/woff', '.ttf': 'font/ttf', '.ico': 'image/x-icon', '.mp4': 'video/mp4', '.webm': 'video/webm', '.ogg': 'audio/ogg', '.mp3': 'audio/mpeg' };
async function handle(req, res) {
  try {
    if (req.headers.host !== new URL(origin).host || req.headers.origin && req.headers.origin !== origin) throw fault(403, '请使用本机游戏页面');
    const url = new URL(req.url, origin);
    if (url.pathname.startsWith('/api/v6/')) {
      const route = url.pathname.slice('/api/v6/'.length);
      if (req.method === 'GET') {
        let result;
        if (route === 'status') result = await controller.status();
        else if (route === 'catalog') result = await controller.catalog();
        else if (route === 'snapshot') { const state = await controller.status(); result = { snapshot: state.snapshot, session: state.session, unchanged: state.snapshot?.tick === Number(url.searchParams.get('afterTick')) }; }
        else if (route === 'viewport') result = await controller.viewport();
        else if (route === 'saves') result = await controller.saves();
        else throw fault(404, '游戏接口不存在');
        return jsonReply(res, 200, result);
      }
      if (req.method !== 'POST') throw fault(405, '请求方式无效');
      const input = await readJson(req);
      const dispatch = async () => {
        if (route === 'session') return controller.session(input);
        if (route === 'ready') return controller.ready(input.ready);
        if (route === 'start') return controller.start();
        if (route === 'leave') { await controller.leave(); return controller.status(); }
        if (route === 'order') return controller.order(input);
        if (route === 'preview') return controller.preview(input);
        if (route === 'pause') return controller.pause(input);
        if (route === 'suggest') return controller.suggest(input);
        if (route === 'camera') return controller.camera(input);
        if (route === 'pick') return controller.pick(input);
        if (route === 'save') return controller.save(input);
        if (route === 'load') return controller.load(input);
        if (route === 'replay') return controller.load(input, true);
        if (route === 'replay-control') return controller.replayControl(input);
        throw fault(404, '游戏接口不存在');
      };
      // Changing sessions cannot race loading/replay/leave. Native owns order sequencing.
      if (['order', 'camera', 'pick', 'preview', 'suggest'].includes(route)) return jsonReply(res, 200, await dispatch());
      const work = mutations.then(dispatch); mutations = work.catch(() => {}); return jsonReply(res, 200, await work);
    }
    if (req.method === 'GET' && url.pathname === '/health') return jsonReply(res, 200, { ok: true, version: 6, enginePid: native.pid, bridgePid: process.pid, root, compilerDisabled: true });
    if (!['GET', 'HEAD'].includes(req.method)) throw fault(405, '请求方式无效');
    if (url.pathname === '/' && (url.searchParams.get('play') !== 'code-sentinels' || url.searchParams.get('version') !== '6')) {
      res.writeHead(302, { location: '/?play=code-sentinels&workspace=portable&standalone=1&version=6' }); return res.end();
    }
    const pathname = decodeURIComponent(url.pathname);
    const file = path.resolve(web, '.' + (pathname === '/' ? '/index.html' : pathname));
    if (!file.startsWith(web + path.sep)) throw fault(403, '路径超出游戏资源范围');
    const stat = await fs.promises.stat(file).catch(() => null);
    if (!stat?.isFile()) throw fault(404, '游戏资源不存在');
    res.writeHead(200, { 'content-type': mime[path.extname(file)] || 'application/octet-stream', 'content-length': stat.size, 'cache-control': 'no-cache', 'x-content-type-options': 'nosniff' });
    if (req.method === 'HEAD') res.end(); else fs.createReadStream(file).pipe(res);
  } catch (error) { jsonReply(res, error.status || 500, { error: { message: error.message || '游戏请求失败' } }); }
}
const server = http.createServer(handle);
server.requestTimeout = 30_000; server.headersTimeout = 15_000;
await new Promise((resolve, reject) => { server.once('error', reject); server.listen(Number(argument('--port', 0)), '127.0.0.1', resolve); });
origin = `http://127.0.0.1:${server.address().port}`;
const url = `${origin}/?play=code-sentinels&workspace=portable&standalone=1&version=6`;
fs.writeFileSync(path.join(logs, 'last-launch.json'), JSON.stringify({ version: 6, url, enginePid: native.pid, bridgePid: process.pid, enginePort: native.port }, null, 2));
console.log(`Code Sentinels V6 is ready: ${url}`); record('ready', { url, enginePid: native.pid });
if (!args.includes('--no-open')) {
  const opener = spawn('powershell.exe', ['-NoProfile', '-Command', `Start-Process '${url}' -WindowStyle Hidden`], { windowsHide: true, stdio: 'ignore' });
  opener.on('error', () => console.log(`Open: ${url}`));
}
async function stop() {
  if (stopped) return; stopped = true;
  server.closeAllConnections(); server.close(); await controller.leave().catch(() => {}); await native.close(); records.end();
}
process.on('SIGINT', () => { stop().then(() => process.exit(0)); });
process.on('SIGTERM', () => { stop().then(() => process.exit(0)); });
if (args.includes('--control-stdin')) {
  process.stdin.setEncoding('utf8'); let controls = '';
  process.stdin.on('data', data => { controls += data; if (/shutdown[\r\n]/.test(controls)) stop().then(() => process.exit(0)); });
}
