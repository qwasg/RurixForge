import http from 'node:http';
import type { PluginFn } from '../ctx.js';
import type { Logger } from './logger.js';

/**
 * forgeProxy 插件:/api/forge/* 选定前缀反向代理到 forge-agentd;
 * F3 扩:agentd REST 面(skills/subagents/swarm/proposals)同代理——desktop/web 场景
 * client 只认 host 单源(3080),agentd REST 必须经 host 透传。
 * F7 wave.1:agent 会话事实源移到 agentd(事件基座)——/api/forge/sessions、
 * /api/forge/chat-folders、/api/forge/design-snapshot 加入代理前缀;host 自有 F0 stub
 * (sessions.ts/eventlog.ts 的 /api/forge/sessions 路由)在运行时被遮蔽(F0 已 closed,契约留痕)。
 * SSE 长连接(/events/stream)豁免 15s 上游超时(setTimeout(0)),保持 pipe 流式;普通请求维持 15s。
 * mcp/call 按体内 tool 名豁免长时生成工具(gen_image 等,真实远程分钟级;agentd 360s 兜底)。
 * 方法与 body 透传;上游不可达 → 502 {error:{code:"UPSTREAM_UNREACHABLE"}}。
 * host 自有 /api/forge/health 不走代理(前缀不重叠)。
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
  // F6:playtest 矩阵执行器(Console 报告行注入链)
  '/api/forge/playtest',
  // F7 wave.1:agent 事件基座(会话 CRUD/fork/revert + SSE 流 + chat-folders + design-snapshot)
  '/api/forge/sessions',
  '/api/forge/chat-folders',
  '/api/forge/workspaces',
  '/api/forge/design-snapshot',
  // F7 wave.2:turn 执行事件化(runs 控制 + todos REST;ask:execute 在 sessions 前缀内)
  '/api/forge/runs',
  '/api/forge/todos',
  // F7 wave.5:工作区文件树只读面(Inspector;llm/key 在已有 /api/forge/llm 前缀内,无需新增)
  '/api/forge/workspace',
  // F9(D5):project 面(pack 引用闭包打包;agentd 已注册路由,host 代理补前缀缺口)
  '/api/forge/project',
  // F11(D-025):资产商店面(源 CRUD / 搜索 / 详情 / 安装长任务 / 已装清单 / 个人库 / 发布)
  '/api/forge/store',
  // 素材创作隐藏会话
  '/api/forge/studio',
  // 角色动画:外部可执行依赖可用性探测(当前仅 ffmpeg,视频截帧用)
  '/api/forge/tools',
];
const UPSTREAM_TIMEOUT_MS = 15_000;

/**
 * MCP 长生命周期工具(/api/forge/mcp/call 体内 tool 字段判定)。
 * 素材创作/Assets 生成链的 gen_image 走 mcp/call,不在 isLongLivedPath 的 pathname 面内;
 * 真实远程后端单次分钟级(D-026 适配器 300s 预算),15s 代理超时必在生成途中断连。
 * agentd 侧 GEN_IMAGE_TIMEOUT 360s 兜底(mcp.rs),代理豁免不自增挂死风险。
 */
const LONG_LIVED_MCP_TOOLS = new Set([
  'mcp__gen-image__gen_image',
  'mcp__gen-image__gen_texture_set',
  'mcp__gen-image__gen_variations',
  // 截帧 = ffmpeg 解整段视频 + 逐帧抠底 + 拼图集,同属分钟量级。
  'mcp__gen-image__gen_video_frames',
]);

/** mcp/call 请求体的 tool 名(非 JSON/缺字段 → null,按普通请求 15s;导出供单测)。 */
export function mcpCallTool(body: Buffer): string | null {
  try {
    const v: unknown = JSON.parse(body.toString('utf8'));
    if (v && typeof v === 'object' && typeof (v as { tool?: unknown }).tool === 'string') {
      return (v as { tool: string }).tool;
    }
    return null;
  } catch {
    return null;
  }
}

/** 代理前缀命中判定(导出供单测)。 */
export function proxyMatches(pathname: string): boolean {
  return PROXY_PREFIXES.some((p) => pathname === p || pathname.startsWith(`${p}/`));
}

/**
 * 长生命周期端点判定(导出供单测;pathname 不含 query)。
 * F7 wave.1 原名 isStreamPath(仅 SSE);wave.2 改名 isLongLivedPath 留痕——
 * ask:execute turn 可能远超 15s(16 迭代 × 60s 上限),与 events/stream 并列豁免。
 * 素材创作波:gen/video(适配器超时 300s)与 gen/audio(120s)远超 15s,并列豁免;
 * gen/video/frames 走 ffmpeg 解码整段视频 + 拼图集,同属分钟量级。
 * F11(D-F11-C):store/install 与 uninstall 含逐文件下载 + 校验 + 走 assetd 构建链,
 * 大包远超 15s——这也是安装不走 MCP 的同一理由(mcp.rs CALL_TIMEOUT 10s 接不住)。
 */
export function isLongLivedPath(pathname: string): boolean {
  return (
    pathname.endsWith('/events/stream') ||
    pathname.endsWith('/ask:execute') ||
    pathname === '/api/forge/gen/video' ||
    pathname === '/api/forge/gen/video/frames' ||
    pathname === '/api/forge/gen/audio' ||
    pathname === '/api/forge/store/install' ||
    pathname === '/api/forge/store/uninstall' ||
    pathname === '/api/forge/store/publish'
  );
}

/** 上游超时毫秒:长生命周期端点与 MCP 长时工具 0(不限时),其余 15s(导出供单测)。 */
export function upstreamTimeoutMs(pathname: string, mcpTool?: string | null): number {
  if (isLongLivedPath(pathname)) return 0;
  if (pathname === '/api/forge/mcp/call' && mcpTool != null && LONG_LIVED_MCP_TOOLS.has(mcpTool)) {
    return 0;
  }
  return UPSTREAM_TIMEOUT_MS;
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
      if (!proxyMatches(pathname)) return false;

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
            const contentType = up.headers['content-type'] ?? 'application/json; charset=utf-8';
            const headers: Record<string, string> = { 'Content-Type': contentType };
            // F8 wave.3 浏览器 SSE 兼容:text/event-stream 透传禁缓冲语义——
            // Cache-Control 透传(上游缺省则 no-cache)+ X-Accel-Buffering: no。
            // host 自身为 node:http 裸管,无 gzip/压缩中间件,SSE 流不被压缩破坏。
            if (contentType.includes('text/event-stream')) {
              const cc = up.headers['cache-control'];
              headers['Cache-Control'] = typeof cc === 'string' ? cc : 'no-cache';
              headers['X-Accel-Buffering'] = 'no';
            }
            res.writeHead(up.statusCode ?? 502, headers);
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
        // F7 wave.1/2:长生命周期端点(SSE 流 / ask:execute turn)豁免 15s 不活动超时;
        // 普通请求维持原超时防挂死。F10-RAG 修复:mcp/call 按体内 tool 名豁免
        // 长时生成工具(gen_image 真实远程分钟级,15s 必断;agentd 360s 预算兜底)。
        const mcpTool = pathname === '/api/forge/mcp/call' ? mcpCallTool(body) : null;
        const timeoutMs = upstreamTimeoutMs(pathname, mcpTool);
        if (timeoutMs > 0) {
          out.setTimeout(timeoutMs, () => {
            out.destroy(new Error(`upstream timeout ${timeoutMs}ms`));
          });
        } else {
          out.setTimeout(0);
        }
        // F8 wave.3:浏览器断开(SSE 关闭/页面卸载/网络中断)→ 同步销毁上游请求,
        // 不留 agentd 侧孤儿长连接;正常 finish 后 writableEnded=true 不误伤。
        res.on('close', () => {
          if (!res.writableEnded) out.destroy();
        });
        out.end(body);
      });
      return true;
    }

    ctx.provide('forgeProxy', { handle } satisfies ForgeProxy);
  };
}
