import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Composer from '@/components/chat/Composer';
import { useAssetStore } from '@/lib/assetStore';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import {
  BASE_PROMPT_TOKENS,
  DEFAULT_CONTEXT_WINDOW,
  computeContextUsage,
  estimateTokens,
  formatTokens,
} from '@/lib/contextUsage';
import { useEditorStore } from '@/lib/editorStore';
import { useSessionStore } from '@/lib/sessionStore';
import { mockForgeBackend } from './forgeMock';

/**
 * Composer 上下文计量环(模型钮右侧灰环 + 百分比)与胶囊上方占用明细表。
 * 覆盖:估算口径 / 实测倒算校准 / 明细行分类与降序 / 环填充与开合 / 面板落位在胶囊之上。
 */

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialEditor = useEditorStore.getState();
const initialAssets = useAssetStore.getState();

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useEditorStore.setState(initialEditor, true);
  useAssetStore.setState(initialAssets, true);
  useSessionStore.setState({ activeSessionId: 'sess_1' });
  useChatStore.setState({
    models: [{ id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' }],
    defaultModelId: 'mock',
    selectedModelId: 'mock',
  });
  vi.stubGlobal('fetch', mockForgeBackend({}, { '/api/forge/skills/list': { skills: [] } }));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function assistantWith(blocks: ChatMsg['blocks']): ChatMsg {
  return { id: 'a1', role: 'assistant', text: '', blocks, time: '10:00', runId: 'run_1' };
}

describe('contextUsage 估算', () => {
  it('中文 0.6 / 其余 0.3 逐字向上取整;formatTokens 分档', () => {
    expect(estimateTokens('')).toBe(0);
    expect(estimateTokens('你好')).toBe(2); // ceil(1.2)
    expect(estimateTokens('abcdefghij')).toBe(3); // ceil(3.0)
    expect(formatTokens(999)).toBe('999');
    expect(formatTokens(1234)).toBe('1.2k');
    expect(formatTokens(65536)).toBe('66k');
  });

  it('无实测:系统行走静态基线;文件/工具/对话/技能/草稿各成行并按 token 降序', () => {
    const usage = computeContextUsage(
      [
        { id: 'u1', role: 'user', text: '把箱子挪到原点', blocks: [], time: '10:00' },
        assistantWith([
          { kind: 'reasoning', text: '先读文件' },
          {
            kind: 'tool',
            toolCallId: 'c1',
            name: 'read_file',
            args: JSON.stringify({ path: 'Content/Scripts/box.rx' }),
            result: 'x'.repeat(400),
            ok: true,
            mcp: null,
          },
          {
            kind: 'tool',
            toolCallId: 'c2',
            name: 'mcp__engine-scene__entity_list',
            args: '{}',
            result: 'entities: 3',
            ok: true,
            mcp: ['engine-scene', 'entity_list'],
          },
          { kind: 'text', text: '已完成', final: true },
        ]),
      ],
      {
        skills: ['scene-greybox'],
        draft: '再加个碰撞体',
        contextWindow: DEFAULT_CONTEXT_WINDOW,
        measured: 0,
      },
    );

    const byKey = new Map(usage.rows.map((r) => [r.key, r]));
    expect(byKey.get('base')?.tokens).toBe(BASE_PROMPT_TOKENS);
    expect(byKey.get('file:Content/Scripts/box.rx')?.label).toBe('box.rx');
    // 工具行标签取 timeline 动词表(2026-09-03 起过程链文案统一英文)
    expect(byKey.get('tool:Listed entities')).toBeDefined();
    expect(byKey.get('skill:scene-greybox')).toBeDefined();
    expect(byKey.get('draft')).toBeDefined();
    expect(byKey.get('reasoning')).toBeDefined();
    // 降序 + 零占用行剔除 + 合计=各行和
    const tokens = usage.rows.map((r) => r.tokens);
    expect([...tokens].sort((a, b) => b - a)).toEqual(tokens);
    expect(tokens.every((t) => t > 0)).toBe(true);
    expect(usage.used).toBe(tokens.reduce((s, t) => s + t, 0));
    expect(usage.window).toBe(DEFAULT_CONTEXT_WINDOW);
    expect(usage.calibrated).toBe(false);
  });

  it('有实测:系统行 = 实测 prompt − 可归因估算(倒算),窗口取所选 Context 档', () => {
    const messages: ChatMsg[] = [
      { id: 'u1', role: 'user', text: '你好', blocks: [], time: '10:00' },
    ];
    const attributed = estimateTokens('你好');
    const usage = computeContextUsage(messages, {
      skills: [],
      draft: '',
      contextWindow: 65536,
      measured: 9000,
    });
    expect(usage.calibrated).toBe(true);
    expect(usage.rows.find((r) => r.key === 'base')?.tokens).toBe(9000 - attributed);
    expect(usage.used).toBe(9000);
    expect(usage.window).toBe(65536);
    expect(usage.percent).toBe(Math.round((9000 / 65536) * 100));
  });

  it('实测小于可归因时基线夹到 0,ratio 超窗封顶 1 而 percent 如实 > 100', () => {
    const huge = 'x'.repeat(400000); // ≈ 120k tokens > 64k 窗口
    const usage = computeContextUsage([{ id: 'u1', role: 'user', text: huge, blocks: [], time: '' }], {
      skills: [],
      draft: '',
      contextWindow: 65536,
      measured: 10,
    });
    expect(usage.rows.find((r) => r.key === 'base')).toBeUndefined(); // 夹 0 → 零行剔除
    expect(usage.ratio).toBe(1);
    expect(usage.percent).toBeGreaterThan(100);
  });
});

describe('<Composer /> 上下文计量环', () => {
  it('环落在模型钮右侧的工具行;显示百分比;填充弧长随占有率', () => {
    render(<Composer />);
    const tools = screen.getByTestId('composer-tools');
    const meter = screen.getByTestId('composer-context');
    expect(tools).toContainElement(meter);
    expect(meter.compareDocumentPosition(screen.getByTestId('composer-model'))).toBe(
      Node.DOCUMENT_POSITION_PRECEDING,
    );
    // 空会话:仅系统基线 8000 / 64K ≈ 12%
    const pct = Math.round((BASE_PROMPT_TOKENS / DEFAULT_CONTEXT_WINDOW) * 100);
    expect(screen.getByTestId('composer-context-percent')).toHaveTextContent(`${pct}%`);
    const fill = screen.getByTestId('context-ring-fill');
    const [dash, gap] = (fill.getAttribute('stroke-dasharray') ?? '').split(' ').map(Number);
    expect(dash / gap).toBeCloseTo(BASE_PROMPT_TOKENS / DEFAULT_CONTEXT_WINDOW, 3);
  });

  it('点击在胶囊上方展开明细表;X 关闭;再点环也关闭', () => {
    render(<Composer />);
    expect(screen.queryByTestId('composer-context-panel')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-context'));
    const panel = screen.getByTestId('composer-context-panel');
    // 落位:胶囊之前(DOM 顺序 = 视觉上方)
    expect(panel.compareDocumentPosition(screen.getByTestId('composer-capsule'))).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(within(panel).getByText('上下文窗口')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-context-close'));
    expect(screen.queryByTestId('composer-context-panel')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-context'));
    fireEvent.click(screen.getByTestId('composer-context'));
    expect(screen.queryByTestId('composer-context-panel')).not.toBeInTheDocument();
  });

  it('表内含文件行与草稿行;上下文引用行随 chip 退役消失', () => {
    useChatStore.setState({
      messages: [
        assistantWith([
          {
            kind: 'tool',
            toolCallId: 'c1',
            name: 'write_file',
            args: JSON.stringify({ path: 'Content/a.rx', content: 'fn main() {}' }),
            ok: true,
            mcp: null,
          },
        ]),
      ],
    });
    useEditorStore.setState({ sceneName: 'Main' });
    render(<Composer />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '继续' } });
    fireEvent.click(screen.getByTestId('composer-context'));
    const panel = screen.getByTestId('composer-context-panel');
    expect(within(panel).getByText('a.rx')).toBeInTheDocument();
    expect(within(panel).getByText('当前草稿')).toBeInTheDocument();
    expect(within(panel).queryByText('@Main')).toBeNull();
  });
});
