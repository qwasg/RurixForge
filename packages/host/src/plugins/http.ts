import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { ApiError, CreateSessionRequest, HealthStatus } from '@forge/protocol';
import type { PluginFn } from '../ctx.js';
import type { HostConfig } from './config.js';
import type { Logger } from './logger.js';
import type { Sessions } from './sessions.js';
import type { Eventlog } from './eventlog.js';
import type { ForgeProxy } from './forgeProxy.js';

export interface HttpService {
  server: http.Server;
  start(port?: number): Promise<number>;
  close(): Promise<void>;
}

const CORS_ORIGIN = 'http://localhost:5173';

const MIME: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  // F8 wave.3 浏览器直开兼容:字体/图片/媒体/wasm/sourcemap 补齐
  // (vite 构建产物含 woff2 字体等,缺省 application/octet-stream 会被浏览器拒用)
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.otf': 'font/otf',
  '.webp': 'image/webp',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.mp4': 'video/mp4',
  '.webm': 'video/webm',
  '.wasm': 'application/wasm',
  '.map': 'application/json; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
};

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

/** HTTP 插件:REST 路由 + 静态托管 + CORS */
export const httpPlugin: PluginFn = (ctx) => {
  const config = ctx.get<HostConfig>('config');
  const logger = ctx.get<Logger>('logger');
  const sessions = ctx.get<Sessions>('sessions');
  const eventlog = ctx.get<Eventlog>('eventlog');
  // forgeProxy 为可选插件(web profile 默认装载,base profile 无)
  let proxy: ForgeProxy | null = null;
  try {
    proxy = ctx.get<ForgeProxy>('forgeProxy');
  } catch {
    proxy = null;
  }

  const startedAt = Date.now();
  let currentPort = config.port;

  // 静态目录:config.staticDir 注入优先(F8 wave.3 测试 seam);
  // 缺省候选——规格路径 <repo>/client/dist,回退 monorepo 内 packages/client/dist
  const hostRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
  const staticDir =
    config.staticDir ??
    [
      path.resolve(hostRoot, '..', '..', 'client', 'dist'),
      path.resolve(hostRoot, '..', 'client', 'dist'),
    ].find((p) => fs.existsSync(p));

  function setCors(res: http.ServerResponse): void {
    res.setHeader('Access-Control-Allow-Origin', CORS_ORIGIN);
    res.setHeader('Access-Control-Allow-Methods', 'GET,POST,OPTIONS');
    res.setHeader('Access-Control-Allow-Headers', 'Content-Type');
  }

  function sendJson(res: http.ServerResponse, status: number, body: unknown): void {
    const data = JSON.stringify(body);
    res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8' });
    res.end(data);
  }

  function sendError(res: http.ServerResponse, status: number, code: string, message: string): void {
    sendJson(res, status, { error: { code, message } } satisfies ApiError);
  }

  function serveStatic(req: http.IncomingMessage, res: http.ServerResponse, pathname: string): void {
    if (!staticDir || req.method !== 'GET') {
      sendError(res, 404, 'NOT_FOUND', `route not found: ${pathname}`);
      return;
    }
    const rel = pathname === '/' ? 'index.html' : pathname.slice(1);
    let file = path.join(staticDir, rel);
    // 防目录穿越 + SPA 回退 index.html
    if (!file.startsWith(staticDir) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
      file = path.join(staticDir, 'index.html');
    }
    if (!fs.existsSync(file)) {
      sendError(res, 404, 'NOT_FOUND', `route not found: ${pathname}`);
      return;
    }
    const headers: Record<string, string> = {
      'Content-Type': MIME[path.extname(file)] ?? 'application/octet-stream',
    };
    // F8 wave.3 浏览器直开缓存默认:index.html(含 SPA 回退)no-cache 防陈旧壳;
    // vite 指纹资产(assets/ 目录,文件名含内容 hash)长缓存 immutable。
    if (path.basename(file) === 'index.html') {
      headers['Cache-Control'] = 'no-cache';
    } else if (path.relative(staticDir, file).split(path.sep)[0] === 'assets') {
      headers['Cache-Control'] = 'public, max-age=31536000, immutable';
    }
    res.writeHead(200, headers);
    fs.createReadStream(file).pipe(res);
  }

  async function handleApi(
    req: http.IncomingMessage,
    res: http.ServerResponse,
    pathname: string,
  ): Promise<void> {
    const segs = pathname.split('/').filter(Boolean); // ['api','forge',...]
    const method = req.method ?? 'GET';

    // 代理优先判定:F7 wave.1 起 sessions/chat-folders/design-snapshot 由 agentd 承接
    // (代理遮蔽下方 host 自有 F0 sessions stub 路由,F0 已 closed,契约留痕);health 仍 host 自有。
    if (proxy && (await proxy.handle(req, res, pathname))) return;

    if (method === 'GET' && pathname === '/api/forge/health') {
      sendJson(res, 200, {
        status: 'ok',
        service: 'forge-host',
        version: config.version,
        port: currentPort,
        uptimeSec: (Date.now() - startedAt) / 1000,
        time: new Date().toISOString(),
      } satisfies HealthStatus);
      return;
    }

    // /api/forge/sessions 集合
    if (segs.length === 3 && segs[2] === 'sessions') {
      if (method === 'GET') {
        sendJson(res, 200, sessions.list());
        return;
      }
      if (method === 'POST') {
        let body: CreateSessionRequest = {};
        const raw = await readBody(req);
        if (raw.trim() !== '') {
          try {
            body = JSON.parse(raw) as CreateSessionRequest;
          } catch {
            sendError(res, 400, 'BAD_JSON', 'request body is not valid JSON');
            return;
          }
        }
        sendJson(res, 201, sessions.create(body.title));
        return;
      }
    }

    // /api/forge/sessions/:id[/events|/messages]
    if (segs.length >= 4 && segs[2] === 'sessions') {
      const id = segs[3];
      const sub = segs[4];

      if (segs.length === 4 && method === 'GET') {
        const s = sessions.get(id);
        if (!s) {
          sendError(res, 404, 'SESSION_NOT_FOUND', `session not found: ${id}`);
          return;
        }
        sendJson(res, 200, s);
        return;
      }

      if (segs.length === 5 && sub === 'events' && method === 'GET') {
        if (!sessions.get(id)) {
          sendError(res, 404, 'SESSION_NOT_FOUND', `session not found: ${id}`);
          return;
        }
        sendJson(res, 200, eventlog.read(id));
        return;
      }

      if (segs.length === 5 && sub === 'messages' && method === 'POST') {
        if (!sessions.get(id)) {
          sendError(res, 404, 'SESSION_NOT_FOUND', `session not found: ${id}`);
          return;
        }
        let body: { text?: unknown };
        try {
          body = JSON.parse(await readBody(req)) as { text?: unknown };
        } catch {
          sendError(res, 400, 'BAD_JSON', 'request body is not valid JSON');
          return;
        }
        if (typeof body.text !== 'string') {
          sendError(res, 400, 'BAD_REQUEST', 'field "text" must be a string');
          return;
        }
        sendJson(res, 201, sessions.postMessage(id, body.text));
        return;
      }
    }

    sendError(res, 404, 'NOT_FOUND', `route not found: ${pathname}`);
  }

  const server = http.createServer((req, res) => {
    setCors(res);
    if (req.method === 'OPTIONS') {
      res.writeHead(204);
      res.end();
      return;
    }
    const pathname = new URL(req.url ?? '/', 'http://localhost').pathname;
    if (pathname.startsWith('/api/')) {
      handleApi(req, res, pathname).catch((err) => {
        logger.error('api handler error', err);
        if (!res.headersSent) sendError(res, 500, 'INTERNAL', 'internal error');
      });
    } else {
      serveStatic(req, res, pathname);
    }
  });

  ctx.provide('http', {
    server,
    start(port) {
      return new Promise((resolve, reject) => {
        server.once('error', reject);
        server.listen(port ?? config.port, config.host, () => {
          const addr = server.address();
          currentPort = typeof addr === 'object' && addr ? addr.port : (port ?? config.port);
          resolve(currentPort);
        });
      });
    },
    close() {
      return new Promise((resolve) => {
        if (!server.listening) return resolve();
        server.close(() => resolve());
      });
    },
  } satisfies HttpService);

  // 插件卸载时确保服务关闭(可逆 effect)
  return () => {
    if (server.listening) server.close();
  };
};
