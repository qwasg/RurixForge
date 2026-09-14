/**
 * F7 wave.4 build_timeline TS 移植(参考 ui/chat.rs 逐规则;G-F7-4)。
 *
 * 规则(与参考逐条对齐):
 * - 连续普通 tool 块合并为 activity segment;milestone 断段(text/subagent 块,
 *   或 tool 名 === "task" | "write_todos")。
 * - 段统计:Edit=写改删类按目标文件去重(args path/file/filePath/scenePath/assetPath/
 *   destFolder 取一)、Explore=读取浏览类按路径去重、Search 计数、Command 计数、Other 计数;
 *   短语「Edited 3 files, explored 2 files, 1 search, ran 1 command」(全空「Working」;
 *   原中文短语见留痕⑥)。
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
 * ⑤ 2026-09-03 用户指令:思考行改「进行中渐变 Thinking / 结束 Thinking for {时长}」,
 *    参考 reasoning_summary(摘录 + N 字)随之下线,时长由 reasoning 事件 ts 之差如实得出。
 * ⑥ 2026-09-03 用户指令(过程链英文 / 正文中文加粗):本文件承担的**过程链文案整体转英文**,
 *    行形态对齐用户给的目标截图 ——「{动词} {目标}」两段式(`Read timeline.ts L90-625`、
 *    `Grepped danger|dot-blocked in theme.css`、`Searched files components/chat` + glob 串、
 *    `Ran cargo test`),段汇总「Exploring 12 files, 9 searches, ran 2 commands」
 *    (段内有 running 工具 → 首动词用现在分词),思考行「Thought 47s」/ 无计时「Thought briefly」。
 *    面向用户的正文仍是中文(后端 SYSTEM_PROMPT 约定),渲染层加粗加黑,与灰色过程链拉开层级。
 * ⑦ 同批指令(报错不特别标明):失败不再进文案(原「· n 失败」「失败:{首行}」下线),
 *    错误原文只在展开的工具详情里如实可见;SegmentStats.errors 仍如实统计,只是不再上屏。
 * ⑧ 同批指令:reasoning 不再是 milestone —— 思考行与工具行同列并进活动段(截图里
 *    「Thought 47s」就夹在 Read/Grepped 之间);孤立 reasoning 段由渲染层裸行呈现。
 */

// ---- 块模型(chatStore 与渲染层共用) ----

export type BlockStatus = 'running' | 'done' | 'error';

export type ToolKind = 'command' | 'fileChange' | 'webSearch' | 'mcp' | 'native';

export interface ToolChange {
  path: string;
  kind?: string;
  diff?: string;
}

export interface ApprovalOption {
  label: string;
  description?: string;
  /** 兼容扩展选项；Codex v2 当前把“其他”标记放在 question 上。 */
  isOther?: boolean;
}

export interface ApprovalQuestion {
  id: string;
  header?: string;
  question?: string;
  options?: ApprovalOption[];
  /** 旧/扩展审批面可显式标记必填；Codex v2 的表单 schema 另由 approval.schema 表达。 */
  required?: boolean;
  /** 旧字段名，原样保留供兼容 UI 使用。 */
  secret?: boolean;
  /** Codex app-server v2 `ToolRequestUserInputQuestion.isSecret`。 */
  isSecret?: boolean;
  /** Codex app-server v2 `ToolRequestUserInputQuestion.isOther`。 */
  isOther?: boolean;
}

export type PermissionDecision = 'accept' | 'acceptForSession' | 'decline';

export type ChatBlock =
  | { kind: 'text'; text: string; final: boolean }
  | {
      kind: 'reasoning';
      text: string;
      /** 该块首个 reasoning 事件 ts(ISO);缺省 = 无计时(旧快照/直接构造的块)。 */
      startedTs?: string;
      /** 该块末个 reasoning 事件 ts(ISO);与 startedTs 之差即思考时长。 */
      endedTs?: string;
    }
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
      /** 命令流式输出；result 为旧组件兼容别名，二者由 chatStore 同步写入。 */
      output?: string;
      exitCode?: number;
      changes?: ToolChange[];
      /** 新事件恒有；旧快照/测试夹具可缺省并由名称推断。 */
      toolKind?: ToolKind;
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
      /**
       * D-036:后台子代理(multitask dispatch)自己的 runId —— Stop 要打它,
       * 不能打全局 activeRunId(后台跑的时候父轮早结束了,activeRunId 是空的)。
       * 同步 task 子代理无此字段,Stop 维持中止父轮的原语义。
       */
      detachedRunId?: string;
      work: ChatBlock[];
    }
  | {
      kind: 'plan';
      text: string;
      final: boolean;
      planPath?: string;
    }
  | {
      kind: 'approval';
      id: string;
      approvalKind: 'command' | 'fileChange' | 'permissions' | 'userInput' | 'elicitation' | string;
      tool?: string;
      command?: string;
      cwd?: string;
      changes?: ToolChange[];
      reason?: string;
      message?: string;
      questions?: ApprovalQuestion[];
      /** `item/permissions/requestApproval` 的 fileSystem/network 请求原文。 */
      permissions?: Record<string, unknown>;
      /** MCP elicitation 的 requestedSchema/schema 原文。 */
      schema?: Record<string, unknown>;
      /** command approval 的托管网络上下文与持久策略建议。 */
      networkApprovalContext?: Record<string, unknown>;
      proposedExecpolicyAmendment?: string[];
      proposedNetworkPolicyAmendments?: Array<Record<string, unknown>>;
      /** file-change approval 请求在本会话授予写权限的根目录。 */
      grantRoot?: string;
      /** MCP elicitation 来源与 URL-mode 外部流程。 */
      mode?: string;
      serverName?: string;
      url?: string;
      elicitationId?: string;
      availableDecisions?: PermissionDecision[];
      decision?: PermissionDecision | string;
      /** elicitation 可包含 string/number/boolean/array，不能收窄为纯字符串。 */
      answers?: Record<string, unknown>;
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

// ---- 本仓动词表(留痕⑥:按 KNOWN_TOOLS 全量核对逐条转英文;done=完成态行首动词、
// running=运行中现在分词、prefix/suffix=同类并组短语「{prefix} {n} {suffix}」) ----

interface ToolMeta {
  /** 完成态行首动词(过去式,如 `Read` / `Created entity`)。 */
  done: string;
  /** 运行中行首动词(现在分词,如 `Reading` / `Creating entity`)。 */
  running: string;
  groupPrefix: string;
  groupSuffix: string;
}

/** 表项构造(四段固定序,免逐行写键名)。 */
const m = (done: string, running: string, groupPrefix: string, groupSuffix: string): ToolMeta => ({
  done,
  running,
  groupPrefix,
  groupSuffix,
});

const TOOL_META: Record<string, ToolMeta> = {
  // engine-scene:实体/组件/变换
  entity_create: m('Created entity', 'Creating entity', 'Created', 'entities'),
  entity_destroy: m('Deleted entity', 'Deleting entity', 'Deleted', 'entities'),
  entity_rename: m('Renamed entity', 'Renaming entity', 'Renamed', 'entities'),
  entity_list: m('Listed entities', 'Listing entities', 'Listed entities', 'times'),
  entity_get: m('Read entity', 'Reading entity', 'Read', 'entities'),
  entity_batch_apply: m('Applied batch', 'Applying batch', 'Applied', 'batches'),
  transform_set: m('Set transform', 'Setting transform', 'Set', 'transforms'),
  transform_get: m('Read transform', 'Reading transform', 'Read', 'transforms'),
  transform_batch_set: m('Set transforms', 'Setting transforms', 'Set', 'transform batches'),
  component_add: m('Added component', 'Adding component', 'Added', 'components'),
  component_remove: m('Removed component', 'Removing component', 'Removed', 'components'),
  component_set: m('Set component', 'Setting component', 'Set', 'components'),
  component_get: m('Read component', 'Reading component', 'Read', 'components'),
  component_list_types: m('Listed component types', 'Listing component types', 'Listed component types', 'times'),
  // engine-scene:场景/编辑
  scene_save: m('Saved scene', 'Saving scene', 'Saved', 'scenes'),
  scene_load: m('Loaded scene', 'Loading scene', 'Loaded', 'scenes'),
  scene_new: m('Created scene', 'Creating scene', 'Created', 'scenes'),
  scene_summary: m('Read scene', 'Reading scene', 'Read scene', 'times'),
  scene_index: m('Indexed scene', 'Indexing scene', 'Indexed scene', 'times'),
  scene_graph_dump: m('Dumped scene graph', 'Dumping scene graph', 'Dumped scene graph', 'times'),
  scene_diff: m('Diffed scene', 'Diffing scene', 'Diffed scene', 'times'),
  scene_checkpoint: m('Checkpointed scene', 'Checkpointing scene', 'Checkpointed scene', 'times'),
  scene_rollback: m('Rolled back scene', 'Rolling back scene', 'Rolled back scene', 'times'),
  edit_undo: m('Undid edit', 'Undoing edit', 'Undid', 'edits'),
  edit_redo: m('Redid edit', 'Redoing edit', 'Redid', 'edits'),
  host_events: m('Read host events', 'Reading host events', 'Read host events', 'times'),
  host_events_drain: m('Drained host events', 'Draining host events', 'Drained host events', 'times'),
  host_ping: m('Pinged host', 'Pinging host', 'Pinged host', 'times'),
  render_once: m('Rendered frame', 'Rendering frame', 'Rendered', 'frames'),
  // engine-scene:播放/视口
  play_enter: m('Entered play', 'Entering play', 'Entered play', 'times'),
  play_exit: m('Exited play', 'Exiting play', 'Exited play', 'times'),
  play_pause: m('Paused', 'Pausing', 'Paused', 'times'),
  play_resume: m('Resumed', 'Resuming', 'Resumed', 'times'),
  play_step: m('Stepped', 'Stepping', 'Stepped', 'times'),
  play_state: m('Read play state', 'Reading play state', 'Read play state', 'times'),
  logic_inject_input: m('Injected input', 'Injecting input', 'Injected', 'inputs'),
  // code-forge / gen-image / gen-model(任务书具名项)
  graph_get: m('Read graph', 'Reading graph', 'Read', 'graphs'),
  graph_validate: m('Validated graph', 'Validating graph', 'Validated', 'graphs'),
  graph_create: m('Wrote graph', 'Writing graph', 'Wrote', 'graphs'),
  code_structured_edit: m('Edited', 'Editing', 'Edited', 'files'),
  code_symbol_search: m('Searched symbols', 'Searching symbols', 'Searched symbols', 'times'),
  code_references: m('Found references', 'Finding references', 'Found references', 'times'),
  rx_check: m('Checked', 'Checking', 'Checked', 'times'),
  rx_fmt: m('Formatted', 'Formatting', 'Formatted', 'files'),
  rx_build: m('Built', 'Building', 'Built', 'times'),
  rx_run: m('Ran', 'Running', 'Ran', 'times'),
  rx_test: m('Tested', 'Testing', 'Tested', 'times'),
  gen_image: m('Generated texture', 'Generating texture', 'Generated', 'textures'),
  gen_texture_set: m('Generated texture set', 'Generating texture set', 'Generated', 'texture sets'),
  gen_accept: m('Imported asset', 'Importing asset', 'Imported', 'assets'),
  gen_mesh: m('Generated mesh', 'Generating mesh', 'Generated', 'meshes'),
  // 资产/精灵/检索(通配族之上的具名项,措辞比「Handled asset」如实)
  asset_list: m('Listed assets', 'Listing assets', 'Listed assets', 'times'),
  asset_import: m('Imported asset', 'Importing asset', 'Imported', 'assets'),
  asset_get_meta: m('Read asset meta', 'Reading asset meta', 'Read asset meta', 'times'),
  asset_refs: m('Queried asset refs', 'Querying asset refs', 'Queried asset refs', 'times'),
  sprite_create: m('Created sprite', 'Creating sprite', 'Created', 'sprites'),
  sprite_set: m('Set sprite', 'Setting sprite', 'Set', 'sprites'),
  material_create: m('Created material', 'Creating material', 'Created', 'materials'),
  texture_process: m('Processed texture', 'Processing texture', 'Processed', 'textures'),
  context_search: m('Searched context', 'Searching context', 'Searched context', 'times'),
  context_index_build: m('Built index', 'Building index', 'Built index', 'times'),
  project_list: m('Listed projects', 'Listing projects', 'Listed projects', 'times'),
  resource_search: m('Searched resources', 'Searching resources', 'Searched resources', 'times'),
  // 合成工具(F3 multitask)
  'swarm.execute': m('Ran swarm', 'Running swarm', 'Ran swarm', 'times'),
  // 运行时原生工具(行首动词与目标截图逐字同款:Read / Grepped / Searched files / Ran)
  read_file: m('Read', 'Reading', 'Read', 'files'),
  read_skill: m('Read skill', 'Reading skill', 'Read', 'skills'),
  list_dir: m('Listed', 'Listing', 'Listed', 'directories'),
  glob: m('Searched files', 'Searching files', 'Searched files', 'times'),
  grep: m('Grepped', 'Grepping', 'Grepped', 'times'),
  write_file: m('Wrote', 'Writing', 'Wrote', 'files'),
  str_replace_edit: m('Edited', 'Editing', 'Edited', 'files'),
  apply_patch: m('Patched', 'Patching', 'Patched', 'files'),
  shell: m('Ran', 'Running', 'Ran', 'commands'),
  web_search: m('Searched web', 'Searching web', 'Searched web', 'times'),
  update_plan: m('Updated plan', 'Updating plan', 'Updated plan', 'times'),
  todo_write: m('Updated todos', 'Updating todos', 'Updated todos', 'times'),
  write_todos: m('Updated todos', 'Updating todos', 'Updated todos', 'times'),
  plan_write: m('Wrote plan', 'Writing plan', 'Wrote plan', 'times'),
  todo_update: m('Updated todo', 'Updating todo', 'Updated', 'todos'),
  task: m('Delegated', 'Delegating', 'Delegated', 'times'),
  // D-036:multitask 异步派发。刻意不进 isMilestoneBlock —— 里程碑工具块在
  // AssistantMessage 里走 TodoMilestoneLine(待办行),派发该并进活动段读作
  // 「Dispatched N subagents」;真正的子代理进展另有卡片(subagent.* 事件)。
  dispatch: m('Dispatched subagent', 'Dispatching subagent', 'Dispatched', 'subagents'),
  // open-computer-use:过程链沿用英文动词；未知的同服务工具仍回退 server / tool。
  list_apps: m('Listed apps', 'Listing apps', 'Listed apps', 'times'),
  get_app_state: m('Read app state', 'Reading app state', 'Read app state', 'times'),
  screenshot: m('Captured screen', 'Capturing screen', 'Captured screen', 'times'),
  click: m('Clicked', 'Clicking', 'Clicked', 'times'),
  double_click: m('Double-clicked', 'Double-clicking', 'Double-clicked', 'times'),
  type_text: m('Typed text', 'Typing text', 'Typed text', 'times'),
  press_key: m('Pressed key', 'Pressing key', 'Pressed key', 'times'),
  scroll: m('Scrolled', 'Scrolling', 'Scrolled', 'times'),
  move_mouse: m('Moved pointer', 'Moving pointer', 'Moved pointer', 'times'),
  open_app: m('Opened app', 'Opening app', 'Opened app', 'times'),
  focus_app: m('Focused app', 'Focusing app', 'Focused app', 'times'),
  wait: m('Waited', 'Waiting', 'Waited', 'times'),
};

/** 通配族(viewport_* / asset_* 的兜底,具名项见上表)。 */
const WILDCARD_META: Array<[string, ToolMeta]> = [
  ['viewport_', m('Adjusted viewport', 'Adjusting viewport', 'Adjusted viewport', 'times')],
  ['asset_', m('Handled asset', 'Handling asset', 'Handled', 'assets')],
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

/** 完成态行首动词(未知 MCP 名 →「server / tool」,其余未知 → 原名)。 */
export function toolVisual(name: string): string {
  const meta = toolMetaOf(name);
  if (meta) return meta.done;
  const mcp = mcpOf(name);
  if (mcp) return `${mcp[0]} / ${mcp[1]}`;
  return name;
}

/** 运行中行首动词(现在分词;无表项回落完成态动词,不硬造分词)。 */
export function toolVisualRunning(name: string): string {
  const meta = toolMetaOf(name);
  return meta ? meta.running : toolVisual(name);
}

/** 展开段内同类并组 key(参考 tool_group_key)。 */
export function toolGroupKey(name: string): string {
  const meta = toolMetaOf(name);
  if (meta) return `kind:${meta.done}`;
  const mcp = mcpOf(name);
  if (mcp) return `mcp:${mcp[0]}/${mcp[1]}`;
  return `name:${name}`;
}

/** 「Created 3 entities」式聚合短语两段式(渲染层分两档灰:动词深 / 计数浅)。 */
export function groupPhraseParts(name: string, n: number): { verb: string; detail: string } {
  const meta = toolMetaOf(name);
  if (meta) return { verb: meta.groupPrefix, detail: `${n} ${meta.groupSuffix}` };
  const mcp = mcpOf(name);
  if (mcp) return { verb: `${mcp[0]} / ${mcp[1]}`, detail: `×${n}` };
  return { verb: name, detail: `×${n}` };
}

/** 「Created 3 entities」式聚合短语整串(参考 group_phrase)。 */
export function groupPhrase(name: string, n: number): string {
  const { verb, detail } = groupPhraseParts(name, n);
  return `${verb} ${detail}`;
}

// ---- 段化(参考 build_timeline / is_milestone_block / group_segment_items) ----

export type TimelineItem =
  | { type: 'block'; index: number }
  | { type: 'activity'; indices: number[] };

export function isMilestoneBlock(block: ChatBlock): boolean {
  // 留痕⑧:reasoning 不断段 —— 思考行与工具行同列并进活动段。
  if (block.kind === 'reasoning') return false;
  if (block.kind !== 'tool') return true; // text / subagent / plan / approval
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
  'shell',
  'rx_build', 'rx_run', 'rx_test', 'swarm.execute',
  'play_enter', 'play_exit', 'play_pause', 'play_resume', 'play_step',
]);

export function toolCategory(name: string): ToolCategory {
  const bare = bareName(name);
  if (EDIT_TOOLS.has(bare)) return 'edit';
  if (EXPLORE_TOOLS.has(bare)) return 'explore';
  if (bare === 'web_search') return 'search';
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
  const obj = parseArgs(args);
  if (Array.isArray(obj?.changes)) {
    const paths = obj.changes
      .map((change) =>
        change && typeof change === 'object' && typeof (change as { path?: unknown }).path === 'string'
          ? (change as { path: string }).path.trim()
          : '',
      )
      .filter((path) => path !== '');
    if (paths.length > 0) return [...new Set(paths)];
  }
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
  if (Array.isArray(obj.changes)) {
    return obj.changes.reduce(
      (sum, change) => {
        if (!change || typeof change !== 'object') return sum;
        const diff = (change as { diff?: unknown }).diff;
        if (typeof diff !== 'string') return sum;
        const stat = patchDiffStats(diff);
        return { added: sum.added + stat.added, removed: sum.removed + stat.removed };
      },
      { added: 0, removed: 0 },
    );
  }
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

/** 「2 files」式计数短语(many 缺省 = one + s)。 */
function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/**
 * 段汇总两段式(留痕⑥,与目标截图逐字对齐):首动词深灰 + 其余浅灰。
 * 「Edited 2 files, explored 2 files, 2 searches, ran 2 commands」;
 * running=true 时首动词换现在分词:「Exploring 12 files, 9 searches, ran 2 commands」。
 */
export function segmentPhraseParts(
  stats: SegmentStats,
  running = false,
): { verb: string; detail: string } {
  const clauses: string[] = [];
  let verb = '';
  /** 首个非零类目定首动词(过去式/现在分词);其余类目退成小写从句。 */
  const lead = (past: string, gerund: string): boolean => {
    if (verb !== '') return false;
    verb = running ? gerund : past;
    return true;
  };
  if (stats.edits > 0) {
    const n = plural(stats.edits, 'file');
    clauses.push(lead('Edited', 'Editing') ? n : `edited ${n}`);
  }
  if (stats.explores > 0) {
    const n = plural(stats.explores, 'file');
    clauses.push(lead('Explored', 'Exploring') ? n : `explored ${n}`);
  }
  if (stats.searches > 0) {
    const n = plural(stats.searches, 'search', 'searches');
    lead('Explored', 'Exploring');
    clauses.push(n);
  }
  if (stats.commands > 0) {
    const n = plural(stats.commands, 'command');
    clauses.push(lead('Ran', 'Running') ? n : `ran ${n}`);
  }
  if (stats.others > 0) {
    const n = plural(stats.others, 'operation');
    clauses.push(lead('Performed', 'Performing') ? n : `${stats.others} other operations`);
  }
  if (verb === '') return { verb: running ? 'Working…' : 'Working', detail: '' };
  return { verb, detail: clauses.join(', ') };
}

/** 段汇总整串(渲染层用 parts 分两档灰;此串供嵌套摘要/测试用)。 */
export function segmentPhrase(stats: SegmentStats, running = false): string {
  const { verb, detail } = segmentPhraseParts(stats, running);
  return detail === '' ? verb : `${verb} ${detail}`;
}

/** 段内是否仍有工具在跑(有 → 段首动词用现在分词);取倒数第一个 running 工具的分词。 */
export function runningLabel(blocks: ChatBlock[], indices: number[]): string | null {
  for (let k = indices.length - 1; k >= 0; k -= 1) {
    const b = blocks[indices[k]];
    if (b.kind === 'tool' && toolStatus(b) === 'running') {
      return toolVisualRunning(b.name);
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
  'command', 'query', 'pattern', 'url', 'dir', 'skill', 'description', 'prompt',
  'name', 'id', 'entityId',
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

/** 末段文件名(带路径分隔符时取 basename;截图里「Read timeline.ts」即此口径)。 */
export function baseName(p: string): string {
  const norm = p.replace(/[\\/]+$/, '');
  const cut = Math.max(norm.lastIndexOf('/'), norm.lastIndexOf('\\'));
  return cut >= 0 ? norm.slice(cut + 1) : norm;
}

/**
 * 行号区间后缀「L90-625」:offset(1 基起行)+ limit,或 startLine/endLine 直给。
 * 参数里没有行号信息 → 空串,绝不按内容长度伪造区间。
 */
export function lineRange(args: string): string {
  const obj = parseArgs(args);
  if (!obj) return '';
  const num = (...keys: string[]): number | null => {
    for (const k of keys) {
      const v = obj[k];
      if (typeof v === 'number' && Number.isInteger(v)) return v;
      if (typeof v === 'string' && /^\d+$/.test(v.trim())) return Number(v.trim());
    }
    return null;
  };
  const start = num('offset', 'startLine', 'start_line', 'lineStart');
  const end = num('endLine', 'end_line', 'lineEnd');
  if (start === null && end === null) return '';
  const from = start ?? 1;
  if (from < 1) return '';
  const limit = num('limit', 'maxLines');
  const to = end ?? (limit === null ? null : from + limit - 1);
  return to === null ? `L${from}` : `L${from}-${to}`;
}

/**
 * 行尾目标串(留痕⑥,截图口径):
 * grep →「{query} in {文件名}」、glob → 整个 pattern、读写类 →「{文件名} L{a}-{b}」、
 * apply_patch → 补丁头里的文件名,其余按 ARG_KEYS 键序兜底(路径值取 basename)。
 */
export function toolTarget(b: Extract<ChatBlock, { kind: 'tool' }>): string {
  const bare = bareName(b.name);
  const str = (k: string): string | null => jsonArgStr(b.args, k);
  if (bare === 'grep') {
    const q = str('query') ?? str('pattern');
    const head = q === null ? '' : ellipsize(compactText(q), 72);
    const scope = str('path');
    if (scope === null) return head;
    return head === '' ? `in ${baseName(scope)}` : `${head} in ${baseName(scope)}`;
  }
  if (bare === 'glob') return ellipsize(str('pattern') ?? str('glob_pattern') ?? '', 72);
  if (bare === 'apply_patch') {
    const hit = /^\*\*\* (?:Add|Update|Delete) File: (.+)$/m.exec(str('patch') ?? '');
    if (hit) return baseName(hit[1].trim());
    const obj = parseArgs(b.args);
    const first = Array.isArray(obj?.changes) ? obj.changes[0] : null;
    const path = first && typeof first === 'object' ? (first as { path?: unknown }).path : null;
    return typeof path === 'string' ? baseName(path) : '';
  }
  const range = lineRange(b.args);
  const withRange = (s: string): string => (range === '' ? s : `${s} ${range}`);
  for (const k of TARGET_KEYS) {
    const v = str(k);
    if (v) return withRange(baseName(v));
  }
  const fallback = argSummary(b.args);
  return fallback === null ? '' : withRange(fallback);
}

/** 行首动词(running → 现在分词)。 */
export function toolVerb(b: Extract<ChatBlock, { kind: 'tool' }>): string {
  return toolStatus(b) === 'running' ? toolVisualRunning(b.name) : toolVisual(b.name);
}

/**
 * 单行工具串「{动词} {目标}」(嵌套摘要与测试取整串,渲染层分两档灰)。
 * 留痕⑦:失败不再进文案(不缀「失败」也不缀错误首行),错误原文只在展开详情里如实可见。
 */
export function toolLine(b: Extract<ChatBlock, { kind: 'tool' }>): string {
  const target = toolTarget(b);
  return target === '' ? toolVerb(b) : `${toolVerb(b)} ${target}`;
}

/**
 * 思考时长(ms):块首末 reasoning 事件 ts 之差。无计时(旧快照/直接构造)或时钟倒挂 → null,
 * 由调用方退到无时长文案,不伪造秒数。
 */
export function reasoningDurationMs(b: Extract<ChatBlock, { kind: 'reasoning' }>): number | null {
  if (b.startedTs === undefined || b.endedTs === undefined) return null;
  const from = Date.parse(b.startedTs);
  const to = Date.parse(b.endedTs);
  if (!Number.isFinite(from) || !Number.isFinite(to) || to < from) return null;
  return to - from;
}

/**
 * 思考行两段式:完成「Thought」+「47s」(不足 1 秒进位 1s,过分钟仍报秒——截图里
 * 「Thought 106s」逐字如此);无计时 →「Thought briefly」。进行中不走这里(渲染层出渐变
 * 「Thinking」)。
 */
export function thinkingParts(durationMs: number | null): { verb: string; detail: string } {
  if (durationMs === null) return { verb: 'Thought', detail: 'briefly' };
  return { verb: 'Thought', detail: `${Math.max(1, Math.round(durationMs / 1000))}s` };
}

/** 思考折叠行整串:「Thought 47s」/「Thought briefly」。 */
export function thinkingLabel(durationMs: number | null): string {
  const { verb, detail } = thinkingParts(durationMs);
  return `${verb} ${detail}`;
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
  if (block.kind === 'tool') return toolLine(block);
  if (block.kind === 'subagent') return block.label;
  if (block.kind === 'plan') return block.text;
  if (block.kind === 'approval') return block.command ?? block.reason ?? 'Approval requested';
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
