/**
 * forgeApi:打 host /api/forge/mcp/call(经 vite proxy 或同源 3080)的最小封装。
 * agentd 返回 MCP result 信封,content[0].text 内是 JSON 字符串,需二次解析。
 * 每次调用带当前工作区 id(workspaceId):agentd 据此把 MCP 子进程/engine-host 落到该工作区
 * 的项目根——视口、层级、资产面与会话 turn 面看同一个项目(此前 REST 面恒锚 projects/demo)。
 */

import { readActiveWorkspaceId } from './activeWorkspace';

const TOOL_PREFIX = 'mcp__engine-scene__';
const ASSET_TOOL_PREFIX = 'mcp__asset-pipeline__';
const CODE_TOOL_PREFIX = 'mcp__code-forge__';
const GEN_TOOL_PREFIX = 'mcp__gen-image__';
const GEN_MODEL_TOOL_PREFIX = 'mcp__gen-model__';

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
 * 调用 MCP 工具(name 不带前缀,prefix 决定路由;mcp__engine-scene__ / mcp__asset-pipeline__)。
 * HTTP 非 2xx → 抛 ForgeApiError(code 取自 {error.code});信封 isError → 抛 TOOL_ERROR。
 */
export async function callToolWithPrefix<T = unknown>(
  prefix: string,
  name: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  let res: Response;
  try {
    const workspaceId = readActiveWorkspaceId();
    res = await fetch('/api/forge/mcp/call', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        tool: `${prefix}${name}`,
        arguments: args,
        ...(workspaceId ? { workspaceId } : {}),
      }),
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
    // F5:工具级结构化 {error: <GEN_* code>} 如实提升为 ForgeApiError.code(无 error 字段退 TOOL_ERROR)。
    const code =
      typeof value === 'object' && value !== null && 'error' in value
        ? String((value as { error: unknown }).error)
        : 'TOOL_ERROR';
    throw new ForgeApiError(code, msg, res.status);
  }
  return value as T;
}

/**
 * 调用 engine-scene 工具(name 不带前缀,内部补 mcp__engine-scene__)。
 */
export async function callTool<T = unknown>(
  name: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return callToolWithPrefix(TOOL_PREFIX, name, args);
}

/** 调用 asset-pipeline 工具(name 不带前缀,内部补 mcp__asset-pipeline__)。 */
export async function callAssetTool<T = unknown>(
  name: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return callToolWithPrefix(ASSET_TOOL_PREFIX, name, args);
}

/** 调用 code-forge 工具(name 不带前缀,内部补 mcp__code-forge__;F4 graph/rx 工具链)。 */
export async function callCodeTool<T = unknown>(
  name: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return callToolWithPrefix(CODE_TOOL_PREFIX, name, args);
}

/** 调用 gen-image 工具(name 不带前缀,内部补 mcp__gen-image__;F5 生成链)。 */
export async function callGenTool<T = unknown>(
  name: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return callToolWithPrefix(GEN_TOOL_PREFIX, name, args);
}

/** 调用 gen-model 工具(name 不带前缀,内部补 mcp__gen-model__;素材创作波 3D 网格链)。 */
export async function callModelGenTool<T = unknown>(
  name: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return callToolWithPrefix(GEN_MODEL_TOOL_PREFIX, name, args);
}

// ---------- 素材创作波:媒体生成 REST 面(agentd gen/video、gen/audio;预留 API 端口) ----------

/** 产物附带的供应商渲染预览图(label = 视角名 front/right/back/left)。 */
export interface MediaPreview {
  label: string;
  fileRef: string;
  mime: string;
  dataUrl: string;
}

/**
 * 媒体产物(fileRef = .forge/tmp/gen/ 项目相对路径)。
 * dataUrl 只对浏览器可内联的类型给出(png/mp4/mp3/wav);glb 数 MB,不内联。
 */
export interface MediaArtifact {
  fileRef: string;
  ext: string;
  mime: string;
  dataUrl?: string;
  previews?: MediaPreview[];
  meta?: Record<string, unknown>;
}

export interface MediaGenResponse {
  backendId: string;
  artifacts: MediaArtifact[];
}

/**
 * POST /api/forge/gen/video(未配置后端 → 501 GEN_BACKEND_NOT_CONFIGURED,如实抛出)。
 * imageRef / imageDataUrl 给了即走图生视频,此时 prompt 转作动作引导可空。
 * imageRef 是项目相对路径(.forge/tmp/gen/ 候选或 Content/ 资产),服务端转 data URI
 * ——前端手里只有路径,不必先把几 MB 参考图 base64 上来一趟。
 */
export async function apiGenVideo(payload: {
  prompt: string;
  imageRef?: string;
  imageDataUrl?: string;
  aspect?: string;
  resolution?: string;
  durationSec?: number;
  backend?: string;
}): Promise<MediaGenResponse> {
  return apiPost<MediaGenResponse>('/api/forge/gen/video', payload);
}

/** 截帧图集(fileRef 落 .forge/tmp/gen/,待 gen_accept 入 Content/)。 */
export interface FrameAtlas {
  fileRef: string;
  mime: string;
  dataUrl: string;
  width: number;
  height: number;
}

/**
 * 截帧响应。只有图集本体 + 逐帧 bbox:逐帧再各附一份 dataUrl 是把同一批像素传两遍,
 * 前端拿图集加 boxes 就能在 canvas 上逐帧画。
 */
export interface VideoFramesResponse {
  atlas: FrameAtlas;
  boxes: Array<[number, number, number, number]>;
  fps: number;
  frameCount: number;
}

/**
 * POST /api/forge/gen/video/frames:mp4 → 精灵图集(ffmpeg 截帧 + 抠底 + 拼图)。
 * 本机无 ffmpeg → 501 GEN_TOOL_MISSING(消息带安装指引),如实抛出不伪造帧。
 */
export async function apiGenVideoFrames(payload: {
  videoFileRef: string;
  fps?: number;
  maxFrames?: number;
  chromaKey?: 'auto' | 'magenta' | 'none';
  crop?: 'union' | 'tight' | 'none';
  padding?: number;
  trimStartSec?: number;
  trimEndSec?: number;
}): Promise<VideoFramesResponse> {
  return apiPost<VideoFramesResponse>('/api/forge/gen/video/frames', payload);
}

/** ffmpeg 可用性(截帧的外部依赖;found=false 时前端显示配置指引而非静默失败)。 */
export interface FfmpegStatus {
  found: boolean;
  path?: string;
  version?: string;
}

/** GET /api/forge/tools/ffmpeg。 */
export async function apiFfmpegStatus(): Promise<FfmpegStatus> {
  return apiGet<FfmpegStatus>('/api/forge/tools/ffmpeg');
}

/** POST /api/forge/gen/audio(mode: tts | music;未配置后端 → 501,如实抛出)。 */
export async function apiGenAudio(payload: {
  mode: 'tts' | 'music';
  prompt: string;
  voice?: string;
  format?: string;
  lyrics?: string;
  instrumental?: boolean;
  backend?: string;
}): Promise<MediaGenResponse> {
  return apiPost<MediaGenResponse>('/api/forge/gen/audio', payload);
}

/**
 * POST /api/forge/gen/mesh(文/图生 3D,默认 meshy;未配置后端 → 501,如实抛出)。
 * 走 REST 而非 MCP:3D 供应商是异步任务制,单次生成常达数分钟,MCP 子进程调用 10s 就断。
 * imageDataUrl 给了即走图生 3D,此时 prompt 转作贴图引导。
 */
export async function apiGenMesh(payload: {
  prompt?: string;
  imageDataUrl?: string;
  targetPolycount?: number;
  texture?: boolean;
  pbr?: boolean;
  textureResolution?: string;
  texturePrompt?: string;
  modelType?: string;
  aiModel?: string;
  topology?: string;
  poseMode?: string;
  timeoutSec?: number;
  backend?: string;
}): Promise<MediaGenResponse> {
  return apiPost<MediaGenResponse>('/api/forge/gen/mesh', payload);
}

/** 非 MCP 的 agentd REST GET(/api/forge/*  plain JSON,非信封)。 */
export async function apiGet<T = unknown>(path: string): Promise<T> {
  const res = await fetch(path);
  const body = (await res.json()) as unknown;
  if (!res.ok) {
    const err = (body as { error?: { code?: string; message?: string } })?.error;
    throw new ForgeApiError(err?.code ?? `HTTP_${res.status}`, err?.message ?? `HTTP ${res.status}`, res.status);
  }
  return body as T;
}

/** F8 wave.1:workspace/file 文本响应(agentd 端点同形;F9 起含 modifiedAt 冲突令牌)。 */
export interface WorkspaceFileResp {
  path: string;
  name: string;
  size: number;
  content: string;
  truncated: boolean;
  /** F9:纳秒级 RFC3339 mtime 令牌(PUT 时经 baseModifiedAt 原样回传做乐观并发)。 */
  modifiedAt: string;
}

export interface WsEntry {
  name: string;
  kind: 'dir' | 'file';
  relPath: string;
  size: number;
  modifiedAt: string;
  hidden: boolean;
}

export interface WorkspaceTreeResp {
  path: string;
  entries: WsEntry[];
  total: number;
  truncated: boolean;
}

function workspaceQuery(path: string, workspaceId?: string | null): string {
  const params = new URLSearchParams();
  params.set('path', path);
  if (workspaceId) params.set('workspaceId', workspaceId);
  return params.toString();
}

/** GET /api/forge/workspace/tree?path=&workspaceId= */
export async function apiWorkspaceTree(
  path: string,
  workspaceId?: string | null,
): Promise<WorkspaceTreeResp> {
  return apiGet<WorkspaceTreeResp>(`/api/forge/workspace/tree?${workspaceQuery(path, workspaceId)}`);
}

/** F8 wave.1:GET /api/forge/workspace/file?path=(窄封装;错误码 ForgeApiError.code 如实)。 */
export async function apiWorkspaceFile(
  path: string,
  workspaceId?: string | null,
): Promise<WorkspaceFileResp> {
  return apiGet<WorkspaceFileResp>(`/api/forge/workspace/file?${workspaceQuery(path, workspaceId)}`);
}

/** F9:workspace/file 写回响应(agentd 端点同形;modifiedAt = 下次写的新基线)。 */
export interface WorkspaceFileWriteResp {
  path: string;
  name: string;
  size: number;
  modifiedAt: string;
}

/**
 * F9:PUT /api/forge/workspace/file(文件编辑器落盘;窄封装)。
 * baseModifiedAt = GET/上次 PUT 返回的 modifiedAt 原样回传;
 * 磁盘已被外部改写 → 409 FILE_CONFLICT(错误码 ForgeApiError.code 如实)。
 */
export async function apiWorkspaceFileWrite(
  path: string,
  content: string,
  baseModifiedAt: string,
  workspaceId?: string | null,
): Promise<WorkspaceFileWriteResp> {
  return apiPut<WorkspaceFileWriteResp>('/api/forge/workspace/file', {
    path,
    content,
    baseModifiedAt,
    ...(workspaceId ? { workspaceId } : {}),
  });
}

/** 非 MCP 的 agentd REST POST(plain JSON)。 */
export async function apiPost<T = unknown>(path: string, payload: unknown): Promise<T> {
  const res = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  const body = (await res.json()) as unknown;
  if (!res.ok) {
    const err = (body as { error?: { code?: string; message?: string } })?.error;
    throw new ForgeApiError(err?.code ?? `HTTP_${res.status}`, err?.message ?? `HTTP ${res.status}`, res.status);
  }
  return body as T;
}

/** 非 MCP 的 agentd REST PUT(plain JSON;F9 workspace/file 写回)。 */
export async function apiPut<T = unknown>(path: string, payload: unknown): Promise<T> {
  const res = await fetch(path, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  const body = (await res.json()) as unknown;
  if (!res.ok) {
    const err = (body as { error?: { code?: string; message?: string } })?.error;
    throw new ForgeApiError(err?.code ?? `HTTP_${res.status}`, err?.message ?? `HTTP ${res.status}`, res.status);
  }
  return body as T;
}

/** 非 MCP 的 agentd REST PATCH(plain JSON;F7 会话 title/pinned/folderId)。 */
export async function apiPatch<T = unknown>(path: string, payload: unknown): Promise<T> {
  const res = await fetch(path, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  const body = (await res.json()) as unknown;
  if (!res.ok) {
    const err = (body as { error?: { code?: string; message?: string } })?.error;
    throw new ForgeApiError(err?.code ?? `HTTP_${res.status}`, err?.message ?? `HTTP ${res.status}`, res.status);
  }
  return body as T;
}

/** 非 MCP 的 agentd REST DELETE(F7 会话/文件夹删除;空响应体容忍)。 */
export async function apiDelete<T = unknown>(path: string): Promise<T> {
  const res = await fetch(path, { method: 'DELETE' });
  const text = await res.text();
  let body: unknown = null;
  if (text !== '') {
    try {
      body = JSON.parse(text);
    } catch {
      throw new ForgeApiError('BAD_RESPONSE', `响应非 JSON(HTTP ${res.status})`, res.status);
    }
  }
  if (!res.ok) {
    const err = (body as { error?: { code?: string; message?: string } } | null)?.error;
    throw new ForgeApiError(err?.code ?? `HTTP_${res.status}`, err?.message ?? `HTTP ${res.status}`, res.status);
  }
  return body as T;
}

/** F8 wave.2:openai-compat 渠道状态面(GET status / POST config 响应同源;绝无 key,R-5)。 */
export interface OpenAiCompatStatus {
  configured: boolean;
  baseUrl: string;
  model: string;
  keyConfigured: boolean;
  /** 该端点后接的模型是否收图片(端点探不出来,由用户声明);为真才把工具产出的图回注给模型 */
  vision?: boolean;
}

/** GET /api/forge/llm/openai-compat/status(响应面无 key)。 */
export async function getOpenAiCompatStatus(): Promise<OpenAiCompatStatus> {
  return apiGet<OpenAiCompatStatus>('/api/forge/llm/openai-compat/status');
}

/** POST /api/forge/llm/openai-compat/config {baseUrl, model, key?, vision?}(key/vision 省略 = 保留既有)。 */
export async function postOpenAiCompatConfig(payload: {
  baseUrl: string;
  model: string;
  key?: string;
  vision?: boolean;
}): Promise<OpenAiCompatStatus & { ok: boolean }> {
  return apiPost<OpenAiCompatStatus & { ok: boolean }>('/api/forge/llm/openai-compat/config', payload);
}

/** F10:embedding 渠道状态面(RAG 向量档;响应与 openai-compat 同形态,绝无 key)。 */
export interface EmbeddingStatus {
  configured: boolean;
  baseUrl: string;
  model: string;
  keyConfigured: boolean;
}

/** GET /api/forge/llm/embedding/status(响应面无 key)。 */
export async function getEmbeddingStatus(): Promise<EmbeddingStatus> {
  return apiGet<EmbeddingStatus>('/api/forge/llm/embedding/status');
}

/** POST /api/forge/llm/embedding/config {baseUrl, model, key?}(key 省略 = 只改 baseUrl/model)。 */
export async function postEmbeddingConfig(payload: {
  baseUrl: string;
  model: string;
  key?: string;
}): Promise<EmbeddingStatus & { ok: boolean }> {
  return apiPost<EmbeddingStatus & { ok: boolean }>('/api/forge/llm/embedding/config', payload);
}

/** Codex 引擎状态面(GET status / POST config 响应同源;绝无密钥,R-5)。 */
export interface CodexAccount {
  authMode?: string | null;
  planType?: string | null;
  email?: string | null;
  rateLimits?: unknown;
  lastError?: string | null;
}

export interface CodexConfigFace {
  codexBin: string;
  codexHome: string;
  defaultEngine: string;
  defaultModel: string;
  autoRegisterMcp: boolean;
  computerUse: boolean;
}

export interface CodexMcpServer {
  name: string;
  command?: string;
  present?: boolean;
  enabled?: boolean;
}

export interface CodexStatus {
  ok?: boolean;
  installed: boolean;
  managedInstalled: boolean;
  computerUseInstalled: boolean;
  command?: string | null;
  npmAvailable: boolean;
  running: boolean;
  transport?: string | null;
  config: CodexConfigFace;
  account: CodexAccount;
  models?: unknown[];
  install: {
    running: boolean;
    log: string;
    error?: string | null;
    finishedAt?: string | null;
  };
  mcp?: { autoRegister?: boolean; servers?: CodexMcpServer[] };
}

export async function getCodexStatus(sessionId?: string | null): Promise<CodexStatus> {
  const q = sessionId ? `?sessionId=${encodeURIComponent(sessionId)}` : '';
  return apiGet<CodexStatus>(`/api/forge/codex/status${q}`);
}

export async function postCodexInstall(): Promise<{ ok: boolean; started?: boolean; running?: boolean }> {
  return apiPost('/api/forge/codex/install', {});
}

export async function postCodexConfig(payload: {
  codexBin?: string;
  codexHome?: string;
  defaultEngine?: string;
  defaultModel?: string;
  autoRegisterMcp?: boolean;
  computerUse?: boolean;
}): Promise<CodexStatus> {
  return apiPost<CodexStatus>('/api/forge/codex/config', payload);
}

export async function postCodexLogin(payload: {
  kind?: 'chatgpt' | 'deviceCode' | 'apiKey';
  apiKey?: string;
}): Promise<Record<string, unknown> & { ok?: boolean; authUrl?: string; loginId?: string }> {
  return apiPost('/api/forge/codex/login', payload);
}

export async function postCodexLoginCancel(loginId?: string): Promise<{ ok: boolean }> {
  return apiPost('/api/forge/codex/login/cancel', loginId ? { loginId } : {});
}

export async function postCodexLogout(): Promise<{ ok: boolean }> {
  return apiPost('/api/forge/codex/logout', {});
}

export async function getCodexModels(refresh = false): Promise<{ ok: boolean; models: unknown[] }> {
  const q = refresh ? '?refresh=true' : '';
  return apiGet(`/api/forge/codex/models${q}`);
}

export async function getCodexRateLimits(): Promise<{ ok: boolean; rateLimits: unknown }> {
  return apiGet('/api/forge/codex/rate-limits');
}

export async function getCodexMcpStatus(sessionId?: string | null): Promise<{
  autoRegister?: boolean;
  servers?: CodexMcpServer[];
}> {
  const q = sessionId ? `?sessionId=${encodeURIComponent(sessionId)}` : '';
  return apiGet(`/api/forge/codex/mcp/status${q}`);
}

export interface GoalFace {
  sessionId?: string;
  objective: string;
  status: string;
  tokenBudget?: number;
  tokensUsed?: number;
  timeUsedSeconds?: number;
  turns?: number;
  note?: string | null;
  engine?: string;
  budgetExhausted?: boolean;
  createdAt?: string;
  updatedAt?: string;
}

export async function getGoal(sessionId: string): Promise<{ goal: GoalFace | null; engine?: string }> {
  return apiGet(`/api/forge/sessions/${encodeURIComponent(sessionId)}/goal`);
}

export async function putGoal(
  sessionId: string,
  payload: { objective: string; tokenBudget?: number },
): Promise<{ goal: GoalFace }> {
  return apiPut(`/api/forge/sessions/${encodeURIComponent(sessionId)}/goal`, payload);
}

export async function deleteGoal(sessionId: string): Promise<{ ok: boolean }> {
  return apiDelete(`/api/forge/sessions/${encodeURIComponent(sessionId)}/goal`);
}

export async function postGoalPause(sessionId: string): Promise<{ goal: GoalFace }> {
  return apiPost(`/api/forge/sessions/${encodeURIComponent(sessionId)}/goal/pause`, {});
}

export async function postGoalResume(sessionId: string): Promise<{ goal: GoalFace }> {
  return apiPost(`/api/forge/sessions/${encodeURIComponent(sessionId)}/goal/resume`, {});
}

/** 删除 skill 的两种结局:真删掉,或被治理门拦下并给出待批提案号。 */
export type SkillDeleteOutcome =
  | { deleted: boolean }
  | { proposalRequired: true; proposalId: string; message: string };

/**
 * F11:skill 删除(409 GOV_PROPOSAL_REQUIRED 时把 proposalId 一并带出,供 UI 走批准流)。
 * ForgeApiError 只带 code/message/status,装不下 proposalId,故此处自行解析响应体;
 * 409 但响应体未给 proposalId 时仍按错误抛出(不编造提案号)。
 */
export async function apiDeleteSkill(name: string): Promise<SkillDeleteOutcome> {
  const res = await fetch(`/api/forge/skills/${encodeURIComponent(name)}`, { method: 'DELETE' });
  const text = await res.text();
  let body: unknown = null;
  if (text !== '') {
    try {
      body = JSON.parse(text);
    } catch {
      throw new ForgeApiError('BAD_RESPONSE', `响应非 JSON(HTTP ${res.status})`, res.status);
    }
  }
  const err = (body as { error?: { code?: string; message?: string; proposalId?: string } } | null)
    ?.error;
  if (
    res.status === 409 &&
    err?.code === 'GOV_PROPOSAL_REQUIRED' &&
    typeof err.proposalId === 'string' &&
    err.proposalId !== ''
  ) {
    return {
      proposalRequired: true,
      proposalId: err.proposalId,
      message: err.message ?? '删除技能须先批准提案',
    };
  }
  if (!res.ok) {
    throw new ForgeApiError(
      err?.code ?? `HTTP_${res.status}`,
      err?.message ?? `HTTP ${res.status}`,
      res.status,
    );
  }
  return { deleted: (body as { deleted?: boolean } | null)?.deleted === true };
}
