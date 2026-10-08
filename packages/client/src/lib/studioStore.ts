import { create } from 'zustand';
import type { KindTone } from './designBoardStore';
import {
  apiGenAudio,
  apiGenMesh,
  apiGenVideo,
  apiGenVideoFrames,
  apiPost,
  callAssetTool,
  callGenTool,
  callModelGenTool,
  ForgeApiError,
  type FrameAtlas,
  type MediaArtifact,
} from './forgeApi';
import { parseFrame, splitFrames } from './sseClient';
import { useSpriteStore } from './spriteStore';
import { useWorkbenchStore } from './workbenchStore';
import { useWorkspaceStore } from './workspaceStore';
import { useAssetStore } from './assetStore';
import { useGenStore } from './genStore';
import { resolveVideoBackend, videoBackendOptions } from './videoBackend';

/**
 * 素材创作 store v1(素材创作波:与画板同位的第四页签)。
 * 主画布 = 创作节点卡(大纲/地图草稿/原画/贴图/UI/3D模型/视频/音频)+ 引用连线
 * (上游产物在生成时拼进上下文);双击卡片下钻进该节点的详情画布(openNodeId,
 * 会话态)——中央产物节点 + 版本历史 + 底部生成输入条(StudioComposer)。
 * selectedNodeId = 主画布右键 / 快捷键的作用对象(会话态,与 openNodeId 同不持久化)。
 * 六通道:text → 隐藏 studio 会话 + /ask:execute(完整工具循环);
 * image → mcp gen_image/gen_accept;model → REST gen/mesh;video/audio → gen/video|audio;
 * sprite(角色动画)→ 参考图 + gen/video 图生视频 → gen/video/frames 截帧成图集 →
 * gen_accept + sprite_create 落 .rxsprite。
 * 持久化按 workspace 分区(forge:studioBoard:<id>),旧 key forge:studioBoard 首次读时迁入。
 */

// ---------- 类型 ----------

export type StudioKind = 'text' | 'image' | 'model' | 'video' | 'audio' | 'sprite';

export type StudioPresetId =
  | 'outline'
  | 'mapdraft'
  | 'concept'
  | 'texture'
  | 'ui'
  | 'mesh'
  | 'video'
  | 'audio'
  | 'charanim';

export interface StudioPresetDef {
  id: StudioPresetId;
  kind: StudioKind;
  label: string;
  tone: KindTone;
  /** prompt 输入框占位文案 */
  hint: string;
  /** 图像/模型入库目标目录(相对 Content/) */
  destFolder?: string;
}

export const STUDIO_PRESETS: StudioPresetDef[] = [
  { id: 'outline', kind: 'text', label: '大纲', tone: 'info', hint: '描述游戏的核心玩法 / 世界观 / 进程结构…' },
  { id: 'mapdraft', kind: 'text', label: '地图草稿', tone: 'sage', hint: '描述地图的区域划分 / 路径 / 兴趣点…' },
  { id: 'concept', kind: 'image', label: '原画', tone: 'acc', hint: '描述角色 / 场景 / 道具的画面…', destFolder: 'Concepts' },
  { id: 'texture', kind: 'image', label: '贴图', tone: 'warn', hint: '描述材质表面(如 岩石 / 木纹 / 金属)…', destFolder: 'Textures' },
  { id: 'ui', kind: 'image', label: 'UI', tone: 'info', hint: '描述界面元素(如 主菜单 / 血条 / 图标)…', destFolder: 'UI' },
  { id: 'mesh', kind: 'model', label: '3D模型', tone: 'sage', hint: '描述要生成的三维模型…', destFolder: 'Meshes' },
  { id: 'video', kind: 'video', label: '视频', tone: 'danger', hint: '描述要生成的视频片段(过场 / 演出参考)…' },
  { id: 'audio', kind: 'audio', label: '音频', tone: 'acc', hint: '输入要朗读的文字,或切到音乐生成描述曲风…' },
  {
    id: 'charanim',
    kind: 'sprite',
    label: '角色动画',
    tone: 'warn',
    hint: '描述角色动作(如 向右行走循环 / 待机呼吸 / 挥剑攻击)…',
    // 落盘的是图集贴图;.rxsprite 由 sprite_create 另落 Sprites/。
    destFolder: 'Textures',
  },
];

/**
 * 角色动画的提示词纪律。视频模型天生爱推镜头、爱加背景、爱切镜,而截帧要的恰是反面:
 * 机位死钉、底色干净、角色不出画。这三条不写进提示词,截出来的帧就没法当动画用。
 */
const CHARANIM_PROMPT_RULES =
  '\n\n【角色动画约束】严格保持参考图里角色的外观、比例与配色;镜头完全固定,不平移不推拉不旋转;' +
  '角色居中且全程完整在画面内;背景为单一纯色,不要场景元素、地面与投影;不切镜、不转场、不加字幕。';

export function presetOf(id: string): StudioPresetDef | undefined {
  return STUDIO_PRESETS.find((p) => p.id === id);
}

/** 文本节点模板(composer 快捷模板;参考截图:剧本生成/策划案生成/提示词生成)。 */
export interface TextTemplateDef {
  id: string;
  label: string;
  /** LLM 请求前缀(空 = 自由编写) */
  prefix: string;
}

export const TEXT_TEMPLATES: TextTemplateDef[] = [
  { id: 'free', label: '自由编写', prefix: '' },
  {
    id: 'outline',
    label: '大纲',
    prefix: '请为当前游戏项目撰写游戏设计大纲(核心玩法 / 世界观 / 进程结构 / 关卡列表),Markdown 输出。先检索当前项目相关资产与文档并引用出处。需求:',
  },
  {
    id: 'mapdraft',
    label: '地图草稿',
    prefix: '请撰写一份地图草稿设计(区域划分 / 路径连接 / 兴趣点 / 敌人与资源分布),文字描述加 ASCII 示意图输出。先检索当前项目相关资产与文档并引用出处。需求:',
  },
  {
    id: 'script',
    label: '剧本生成',
    prefix: '请撰写游戏剧本(场景 / 角色 / 对白 / 分镜提示),Markdown 输出。先检索当前项目相关资产与文档并引用出处。需求:',
  },
  {
    id: 'plandoc',
    label: '策划案生成',
    prefix: '请撰写游戏策划案(目标 / 玩法循环 / 系统拆解 / 数值框架 / 里程碑),Markdown 输出。先检索当前项目相关资产与文档并引用出处。需求:',
  },
  {
    id: 'promptcraft',
    label: '提示词生成',
    prefix: '请把以下需求改写为高质量的图像/视频生成提示词(英文为主,附中文注释,包含风格 / 构图 / 光照 / 质量词)。先对照项目已有资产再写。需求:',
  },
];

/** 一次生成产出的版本(text 全文持久化;媒体存 fileRef,dataUrl 会话态)。 */
export interface StudioVersion {
  blenderJobId?: string;
  blenderWorkspaceId?: string;
  blenderRevision?: number;
  prefabGuid?: string;
  animationClips?: string[];
  id: string;
  createdAt: number;
  /** 生成来源(后端 id 或 llm provider 标签) */
  backendId: string;
  prompt: string;
  /** text 产物 */
  text?: string;
  /** 媒体产物(.forge/tmp/gen/ 项目相对路径) */
  fileRef?: string;
  mime?: string;
  seed?: number;
  /** 会话态 base64 预览(不持久化;重开后凭 fileRef 如实占位) */
  dataUrl?: string;
  /** 供应商侧渲染的缩略图(3D 产物无浏览器内联渲染面,只有它能给出真实观感;签名 URL 会过期) */
  thumbnailUrl?: string;
  /** 已落盘的多视角预览(图生 3D 四向,文生 3D 仅正面;dataUrl 会话态) */
  previews?: { label: string; dataUrl: string }[];
  /** 角色动画:源视频产物(截帧的输入;换参数重切帧不必重新生成视频) */
  videoFileRef?: string;
  /** 角色动画:截帧拼出的图集(dataUrl 会话态,重开后凭 fileRef 如实占位) */
  atlas?: FrameAtlas;
  /** 角色动画:图集内逐帧 bbox([x,y,w,h]),与 .rxsprite frames 同形 */
  boxes?: Array<[number, number, number, number]>;
  /** 角色动画:实际截帧率(clip 时长据此换算) */
  fps?: number;
  /** 入库后(gen_accept) */
  assetPath?: string;
  guid?: string;
  /** 角色动画入库后:.rxsprite 资产路径(精灵编辑器据此打开) */
  spritePath?: string;
  /** 文本 Agent turn 审计 */
  runId?: string;
  toolCalls?: StudioToolRecord[];
  resourceRefs?: string[];
}

export interface StudioToolRecord {
  name: string;
  status: 'running' | 'ok' | 'error' | 'denied';
  summary?: string;
}

export type StudioRunPhase = 'retrieving' | 'read' | 'pending-approval' | 'running' | 'failed';

export interface StudioNodeRun {
  sessionId: string;
  runId: string | null;
  phase: StudioRunPhase;
  tools: StudioToolRecord[];
}

export interface StudioPermission {
  nodeId: string;
  id: string;
  tool: string;
  targetProjectId?: string;
  argsSummary?: string;
}

export interface StudioNode {
  id: string;
  preset: StudioPresetId;
  name: string;
  pos: [number, number];
  /** 当前输入草稿(持久化,切页签不丢) */
  prompt: string;
  /** 通道参数(image: size/n;video: aspect/resolution/durationSec;audio: mode/voice/
   * format/instrumental;text: template) */
  params: Record<string, unknown>;
  versions: StudioVersion[];
  /** 当前展示版本(null = 空态) */
  currentVersionId: string | null;
}

/** 引用连线:from 节点产物在 to 节点生成时拼进上下文。 */
export interface StudioEdge {
  id: string;
  from: string;
  to: string;
  label: string;
}

// ---------- 持久化 ----------

const STUDIO_KEY = 'forge:studioBoard';
const ACTIVE_WS_KEY = 'forge:activeWorkspace';

interface PersistedStudioV1 {
  version: 1 | 2;
  seq: number;
  nodes: StudioNode[];
  edges: StudioEdge[];
  readonlyWorkspaceIds?: string[];
  includeLibrary?: boolean;
}

export function studioStorageKey(workspaceId: string | null): string {
  return `${STUDIO_KEY}:${workspaceId ?? 'default'}`;
}

function readActiveWorkspaceId(): string | null {
  try {
    const v = globalThis.localStorage?.getItem(ACTIVE_WS_KEY);
    return v === '' || v === 'null' || v == null ? null : v;
  } catch {
    return null;
  }
}

function isVersion(v: unknown): v is StudioVersion {
  if (typeof v !== 'object' || v === null) return false;
  const x = v as Partial<StudioVersion>;
  return (
    typeof x.id === 'string' &&
    typeof x.createdAt === 'number' &&
    typeof x.backendId === 'string' &&
    typeof x.prompt === 'string'
  );
}

function isNode(v: unknown): v is StudioNode {
  if (typeof v !== 'object' || v === null) return false;
  const n = v as Partial<StudioNode>;
  return (
    typeof n.id === 'string' &&
    typeof n.preset === 'string' &&
    presetOf(n.preset) !== undefined &&
    typeof n.name === 'string' &&
    Array.isArray(n.pos) &&
    n.pos.length === 2 &&
    n.pos.every((x) => typeof x === 'number' && Number.isFinite(x)) &&
    typeof n.prompt === 'string' &&
    typeof n.params === 'object' &&
    n.params !== null &&
    Array.isArray(n.versions) &&
    n.versions.every(isVersion) &&
    (n.currentVersionId === null || typeof n.currentVersionId === 'string')
  );
}

function isEdge(v: unknown): v is StudioEdge {
  if (typeof v !== 'object' || v === null) return false;
  const e = v as Partial<StudioEdge>;
  return (
    typeof e.id === 'string' &&
    typeof e.from === 'string' &&
    typeof e.to === 'string' &&
    typeof e.label === 'string'
  );
}

/**
 * dataUrl 是会话态,落盘前剥离(防 localStorage 撑爆)。
 * 图集的 dataUrl 藏在 atlas 里,一张几千像素的角色动画图集 base64 后就是数 MB
 * ——不单独剥这一层,localStorage 会被一个节点吃满。
 */
function stripSessionFields(nodes: StudioNode[]): StudioNode[] {
  return nodes.map((n) => ({
    ...n,
    versions: n.versions.map((v) => {
      const { dataUrl: _drop, atlas, ...rest } = v;
      if (atlas === undefined) return rest;
      const { dataUrl: _dropAtlas, ...atlasRest } = atlas;
      return { ...rest, atlas: atlasRest as FrameAtlas };
    }),
  }));
}

export function loadStudio(workspaceId?: string | null): {
  seq: number;
  nodes: StudioNode[];
  edges: StudioEdge[];
  readonlyWorkspaceIds: string[];
  includeLibrary: boolean;
} {
  const empty = {
    seq: 1,
    nodes: [] as StudioNode[],
    edges: [] as StudioEdge[],
    readonlyWorkspaceIds: [] as string[],
    includeLibrary: true,
  };
  const wsId = workspaceId === undefined ? readActiveWorkspaceId() : workspaceId;
  try {
    const key = studioStorageKey(wsId);
    let raw = globalThis.localStorage?.getItem(key);
    if (!raw) {
      const legacy = globalThis.localStorage?.getItem(STUDIO_KEY);
      if (legacy) {
        raw = legacy;
        globalThis.localStorage?.setItem(key, legacy);
        globalThis.localStorage?.removeItem(STUDIO_KEY);
      }
    }
    if (!raw) return empty;
    const p = JSON.parse(raw) as Partial<PersistedStudioV1>;
    if (p.version !== 1 && p.version !== 2) return empty;
    const nodes = Array.isArray(p.nodes) ? p.nodes.filter(isNode) : [];
    const ids = new Set(nodes.map((n) => n.id));
    const edges = Array.isArray(p.edges)
      ? p.edges.filter(isEdge).filter((e) => ids.has(e.from) && ids.has(e.to) && e.from !== e.to)
      : [];
    const seq =
      typeof p.seq === 'number' && Number.isFinite(p.seq) && p.seq >= 1
        ? Math.floor(p.seq)
        : nodes.length + edges.length + 1;
    // currentVersionId 悬空修正(指向被过滤掉的版本时回落最新)。
    for (const n of nodes) {
      if (n.currentVersionId !== null && !n.versions.some((v) => v.id === n.currentVersionId)) {
        n.currentVersionId = n.versions.length > 0 ? n.versions[n.versions.length - 1].id : null;
      }
    }
    return {
      seq,
      nodes,
      edges,
      readonlyWorkspaceIds: Array.isArray(p.readonlyWorkspaceIds)
        ? p.readonlyWorkspaceIds.filter((x): x is string => typeof x === 'string')
        : [],
      includeLibrary: p.includeLibrary !== false,
    };
  } catch {
    return empty;
  }
}

function persistStudio(
  doc: {
    seq: number;
    nodes: StudioNode[];
    edges: StudioEdge[];
    readonlyWorkspaceIds: string[];
    includeLibrary: boolean;
  },
  workspaceId: string | null,
): void {
  try {
    globalThis.localStorage?.setItem(
      studioStorageKey(workspaceId),
      JSON.stringify({
        version: 2,
        seq: doc.seq,
        nodes: stripSessionFields(doc.nodes),
        edges: doc.edges,
        readonlyWorkspaceIds: doc.readonlyWorkspaceIds,
        includeLibrary: doc.includeLibrary,
      } satisfies PersistedStudioV1),
    );
  } catch {
    // 写不进静默
  }
}

// ---------- 生成通道 ----------

/** 各 preset 的缺省参数(节点创建时灌入)。 */
export function defaultParams(preset: StudioPresetDef): Record<string, unknown> {
  switch (preset.kind) {
    case 'text':
      return { template: preset.id === 'mapdraft' ? 'mapdraft' : preset.id === 'outline' ? 'outline' : 'free' };
    case 'image':
      return { size: 512, n: 1 };
    case 'model':
      return {};
    case 'video':
      return { aspect: '16:9', resolution: '720p', durationSec: 5 };
    case 'audio':
      return { mode: 'tts', voice: 'alloy', format: 'mp3', instrumental: false };
    case 'sprite':
      // 1:1 + union 裁切 = 帧等大、脚底锚不抖(D-031);8fps × 5s 上限 32 帧,
      // 够一条走路/待机循环,又不至于把图集撑到几千像素。
      return {
        aspect: '1:1',
        resolution: '720p',
        durationSec: 5,
        fps: 8,
        maxFrames: 32,
        chromaKey: 'auto',
        crop: 'union',
        clipName: 'walk',
        refAssetPath: '',
      };
  }
}

/** 截帧参数白名单(字符串枚举越界即当未给,交由后端缺省;非法值不冒充合法值)。 */
const CHROMA_KEYS = ['auto', 'magenta', 'none'] as const;
const CROP_MODES = ['union', 'tight', 'none'] as const;
export type ChromaKeyParam = (typeof CHROMA_KEYS)[number];
export type CropParam = (typeof CROP_MODES)[number];

/** 节点参数 → 截帧请求参数(生成链与「重新截帧」共用一份读法)。 */
function frameParams(params: Record<string, unknown>): {
  fps?: number;
  maxFrames?: number;
  chromaKey?: ChromaKeyParam;
  crop?: CropParam;
} {
  const chroma = params.chromaKey;
  const crop = params.crop;
  return {
    fps: typeof params.fps === 'number' ? params.fps : undefined,
    maxFrames: typeof params.maxFrames === 'number' ? params.maxFrames : undefined,
    chromaKey: CHROMA_KEYS.find((k) => k === chroma),
    crop: CROP_MODES.find((c) => c === crop),
  };
}

/**
 * 图像产物 → 可交给后端解析的项目相对路径。
 * 已入库资产的 assetPath 是 Content/ 相对(如 "Concepts/hero.png"),
 * 未入库候选的 fileRef 是项目根相对(".forge/tmp/gen/…"),两者不能混着传。
 */
function projectRelImage(ref: string): string {
  const r = ref.replace(/\\/g, '/');
  return r.startsWith('.forge/') || r.startsWith('Content/') ? r : `Content/${r}`;
}

/**
 * 角色动画的参考图解析:上游连过来的图像节点当前版本优先(已入库的用 assetPath,
 * 否则用尚在 tmp 的候选),其次是参数条里手选的贴图资产。都没有 → undefined,
 * 由调用方如实报错——图生视频没有参考图就只是文生视频,不能悄悄降级。
 */
function spriteRefImage(
  nodeId: string,
  nodes: StudioNode[],
  edges: StudioEdge[],
  params: Record<string, unknown>,
): string | undefined {
  for (const e of edges.filter((e) => e.to === nodeId)) {
    const up = nodes.find((n) => n.id === e.from);
    if (up === undefined || presetOf(up.preset)?.kind !== 'image') continue;
    const cur = up.versions.find((v) => v.id === up.currentVersionId);
    const ref = cur?.assetPath ?? cur?.fileRef;
    if (ref !== undefined && ref !== '') return projectRelImage(ref);
  }
  const manual = typeof params.refAssetPath === 'string' ? params.refAssetPath.trim() : '';
  return manual !== '' ? projectRelImage(manual) : undefined;
}

/** 上游上下文(引用连线 from 端产物;text 截 1500 字,媒体给 prompt/入库路径摘要)。 */
function upstreamContext(nodeId: string, nodes: StudioNode[], edges: StudioEdge[]): string {
  const lines: string[] = [];
  for (const e of edges.filter((e) => e.to === nodeId)) {
    const up = nodes.find((n) => n.id === e.from);
    if (!up) continue;
    const preset = presetOf(up.preset);
    const cur = up.versions.find((v) => v.id === up.currentVersionId);
    const tag = e.label.trim() !== '' ? `(${e.label.trim()})` : '';
    if (cur?.text !== undefined && cur.text !== '') {
      const t = cur.text.length > 1500 ? `${cur.text.slice(0, 1500)}…(截断)` : cur.text;
      lines.push(`【上游·${up.name}${tag}】\n${t}`);
    } else if (cur !== undefined) {
      const where = cur.assetPath ?? cur.fileRef ?? '(未入库)';
      lines.push(`【上游·${up.name}${tag}】${preset?.label ?? up.preset}:提示词「${cur.prompt}」,产物 ${where}`);
    } else if (up.prompt.trim() !== '') {
      lines.push(`【上游·${up.name}${tag}】${preset?.label ?? up.preset}(未生成):意图「${up.prompt.trim()}」`);
    }
  }
  return lines.join('\n\n');
}

/** gen_image 候选(gen-image-mcp 响应面)。 */
interface ImageCandidate {
  imageFileRef: string;
  seed: number;
  backendId: string;
  dataUrl?: string;
}

/** prompt + seed → 入库文件名(ascii slug;与 genStore.slugifyName 同规则)。 */
function slugName(prompt: string, seed: number): string {
  const slug =
    prompt
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+/, '')
      .replace(/-+$/, '')
      .slice(0, 24)
      .replace(/-+$/, '') || 'gen';
  return `${slug}-${seed}`;
}

// ---------- store ----------

export interface StudioError {
  nodeId: string;
  code: string;
  message: string;
}

interface StudioState {
  nodes: StudioNode[];
  edges: StudioEdge[];
  seq: number;
  workspaceId: string | null;
  readonlyWorkspaceIds: string[];
  includeLibrary: boolean;
  nodeRuns: Record<string, StudioNodeRun>;
  pendingPermission: StudioPermission | null;
  /** 下钻中的创作节点详情画布(null = 主画布;会话态,不持久化) */
  openNodeId: string | null;
  /** 选中的创作节点:主画布快捷键的作用对象(会话态,不持久化) */
  selectedNodeId: string | null;
  selectedNodeIds: string[];
  /** 待进入行内改名态的节点 / 连线(卡片与标签自己各存一份开合态会与菜单打架) */
  pendingRenameId: string | null;
  pendingEdgeId: string | null;
  /** 生成中的节点(允许多节点并行) */
  busyIds: string[];
  /** 最近一次生成/入库错误(按节点;NOT_CONFIGURED 等如实显示) */
  lastError: StudioError | null;

  /** pos 缺省时按已有节点数排成网格;右键「在此新建」传画布落点 */
  addNode: (preset: StudioPresetId, pos?: [number, number]) => string | null;
  renameNode: (id: string, name: string) => void;
  moveNode: (id: string, pos: [number, number]) => void;
  removeNode: (id: string) => void;
  /** 复制 prompt 与参数另起一张草稿卡;版本历史与连线不复制 */
  duplicateNode: (id: string) => string | null;
  setPrompt: (id: string, prompt: string) => void;
  setParam: (id: string, key: string, value: unknown) => void;

  selectNode: (id: string | null, additive?: boolean) => void;
  /** 让节点卡标题进入行内改名态(右键菜单「重命名」与 F2 共用) */
  editNodeName: (id: string) => void;
  clearPendingRename: () => void;

  addEdge: (from: string, to: string) => string | null;
  setEdgeLabel: (id: string, label: string) => void;
  removeEdge: (id: string) => void;
  /** 让连线标签进入行内编辑态(右键菜单「编辑引用说明」与刚拉出的新边共用) */
  editEdgeLabel: (id: string) => void;
  clearPendingEdge: () => void;

  openNode: (id: string) => void;
  closeNode: () => void;
  clearBoard: () => void;
  clearError: () => void;

  /** 手工写入文本产物(text 节点「自己编写内容」;新建一个版本) */
  writeText: (id: string, text: string) => void;
  /** 更新当前文本版本内容(详情画布内联编辑) */
  editCurrentText: (id: string, text: string) => void;
  setCurrentVersion: (nodeId: string, versionId: string) => void;
  removeVersion: (nodeId: string, versionId: string) => void;

  /** 按 preset.kind 分派六通道生成;错误如实进 lastError(码保留)。 */
  generate: (id: string) => Promise<void>;
  cancelGenerate: (id: string) => Promise<void>;
  resolvePermission: (allow: boolean) => Promise<void>;
  bindWorkspace: (workspaceId: string | null) => void;
  setReadonlyWorkspaceIds: (ids: string[]) => void;
  setIncludeLibrary: (on: boolean) => void;
  /** 图像/模型/角色动画版本入库(gen_accept → Content/<destFolder>);成功回写 assetPath/guid。 */
  acceptVersion: (nodeId: string, versionId: string) => Promise<void>;
  recordBlenderVersion: (nodeId: string, job: import('../../../protocol/src/blender').BlenderJob) => void;
  /**
   * 角色动画:按当前截帧参数重切帧(不重新生成视频)。
   * 抠底/裁切/帧率是要反复试的旋钮,每试一次都重出一段视频既慢又不同源。
   */
  resliceVersion: (nodeId: string, versionId: string) => Promise<void>;
  /** 角色动画:已入库版本 → 打开精灵编辑器继续调帧/编 clip。 */
  openInSpriteEditor: (nodeId: string, versionId: string) => void;
}

const studioAborts = new Map<string, AbortController>();
const studioStreams = new Map<string, { close: () => void }>();

function payloadStr(data: unknown, key: string): string | undefined {
  if (!data || typeof data !== 'object') return undefined;
  const v = (data as { payload?: Record<string, unknown> }).payload?.[key];
  return typeof v === 'string' ? v : undefined;
}

function subscribeStudioOnce(
  sessionId: string,
  onEvent: (type: string, data: unknown) => void,
): { close: () => void } {
  const abort = new AbortController();
  void (async () => {
    try {
      const res = await fetch(
        `/api/forge/sessions/${encodeURIComponent(sessionId)}/events/stream?fromSeq=0`,
        { signal: abort.signal, headers: { Accept: 'text/event-stream' } },
      );
      if (!res.ok || !res.body) return;
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = '';
      for (;;) {
        const { done, value } = await reader.read();
        if (done || abort.signal.aborted) break;
        buf += decoder.decode(value, { stream: true });
        const [frames, rest] = splitFrames(buf);
        buf = rest;
        for (const raw of frames) {
          const frame = parseFrame(raw);
          if (frame) onEvent(frame.event, frame.data);
        }
      }
    } catch {
      /* 单次订阅:HTTP 正文是终态,SSE 失败不重试 */
    }
  })();
  return { close: () => abort.abort() };
}

async function postJson<T>(path: string, payload: unknown, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
    signal,
  });
  const body = (await res.json()) as unknown;
  if (!res.ok) {
    const err = (body as { error?: { code?: string; message?: string } })?.error;
    throw new ForgeApiError(err?.code ?? `HTTP_${res.status}`, err?.message ?? `HTTP ${res.status}`, res.status);
  }
  return body as T;
}

export const useStudioStore = create<StudioState>((set, get) => {
  const videoRequests = new Map<string, { workspaceId: string | null; nodeId: string; token: symbol }>();
  const videoErrors = new Map<string | null, StudioError>();
  const videoRequestKey = (workspaceId: string | null, nodeId: string) => JSON.stringify([workspaceId, nodeId]);
  const commit = (
    patch: Partial<Pick<StudioState, 'nodes' | 'edges' | 'seq' | 'readonlyWorkspaceIds' | 'includeLibrary'>>,
  ): void => {
    set(patch);
    const { seq, nodes, edges, readonlyWorkspaceIds, includeLibrary, workspaceId } = get();
    persistStudio({ seq, nodes, edges, readonlyWorkspaceIds, includeLibrary }, workspaceId);
  };

  const patchNode = (id: string, fn: (n: StudioNode) => StudioNode): void => {
    commit({ nodes: get().nodes.map((n) => (n.id === id ? fn(n) : n)) });
  };

  const setBusy = (id: string, busy: boolean): void => {
    set((s) => ({
      busyIds: busy ? [...s.busyIds, id] : s.busyIds.filter((x) => x !== id),
    }));
  };

  /** 版本追加 + 置为当前(seq 消耗由调用方负责)。 */
  const pushVersions = (nodeId: string, versions: StudioVersion[]): void => {
    if (versions.length === 0) return;
    patchNode(nodeId, (n) => ({
      ...n,
      versions: [...n.versions, ...versions],
      currentVersionId: versions[versions.length - 1].id,
    }));
  };

  // 视频可能在切换画板后完成:只更新发起时的画板,并在完成时分配版本 id。
  const pushVideoVersions = (workspaceId: string | null, nodeId: string, versions: StudioVersion[]): void => {
    if (versions.length === 0) return;
    const active = get().workspaceId === workspaceId;
    const doc = active ? get() : loadStudio(workspaceId);
    if (!doc.nodes.some((n) => n.id === nodeId)) return;
    const appended = versions.map((v, i) => ({ ...v, id: `v${doc.seq + i}` }));
    const nodes = doc.nodes.map((n) => n.id === nodeId ? {
      ...n, versions: [...n.versions, ...appended], currentVersionId: appended[appended.length - 1].id,
    } : n);
    const patch = { nodes, seq: doc.seq + appended.length };
    if (active) commit(patch);
    else persistStudio({ ...doc, ...patch }, workspaceId);
  };

  const initial = loadStudio();
  return {
    ...initial,
    workspaceId: readActiveWorkspaceId(),
    nodeRuns: {},
    pendingPermission: null,
    openNodeId: null,
    selectedNodeId: null,
    selectedNodeIds: [],
    pendingRenameId: null,
    pendingEdgeId: null,
    busyIds: [],
    lastError: null,

    addNode: (presetId, pos) => {
      const preset = presetOf(presetId);
      if (!preset) return null;
      if (pos !== undefined && !pos.every((v) => Number.isFinite(v))) return null;
      const { nodes, seq } = get();
      const id = `s${seq}`;
      let i = nodes.filter((n) => n.preset === presetId).length + 1;
      while (nodes.some((n) => n.name === `${preset.label} ${i}`)) i += 1;
      const idx = nodes.length;
      const node: StudioNode = {
        id,
        preset: presetId,
        name: `${preset.label} ${i}`,
        pos: pos ?? [40 + (idx % 4) * 260, 48 + Math.floor(idx / 4) * 220],
        prompt: '',
        params: defaultParams(preset),
        versions: [],
        currentVersionId: null,
      };
      commit({ nodes: [...nodes, node], seq: seq + 1 });
      set({ selectedNodeId: id, selectedNodeIds: [id] });
      return id;
    },

    renameNode: (id, name) => {
      const t = name.trim();
      if (t === '') return;
      patchNode(id, (n) => ({ ...n, name: t }));
    },

    // 无限画布:坐标四向无界,只挡 NaN/Infinity
    moveNode: (id, pos) => {
      if (!pos.every((v) => Number.isFinite(v))) return;
      patchNode(id, (n) => ({ ...n, pos: [pos[0], pos[1]] }));
    },

    removeNode: (id) => {
      if (get().openNodeId === id) set({ openNodeId: null });
      set((state) => ({ selectedNodeId: state.selectedNodeId === id ? null : state.selectedNodeId, selectedNodeIds: state.selectedNodeIds.filter((value) => value !== id) }));
      if (get().pendingRenameId === id) set({ pendingRenameId: null });
      commit({
        nodes: get().nodes.filter((n) => n.id !== id),
        edges: get().edges.filter((e) => e.from !== id && e.to !== id),
      });
    },

    // 版本历史不复制:产物是生成出来的,连着复制等于凭空造出一段没发生过的生成记录。
    // 连线也不复制——引用是两端的约定(与画板 duplicateNode 同取舍)。
    duplicateNode: (id) => {
      const { nodes, seq } = get();
      const src = nodes.find((n) => n.id === id);
      if (!src) return null;
      const newId = `s${seq}`;
      let name = `${src.name} 副本`;
      let i = 2;
      while (nodes.some((n) => n.name === name)) {
        name = `${src.name} 副本 ${i}`;
        i += 1;
      }
      const node: StudioNode = {
        id: newId,
        preset: src.preset,
        name,
        // 错开一格落在原卡右下,不完全盖住原件
        pos: [src.pos[0] + 32, src.pos[1] + 32],
        prompt: src.prompt,
        params: { ...src.params },
        versions: [],
        currentVersionId: null,
      };
      commit({ nodes: [...nodes, node], seq: seq + 1 });
      set({ selectedNodeId: newId, selectedNodeIds: [newId] });
      return newId;
    },

    setPrompt: (id, prompt) => patchNode(id, (n) => ({ ...n, prompt })),

    setParam: (id, key, value) =>
      patchNode(id, (n) => ({ ...n, params: { ...n.params, [key]: value } })),

    selectNode: (id, additive = false) => set((state) => { const selectedNodeIds = id === null ? [] : additive ? state.selectedNodeIds.includes(id) ? state.selectedNodeIds.filter((value) => value !== id) : [...state.selectedNodeIds, id] : [id]; return { selectedNodeId: selectedNodeIds.at(-1) ?? null, selectedNodeIds }; }),

    editNodeName: (id) => {
      if (!get().nodes.some((n) => n.id === id)) return;
      set({ selectedNodeId: id, pendingRenameId: id });
    },

    clearPendingRename: () => set({ pendingRenameId: null }),

    addEdge: (from, to) => {
      const { nodes, edges, seq } = get();
      if (from === to) return null;
      if (!nodes.some((n) => n.id === from) || !nodes.some((n) => n.id === to)) return null;
      if (edges.some((e) => e.from === from && e.to === to)) return null;
      const id = `e${seq}`;
      commit({ edges: [...edges, { id, from, to, label: '' }], seq: seq + 1 });
      set({ pendingEdgeId: id });
      return id;
    },

    setEdgeLabel: (id, label) =>
      commit({ edges: get().edges.map((e) => (e.id === id ? { ...e, label } : e)) }),

    removeEdge: (id) => commit({ edges: get().edges.filter((e) => e.id !== id) }),

    editEdgeLabel: (id) => {
      if (!get().edges.some((e) => e.id === id)) return;
      set({ pendingEdgeId: id });
    },

    clearPendingEdge: () => set({ pendingEdgeId: null }),

    openNode: (id) => {
      if (!get().nodes.some((n) => n.id === id)) return;
      set({ openNodeId: id, lastError: null });
    },

    closeNode: () => set({ openNodeId: null }),

    clearBoard: () => {
      set({ openNodeId: null, selectedNodeId: null, selectedNodeIds: [], pendingRenameId: null, lastError: null });
      commit({ nodes: [], edges: [] });
    },

    clearError: () => {
      videoErrors.delete(get().workspaceId);
      set({ lastError: null });
    },

    writeText: (id, text) => {
      const { seq } = get();
      const version: StudioVersion = {
        id: `v${seq}`,
        createdAt: Date.now(),
        backendId: 'manual',
        prompt: '',
        text,
      };
      commit({ seq: seq + 1 });
      pushVersions(id, [version]);
    },

    editCurrentText: (id, text) => {
      patchNode(id, (n) => {
        if (n.currentVersionId === null) return n;
        return {
          ...n,
          versions: n.versions.map((v) =>
            v.id === n.currentVersionId && v.text !== undefined ? { ...v, text } : v,
          ),
        };
      });
    },

    setCurrentVersion: (nodeId, versionId) =>
      patchNode(nodeId, (n) =>
        n.versions.some((v) => v.id === versionId) ? { ...n, currentVersionId: versionId } : n,
      ),

    removeVersion: (nodeId, versionId) =>
      patchNode(nodeId, (n) => {
        const versions = n.versions.filter((v) => v.id !== versionId);
        return {
          ...n,
          versions,
          currentVersionId:
            n.currentVersionId === versionId
              ? versions.length > 0
                ? versions[versions.length - 1].id
                : null
              : n.currentVersionId,
        };
      }),

    generate: async (id) => {
      const { nodes: requestNodes, edges: requestEdges, workspaceId: boardWorkspaceId } = get();
      const videoWorkspaceId = boardWorkspaceId ?? useWorkspaceStore.getState().activeWorkspaceId ?? readActiveWorkspaceId();
      const node = requestNodes.find((n) => n.id === id);
      if (!node) return;
      const preset = presetOf(node.preset);
      if (!preset) return;
      const prompt = node.prompt.trim();
      if (prompt === '') {
        set({ lastError: { nodeId: id, code: 'EMPTY_PROMPT', message: '请先输入创作描述' } });
        return;
      }
      if (get().busyIds.includes(id)) return;
      const videoToken = preset.kind === 'video' || preset.kind === 'sprite' ? Symbol() : null;
      const videoKey = videoRequestKey(boardWorkspaceId, id);
      if (videoToken !== null) {
        videoRequests.set(videoKey, { workspaceId: boardWorkspaceId, nodeId: id, token: videoToken });
        videoErrors.delete(boardWorkspaceId);
      }
      const recordVideoError = (error: StudioError | null): void => {
        if (videoRequests.get(videoKey)?.token !== videoToken) return;
        if (error === null) {
          if (videoErrors.get(boardWorkspaceId)?.nodeId === id) videoErrors.delete(boardWorkspaceId);
          return;
        }
        videoErrors.set(boardWorkspaceId, error);
        if (get().workspaceId === boardWorkspaceId && get().nodes.some((n) => n.id === id)) set({ lastError: error });
      };
      setBusy(id, true);
      set({ lastError: null });
      const context = upstreamContext(id, get().nodes, get().edges);
      const withContext = (p: string): string =>
        context !== '' ? `${context}\n\n${p}` : p;
      try {
        let videoParams = node.params;
        if (preset.kind === 'video' || preset.kind === 'sprite') {
          if (!useGenStore.getState().backendsLoaded) await useGenStore.getState().loadBackends();
          const backend = resolveVideoBackend(useGenStore.getState().backends, node.params,
            preset.kind === 'sprite' ? 'image2video' : 'text2video');
          if (backend) {
            const { aspect, resolution, durationSec } = videoBackendOptions(backend, node.params);
            videoParams = { ...node.params, aspect, resolution, durationSec };
          }
        }
        const { seq } = get();
        let consumed = 0;
        const nextId = (): string => {
          consumed += 1;
          return `v${seq + consumed - 1}`;
        };
        const versions: StudioVersion[] = [];
        // 截帧失败不该连坐视频:mp4 已经出片了(花了钱和几分钟),错误留到版本入库后再报,
        // 用户可以换参数点「重新截帧」而不必重新生成。
        let deferredError: StudioError | null = null;

        if (preset.kind === 'text') {
          const templateId = typeof node.params.template === 'string' ? node.params.template : 'free';
          const tpl = TEXT_TEMPLATES.find((t) => t.id === templateId) ?? TEXT_TEMPLATES[0];
          const text = tpl.prefix !== '' ? `${tpl.prefix}\n${withContext(prompt)}` : withContext(prompt);
          const workspaceId = get().workspaceId ?? useWorkspaceStore.getState().activeWorkspaceId;
          const abort = new AbortController();
          studioAborts.get(id)?.abort();
          studioAborts.set(id, abort);
          const sess = await postJson<{ session: { id: string } }>(
            '/api/forge/studio/sessions',
            { workspaceId, nodeId: id },
            abort.signal,
          );
          const sessionId = sess.session.id;
          const tools: StudioToolRecord[] = [];
          const locators: string[] = [];
          const patchRun = (partial: Partial<StudioNodeRun>): void => {
            set((s) => ({
              nodeRuns: {
                ...s.nodeRuns,
                [id]: {
                  sessionId,
                  runId: s.nodeRuns[id]?.runId ?? null,
                  phase: s.nodeRuns[id]?.phase ?? 'running',
                  tools: s.nodeRuns[id]?.tools ?? tools,
                  ...partial,
                },
              },
            }));
          };
          patchRun({ sessionId, runId: null, phase: 'retrieving', tools });
          const stream = subscribeStudioOnce(sessionId, (type, data) => {
            if (type === 'agent.started') {
              const runId = payloadStr(data, 'runId');
              if (runId) patchRun({ runId });
            }
            if (type === 'agent.tool.invoked') {
              const name = payloadStr(data, 'name') ?? 'tool';
              tools.push({ name, status: 'running' });
              const phase: StudioRunPhase =
                name.includes('search') || name === 'project_list' || name === 'resource_search'
                  ? 'retrieving'
                  : 'running';
              patchRun({ phase, tools: [...tools] });
            }
            if (type === 'agent.tool.completed' || type === 'agent.tool.failed' || type === 'agent.tool.denied') {
              const name = payloadStr(data, 'name');
              const rec = [...tools].reverse().find((t) => t.name === name && t.status === 'running') ?? tools[tools.length - 1];
              if (rec) {
                rec.status =
                  type === 'agent.tool.denied' ? 'denied' : type === 'agent.tool.failed' ? 'error' : 'ok';
                rec.summary = payloadStr(data, 'output') ?? payloadStr(data, 'error');
                const loc = rec.summary?.match(/forge:\/\/[^\s"']+/);
                if (loc) locators.push(loc[0]);
              }
              const phase: StudioRunPhase =
                type === 'agent.tool.completed' &&
                (name === 'resource_get' || name === 'resource_search' || name === 'project_list')
                  ? 'read'
                  : 'running';
              patchRun({ phase, tools: [...tools] });
            }
            if (type === 'permission.requested') {
              const permId = payloadStr(data, 'id');
              const tool = payloadStr(data, 'tool') ?? 'tool';
              if (permId) {
                const payload = (data as { payload?: Record<string, unknown> }).payload;
                set({
                  pendingPermission: {
                    nodeId: id,
                    id: permId,
                    tool,
                    targetProjectId: typeof payload?.targetProjectId === 'string' ? payload.targetProjectId : undefined,
                    argsSummary: typeof payload?.argsSummary === 'string' ? payload.argsSummary : undefined,
                  },
                });
                patchRun({ phase: 'pending-approval' });
              }
            }
            if (type === 'permission.resolved') {
              const permId = payloadStr(data, 'id');
              const pending = get().pendingPermission;
              if (pending && pending.id === permId) set({ pendingPermission: null });
              patchRun({ phase: 'running' });
            }
          });
          studioStreams.get(id)?.close();
          studioStreams.set(id, stream);
          try {
            const r = await postJson<{
              message?: { text?: string };
              run?: { id?: string; status?: string };
              error?: string;
            }>(
              `/api/forge/sessions/${encodeURIComponent(sessionId)}/ask:execute`,
              {
                userInput: text,
                mode: 'build',
                readonlyWorkspaceIds: get().readonlyWorkspaceIds,
                includeLibrary: get().includeLibrary,
              },
              abort.signal,
            );
            if (r.error || r.run?.status === 'failed' || r.run?.status === 'cancelled') {
              patchRun({ phase: 'failed', runId: r.run?.id ?? null });
              throw new ForgeApiError(
                r.run?.status === 'cancelled' ? 'CANCELLED' : 'AGENT_FAILED',
                r.error ?? '素材创作未完成',
              );
            }
            const out = r.message?.text?.trim() ?? '';
            if (out === '') {
              throw new ForgeApiError('EMPTY_OUTPUT', '模型没有返回文本');
            }
            versions.push({
              id: nextId(),
              createdAt: Date.now(),
              backendId: `studio:${r.run?.id ?? 'turn'}`,
              prompt,
              text: out,
              runId: r.run?.id,
              toolCalls: [...tools],
              resourceRefs: locators,
            });
          } finally {
            stream.close();
            studioStreams.delete(id);
            studioAborts.delete(id);
          }
        } else if (preset.kind === 'image') {
          const size = typeof node.params.size === 'number' ? node.params.size : 512;
          const n = typeof node.params.n === 'number' ? node.params.n : 1;
          const args: Record<string, unknown> = { prompt: withContext(prompt), size, n };
          if (typeof node.params.backend === 'string' && node.params.backend !== '') {
            args.backend = node.params.backend;
          }
          const r = await callGenTool<{ candidates: ImageCandidate[] }>('gen_image', args);
          for (const c of r.candidates) {
            versions.push({
              id: nextId(),
              createdAt: Date.now(),
              backendId: c.backendId,
              prompt,
              fileRef: c.imageFileRef,
              mime: 'image/png',
              seed: c.seed,
              dataUrl: c.dataUrl,
            });
          }
        } else if (preset.kind === 'model') {
          // 走 REST 而非 MCP:3D 是异步任务制,MCP 调用 10s 上限接不住。
          const r = await apiGenMesh({
            prompt: withContext(prompt),
            targetPolycount:
              typeof node.params.targetPolycount === 'number' ? node.params.targetPolycount : undefined,
            texture: typeof node.params.texture === 'boolean' ? node.params.texture : undefined,
            textureResolution:
              typeof node.params.textureResolution === 'string' ? node.params.textureResolution : undefined,
            backend:
              typeof node.params.backend === 'string' && node.params.backend !== '' ? node.params.backend : undefined,
          });
          for (const a of r.artifacts) {
            versions.push(mediaVersion(nextId(), r.backendId, prompt, a));
          }
        } else if (preset.kind === 'video') {
          const r = await apiGenVideo({
            workspaceId: videoWorkspaceId,
            prompt: withContext(prompt),
            imageRef: spriteRefImage(id, requestNodes, requestEdges, node.params),
            aspect: typeof videoParams.aspect === 'string' ? videoParams.aspect : undefined,
            resolution: typeof videoParams.resolution === 'string' ? videoParams.resolution : undefined,
            durationSec: typeof videoParams.durationSec === 'number' ? videoParams.durationSec : undefined,
            backend: typeof node.params.backend === 'string' && node.params.backend !== '' ? node.params.backend : undefined,
          });
          for (const a of r.artifacts) {
            versions.push(mediaVersion(nextId(), r.backendId, prompt, a));
          }
        } else if (preset.kind === 'sprite') {
          const ref = spriteRefImage(id, requestNodes, requestEdges, node.params);
          if (ref === undefined) {
            throw new ForgeApiError(
              'NO_REFERENCE_IMAGE',
              '角色动画需要一张参考图:把上游「原画」节点连过来,或在参数条里选一张贴图资产',
            );
          }
          const r = await apiGenVideo({
            workspaceId: videoWorkspaceId,
            prompt: `${withContext(prompt)}${CHARANIM_PROMPT_RULES}`,
            imageRef: ref,
            aspect: typeof videoParams.aspect === 'string' ? videoParams.aspect : undefined,
            resolution: typeof videoParams.resolution === 'string' ? videoParams.resolution : undefined,
            durationSec: typeof videoParams.durationSec === 'number' ? videoParams.durationSec : undefined,
            backend: typeof node.params.backend === 'string' && node.params.backend !== '' ? node.params.backend : undefined,
          });
          for (const a of r.artifacts) {
            versions.push({ ...mediaVersion(nextId(), r.backendId, prompt, a), videoFileRef: a.fileRef });
          }
          const first = versions[0];
          if (first?.videoFileRef !== undefined) {
            try {
              const f = await apiGenVideoFrames({
                workspaceId: videoWorkspaceId,
                videoFileRef: first.videoFileRef,
                ...frameParams(node.params),
              });
              first.atlas = f.atlas;
              first.boxes = f.boxes;
              first.fps = f.fps;
            } catch (err) {
              deferredError = {
                nodeId: id,
                code: err instanceof ForgeApiError ? err.code : 'ERROR',
                message: `视频已生成,但截帧失败:${(err as Error).message}`,
              };
            }
          }
        } else {
          const mode = node.params.mode === 'music' ? 'music' : 'tts';
          const r = await apiGenAudio({
            mode,
            prompt: mode === 'tts' ? prompt : withContext(prompt),
            voice: typeof node.params.voice === 'string' ? node.params.voice : undefined,
            format: typeof node.params.format === 'string' ? node.params.format : undefined,
            lyrics: typeof node.params.lyrics === 'string' && node.params.lyrics !== '' ? node.params.lyrics : undefined,
            instrumental: typeof node.params.instrumental === 'boolean' ? node.params.instrumental : undefined,
            backend: typeof node.params.backend === 'string' && node.params.backend !== '' ? node.params.backend : undefined,
          });
          for (const a of r.artifacts) {
            versions.push(mediaVersion(nextId(), r.backendId, prompt, a));
          }
        }

        if (videoToken !== null) {
          pushVideoVersions(boardWorkspaceId, id, versions);
          recordVideoError(deferredError);
        } else {
          commit({ seq: get().seq + consumed });
          pushVersions(id, versions);
          if (deferredError !== null) set({ lastError: deferredError });
        }
      } catch (err) {
        if (videoToken !== null) {
          recordVideoError({ nodeId: id, code: err instanceof ForgeApiError ? err.code : 'ERROR', message: (err as Error).message });
          return;
        }
        if ((err as Error).name === 'AbortError') {
          set((s) => ({
            nodeRuns: {
              ...s.nodeRuns,
              [id]: { ...(s.nodeRuns[id] ?? { sessionId: '', runId: null, tools: [] }), phase: 'failed', runId: s.nodeRuns[id]?.runId ?? null, tools: s.nodeRuns[id]?.tools ?? [] },
            },
          }));
          return;
        }
        const code = err instanceof ForgeApiError ? err.code : 'ERROR';
        set((s) => ({
          lastError: { nodeId: id, code, message: (err as Error).message },
          nodeRuns: {
            ...s.nodeRuns,
            [id]: { ...(s.nodeRuns[id] ?? { sessionId: '', runId: null, tools: [] }), phase: 'failed', runId: s.nodeRuns[id]?.runId ?? null, tools: s.nodeRuns[id]?.tools ?? [] },
          },
        }));
      } finally {
        if (videoToken === null) setBusy(id, false);
        else if (videoRequests.get(videoKey)?.token === videoToken) {
          videoRequests.delete(videoKey);
          if (get().workspaceId === boardWorkspaceId) setBusy(id, false);
        }
      }
    },

    cancelGenerate: async (id) => {
      videoRequests.delete(videoRequestKey(get().workspaceId, id));
      studioAborts.get(id)?.abort();
      studioStreams.get(id)?.close();
      studioAborts.delete(id);
      studioStreams.delete(id);
      const runId = get().nodeRuns[id]?.runId;
      if (runId) {
        try {
          await apiPost(`/api/forge/runs/${encodeURIComponent(runId)}/cancel`, {});
        } catch {
          /* 取消失败不挡本地收尾 */
        }
      }
      set((s) => ({
        busyIds: s.busyIds.filter((x) => x !== id),
        pendingPermission: s.pendingPermission?.nodeId === id ? null : s.pendingPermission,
        nodeRuns: { ...s.nodeRuns, [id]: { ...(s.nodeRuns[id] ?? { sessionId: '', tools: [] }), runId: runId ?? null, phase: 'failed', tools: s.nodeRuns[id]?.tools ?? [] } },
      }));
    },

    resolvePermission: async (allow) => {
      const pending = get().pendingPermission;
      if (!pending) return;
      try {
        await apiPost(
          `/api/forge/permissions/${encodeURIComponent(pending.id)}/${allow ? 'approve' : 'deny'}`,
          {},
        );
      } catch (err) {
        const code = err instanceof ForgeApiError ? err.code : 'ERROR';
        set({ lastError: { nodeId: pending.nodeId, code, message: (err as Error).message } });
      }
    },

    bindWorkspace: (workspaceId) => {
      if (get().workspaceId === workspaceId) return;
      const cur = get();
      persistStudio(
        {
          seq: cur.seq,
          nodes: cur.nodes,
          edges: cur.edges,
          readonlyWorkspaceIds: cur.readonlyWorkspaceIds,
          includeLibrary: cur.includeLibrary,
        },
        cur.workspaceId,
      );
      const loaded = loadStudio(workspaceId);
      const videoError = videoErrors.get(workspaceId);
      set({
        ...loaded,
        workspaceId,
        openNodeId: null,
        selectedNodeId: null,
        pendingRenameId: null,
        pendingEdgeId: null,
        busyIds: [...videoRequests.values()].filter((r) => r.workspaceId === workspaceId
          && loaded.nodes.some((n) => n.id === r.nodeId)).map((r) => r.nodeId),
        lastError: videoError && loaded.nodes.some((n) => n.id === videoError.nodeId) ? videoError : null,
        nodeRuns: {},
        pendingPermission: null,
      });
    },

    setReadonlyWorkspaceIds: (ids) => commit({ readonlyWorkspaceIds: [...new Set(ids)] }),

    setIncludeLibrary: (on) => commit({ includeLibrary: on }),

    acceptVersion: async (nodeId, versionId) => {
      const node = get().nodes.find((n) => n.id === nodeId);
      if (!node) return;
      const preset = presetOf(node.preset);
      const version = node.versions.find((v) => v.id === versionId);
      if (!preset || !version) return;
      if (!canAccept(preset, version)) return;
      const dest = preset.destFolder ?? 'Textures';
      const name = slugName(version.prompt !== '' ? version.prompt : node.name, version.seed ?? 0);
      set({ lastError: null });
      try {
        if (preset.kind === 'sprite') {
          const atlas = version.atlas;
          const boxes = version.boxes;
          if (atlas === undefined || boxes === undefined || boxes.length === 0) return;
          // 图集先以贴图身份入库(拿到 GUID),.rxsprite 才有东西可引用。
          const tex = await callGenTool<{ assetPath: string; guid: string }>('gen_accept', {
            imageFileRef: atlas.fileRef,
            destFolder: dest,
            name,
            origin: 'gen-video',
          });
          const clipName =
            (typeof node.params.clipName === 'string' ? node.params.clipName.trim() : '') || 'clip';
          const frameNames = boxes.map((_, i) =>
            boxes.length > 10 ? `frame_${String(i).padStart(2, '0')}` : `frame_${i}`,
          );
          const sprite = await callAssetTool<{ assetPath?: string; error?: string; message?: string }>(
            'sprite_create',
            {
              name,
              texture: tex.guid,
              frames: Object.fromEntries(frameNames.map((fn, i) => [fn, { bbox: boxes[i] }])),
              clips: {
                [clipName]: {
                  frames: frameNames,
                  fps: version.fps ?? 8,
                  loop: true,
                  onFinish: 'hold',
                },
              },
            },
          );
          if (sprite.error !== undefined || sprite.assetPath === undefined) {
            // 贴图已经入库了,如实说清「精灵没建成」,别把已发生的事一起报成失败。
            throw new ForgeApiError(
              sprite.error ?? 'SPRITE_CREATE_FAILED',
              `图集已入库 ${tex.assetPath},但 .rxsprite 创建失败:${sprite.message ?? sprite.error ?? '空响应'}`,
            );
          }
          patchNode(nodeId, (n) => ({
            ...n,
            versions: n.versions.map((v) =>
              v.id === versionId
                ? { ...v, assetPath: tex.assetPath, guid: tex.guid, spritePath: sprite.assetPath }
                : v,
            ),
          }));
          return;
        }
        if (version.fileRef === undefined) return;
        const r =
          preset.kind === 'image'
            ? await callGenTool<{ assetPath: string; guid: string }>('gen_accept', {
                imageFileRef: version.fileRef,
                destFolder: dest,
                name,
              })
            : await callModelGenTool<{ assetPath: string; guid: string }>('gen_accept', {
                meshFileRef: version.fileRef,
                destFolder: dest,
                name,
              });
        patchNode(nodeId, (n) => ({
          ...n,
          versions: n.versions.map((v) =>
            v.id === versionId ? { ...v, assetPath: r.assetPath, guid: r.guid } : v,
          ),
        }));
      } catch (err) {
        const code = err instanceof ForgeApiError ? err.code : 'ERROR';
        set({ lastError: { nodeId, code, message: (err as Error).message } });
      }
    },

    resliceVersion: async (nodeId, versionId) => {
      const videoWorkspaceId = get().workspaceId ?? useWorkspaceStore.getState().activeWorkspaceId ?? readActiveWorkspaceId();
      const node = get().nodes.find((n) => n.id === nodeId);
      const version = node?.versions.find((v) => v.id === versionId);
      if (!node || !version || version.videoFileRef === undefined) return;
      if (get().busyIds.includes(nodeId)) return;
      setBusy(nodeId, true);
      set({ lastError: null });
      try {
        const f = await apiGenVideoFrames({
          workspaceId: videoWorkspaceId,
          videoFileRef: version.videoFileRef,
          ...frameParams(node.params),
        });
        patchNode(nodeId, (n) => ({
          ...n,
          versions: n.versions.map((v) =>
            v.id === versionId ? { ...v, atlas: f.atlas, boxes: f.boxes, fps: f.fps } : v,
          ),
        }));
      } catch (err) {
        const code = err instanceof ForgeApiError ? err.code : 'ERROR';
        set({ lastError: { nodeId, code, message: (err as Error).message } });
      } finally {
        setBusy(nodeId, false);
      }
    },

    recordBlenderVersion: (nodeId, job) => {
      if (!job.published) return;
      const node = get().nodes.find((n) => n.id === nodeId);
      if (!node || node.versions.some((v) => v.blenderJobId === job.id && v.blenderRevision === job.revision)) return;
      const version: StudioVersion = {
        id: `blender-${job.id}-${job.revision}`, createdAt: Date.now(), backendId: 'blender', prompt: job.prompt,
        assetPath: job.published.modelPath, guid: job.published.modelGuid, prefabGuid: job.published.prefabGuid,
        blenderJobId: job.id, blenderWorkspaceId: job.workspaceId, blenderRevision: job.revision,
        animationClips: job.published.clips ?? [],
      };
      patchNode(nodeId, (n) => ({ ...n, versions: [...n.versions, version], currentVersionId: version.id }));
      void useAssetStore.getState().load();
    },

    openInSpriteEditor: (nodeId, versionId) => {
      const version = get()
        .nodes.find((n) => n.id === nodeId)
        ?.versions.find((v) => v.id === versionId);
      const path = version?.spritePath;
      if (path === undefined) return;
      void useSpriteStore.getState().openSprite(path);
      useWorkbenchStore.getState().openTab('sprite-editor');
    },
  };
});

function mediaVersion(
  id: string,
  backendId: string,
  prompt: string,
  a: MediaArtifact,
): StudioVersion {
  const thumb = a.meta?.thumbnailUrl;
  const previews = (a.previews ?? [])
    .filter((p) => p.dataUrl !== '')
    .map((p) => ({ label: p.label, dataUrl: p.dataUrl }));
  return {
    id,
    createdAt: Date.now(),
    backendId,
    prompt,
    fileRef: a.fileRef,
    mime: a.mime,
    dataUrl: a.dataUrl,
    previews: previews.length > 0 ? previews : undefined,
    thumbnailUrl: typeof thumb === 'string' && thumb !== '' ? thumb : undefined,
  };
}

/** 节点当前版本(空态 = undefined)。 */
export function currentVersion(node: StudioNode): StudioVersion | undefined {
  if (node.currentVersionId === null) return undefined;
  return node.versions.find((v) => v.id === node.currentVersionId);
}

/**
 * 该版本能否入库(gen_accept):图像/模型看落盘产物,角色动画看截好的图集
 * (光有 mp4 不算——视频不是引擎资产,能入库的是那张图集),且都尚未入库过。
 * 版本卡上的「入库」按钮与右键菜单项共用这一份判定,免得两处规则各走一套。
 */
export function canAccept(
  preset: StudioPresetDef | undefined,
  version: StudioVersion | undefined,
): boolean {
  if (preset === undefined || version === undefined) return false;
  if (version.guid !== undefined) return false;
  if (preset.kind === 'sprite') {
    return version.atlas !== undefined && (version.boxes?.length ?? 0) > 0;
  }
  if (preset.kind !== 'image' && preset.kind !== 'model') return false;
  return version.fileRef !== undefined;
}

/** 该版本能否重新截帧:有源视频即可(图集是否已切好、已入库都不妨碍换参数重切)。 */
export function canReslice(
  preset: StudioPresetDef | undefined,
  version: StudioVersion | undefined,
): boolean {
  return preset?.kind === 'sprite' && version?.videoFileRef !== undefined;
}

/** 节点状态点(主画布创作卡):empty 草稿 / busy 生成中 / done 有产物 / accepted 已入库。 */
export type StudioNodeStatus = 'empty' | 'busy' | 'done' | 'accepted';

export function nodeStatus(node: StudioNode, busyIds: string[]): StudioNodeStatus {
  if (busyIds.includes(node.id)) return 'busy';
  const cur = currentVersion(node);
  if (cur === undefined) return 'empty';
  if (cur.guid !== undefined) return 'accepted';
  return 'done';
}
