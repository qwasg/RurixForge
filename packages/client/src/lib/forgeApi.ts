/**
 * forgeApi:打 host /api/forge/mcp/call(经 vite proxy 或同源 3080)的最小封装。
 * agentd 返回 MCP result 信封,content[0].text 内是 JSON 字符串,需二次解析。
 * 每次调用带当前工作区 id(workspaceId):agentd 据此把 MCP 子进程/engine-host 落到该工作区
 * 的项目根——视口、层级、资产面与会话 turn 面看同一个项目(此前 REST 面恒锚 projects/demo)。
 */

import { readActiveWorkspaceId } from './activeWorkspace';
import type { AgentMessage, AgentParticipant, SendAgentMessage, TeamAction, TeamState } from '@forge/protocol';

const agentSessionPath = (sessionId: string) => `/api/forge/sessions/${encodeURIComponent(sessionId)}`;

export const getAgentParticipants = (sessionId: string) =>
  apiGet<{ agents: AgentParticipant[] }>(`${agentSessionPath(sessionId)}/agents`);

export const getAgentMessages = (sessionId: string, agentId: string) =>
  apiGet<{ messages: AgentMessage[] }>(`${agentSessionPath(sessionId)}/agents/${encodeURIComponent(agentId)}/messages`);

export const sendAgentMessage = (sessionId: string, agentId: string, message: SendAgentMessage) =>
  apiPost<{ message: AgentMessage }>(`${agentSessionPath(sessionId)}/agents/${encodeURIComponent(agentId)}/messages`, message);

export const getSessionTeam = (sessionId: string) =>
  apiGet<{ team: TeamState | null }>(`${agentSessionPath(sessionId)}/team`);

/** 手动压缩上下文的结果。本地引擎回报估算 token;Codex 引擎由 Codex 自己总结,只回 engine。 */
export interface CompactResult {
  engine: 'local' | 'codex';
  manual: boolean;
  /** 本次新并入摘要的轮数。 */
  turns?: number;
  tokensBefore?: number;
  tokensAfter?: number;
}

/** POST /sessions/{id}/compact:把较早的对话总结成摘要(会话运行中 409 SESSION_BUSY)。 */
export const compactSession = (sessionId: string) =>
  apiPost<CompactResult>(`${agentSessionPath(sessionId)}/compact`, {});

export const changeTeamState = (sessionId: string, teamId: string, action: TeamAction) =>
  apiPatch<{ team: TeamState }>(`${agentSessionPath(sessionId)}/teams/${encodeURIComponent(teamId)}`, { action });

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

export type AgentPermissionMode = 'plan' | 'auto' | 'bypass';

export interface AgentPermissionOption {
  id: AgentPermissionMode;
  label: string;
  description: string;
}

export interface AgentConfigFace {
  config: {
    exploreModel: string;
    defaultPermissionMode: AgentPermissionMode;
  };
  options: {
    exploreModels: Array<{ id: string; label: string; group: string; availability: string }>;
    permissionModes: AgentPermissionOption[];
  };
}

export interface AgentPermissionFace {
  mode: AgentPermissionMode;
  options: AgentPermissionOption[];
}

export const getAgentConfig = () => apiGet<AgentConfigFace>('/api/forge/agent/config');

export interface AgentRecommendation {
  id: string;
  label: string;
  desc: string;
  draft: string;
  mode: string;
  image: 'level' | 'defense' | 'plan' | 'debug';
}

export interface AgentRecommendationsFace {
  source: 'agent-capabilities';
  context: { workspaceId: string | null; projectName: string; gameMode: '2d' | '3d'; agentEngine: 'local' | 'codex'; agentKind: string; permissionMode: AgentPermissionMode };
  recommendations: AgentRecommendation[];
}

export const getAgentRecommendations = (context: { sessionId: string | null; workspaceId: string | null; agentEngine: 'local' | 'codex' }) => {
  const query = new URLSearchParams({ agentEngine: context.agentEngine });
  if (context.sessionId) query.set('sessionId', context.sessionId);
  if (context.workspaceId) query.set('workspaceId', context.workspaceId);
  return apiGet<AgentRecommendationsFace>(`/api/forge/agent/recommendations?${query}`);
};
export const patchAgentConfig = (patch: Partial<AgentConfigFace['config']>) =>
  apiPatch<AgentConfigFace>('/api/forge/agent/config', patch);
export const getAgentPermission = (sessionId: string) =>
  apiGet<AgentPermissionFace>(`${agentSessionPath(sessionId)}/permission`);
export const patchAgentPermission = (sessionId: string, mode: AgentPermissionMode) =>
  apiPatch<AgentPermissionFace>(`${agentSessionPath(sessionId)}/permission`, { mode });

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
  /** 省略时取当前工作区;显式 null 固定为默认项目,不随后续切换变化。 */
  workspaceId?: string | null;
  prompt: string;
  imageRef?: string;
  imageDataUrl?: string;
  aspect?: string;
  resolution?: string;
  durationSec?: number;
  backend?: string;
}): Promise<MediaGenResponse> {
  const workspaceId = payload.workspaceId === undefined ? readActiveWorkspaceId() : payload.workspaceId;
  return apiPost<MediaGenResponse>('/api/forge/gen/video', { ...payload, workspaceId });
}

/** 保存的视频按原项目流式读取;缺失/非法引用不生成可播放 URL。 */
export function apiGenVideoFileUrl(fileRef: string | undefined, workspaceId?: string | null): string | undefined {
  if (typeof fileRef !== 'string' || !fileRef.startsWith('.forge/tmp/gen/')
    || !/\.mp4$/i.test(fileRef) || fileRef.includes('..') || /[:\\\u0000-\u001f]/.test(fileRef)) return undefined;
  const scope = workspaceId === undefined ? readActiveWorkspaceId() : workspaceId;
  const params = new URLSearchParams({ fileRef });
  if (scope) params.set('workspaceId', scope);
  return `/api/forge/gen/video/file?${params.toString()}`;
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
  workspaceId?: string | null;
  videoFileRef: string;
  fps?: number;
  maxFrames?: number;
  chromaKey?: 'auto' | 'magenta' | 'none';
  crop?: 'union' | 'tight' | 'none';
  padding?: number;
  trimStartSec?: number;
  trimEndSec?: number;
}): Promise<VideoFramesResponse> {
  const workspaceId = payload.workspaceId === undefined ? readActiveWorkspaceId() : payload.workspaceId;
  return apiPost<VideoFramesResponse>('/api/forge/gen/video/frames', { ...payload, workspaceId });
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

export interface ForgeRenderConfig {
  backend: string;
  method: string | null;
  driver: string | null;
}

/** RenderBackendInfo 的字段直接对应 engine-host 的 render.backendInfo RPC。 */
export interface RenderBackendInfo {
  renderBackend: string;
  method: string | null;
  driver: string | null;
  source: 'default' | 'forge.toml' | 'env' | 'cli' | string;
  ready: boolean;
  deviceName: string | null;
  versions?: { engineHost?: string; godot?: string | null; gdext?: string | null };
  frameChannels?: {
    l2?: string;
    l1Available?: boolean;
    l1Active?: boolean;
    l1Reason?: string | null;
    adapter?: string | null;
    l1Frames?: number;
    l2Frames?: number;
    l1Lag?: number | null;
    l2Lag?: number | null;
  };
}

export interface RenderCapabilityFeature {
  feature: string;
  reason: string;
}

/** RenderCapabilities 直接对应 render.capabilities；后端可增加 coverage 子字段。 */
export interface RenderCapabilities {
  renderBackend: string;
  pipelined: boolean;
  legs: string[];
  preview: boolean;
  particles: boolean;
  frameExits: { cpuRgba8: boolean; sharedD3d12: boolean; zeroCopy: boolean };
  stats: { nonzero: boolean; triangles: boolean; truncated: boolean; meshFallbacks: boolean; meshClasses: boolean };
  maxDraws: { spriteMesh: number | null; model: number | null; sentinelsV6: number | null };
  maxSize: { rpc: [number, number]; stream: [number, number] };
  coverage?: {
    skipped?: string[];
    unsupported?: RenderCapabilityFeature[];
    limited?: RenderCapabilityFeature[];
  };
}

/** 解析 forge.toml 的维度与 [render]；2D 缺省 Godot，3D 缺省 rurix，与 assetd 一致。 */
export function parseForgeRenderConfig(text: string): ForgeRenderConfig {
  // 某些旧代理/测试桩会把换行转义成两个字符；先还原，不改变正常 TOML 文本。
  const source = text.replace(/\\n/g, '\n');
  let sectionName = '';
  let mode = '3d';
  let backend: string | null = null;
  let method: string | null = null;
  let driver: string | null = null;
  for (const line of source.split(/\r?\n/)) {
    const section = /^\s*\[([^\]]+)\]\s*(?:#.*)?$/.exec(line);
    if (section) {
      sectionName = section[1].trim();
      continue;
    }
    if (sectionName !== 'render' && sectionName !== 'project') continue;
    const entry = /^\s*(mode|backend|method|driver)\s*=\s*("(?:[^"\\]|\\.)*"|'[^']*')\s*(?:#.*)?$/.exec(line);
    if (!entry) continue;
    let value = entry[2];
    if (value.startsWith('"')) {
      try { value = JSON.parse(value) as string; } catch { value = value.slice(1, -1); }
    } else {
      value = value.slice(1, -1);
    }
    if (sectionName === 'project') {
      if (entry[1] === 'mode') mode = value;
      continue;
    }
    if (entry[1] === 'backend') backend = value;
    else if (entry[1] === 'method') method = value;
    else if (entry[1] === 'driver') driver = value;
  }
  backend ??= mode === '2d' ? 'godot' : 'rurix';
  if (backend !== 'godot') return { backend, method: null, driver: null };
  const actualMethod = method ?? 'forward_plus';
  return {
    backend,
    method: actualMethod,
    driver: driver ?? (actualMethod === 'gl_compatibility' ? 'opengl3' : 'd3d12'),
  };
}

/** 只在 forge.toml/default 为生效来源时比较配置；环境变量/CLI 覆盖时避免误报。 */
export function renderConfigMismatch(
  info: RenderBackendInfo,
  config: ForgeRenderConfig,
): string | null {
  if (info.source !== 'forge.toml' && info.source !== 'default') return null;
  if (info.renderBackend === config.backend && info.method === config.method && info.driver === config.driver) return null;
  const actual = [info.renderBackend, info.method, info.driver].filter(Boolean).join(' / ');
  const configured = [config.backend, config.method, config.driver].filter(Boolean).join(' / ');
  return `forge.toml 当前配置为 ${configured}，运行中的渲染后端为 ${actual}。重启宿主后配置才会生效；当前实例保持不变，未自动切换。`;
}

/** 读取活动项目 forge.toml；常规项目根优先，其次仓库根下 projects/demo。 */
export async function getForgeRenderConfig(workspaceId?: string | null): Promise<ForgeRenderConfig> {
  const paths = ['forge.toml', 'projects/demo/forge.toml'];
  for (const path of paths) {
    try {
      const file = await apiWorkspaceFile(path, workspaceId);
      return parseForgeRenderConfig(file.content);
    } catch (err) {
      if (err instanceof ForgeApiError && err.code === 'PATH_NOT_FOUND') continue;
      throw err;
    }
  }
  return { backend: 'rurix', method: null, driver: null };
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

/** D-040:文件搜索命中(工作区相对路径)。 */
export interface WorkspaceSearchHit {
  path: string;
  name: string;
  dir: string;
}

export interface WorkspaceSearchResp {
  query: string;
  results: WorkspaceSearchHit[];
  total: number;
  truncated: boolean;
  /** 候选来源:git = ls-files(遵守 .gitignore);walk = 非仓库有界遍历。 */
  source: 'git' | 'walk';
  scanned: number;
  scanTruncated: boolean;
}

/** D-040:GET /api/forge/workspace/search?q=&workspaceId=&limit=(空 q = 只预热候选缓存)。 */
export async function apiWorkspaceSearch(
  q: string,
  workspaceId?: string | null,
  limit = 30,
): Promise<WorkspaceSearchResp> {
  const params = new URLSearchParams({ q, limit: String(limit) });
  if (workspaceId) params.set('workspaceId', workspaceId);
  return apiGet<WorkspaceSearchResp>(`/api/forge/workspace/search?${params.toString()}`);
}

/** D-040:git 状态字母(VS Code 口径):M 修改 / A 新增 / D 删除 / R 重命名 / U 未跟踪 / C 冲突。 */
export type GitFileStatus = 'M' | 'A' | 'D' | 'R' | 'U' | 'C';

export interface GitFileEntry {
  path: string;
  status: GitFileStatus;
  staged: boolean;
  /** 未跟踪目录整条(其下文件都算未跟踪)。 */
  dir: boolean;
  insertions: number | null;
  deletions: number | null;
  origPath: string | null;
}

export interface WorkspaceGitResp {
  isRepo: boolean;
  /** isRepo=false 时的原因(stderr 首行等)。 */
  reason?: string;
  branch?: string | null;
  upstream?: string | null;
  ahead?: number;
  behind?: number;
  detached?: boolean;
  unborn?: boolean;
  /** 工作区目录整体未被跟踪(files 为空,不逐一展开)。 */
  rootUntracked?: boolean;
  files?: GitFileEntry[];
  counts?: Record<'modified' | 'added' | 'deleted' | 'renamed' | 'untracked' | 'conflicted', number>;
  insertions?: number;
  deletions?: number;
  total?: number;
  truncated?: boolean;
}

/** D-040:GET /api/forge/workspace/git?workspaceId= */
export async function apiWorkspaceGit(workspaceId?: string | null): Promise<WorkspaceGitResp> {
  const q = workspaceId ? `?workspaceId=${encodeURIComponent(workspaceId)}` : '';
  return apiGet<WorkspaceGitResp>(`/api/forge/workspace/git${q}`);
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

// ---------- Antigravity 订阅反代渠道状态与配置面 (R1 & R3) ----------

export interface AntigravityQuotaBucket {
  /** 剩余配额百分比 (0-100) */
  remainingPercent?: number;
  /** 已使用配额百分比 (0-100) */
  usedPercent?: number;
  /** 重置时间戳 (秒或毫秒) 或 ISO 字符串 */
  resetsAt?: number | string;
  /** 时间窗口大小 (分钟) */
  windowDurationMins?: number;
  [key: string]: unknown;
}

export interface AntigravityQuota {
  /** 主要额度 (如每分钟或单请求窗口配额) */
  primary?: AntigravityQuotaBucket | null;
  /** 次要额度 (如每日或长期累计配额) */
  secondary?: AntigravityQuotaBucket | null;
  [key: string]: unknown;
}

export type AntigravityAvailability = 'available' | 'needs-config' | 'offline' | 'disconnected';

export interface AntigravityStatus {
  ok?: boolean;
  configured: boolean;
  baseUrl: string;
  model: string;
  enabled?: boolean;
  keyConfigured: boolean;
  availability?: AntigravityAvailability;
  /** 支持后端返回的 rateLimits 或 quota 字段 */
  rateLimits?: AntigravityQuota | null;
  quota?: AntigravityQuota | null;
  models?: string[];
  latencyMs?: number | null;
  lastProbedAt?: number | null;
  lastError?: string | null;
  [key: string]: unknown;
}

export interface AntigravityConfigReq {
  baseUrl: string;
  model: string;
  /** 可选: 留空表示保留既有 key (R-5: 密钥永不回显) */
  key?: string;
  enabled?: boolean;
}

export interface AntigravityProbeReq {
  baseUrl?: string;
  model?: string;
  key?: string;
}

export interface AntigravityProbeRes {
  ok: boolean;
  status?: string;
  availability?: AntigravityAvailability;
  latencyMs?: number;
  rateLimits?: AntigravityQuota | null;
  quota?: AntigravityQuota | null;
  models?: string[];
  error?: string | null;
  [key: string]: unknown;
}

/** GET /api/forge/llm/antigravity/status (R-5: 绝无密钥明文) */
export async function getAntigravityStatus(): Promise<AntigravityStatus> {
  return apiGet<AntigravityStatus>('/api/forge/llm/antigravity/status');
}

/** POST /api/forge/llm/antigravity/config (落盘配置并写入加密 keystore) */
export async function postAntigravityConfig(
  payload: AntigravityConfigReq,
): Promise<AntigravityStatus & { ok: boolean }> {
  return apiPost<AntigravityStatus & { ok: boolean }>('/api/forge/llm/antigravity/config', payload);
}

/** POST /api/forge/llm/antigravity/probe (发起实时连通性与额度探针) */
export async function postAntigravityProbe(
  payload?: AntigravityProbeReq,
): Promise<AntigravityProbeRes> {
  return apiPost<AntigravityProbeRes>('/api/forge/llm/antigravity/probe', payload ?? {});
}

/** Codex 引擎状态面(GET status / POST config 响应同源;绝无密钥,R-5)。 */
export interface CodexAccount {
  authMode?: string | null;
  planType?: string | null;
  email?: string | null;
  rateLimits?: CodexRateLimits | null;
  lastError?: string | null;
}

export interface CodexConfigFace {
  codexBin: string;
  codexHome: string;
  defaultEngine: 'local' | 'codex';
  defaultModel: string;
  autoRegisterMcp: boolean;
  computerUse: boolean;
  /** D-041:认证来源配置值(auto = 已登录云账号时用云);旧 agentd 缺省。 */
  authSource?: CodexAuthSource;
}

/** Codex 认证来源(15 §8.4)。 */
export type CodexAuthSource = 'auto' | 'cloud' | 'chatgpt';

export interface CodexMcpServer {
  name: string;
  command: string;
  args: string[];
  available: boolean;
  envKeys: string[];
  /** 旧设置页兼容别名；新后端以 available 为事实源。 */
  present?: boolean;
  enabled?: boolean;
}

/** Codex app-server v2 `McpServerConnectionStatus`。 */
export type CodexMcpServerConnectionStatus =
  | 'notStarted'
  | 'starting'
  | 'connected'
  | 'authenticationRequired'
  | 'failed'
  | 'cancelled'
  | 'disabled';

/** Codex app-server v2 `McpAuthStatus`。 */
export type CodexMcpAuthStatus =
  | 'unknown'
  | 'unsupported'
  | 'notLoggedIn'
  | 'bearerToken'
  | 'oAuth';

export interface CodexMcpServerInfo {
  name: string;
  title: string | null;
  version: string;
  description: string | null;
  icons: unknown[] | null;
  websiteUrl: string | null;
}

export interface CodexMcpTool {
  name: string;
  title?: string;
  description?: string;
  inputSchema: unknown;
  outputSchema?: unknown;
  annotations?: unknown;
  icons?: unknown[];
  _meta?: unknown;
}

export interface CodexMcpResource {
  annotations?: unknown;
  description?: string;
  mimeType?: string;
  name: string;
  size?: number;
  title?: string;
  uri: string;
  icons?: unknown[];
  _meta?: unknown;
}

export interface CodexMcpResourceTemplate {
  annotations?: unknown;
  uriTemplate: string;
  name: string;
  title?: string;
  description?: string;
  mimeType?: string;
}

/** `mcpServerStatus/list` 的单服务项（来自 Codex app-server v2 生成协议）。 */
export interface CodexMcpRuntimeServer {
  name: string;
  runtimeStatus: CodexMcpServerConnectionStatus | null;
  pluginId: string | null;
  serverInfo: CodexMcpServerInfo | null;
  tools: Record<string, CodexMcpTool | undefined>;
  resources: CodexMcpResource[];
  resourceTemplates: CodexMcpResourceTemplate[];
  authStatus: CodexMcpAuthStatus;
}

/** Codex app-server v2 `ListMcpServerStatusResponse`。 */
export interface CodexMcpRuntimeStatus {
  data: CodexMcpRuntimeServer[];
  nextCursor: string | null;
}

export interface CodexMcpStaticStatus {
  autoRegisterMcp: boolean;
  projectRoot: string;
  servers: CodexMcpServer[];
}

/** GET /codex/mcp/status：Forge 静态注入表 + 可选 app-server 实时状态。 */
export interface CodexMcpStatus extends CodexMcpStaticStatus {
  live: boolean;
  runtime?: CodexMcpRuntimeStatus;
  runtimeError?: string;
}

export interface CodexRateLimitWindow {
  usedPercent?: number;
  resetsAt?: number;
  windowDurationMins?: number;
  [key: string]: unknown;
}

export interface CodexRateLimits {
  primary?: CodexRateLimitWindow | null;
  secondary?: CodexRateLimitWindow | null;
  [key: string]: unknown;
}

export interface CodexModel {
  id: string;
  displayName?: string;
  description?: string;
  supportedReasoningEfforts?: string[];
  defaultReasoningEffort?: string;
  [key: string]: unknown;
}

export interface CodexStatus {
  ok?: boolean;
  installed: boolean;
  managedInstalled: boolean;
  computerUseInstalled: boolean;
  command?: string | null;
  npmAvailable: boolean;
  running: boolean;
  version?: string | null;
  transport?: string | null;
  config: CodexConfigFace;
  account: CodexAccount;
  models?: CodexModel[];
  install: {
    running: boolean;
    log: string;
    error?: string | null;
    finishedAt?: string | null;
  };
  mcp?: CodexMcpStaticStatus;
  /** D-041:生效的认证来源(auto 已解析为 cloud / chatgpt)。 */
  authSource?: CodexAuthSource;
  cloud?: { loggedIn: boolean; serverUrl: string };
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
  authSource?: CodexAuthSource;
}): Promise<CodexStatus> {
  return apiPost<CodexStatus>('/api/forge/codex/config', payload);
}

export async function postCodexLogin(payload: {
  kind?: 'chatgpt' | 'deviceCode' | 'apiKey';
  apiKey?: string;
}): Promise<Record<string, unknown> & {
  ok?: boolean;
  authUrl?: string;
  loginId?: string;
  deviceCode?: string;
  userCode?: string;
  verificationUrl?: string;
  account?: CodexAccount;
}> {
  return apiPost('/api/forge/codex/login', payload);
}

export async function postCodexLoginCancel(loginId?: string): Promise<{ ok: boolean }> {
  return apiPost('/api/forge/codex/login/cancel', loginId ? { loginId } : {});
}

export async function postCodexLogout(): Promise<{ ok: boolean }> {
  return apiPost('/api/forge/codex/logout', {});
}

export async function getCodexAccount(): Promise<CodexAccount & { ok: boolean }> {
  return apiGet('/api/forge/codex/account');
}

export async function getCodexModels(refresh = false): Promise<{ ok: boolean; models: CodexModel[] }> {
  const q = refresh ? '?refresh=true' : '';
  return apiGet(`/api/forge/codex/models${q}`);
}

export async function getCodexRateLimits(): Promise<{ ok: boolean; rateLimits: CodexRateLimits | null }> {
  return apiGet('/api/forge/codex/rate-limits');
}

export async function getCodexMcpStatus(sessionId?: string | null): Promise<CodexMcpStatus> {
  const q = sessionId ? `?sessionId=${encodeURIComponent(sessionId)}` : '';
  return apiGet(`/api/forge/codex/mcp/status${q}`);
}

export interface GoalFace {
  sessionId?: string;
  objective: string;
  status: 'active' | 'paused' | 'completed' | 'blocked';
  tokenBudget?: number;
  tokensUsed?: number;
  timeUsedSeconds?: number;
  turns?: number;
  note?: string | null;
  engine?: 'local' | 'codex';
  budgetExhausted?: boolean;
  /** Codex app-server 用 Unix 整数，本地 Goal 用 RFC3339。 */
  createdAt?: string | number;
  updatedAt?: string | number;
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
