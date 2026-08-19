import { describe, expect, it } from 'vitest';
import {
  argSummary,
  bareName,
  buildTimeline,
  diffCounts,
  editTargetFiles,
  ellipsize,
  groupPhrase,
  groupSegmentItems,
  isMilestoneBlock,
  mcpOf,
  reasoningSummary,
  runningLabel,
  segmentPhrase,
  segmentStats,
  subagentDispatchSummary,
  subagentLiveSummary,
  todoMilestoneLabel,
  toolCategory,
  toolDiffStats,
  toolSummary,
  toolVisual,
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

  it('本仓动词表:具名/通配/未知 MCP/原名', () => {
    expect(toolVisual('mcp__engine-scene__entity_create')).toBe('创建实体');
    expect(toolVisual('mcp__engine-scene__entity_destroy')).toBe('删除实体');
    expect(toolVisual('mcp__engine-scene__scene_save')).toBe('保存场景');
    expect(toolVisual('mcp__engine-scene__play_step')).toBe('步进');
    expect(toolVisual('mcp__engine-scene__viewport_set_camera')).toBe('视口操作');
    expect(toolVisual('mcp__asset-pipeline__asset_import')).toBe('资产操作');
    expect(toolVisual('mcp__gen-image__gen_image')).toBe('生成贴图');
    expect(toolVisual('mcp__code-forge__graph_validate')).toBe('校验图');
    expect(toolVisual('swarm.execute')).toBe('集群执行');
    // 未知 MCP 名 → server / tool
    expect(toolVisual('mcp__foo__bar_baz')).toBe('foo / bar_baz');
    // 非 MCP 未知名 → 原名
    expect(toolVisual('some_tool')).toBe('some_tool');
  });

  it('聚合短语 groupPhrase', () => {
    expect(groupPhrase('mcp__engine-scene__entity_create', 3)).toBe('创建 3 个实体');
    expect(groupPhrase('mcp__engine-scene__entity_list', 2)).toBe('列出实体 2 次');
    expect(groupPhrase('mcp__foo__bar', 2)).toBe('foo / bar ×2');
    expect(groupPhrase('custom_tool', 4)).toBe('custom_tool ×4');
  });
});

describe('build_timeline 段化规则', () => {
  it('连续普通 tool 合并;milestone(text/reasoning/subagent/write_todos/task)断段', () => {
    const blocks: ChatBlock[] = [
      tool('t1', 'mcp__engine-scene__entity_list', {}, true),
      tool('t2', 'mcp__engine-scene__entity_create', { name: 'a' }, true),
      { kind: 'reasoning', text: '思考' },
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
      { type: 'block', index: 4 },
      { type: 'activity', indices: [5] },
      { type: 'block', index: 6 },
    ]);
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
    expect(segmentPhrase(st)).toBe('编辑 1 个文件，探索 1 个文件，1 次搜索，执行 1 条命令');
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

  it('失败计数 + 运行中短语', () => {
    const blocks: ChatBlock[] = [
      tool('t1', 'mcp__engine-scene__entity_create', { name: 'a' }, false, 'boom'),
      tool('t2', 'mcp__engine-scene__entity_create', { name: 'b' }),
    ];
    const st = segmentStats(blocks, [0, 1]);
    expect(st.errors).toBe(1);
    expect(runningLabel(blocks, [0, 1])).toBe('正在创建实体…');
    // 全完成 → null
    expect(runningLabel([tool('t3', 'mcp__engine-scene__entity_list', {}, true)], [0])).toBeNull();
  });

  it('空段短语「工作中」', () => {
    expect(segmentPhrase(segmentStats([], []))).toBe('工作中');
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

  it('toolSummary:arg + 运行中/失败后缀;done 无结果摘要(本仓差异留痕)', () => {
    const running = tool('t1', 'mcp__engine-scene__entity_create', { name: 'a' });
    expect(toolSummary(running)).toBe('a · 运行中…');
    const failed = tool('t2', 'mcp__engine-scene__entity_create', { name: 'a' }, false, '首行错\n次行');
    expect(toolSummary(failed)).toBe('a · 失败：首行错');
    const failedNoMsg = tool('t3', 'mcp__engine-scene__entity_create', {}, false, '');
    expect(toolSummary(failedNoMsg)).toBe('失败');
    const done = tool('t4', 'mcp__engine-scene__entity_create', { name: 'a' }, true);
    expect(toolSummary(done)).toBe('a');
  });

  it('reasoningSummary/ellipsize/todoMilestoneLabel/subagent 摘要', () => {
    expect(reasoningSummary('')).toBe('0 字');
    // N 字 = 原文全字符数(参考 text.chars().count(),含空白换行)
    expect(reasoningSummary('  第一行\n第二行  ')).toBe('第一行 第二行 · 11 字');
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
    expect(subagentLiveSummary('', ['working…'], 'running')).toBe('working…');
    expect(subagentLiveSummary('', [], 'running')).toBe('Planning next moves');
  });
});
