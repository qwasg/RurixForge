/** Standalone LAN preparation room. This process never launches the native
 * engine and exposes no Forge RPC, filesystem browsing or viewport controls. */
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { createMultiplayerService } from './multiplayer-v4.mjs';

const root = path.dirname(fileURLToPath(import.meta.url));
const web = fs.existsSync(path.join(root, 'Web')) ? path.join(root, 'Web') : path.resolve(root, '../../../packages/client/dist');
const loopback = process.argv.includes('--local-only');
const requestedPort = Number(process.env.SENTINELS_LOBBY_PORT || 0);
if (!Number.isInteger(requestedPort) || requestedPort < 0 || requestedPort > 65535) throw new Error('SENTINELS_LOBBY_PORT must be 0..65535');
let urls = [], boundPort = 0;
const service = createMultiplayerService({ journalDir: path.join(root, '.forge', 'multiplayer'), address: () => urls[0] ?? null });
const mime = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.woff2': 'font/woff2', '.woff': 'font/woff', '.ico': 'image/x-icon' };
const server = http.createServer(async (req, res) => {
  try {
    const host = req.headers.host;
    if (!host || !allowedHosts.has(host.toLowerCase())) { res.writeHead(403); res.end('Unknown lobby address'); return; }
    const url = new URL(req.url, 'http://' + host);
    if (await service.handle(req, res, url)) return;
    if (url.pathname.startsWith('/api/')) { res.writeHead(404); res.end('This server provides the preparation lobby only.'); return; }
    if (!['GET', 'HEAD'].includes(req.method)) { res.writeHead(405); res.end(); return; }
    if (url.pathname === '/' && url.searchParams.get('lobby') !== '1') {
      res.writeHead(302, { location: '/?play=code-sentinels&lobby=1' }); res.end(); return;
    }
    const filename = path.resolve(web, '.' + decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname));
    if (!filename.startsWith(web + path.sep)) { res.writeHead(403); res.end(); return; }
    const stat = await fs.promises.stat(filename).catch(() => null);
    if (!stat?.isFile()) { res.writeHead(404); res.end('Lobby file not found. Build or use the V4 portable package.'); return; }
    res.writeHead(200, { 'content-type': mime[path.extname(filename)] || 'application/octet-stream', 'content-length': stat.size,
      'cache-control': 'no-cache', 'x-content-type-options': 'nosniff' });
    if (req.method === 'HEAD') res.end(); else fs.createReadStream(filename).pipe(res);
  } catch { if (!res.headersSent) res.writeHead(500); res.end('Lobby request could not be completed.'); }
});
const allowedHosts = new Set();
server.listen(requestedPort, loopback ? '127.0.0.1' : '0.0.0.0', () => {
  boundPort = server.address().port;
  const addresses = [...new Set(Object.values(os.networkInterfaces()).flat().filter(n => n && n.family === 'IPv4' && !n.internal).map(n => n.address))];
  const all = loopback ? ['127.0.0.1'] : [...addresses, '127.0.0.1'];
  for (const ip of all) allowedHosts.add(`${ip}:${boundPort}`);
  allowedHosts.add(`localhost:${boundPort}`);
  urls = all.map(ip => `http://${ip}:${boundPort}/?play=code-sentinels&lobby=1`);
  console.log('CODE SENTINELS / PVP PREPARATION ROOM');
  console.log('Players on the same network open ONE of these server addresses, then use the same room code:');
  for (const url of urls) console.log(url);
  console.log('Rooms and player seats are available. Multiplayer battle synchronization is reserved for a future native adapter.');
  console.log('The game engine is NOT exposed on this server. Close this window to end all rooms.');
});
server.on('error', error => { console.error(error.message); service.dispose(); process.exitCode = 1; });
const stop = () => { service.dispose(); server.close(); setTimeout(() => process.exit(0), 100).unref(); };
process.on('SIGINT', stop); process.on('SIGTERM', stop);
