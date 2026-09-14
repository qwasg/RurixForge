import { describe, expect, it } from 'vitest';
import {
  argSummary,
  bareName,
  baseName,
  buildTimeline,
  diffCounts,
  editTargetFiles,
  ellipsize,
  groupPhrase,
  groupSegmentItems,
  isMilestoneBlock,
  lineRange,
  mcpOf,
  reasoningDurationMs,
  runningLabel,
  segmentPhrase,
  segmentStats,
  subagentDispatchSummary,
  subagentLiveSummary,
  thinkingLabel,
  todoMilestoneLabel,
  toolCategory,
  toolDiffStats,
  toolLine,
  toolTarget,
  toolVisual,
  toolVisualRunning,
  type ChatBlock,
} from '@/lib/timeline';

type ToolBlock = Extract<ChatBlock, { kind: 'tool' }>;

function tool(
  toolCallId: string,
  name: string,
  args: Record<string, unknown>,
  ok?: boolean,
  error?: string,
): ToolBlock {
  return {
    kind: 'tool',
    toolCallId,
    name,
    args: JSON.stringify(args, null, 2),
    ok,
    error,
    durationMs: ok === undefined ? undefined : 12,
    mcp: mcpOf(name),
  };
}

describe('timeline 名称工具', () => {
  it('mcpOf/bareName:mcp__server__tool 剥分', () => {
    expect(mcpOf('mcp__engine-scene__entity_create')).toEqual(['engine-scene', 'entity_create']);
    expect(bareName('mcp__engine-scene__entity_create')).toBe('entity_create');
    expect(mcpOf('swarm.execute')).toBeNull();
    expect(bareName('swarm.execute')).toBe('swarm.execute');
  });

  it('英文动词表:具名/通配/未知 MCP/原名', () => {
    expect(toolVisual('mcp__engine-scene__entity_create')).toBe('Created entity');
    expect(toolVisual('mcp__engine-scene__entity_destroy')).toBe('Deleted entity');
    expect(toolVisual('mcp__engine-scene__scene_save')).toBe('Saved scene');
    expect(toolVisual('mcp__engine-scene__play_step')).toBe('Stepped');
    expect(toolVisual('mcp__engine-scene__viewport_set_camera')).toBe('Adjusted viewport');
    expect(toolVisual('mcp__asset-pipeline__asset_reimport')).toBe('Handled asset');
    expect(toolVisual('mcp__asset-pipeline__asset_import')).toBe('Imported asset');
    expect(toolVisual('mcp__gen-image__gen_image')).toBe('Generated texture');
    expect(toolVisual('mcp__code-forge__graph_validate')).toBe('Validated graph');
    expect(toolVisual('swarm.execute')).toBe('Ran swarm');
    expect(toolVisual('dispatch')).toBe('Dispatched subagent');
    expect(toolVisual('read_file')).toBe('Read');
    expect(toolVisual('grep')).toBe('Grepped');
    expect(toolVisual('glob')).toBe('Searched files');
    expect(toolVisual('shell')).toBe('Ran');
    expect(toolVisual('web_search')).toBe('Searched web');
    expect(toolVisual('mcp__computer-use__click')).toBe('Clicked');
    expect(toolVisual('mcp__computer-use__type_text')).toBe('Typed text');
    // 未知 MCP 名 → server / tool
    expect(toolVisual('mcp__foo__bar_baz')).toBe('foo / bar_baz');
    // 非 MCP 未知名 → 原名
    expect(toolVisual('some_tool')).toBe('some_tool');
  });

  it('运行中动词用现在分词;无表项回落完成态动词不硬造分词', () => {
    expect(toolVisualRunning('mcp__engine-scene__entity_create')).toBe('Creating entity');
    expect(toolVisualRunning('read_file')).toBe('Reading');
    expect(toolVisualRunning('glob')).toBe('Searching files');
    expect(toolVisualRunning('mcp__foo__bar_baz')).toBe('foo / bar_baz');
    expect(toolVisualRunning('some_tool')).toBe('some_tool');
  });

  /// D-036:dispatch 不是里程碑 —— 里程碑工具块在 AssistantMessage 里渲染成待办行,
  /// 派发该并进活动段读作「派发子代理 · N 次」;真进展在各自的子代理卡片上。
  it('dispatch 并进活动段(非 milestone),task 仍断段', () => {
    const blocks: ChatBlock[] = [
      tool('d1', 'dispatch', { description: '甲' }, true),
      tool('d2', 'dispatch', { description: '乙' }, true),
      tool('t1', 'task', { prompt: 'p' }),
    ];
    expect(buildTimeline(blocks)).toEqual([
      { type: 'activity', indices: [0, 1] },
      { type: 'block', index: 2 },
    ]);
    expect(groupPhrase('dispatch', 2)).toBe('Dispatched 2 subagents');
  });

  it('聚合短语 groupPhrase', () => {
    // 并组只在 n ≥ 2 时成立(groupSegmentItems),故短语固定用复数尾词
    expect(groupPhrase('mcp__engine-scene__entity_create', 3)).toBe('Created 3 entities');
    expect(groupPhrase('mcp__engine-scene__entity_list', 2)).toBe('Listed entities 2 times');
    expect(groupPhrase('read_file', 5)).toBe('Read 5 files');
    expect(groupPhrase('mcp__foo__bar', 2)).toBe('foo / bar ×2');
    expect(groupPhrase('custom_tool', 4)).toBe('custom_tool ×4');
  });
});

describe('build_timeline 段化规则', () => {
  it('连续普通 tool 合并;milestone(text/subagent/write_todos/task)断段', () => {
    const blocks: ChatBlock[] = [
      tool('t1', 'mcp__engine-scene__entity_list', {}, true),
      tool('t2', 'mcp__engine-scene__entity_create', { name: 'a' }, true),
      tool('t3', 'write_todos', { todos: [] }, true),
      tool('t4', 'task', { prompt: 'p' }), // task → milestone(参考同口径)
      tool('t5', 'mcp__engine-scene__scene_save', {}, true),
      { kind: 'text', text: '最终回答', final: true },
    ];
    const tl = buildTimeline(blocks);
    expect(tl).toEqual([
      { type: 'activity', indices: [0, 1] },
      { type: 'block', index: 2 },
      { type: 'block', index: 3 },
      { type: 'activity', indices: [4] },
      { type: 'block', index: 5 },
    ]);
  });

  /// 留痕⑧:reasoning 不再断段 —— 思考行与工具行同列并进活动段(目标截图里
  /// 「Thought 47s」就夹在 Read/Grepped 之间)。
  it('reasoning 并进活动段,不再作 milestone', () => {
    const blocks: ChatBlock[] = [
      tool('t1', 'read_file', { path: 'a.ts' }, true),
      { kind: 'reasoning', text: '想一下' },
      tool('t2', 'grep', { query: 'foo' }, true),
      { kind: 'text', text: '答', final: true },
    ];
    expect(buildTimeline(blocks)).toEqual([
      { type: 'activity', indices: [0, 1, 2] },
      { type: 'block', index: 3 },
    ]);
    expect(isMilestoneBlock({ kind: 'reasoning', text: 'x' })).toBe(false);
  });

  it('最终回答永不折叠:text 块恒为独立 block 项(不并入 activity)', () => {
    const blocks: ChatBlock[] = [
      tool('t1', 'mcp__engine-scene__entity_list', {}, true),
      { kind: 'text', text: '中间叙述', final: false },
      tool('t2', 'mcp__engine-scene__entity_list', {}, true),
      { kind: 'text', text: '最终回答', final: true },
    ];
    const tl = buildTimeline(blocks);
    expect(tl).toEqual([
      { type: 'activity', indices: [0] },
      { type: 'block', index: 1 },
      { type: 'activity', indices: [2] },
      { type: 'block', index: 3 },
    ]);
    expect(isMilestoneBlock({ kind: 'text', text: 'x', final: true })).toBe(true);
  });

  it('Codex plan / approval 是独立 milestone', () => {
    const blocks: ChatBlock[] = [
      tool('c1', 'shell', { command: 'cargo test' }, true),
      { kind: 'plan', text: '先审计，再修改', final: false },
      {
        kind: 'approval',
        id: 'perm_1',
        approvalKind: 'command',
        command: 'cargo test',
      },
      tool('c2', 'apply_patch', { changes: [{ path: 'src/a.ts' }] }, true),
    ];
    expect(buildTimeline(blocks)).toEqual([
      { type: 'activity', indices: [0] },
      { type: 'block', index: 1 },
      { type: 'block', index: 2 },
      { type: 'activity', indices: [3] },
    ]);
  });

  it('段内同类并组:连续同组非 running 成 run;running 单独', () => {
    const blocks: ChatBlock[] = [
      tool('t1', 'mcp__engine-scene__entity_list', {}, true),
      tool('t2', 'mcp__engine-scene__entity_list', {}, true),
      tool('t3', 'mcp__engine-scene__entity_create', { name: 'a' }, true),
      tool('t4', 'mcp__engine-scene__entity_list', {}), // running → 单独
      tool('t5', 'mcp__engine-scene__entity_list', {}, true), // 与 t1 不连续(running 隔断)
    ];
    const runs = groupSegmentItems(blocks, [0, 1, 2, 3, 4]);
    expect(runs).toEqual([[0, 1], [2], [3], [4]]);
  });
});

describe('segment 统计与短语', () => {
  it('目标去重 + 短语拼装 + +/-估算', () => {
    const blocks: ChatBlock[] = [
      tool('t1', 'mcp__code-forge__code_structured_edit', {
        path: 'a.rx',
        old_str: 'a\nb',
        new_str: 'a\nc\nd',
      }, true),
      tool('t2', 'mcp__engine-scene__entity_list', { scenePath: 's1' }, true),
      tool('t3', 'mcp__engine-scene__entity_list', { scenePath: 's1' }, true), // 同路径去重
      tool('t4', 'mcp__code-forge__code_symbol_search', { query: 'foo' }, true),
      tool('t5', 'mcp__code-forge__rx_build', {}, true),
    ];
    const st = segmentStats(blocks, [0, 1, 2, 3, 4]);
    expect(st).toMatchObject({
      edits: 1,
      explores: 1,
      searches: 1,
      commands: 1,
      others: 0,
      added: 2,
      removed: 1,
      errors: 0,
    });
    expect(segmentPhrase(st)).toBe('Edited 1 file, explored 1 file, 1 search, ran 1 command');
  });

  /// 目标截图逐字:running 时首动词换现在分词,其余从句维持过去式。
  it('段短语英文语法:首动词大写(running → 现在分词),后续从句小写', () => {
    const stats = { ...segmentStats([], []), explores: 12, searches: 9, commands: 2 };
    expect(segmentPhrase(stats, true)).toBe('Exploring 12 files, 9 searches, ran 2 commands');
    expect(segmentPhrase(stats)).toBe('Explored 12 files, 9 searches, ran 2 commands');
    expect(segmentPhrase({ ...segmentStats([], []), searches: 2 })).toBe('Explored 2 searches');
    expect(segmentPhrase({ ...segmentStats([], []), commands: 2 })).toBe('Ran 2 commands');
    expect(segmentPhrase({ ...segmentStats([], []), edits: 2, explores: 2 })).toBe(
      'Edited 2 files, explored 2 files',
    );
    expect(segmentPhrase({ ...segmentStats([], []), others: 2 })).toBe('Performed 2 operations');
  });

  it('目标键序:path/file/filePath/scenePath/assetPath/destFolder 取一;无键按块占位', () => {
    expect(editTargetFiles(JSON.stringify({ filePath: 'x.ts', path: 'y.ts' }))).toEqual(['y.ts']);
    expect(editTargetFiles(JSON.stringify({ destFolder: 'Content/T' }))).toEqual(['Content/T']);
    expect(editTargetFiles(JSON.stringify({}))).toEqual([]);
    const blocks: ChatBlock[] = [
      tool('t1', 'mcp__engine-scene__entity_create', { name: 'a' }, true),
      tool('t2', 'mcp__engine-scene__entity_create', { name: 'b' }, true),
    ];
    const st = segmentStats(blocks, [0, 1]);
    expect(st.edits).toBe(2); // 无目标键按 #index 占位,两调用不合并
  });

  it('+/- 估算:old_str/new_str 行差;仅 content 全行 added;无内容键 → 0(不显示)', () => {
    expect(toolDiffStats(JSON.stringify({ old_str: 'a\nb', new_str: 'a\nc\nd' }))).toEqual({
      added: 2,
      removed: 1,
    });
    expect(toolDiffStats(JSON.stringify({ oldContent: 'x', newContent: 'x\ny' }))).toEqual({
      added: 1,
      removed: 0,
    });
    expect(toolDiffStats(JSON.stringify({ content: '1\n2\n3' }))).toEqual({ added: 3, removed: 0 });
    expect(toolDiffStats(JSON.stringify({ name: 'e1' }))).toEqual({ added: 0, removed: 0 });
    expect(diffCounts('', 'a\nb')).toEqual({ added: 2, removed: 0 });
    expect(diffCounts('a\nb', '')).toEqual({ added: 0, removed: 2 });
  });

  it('Codex command/fileChange/webSearch 分类与结构化 changes 汇总', () => {
    const changes = {
      changes: [
        { path: 'src/a.ts', diff: '@@\n-old\n+new\n+line' },
        { path: 'src/b.ts', diff: '@@\n+added' },
      ],
    };
    const blocks: ChatBlock[] = [
      tool('shell-1', 'shell', { command: 'pnpm test' }, true),
      tool('file-1', 'apply_patch', changes, true),
      tool('web-1', 'web_search', { query: 'Codex app server' }, true),
    ];
    expect(toolCategory('shell')).toBe('command');
    expect(toolCategory('apply_patch')).toBe('edit');
    expect(toolCategory('web_search')).toBe('search');
    expect(editTargetFiles(JSON.stringify(changes))).toEqual(['src/a.ts', 'src/b.ts']);
    expect(toolDiffStats(JSON.stringify(changes))).toEqual({ added: 3, removed: 1 });
    expect(segmentStats(blocks, [0, 1, 2])).toMatchObject({ edits: 2, commands: 1, searches: 1 });
  });

  /// 留痕⑦:errors 仍如实统计(渲染层不再上屏,颜色与「n 失败」后缀一并下线)。
  it('失败仍进统计但不进文案;运行中取现在分词', () => {
    const blocks: ChatBlock[] = [
      tool('t1', 'mcp__engine-scene__entity_create', { name: 'a' }, false, 'boom'),
      tool('t2', 'mcp__engine-scene__entity_create', { name: 'b' }),
    ];
    const st = segmentStats(blocks, [0, 1]);
    expect(st.errors).toBe(1);
    expect(segmentPhrase(st, true)).not.toContain('fail');
    expect(runningLabel(blocks, [0, 1])).toBe('Creating entity');
    // 全完成 → null
    expect(runningLabel([tool('t3', 'mcp__engine-scene__entity_list', {}, true)], [0])).toBeNull();
  });

  it('空段短语 Working(running 带省略号)', () => {
    expect(segmentPhrase(segmentStats([], []))).toBe('Working');
    expect(segmentPhrase(segmentStats([], []), true)).toBe('Working…');
  });
});

describe('行级摘要', () => {
  it('argSummary 键序截 48;数字 id/entityId 取字符串', () => {
    expect(argSummary(JSON.stringify({ command: 'cargo test', path: 'a.rs' }))).toBe('a.rs');
    expect(argSummary(JSON.stringify({ query: 'foo' }))).toBe('foo');
    expect(argSummary(JSON.stringify({ entityId: 42 }))).toBe('42');
    expect(argSummary(JSON.stringify({ name: 'x'.repeat(60) }))).toBe('x'.repeat(48) + '…');
    expect(argSummary('not-json')).toBeNull();
    expect(argSummary(JSON.stringify({ other: 1 }))).toBeNull();
  });

  /// 目标截图逐字:「Read timeline.ts L90-625」「Grepped danger|--fg in theme.css」
  /// 「Searched files packages/client/src/components/chat/*.tsx」。
  it('toolTarget/toolLine:读写取 basename + 行区间;grep 带 in 作用域;glob 给整串 pattern', () => {
    const read = tool('t1', 'read_file', { path: 'packages/client/src/lib/timeline.ts' }, true);
    expect(toolLine(read)).toBe('Read timeline.ts');
    const win = tool('t2', 'read_file', { path: 'src/lib/timeline.ts', offset: 90, limit: 536 }, true);
    expect(toolLine(win)).toBe('Read timeline.ts L90-625');
    const grepped = tool('t3', 'grep', { query: 'danger|--fg', path: 'src/styles/theme.css' }, true);
    expect(toolLine(grepped)).toBe('Grepped danger|--fg in theme.css');
    expect(toolTarget(tool('t4', 'grep', { query: 'foo' }, true))).toBe('foo');
    const globbed = tool('t5', 'glob', { pattern: 'packages/client/src/components/chat/*.tsx' }, true);
    expect(toolLine(globbed)).toBe('Searched files packages/client/src/components/chat/*.tsx');
    const patched = tool('t6', 'apply_patch', {
      patch: '*** Begin Patch\n*** Update File: packages/client/src/lib/cn.ts\n+x\n*** End Patch',
    }, true);
    expect(toolLine(patched)).toBe('Patched cn.ts');
    // 无目标键 → 只剩动词;运行中换现在分词
    expect(toolLine(tool('t7', 'mcp__engine-scene__scene_save', {}, true))).toBe('Saved scene');
    expect(toolLine(tool('t8', 'mcp__engine-scene__entity_create', { name: 'e1' }))).toBe(
      'Creating entity e1',
    );
  });

  /// 留痕⑦:失败不再进行内文案(不缀「失败」、不缀错误首行),错误只在展开详情里可见。
  it('失败/成功行文案同形,不夹带状态词与结果摘录', () => {
    const failed = tool('t1', 'read_file', { path: 'a/b.ts' }, false, '首行错\n次行');
    expect(toolLine(failed)).toBe('Read b.ts');
    const done = tool('t2', 'read_file', { path: 'a/b.ts' }, true);
    expect(toolLine({ ...done, result: 'created #12\nmore' })).toBe('Read b.ts');
  });

  it('baseName/lineRange:分隔符两制;缺行号参数不伪造区间', () => {
    expect(baseName('packages/client/src/lib/timeline.ts')).toBe('timeline.ts');
    expect(baseName('crates\\forge-agentd\\src\\llm.rs')).toBe('llm.rs');
    expect(baseName('Content/Textures/')).toBe('Textures');
    expect(baseName('theme.css')).toBe('theme.css');
    expect(lineRange(JSON.stringify({ offset: 66, limit: 20 }))).toBe('L66-85');
    expect(lineRange(JSON.stringify({ startLine: 1, endLine: 96 }))).toBe('L1-96');
    expect(lineRange(JSON.stringify({ offset: 4 }))).toBe('L4');
    expect(lineRange(JSON.stringify({ limit: 50 }))).toBe('');
    expect(lineRange(JSON.stringify({ path: 'a.ts' }))).toBe('');
    expect(lineRange('not-json')).toBe('');
  });

  it('ellipsize/todoMilestoneLabel/subagent 摘要', () => {
    expect(ellipsize('abcdef', 3)).toBe('abc…');
    expect(todoMilestoneLabel(JSON.stringify({ todos: [
      { content: '已完成项', status: 'completed' },
      { content: '进行中项', status: 'in_progress' },
    ] }))).toBe('进行中项');
    expect(todoMilestoneLabel('not-json')).toBe('更新待办');
    expect(subagentDispatchSummary('探索后端', 'prompt')).toBe('探索后端');
    expect(subagentDispatchSummary('', 'prompt')).toBe('prompt');
    expect(subagentDispatchSummary('', '')).toBe('子 Agent 任务');
    expect(subagentLiveSummary('最终摘要', [], 'done')).toBe('最终摘要');
    expect(subagentLiveSummary('a\nb', [], 'done')).toBe('已完成（a b）');
    expect(subagentLiveSummary('', [{ kind: 'text', text: 'working…', final: false }], 'running')).toBe('working…');
    expect(subagentLiveSummary('', [], 'running')).toBe('Planning next moves');
    expect(todoMilestoneLabel(JSON.stringify({ todos: [{ title: '写材质' }] }))).toBe('写材质');
    expect(isMilestoneBlock(tool('t9', 'todo_write', { todos: [] }, true))).toBe(true);
    expect(toolVisual('apply_patch')).toBe('Patched');
    expect(toolCategory('grep')).toBe('search');
    expect(toolDiffStats(JSON.stringify({
      patch: '*** Begin Patch\n*** Add File: a.txt\n+one\n+two\n*** End Patch',
    }))).toEqual({ added: 2, removed: 0 });
  });

  it('思考时长取事件 ts 之差;缺计时/倒挂不伪造秒数', () => {
    const at = (ms: number) => new Date(Date.parse('2026-09-03T10:00:00.000Z') + ms).toISOString();
    const think = (from: string | undefined, to: string | undefined): Extract<ChatBlock, { kind: 'reasoning' }> =>
      ({ kind: 'reasoning', text: '想', startedTs: from, endedTs: to });
    expect(reasoningDurationMs(think(at(0), at(12_400)))).toBe(12_400);
    expect(reasoningDurationMs(think(undefined, at(0)))).toBeNull();
    expect(reasoningDurationMs(think(at(500), at(0)))).toBeNull();
    expect(reasoningDurationMs(think('不是时间', at(0)))).toBeNull();
  });

  /// 目标截图逐字:「Thought 47s」「Thought briefly」「Thought 106s」(过分钟仍报秒)。
  it('思考行文案:无计时 Thought briefly;有计时进位到秒且不进位到分钟', () => {
    expect(thinkingLabel(null)).toBe('Thought briefly');
    expect(thinkingLabel(0)).toBe('Thought 1s');
    expect(thinkingLabel(12_400)).toBe('Thought 12s');
    expect(thinkingLabel(47_000)).toBe('Thought 47s');
    expect(thinkingLabel(106_000)).toBe('Thought 106s');
  });
});
