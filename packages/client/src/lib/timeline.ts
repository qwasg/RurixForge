/**
 * F7 wave.4 build_timeline TS 移植(参考 ui/chat.rs 逐规则;G-F7-4)。
 *
 * 规则(与参考逐条对齐):
 * - 连续普通 tool 块合并为 activity segment;milestone 断段(text/reasoning/subagent 块,
 *   或 tool 名 === "task" | "write_todos")。
 * - 段统计:Edit=写改删类按目标文件去重(args path/file/filePath/scenePath/assetPath/
 *   destFolder 取一)、Explore=读取浏览类按路径去重、Search 计数、Command 计数、Other 计数;
 *   短语「编辑 3 个文件，探索 2 个文件，1 次搜索，执行 1 条命令，2 次其他操作」(全空「工作中」)。
 * - +added/-removed 估算:args 含 old_str/new_str 或 oldContent/newContent → 行级 LCS 最小
 *   增删数(与参考 Myers 同计数语义);仅 content/newContent → 全行计 added;都没有 → 0(不显示)。
 * - 「· n 失败」;段尾仍运行 →「{汇总} · 正在{动词}…」。
 * - 最终回答永不折叠:text 块恒为独立 TimelineItem(kind=block),由渲染层判 final 全量 markdown。
 *
 * 本仓差异留痕:
 * ① 动词表按本仓工具面重写(见 TOOL_META,参考表是 read_file/str_replace_edit 等 IDE 工具);
 * ② 参考对一切 mcp__server__tool 直接显示「server / tool」——本仓工具全走 MCP 前缀,
 *    故先剥前缀查动词表,仅未知 MCP 名才回退「server / tool」;
 * ③ 参考 apply_patch 的 *** Add/Update/Delete File 头解析不移植(本仓无该工具);
 * ④ 参考段短语「探索」= read_file/list_dir,本仓 Explore 为实体/场景/资产读取类,措辞保持参考。
 */

// ---- 块模型(chatStore 与渲染层共用) ----

export type BlockStatus = 'running' | 'done' | 'error';

export type ChatBlock =
  | { kind: 'text'; text: string; final: boolean }
  | { kind: 'reasoning'; text: string }
  | {
      kind: 'tool';
      /** = 事件 payload.toolCallId。 */
      toolCallId: string;
      name: string;
      /** pretty JSON 串(参考 args 形态);无 args 载荷时空串。 */
      args: string;
      /** completed → true;failed/denied → false 且 error 填。 */
      ok?: boolean;
      error?: string;
      durationMs?: number;
      /** 成功工具截断后的输出(本仓 completed 带 output/outputPreview)。 */
      result?: string;
      status?: BlockStatus;
      /** mcp__server__tool 形态剥出;否则 null。 */
      mcp: [string, string] | null;
    }
  | {
      kind: 'subagent';
      id: string;
      label: string;
      status: BlockStatus;
      summary?: string;
      prompt?: string;
      parentToolCallId?: string;
      work: ChatBlock[];
    };

/** tool 块状态派生(显式 status 优先;否则 ok 未回填 = running)。 */
export function toolStatus(b: Extract<ChatBlock, { kind: 'tool' }>): BlockStatus {
  if (b.status) return b.status;
  if (b.ok === true) return 'done';
  if (b.error !== undefined || b.ok === false) return 'error';
  return 'running';
}

// ---- 名称工具 ----

/** mcp__server__tool → [server, tool];否则 null(参考 strip_prefix+split_once 语义)。 */
export function mcpOf(name: string): [string, string] | null {
  if (!name.startsWith('mcp__')) return null;
  const rest = name.slice('mcp__'.length);
  const idx = rest.indexOf('__');
  if (idx < 0) return null;
  return [rest.slice(0, idx), rest.slice(idx + 2)];
}

/** 剥 MCP 前缀后的工具本名(非 MCP 名原样)。 */
export function bareName(name: string): string {
  const m = mcpOf(name);
  return m ? m[1] : name;
}

// ---- 本仓动词表(留痕:按 KNOWN_TOOLS 全量核对重写;label=单次行首标签,prefix/suffix=聚合短语) ----

interface ToolMeta {
  label: string;
  groupPrefix: string;
  groupSuffix: string;
}

const TOOL_META: Record<string, ToolMeta> = {
  // engine-scene:实体/组件/变换
  entity_create: { label: '创建实体', groupPrefix: '创建', groupSuffix: '个实体' },
  entity_destroy: { label: '删除实体', groupPrefix: '删除', groupSuffix: '个实体' },
  entity_rename: { label: '重命名', groupPrefix: '重命名', groupSuffix: '个实体' },
  entity_list: { label: '列出实体', groupPrefix: '列出实体', groupSuffix: '次' },
  entity_get: { label: '读取实体', groupPrefix: '读取实体', groupSuffix: '次' },
  entity_batch_apply: { label: '批量应用', groupPrefix: '批量应用', groupSuffix: '次' },
  transform_set: { label: '设置变换', groupPrefix: '设置变换', groupSuffix: '次' },
  transform_get: { label: '读取变换', groupPrefix: '读取变换', groupSuffix: '次' },
  transform_batch_set: { label: '批量设置变换', groupPrefix: '批量设置变换', groupSuffix: '次' },
  component_add: { label: '添加组件', groupPrefix: '添加组件', groupSuffix: '次' },
  component_remove: { label: '移除组件', groupPrefix: '移除组件', groupSuffix: '次' },
  component_set: { label: '设置组件', groupPrefix: '设置组件', groupSuffix: '次' },
  component_get: { label: '读取组件', groupPrefix: '读取组件', groupSuffix: '次' },
  component_list_types: { label: '列出组件类型', groupPrefix: '列出组件类型', groupSuffix: '次' },
  // engine-scene:场景/编辑
  scene_save: { label: '保存场景', groupPrefix: '保存场景', groupSuffix: '次' },
  scene_load: { label: '加载场景', groupPrefix: '加载场景', groupSuffix: '次' },
  scene_new: { label: '新建场景', groupPrefix: '新建场景', groupSuffix: '次' },
  scene_summary: { label: '场景摘要', groupPrefix: '场景摘要', groupSuffix: '次' },
  scene_graph_dump: { label: '导出场景图', groupPrefix: '导出场景图', groupSuffix: '次' },
  scene_diff: { label: '场景对比', groupPrefix: '场景对比', groupSuffix: '次' },
  scene_checkpoint: { label: '场景检查点', groupPrefix: '场景检查点', groupSuffix: '次' },
  scene_rollback: { label: '场景回滚', groupPrefix: '场景回滚', groupSuffix: '次' },
  edit_undo: { label: '撤销', groupPrefix: '撤销', groupSuffix: '次' },
  edit_redo: { label: '重做', groupPrefix: '重做', groupSuffix: '次' },
  host_events: { label: '读取事件', groupPrefix: '读取事件', groupSuffix: '次' },
  host_events_drain: { label: '排空事件', groupPrefix: '排空事件', groupSuffix: '次' },
  host_ping: { label: '探测宿主', groupPrefix: '探测宿主', groupSuffix: '次' },
  render_once: { label: '渲染一帧', groupPrefix: '渲染一帧', groupSuffix: '次' },
  // engine-scene:播放/视口
  play_enter: { label: '进入播放', groupPrefix: '进入播放', groupSuffix: '次' },
  play_exit: { label: '退出播放', groupPrefix: '退出播放', groupSuffix: '次' },
  play_pause: { label: '暂停', groupPrefix: '暂停', groupSuffix: '次' },
  play_resume: { label: '继续', groupPrefix: '继续', groupSuffix: '次' },
  play_step: { label: '步进', groupPrefix: '步进', groupSuffix: '次' },
  play_state: { label: '播放状态', groupPrefix: '播放状态', groupSuffix: '次' },
  logic_inject_input: { label: '注入输入', groupPrefix: '注入输入', groupSuffix: '次' },
  // code-forge / gen-image / gen-model(任务书具名项)
  graph_get: { label: '读取节点图', groupPrefix: '读取节点图', groupSuffix: '次' },
  graph_validate: { label: '校验图', groupPrefix: '校验图', groupSuffix: '次' },
  graph_create: { label: '写入图', groupPrefix: '写入图', groupSuffix: '次' },
  gen_image: { label: '生成贴图', groupPrefix: '生成贴图', groupSuffix: '次' },
  gen_accept: { label: '入库', groupPrefix: '入库', groupSuffix: '次' },
  gen_mesh: { label: '生成网格', groupPrefix: '生成网格', groupSuffix: '次' },
  // 合成工具(F3 multitask)
  'swarm.execute': { label: '集群执行', groupPrefix: '集群执行', groupSuffix: '次' },
  // 运行时原生工具(对齐参考仓 IDE 动词)
  read_file: { label: '读取', groupPrefix: '读取', groupSuffix: '个文件' },
  list_dir: { label: '列出目录', groupPrefix: '列出', groupSuffix: '个目录' },
  glob: { label: '查找文件', groupPrefix: '查找', groupSuffix: '次' },
  grep: { label: '搜索', groupPrefix: '搜索', groupSuffix: '次' },
  write_file: { label: '写入', groupPrefix: '写入', groupSuffix: '个文件' },
  str_replace_edit: { label: '替换', groupPrefix: '替换', groupSuffix: '个文件' },
  apply_patch: { label: '补丁', groupPrefix: '补丁', groupSuffix: '个文件' },
  todo_write: { label: '待办', groupPrefix: '待办', groupSuffix: '次' },
  write_todos: { label: '待办', groupPrefix: '待办', groupSuffix: '次' },
  plan_write: { label: '计划', groupPrefix: '计划', groupSuffix: '次' },
  todo_update: { label: '更新待办', groupPrefix: '更新待办', groupSuffix: '次' },
  task: { label: '委派', groupPrefix: '委派', groupSuffix: '次' },
};

/** 通配族(viewport_*=视口操作 / asset_*=资产操作,任务书具名)。 */
const WILDCARD_META: Array<[string, ToolMeta]> = [
  ['viewport_', { label: '视口操作', groupPrefix: '视口操作', groupSuffix: '次' }],
  ['asset_', { label: '资产操作', groupPrefix: '资产操作', groupSuffix: '次' }],
];

export function toolMetaOf(name: string): ToolMeta | null {
  const bare = bareName(name);
  const exact = TOOL_META[bare];
  if (exact) return exact;
  for (const [prefix, meta] of WILDCARD_META) {
    if (bare.startsWith(prefix)) return meta;
  }
  return null;
}

/** 单次行首标签(参考 tool_visual;未知 MCP 名 →「server / tool」,其余未知 → 原名)。 */
export function toolVisual(name: string): string {
  const meta = toolMetaOf(name);
  if (meta) return meta.label;
  const m = mcpOf(name);
  if (m) return `${m[0]} / ${m[1]}`;
  return name;
}

/** 展开段内同类并组 key(参考 tool_group_key)。 */
export function toolGroupKey(name: string): string {
  const meta = toolMetaOf(name);
  if (meta) return `kind:${meta.label}`;
  const m = mcpOf(name);
  if (m) return `mcp:${m[0]}/${m[1]}`;
  return `name:${name}`;
}

/** 「创建 3 个实体」式聚合短语(参考 group_phrase)。 */
export function groupPhrase(name: string, n: number): string {
  const meta = toolMetaOf(name);
  if (meta) return `${meta.groupPrefix} ${n} ${meta.groupSuffix}`;
  const m = mcpOf(name);
  if (m) return `${m[0]} / ${m[1]} ×${n}`;
  return `${name} ×${n}`;
}

// ---- 段化(参考 build_timeline / is_milestone_block / group_segment_items) ----

export type TimelineItem =
  | { type: 'block'; index: number }
  | { type: 'activity'; indices: number[] };

export function isMilestoneBlock(block: ChatBlock): boolean {
  if (block.kind !== 'tool') return true; // text / reasoning / subagent
  const bare = bareName(block.name);
  return bare === 'write_todos' || bare === 'todo_write' || bare === 'plan_write' || bare === 'task';
}

export function buildTimeline(blocks: ChatBlock[]): TimelineItem[] {
  const items: TimelineItem[] = [];
  let segment: number[] = [];
  blocks.forEach((block, i) => {
    if (isMilestoneBlock(block)) {
      if (segment.length > 0) {
        items.push({ type: 'activity', indices: segment });
        segment = [];
      }
      items.push({ type: 'block', index: i });
    } else {
      segment.push(i);
    }
  });
  if (segment.length > 0) items.push({ type: 'activity', indices: segment });
  return items;
}

/** 段内同类并组:连续且同 groupKey 的非 running 工具成一个 run(参考 group_segment_items)。 */
export function groupSegmentItems(blocks: ChatBlock[], indices: number[]): number[][] {
  const runs: number[][] = [];
  let i = 0;
  while (i < indices.length) {
    const bi = indices[i];
    const b = blocks[bi];
    if (b.kind === 'tool' && toolStatus(b) !== 'running') {
      const gk = toolGroupKey(b.name);
      let j = i + 1;
      while (j < indices.length) {
        const b2 = blocks[indices[j]];
        if (b2.kind === 'tool' && toolStatus(b2) !== 'running' && toolGroupKey(b2.name) === gk) {
          j += 1;
        } else {
          break;
        }
      }
      runs.push(indices.slice(i, j));
      i = j;
      continue;
    }
    runs.push([bi]);
    i += 1;
  }
  return runs;
}

// ---- 段统计(参考 tool_category / segment_stats / segment_phrase) ----

export type ToolCategory = 'edit' | 'explore' | 'search' | 'command' | 'other';

/** 本仓分类(留痕:按 KNOWN_TOOLS 判定;参考为 IDE 工具面)。 */
const EDIT_TOOLS = new Set([
  'entity_create', 'entity_destroy', 'entity_rename', 'entity_batch_apply',
  'transform_set', 'transform_batch_set',
  'component_add', 'component_remove', 'component_set',
  'scene_save', 'scene_load', 'scene_new', 'scene_checkpoint', 'scene_rollback',
  'edit_undo', 'edit_redo', 'logic_inject_input',
  'viewport_set_camera', 'viewport_share_open', 'viewport_share_close',
  'asset_import', 'asset_delete', 'asset_move', 'asset_fix_redirectors', 'asset_reimport',
  'asset_set_meta', 'material_create', 'texture_process',
  'rx_fmt', 'graph_create', 'code_structured_edit',
  'gen_image', 'gen_texture_set', 'gen_accept', 'gen_variations', 'gen_mesh', 'gen_mesh_refine',
  'write_file', 'str_replace_edit', 'apply_patch',
]);
const EXPLORE_TOOLS = new Set([
  'entity_list', 'entity_get', 'transform_get', 'component_get', 'component_list_types',
  'scene_summary', 'scene_graph_dump', 'scene_diff', 'host_events', 'host_events_drain',
  'host_ping', 'render_once', 'viewport_frame', 'viewport_pick', 'viewport_get_camera',
  'asset_list', 'asset_get_meta', 'asset_build_status', 'asset_refs', 'asset_thumbnail',
  'mesh_inspect', 'asset_cleanup_scan', 'graph_get', 'graph_validate',
  'rx_check', 'gen_backends_list', 'play_state',
  'read_file', 'list_dir', 'glob',
]);
const SEARCH_TOOLS = new Set(['code_symbol_search', 'code_references', 'grep']);
const COMMAND_TOOLS = new Set([
  'rx_build', 'rx_run', 'rx_test', 'swarm.execute',
  'play_enter', 'play_exit', 'play_pause', 'play_resume', 'play_step',
]);

export function toolCategory(name: string): ToolCategory {
  const bare = bareName(name);
  if (EDIT_TOOLS.has(bare)) return 'edit';
  if (EXPLORE_TOOLS.has(bare)) return 'explore';
  if (SEARCH_TOOLS.has(bare)) return 'search';
  if (COMMAND_TOOLS.has(bare)) return 'command';
  return 'other';
}

export interface SegmentStats {
  edits: number;
  explores: number;
  searches: number;
  commands: number;
  others: number;
  added: number;
  removed: number;
  errors: number;
}

/** 目标去重取值键序(任务书:path/file/filePath/scenePath/assetPath/destFolder 取一)。 */
const TARGET_KEYS = ['path', 'file', 'filePath', 'scenePath', 'assetPath', 'destFolder'] as const;

function parseArgs(args: string): Record<string, unknown> | null {
  if (args === '') return null;
  try {
    const v: unknown = JSON.parse(args);
    return v !== null && typeof v === 'object' && !Array.isArray(v)
      ? (v as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function jsonArgStr(args: string, key: string): string | null {
  const obj = parseArgs(args);
  const v = obj?.[key];
  if (typeof v !== 'string') return null;
  const t = v.trim();
  return t === '' ? null : t;
}

/** Edit 类目标文件(无目标键 → 空调用方按 #{index} 占位去重,参考同口径)。 */
export function editTargetFiles(args: string): string[] {
  for (const k of TARGET_KEYS) {
    const v = jsonArgStr(args, k);
    if (v) return [v];
  }
  return [];
}

/** 行级 LCS 最小增删(与参考 compute_line_diff[Myers] 的 insert/delete 计数同语义)。 */
export function diffCounts(oldText: string, newText: string): { added: number; removed: number } {
  const a = oldText === '' ? [] : oldText.split('\n');
  const b = newText === '' ? [] : newText.split('\n');
  const n = a.length;
  const m = b.length;
  if (n === 0) return { added: m, removed: 0 };
  if (m === 0) return { added: 0, removed: n };
  // LCS 长度(滚动行,O(n·m) 时间 O(m) 空间;args 体量足够)
  let prev = new Array<number>(m + 1).fill(0);
  for (let i = 1; i <= n; i += 1) {
    const cur = new Array<number>(m + 1).fill(0);
    for (let j = 1; j <= m; j += 1) {
      cur[j] = a[i - 1] === b[j - 1] ? prev[j - 1] + 1 : Math.max(prev[j], cur[j - 1]);
    }
    prev = cur;
  }
  const lcs = prev[m];
  return { added: m - lcs, removed: n - lcs };
}

/** (added, removed) 估算(任务书口径:含 content/old_str/new_str/newContent/oldContent 时估算,否则 0)。 */
export function toolDiffStats(args: string): { added: number; removed: number } {
  const obj = parseArgs(args);
  if (!obj) return { added: 0, removed: 0 };
  const pick = (k: string): string => {
    const v = obj[k];
    return typeof v === 'string' ? v : '';
  };
  const patch = pick('patch');
  if (patch !== '') return patchDiffStats(patch);
  const oldS = pick('old_str') || pick('oldContent') || pick('old_string');
  const newS = pick('new_str') || pick('newContent') || pick('new_string');
  if (oldS !== '' || newS !== '') return diffCounts(oldS, newS);
  const content = pick('content') || pick('contents') || pick('text');
  if (content !== '') return { added: content.split('\n').length, removed: 0 };
  return { added: 0, removed: 0 };
}

/** apply_patch `*** Add/Update/Delete File` 头：按 +/- 行计增删。 */
export function patchDiffStats(patch: string): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const raw of patch.split('\n')) {
    if (raw.startsWith('*** ') || raw.startsWith('@@')) continue;
    if (raw.startsWith('+')) added += 1;
    else if (raw.startsWith('-')) removed += 1;
  }
  return { added, removed };
}

export function segmentStats(blocks: ChatBlock[], indices: number[]): SegmentStats {
  const stats: SegmentStats = {
    edits: 0,
    explores: 0,
    searches: 0,
    commands: 0,
    others: 0,
    added: 0,
    removed: 0,
    errors: 0,
  };
  const editFiles = new Set<string>();
  const exploreTargets = new Set<string>();
  for (const bi of indices) {
    const b = blocks[bi];
    if (b.kind !== 'tool') continue;
    if (toolStatus(b) === 'error') stats.errors += 1;
    switch (toolCategory(b.name)) {
      case 'edit': {
        const files = editTargetFiles(b.args);
        if (files.length === 0) editFiles.add(`#${bi}`);
        else files.forEach((f) => editFiles.add(f));
        const d = toolDiffStats(b.args);
        stats.added += d.added;
        stats.removed += d.removed;
        break;
      }
      case 'explore': {
        let target: string | null = null;
        for (const k of TARGET_KEYS) {
          target = jsonArgStr(b.args, k);
          if (target) break;
        }
        exploreTargets.add(target ?? `#${bi}`);
        break;
      }
      case 'search':
        stats.searches += 1;
        break;
      case 'command':
        stats.commands += 1;
        break;
      case 'other':
        stats.others += 1;
        break;
    }
  }
  stats.edits = editFiles.size;
  stats.explores = exploreTargets.size;
  return stats;
}

/** 「编辑 3 个文件，探索 2 个文件，1 次搜索，执行 1 条命令，2 次其他操作」(参考 segment_phrase 逐字)。 */
export function segmentPhrase(stats: SegmentStats): string {
  const parts: string[] = [];
  if (stats.edits > 0) parts.push(`编辑 ${stats.edits} 个文件`);
  if (stats.explores > 0) parts.push(`探索 ${stats.explores} 个文件`);
  if (stats.searches > 0) parts.push(`${stats.searches} 次搜索`);
  if (stats.commands > 0) parts.push(`执行 ${stats.commands} 条命令`);
  if (stats.others > 0) parts.push(`${stats.others} 次其他操作`);
  return parts.length === 0 ? '工作中' : parts.join('，');
}

/** 段尾运行中标签(参考:「正在{动词}…」,取倒数第一个 running 工具)。 */
export function runningLabel(blocks: ChatBlock[], indices: number[]): string | null {
  for (let k = indices.length - 1; k >= 0; k -= 1) {
    const b = blocks[indices[k]];
    if (b.kind === 'tool' && toolStatus(b) === 'running') {
      return `正在${toolVisual(b.name)}…`;
    }
  }
  return null;
}

// ---- 行级摘要(参考 arg_summary / tool_summary / reasoning_summary 等) ----

/** 截断(按 Unicode code point,同参考 chars().take)。 */
export function ellipsize(s: string, max: number): string {
  const chars = [...s];
  if (chars.length <= max) return s;
  return `${chars.slice(0, max).join('')}…`;
}

export function firstLine(s: string): string {
  for (const l of s.split('\n')) {
    const t = l.trim();
    if (t !== '') return t;
  }
  return '';
}

export function compactText(s: string): string {
  return s
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l !== '')
    .join(' ');
}

/** arg_summary:键序截 48(任务书键序;参考无 scenePath/assetPath/destFolder/id/entityId,本仓扩)。 */
const ARG_KEYS = [
  'path', 'file', 'filePath', 'scenePath', 'assetPath', 'destFolder',
  'command', 'query', 'pattern', 'url', 'dir', 'skill', 'prompt', 'name', 'id', 'entityId',
] as const;

export function argSummary(args: string): string | null {
  const obj = parseArgs(args);
  if (!obj) return null;
  for (const k of ARG_KEYS) {
    const v = obj[k];
    if (typeof v === 'string' && v.trim() !== '') return ellipsize(v.trim(), 48);
    if (typeof v === 'number') return String(v);
  }
  return null;
}

/**
 * 单行工具摘要(参考 tool_summary):arg + 状态后缀。
 * 留痕:参考 completed 追加结果首行 48 摘要;本仓工具事件无 result 载荷(仅 ok/durationMs),
 * 如实不显示结果摘要。
 */
export function toolSummary(b: Extract<ChatBlock, { kind: 'tool' }>): string {
  const parts: string[] = [];
  const a = argSummary(b.args);
  if (a) parts.push(a);
  const st = toolStatus(b);
  if (st === 'running') parts.push('运行中…');
  else if (st === 'error') {
    const head = firstLine(b.error ?? '');
    parts.push(head === '' ? '失败' : `失败：${ellipsize(head, 48)}`);
  } else if (st === 'done' && b.result) {
    const head = firstLine(b.result);
    if (head !== '') parts.push(ellipsize(head, 48));
  }
  return parts.join(' · ');
}

/** 思考折叠行摘要(参考 reasoning_summary:「{摘录} · {N} 字」)。 */
export function reasoningSummary(text: string): string {
  const head = compactText(text);
  const chars = [...text].length;
  if (head === '') return `${chars} 字`;
  return `${ellipsize(head, 200)} · ${chars} 字`;
}

/** write_todos 里程碑标签(参考 todo_milestone_label;本仓无该工具,组件就绪)。 */
export function todoMilestoneLabel(args: string): string {
  const obj = parseArgs(args);
  const todos = obj?.todos;
  if (!Array.isArray(todos)) return '更新待办';
  const content = (item: unknown): string | null => {
    const rec = item as { content?: unknown; title?: unknown } | null;
    const raw = typeof rec?.content === 'string' ? rec.content : typeof rec?.title === 'string' ? rec.title : null;
    if (raw === null) return null;
    const t = raw.trim();
    return t === '' ? null : t;
  };
  const inProgress = todos.find(
    (t) => (t as { status?: unknown })?.status === 'in_progress',
  );
  const headline = content(inProgress) ?? todos.map(content).find((c) => c !== null) ?? null;
  return headline ? ellipsize(headline, 60) : '更新待办';
}

/** 子代理首行(参考 subagent_dispatch_summary)。 */
export function subagentDispatchSummary(label: string, prompt: string): string {
  const l = label.trim();
  if (l !== '') return ellipsize(l, 84);
  const p = prompt.trim();
  return p === '' ? '子 Agent 任务' : ellipsize(p, 84);
}

/** 把子代理 work 块压成一行摘录。 */
export function workLine(block: ChatBlock): string {
  if (block.kind === 'text') return block.text;
  if (block.kind === 'reasoning') return block.text;
  if (block.kind === 'tool') return toolSummary(block) || toolVisual(block.name);
  if (block.kind === 'subagent') return block.label;
  return '';
}

/** 子代理进展行(参考 subagent_live_summary;work 为嵌套 ChatBlock[])。 */
export function subagentLiveSummary(
  summary: string | undefined,
  work: ChatBlock[],
  status: BlockStatus,
): string {
  const trimmed = (summary ?? '').trim();
  if (trimmed !== '') {
    const lineCount = trimmed.split('\n').filter((l) => l.trim() !== '').length;
    if (lineCount > 1) return `已完成（${ellipsize(compactText(trimmed), 120)}）`;
    return ellipsize(trimmed, 120);
  }
  for (let i = work.length - 1; i >= 0; i -= 1) {
    const head = compactText(workLine(work[i]));
    if (head !== '') return ellipsize(head, 120);
  }
  if (status === 'running') return 'Planning next moves';
  if (status === 'done') return 'No summary';
  return 'Failed';
}
