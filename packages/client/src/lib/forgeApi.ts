/**
 * forgeApi:打 host /api/forge/mcp/call(经 vite proxy 或同源 3080)的最小封装。
 * agentd 返回 MCP result 信封,content[0].text 内是 JSON 字符串,需二次解析。
 */

const TOOL_PREFIX = 'mcp__engine-scene__';

/** 结构化 API 错误(code 来自 host/agentd,或 TOOL_ERROR / BAD_RESPONSE) */
export class ForgeApiError extends Error {
  code: string;
  status: number;
  constructor(code: string, message: string, status = 0) {
    super(message);
    this.name = 'ForgeApiError';
    this.code = code;
    this.status = status;
  }
}

/** MCP result 信封形态(content 数组 + 可选 isError) */
export interface McpEnvelope {
  content?: Array<{ type?: string; text?: string }>;
  structuredContent?: unknown;
  isError?: boolean;
}

/**
 * 拆 MCP result 信封:优先 content[0].text 二次 JSON 解析;
 * 退化 structuredContent;再退化本体;text 非 JSON 时回传原文。
 */
export function unwrapToolResult(result: unknown): unknown {
  if (result && typeof result === 'object') {
    const o = result as McpEnvelope;
    const text = o.content?.[0]?.text;
    if (typeof text === 'string') {
      try {
        return JSON.parse(text);
      } catch {
        return text;
      }
    }
    if (o.structuredContent !== undefined) return o.structuredContent;
  }
  return result;
}

/**
 * 调用 engine-scene 工具(name 不带前缀,内部补 mcp__engine-scene__)。
 * HTTP 非 2xx → 抛 ForgeApiError(code 取自 {error.code});信封 isError → 抛 TOOL_ERROR。
 */
export async function callTool<T = unknown>(
  name: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  let res: Response;
  try {
    res = await fetch('/api/forge/mcp/call', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ tool: `${TOOL_PREFIX}${name}`, arguments: args }),
    });
  } catch (err) {
    throw new ForgeApiError('NETWORK', `请求失败: ${(err as Error).message}`);
  }

  let body: unknown;
  try {
    body = await res.json();
  } catch {
    throw new ForgeApiError('BAD_RESPONSE', `响应非 JSON(HTTP ${res.status})`, res.status);
  }

  if (!res.ok) {
    const err = (body as { error?: { code?: string; message?: string } })?.error;
    throw new ForgeApiError(
      err?.code ?? `HTTP_${res.status}`,
      err?.message ?? `HTTP ${res.status}`,
      res.status,
    );
  }

  const envelope = body as McpEnvelope;
  const value = unwrapToolResult(envelope);
  if (envelope.isError === true) {
    const msg =
      typeof value === 'object' && value !== null && 'message' in value
        ? String((value as { message: unknown }).message)
        : JSON.stringify(value);
    throw new ForgeApiError('TOOL_ERROR', msg, res.status);
  }
  return value as T;
}
