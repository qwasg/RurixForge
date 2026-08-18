import http from 'node:http';
import type { PluginFn } from '../ctx.js';
import type { Logger } from './logger.js';

/**
 * forgeProxy 插件:/api/forge/mcp/* 与 /api/forge/llm/* 反向代理到 forge-agentd;
 * F3 扩:agentd REST 面(skills/subagents/swarm/proposals)同代理——desktop/web 场景
 * client 只认 host 单源(3080),agentd REST 必须经 host 透传。
 * 方法与 body 透传;上游不可达 → 502 {error:{code:"UPSTREAM_UNREACHABLE"}}。
 * host 自有 /api/forge/health、/api/forge/sessions 不走代理(前缀不重叠)。
 */
export interface ForgeProxy {
  /** 命中代理前缀时转发并返回 true;否则返回 false 交给 host 自有路由 */
  handle(req: http.IncomingMessage, res: http.ServerResponse, pathname: string): Promise<boolean>;
}

const DEFAULT_UPSTREAM = 'http://127.0.0.1:8103';
const PROXY_PREFIXES = [
  '/api/forge/mcp',
  '/api/forge/llm',
  // F3:agentd REST 面(skills 管理 / subagent 清单 / swarm 分片 / F2 proposals)
  '/api/forge/skills',
  '/api/forge/subagents',
  '/api/forge/swarm',
  '/api/forge/proposals',
  // F5 wave.3:gen 配置 REST 面(backends 清单 / configure;密钥经此面写 keystore,不出)
  '/api/forge/gen',
];
const UPSTREAM_TIMEOUT_MS = 15_000;

function matches(pathname: string): boolean {
  return PROXY_PREFIXES.some((p) => pathname === p || pathname.startsWith(`${p}/`));
}

function readBody(req: http.IncomingMessage): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => resolve(Buffer.concat(chunks)));
    req.on('error', reject);
  });
}

/** upstream 传 undefined 时读 env FORGE_AGENTD_ORIGIN,缺省 127.0.0.1:8103 */
export function forgeProxyPlugin(upstream?: string): PluginFn {
  return (ctx) => {
    const logger = ctx.get<Logger>('logger');
    const origin = upstream ?? process.env.FORGE_AGENTD_ORIGIN ?? DEFAULT_UPSTREAM;
    const target = new URL(origin);

    async function handle(
      req: http.IncomingMessage,
      res: http.ServerResponse,
      pathname: string,
    ): Promise<boolean> {
      if (!matches(pathname)) return false;

      const body = await readBody(req);
      const headers: Record<string, string> = {};
      // 只透传内容相关头,host/connection/content-length 由本层重建
      for (const name of ['content-type', 'accept']) {
        const v = req.headers[name];
        if (typeof v === 'string') headers[name] = v;
      }
      if (body.length > 0) headers['content-length'] = String(body.length);

      await new Promise<void>((resolve) => {
        const out = http.request(
          {
            hostname: target.hostname,
            port: target.port,
            path: req.url ?? pathname, // 保留 query
            method: req.method ?? 'GET',
            headers,
          },
          (up) => {
            res.writeHead(up.statusCode ?? 502, {
              'Content-Type': up.headers['content-type'] ?? 'application/json; charset=utf-8',
            });
            up.pipe(res);
            res.on('finish', () => resolve());
          },
        );
        out.on('error', (err) => {
          logger.warn(`forgeProxy upstream unreachable: ${err.message}`);
          if (!res.headersSent) {
            const data = JSON.stringify({
              error: { code: 'UPSTREAM_UNREACHABLE', message: `agentd 不可达: ${origin}` },
            });
            res.writeHead(502, { 'Content-Type': 'application/json; charset=utf-8' });
            res.end(data);
          } else if (!res.writableEnded) {
            res.end();
          }
          resolve();
        });
        out.setTimeout(UPSTREAM_TIMEOUT_MS, () => {
          out.destroy(new Error(`upstream timeout ${UPSTREAM_TIMEOUT_MS}ms`));
        });
        out.end(body);
      });
      return true;
    }

    ctx.provide('forgeProxy', { handle } satisfies ForgeProxy);
  };
}
