import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Composer from '@/components/chat/Composer';
import MessageList from '@/components/chat/MessageList';
import { useAssetStore } from '@/lib/assetStore';
import { useChatStore, type ChatMsg, type ForgeEventWire } from '@/lib/chatStore';
import {
  BASE_PROMPT_TOKENS,
  DEFAULT_CONTEXT_WINDOW,
  computeContextUsage,
  estimateTokens,
  formatTokens,
  messagesAfter,
  percentLabel,
} from '@/lib/contextUsage';
import { useEditorStore } from '@/lib/editorStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { mockForgeBackend } from './forgeMock';

/**
 * Composer 上下文计量环(模型钮右侧灰环 + 百分比)与胶囊上方面板。
 * 覆盖:估算口径 / 实测校准 / 三行分项 / 压缩分隔点之后才计入 / Codex 用量取最近一次请求 /
 * 环填充与开合 / 面板落位 / 压缩上下文按钮的可用性、请求与分隔线。
 */

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialEditor = useEditorStore.getState();
const initialAssets = useAssetStore.getState();
const initialToasts = useToastStore.getState();

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useEditorStore.setState(initialEditor, true);
  useAssetStore.setState(initialAssets, true);
  useToastStore.setState(initialToasts, true);
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

function assistantWith(blocks: ChatMsg['blocks'], id = 'a1'): ChatMsg {
  return { id, role: 'assistant', text: '', blocks, time: '10:00', runId: 'run_1' };
}

const userMsg = (id: string, text: string): ChatMsg => ({ id, role: 'user', text, blocks: [], time: '10:00' });

function event(type: string, payload: Record<string, unknown>, seq = 1): ForgeEventWire {
  return { id: `evt_${seq}`, sessionId: 'sess_1', seq, type, ts: '2026-10-07T10:00:00Z', payload };
}

describe('contextUsage 估算', () => {
  it('中文 0.6 / 其余 0.3 逐字向上取整;formatTokens 分档;不足 1% 写 <1%', () => {
    expect(estimateTokens('')).toBe(0);
    expect(estimateTokens('你好')).toBe(2); // ceil(1.2)
    expect(estimateTokens('abcdefghij')).toBe(3); // ceil(3.0)
    expect(formatTokens(999)).toBe('999');
    expect(formatTokens(1234)).toBe('1.2k');
    expect(formatTokens(65536)).toBe('66k');
    expect(formatTokens(1_048_576)).toBe('1M');
    expect(formatTokens(1_500_000)).toBe('1.5M');
    const tiny = computeContextUsage([], { skills: [], draft: '', contextWindow: 1_048_576, measured: 0 });
    expect(tiny.percent).toBe(1);
    const tinier = computeContextUsage([], { skills: [], draft: '', contextWindow: 10_000_000, measured: 0 });
    expect(percentLabel(tinier)).toBe('<1%');
  });

  it('无实测:三行 = 静态基线 / 对话历史(消息、工具结果、思考合计) / 待发送(草稿 + 技能)', () => {
    const toolResult = 'x'.repeat(400);
    const usage = computeContextUsage(
      [
        userMsg('u1', '把箱子挪到原点'),
        assistantWith([
          { kind: 'reasoning', text: '先读文件' },
          {
            kind: 'tool',
            toolCallId: 'c1',
            name: 'read_file',
            args: JSON.stringify({ path: 'Content/Scripts/box.rx' }),
            result: toolResult,
            ok: true,
            mcp: null,
          },
          { kind: 'text', text: '已完成', final: true },
        ]),
      ],
      { skills: ['scene-greybox'], draft: '再加个碰撞体', contextWindow: DEFAULT_CONTEXT_WINDOW, measured: 0 },
    );

    expect(usage.rows.map((r) => r.kind)).toEqual(['base', 'history', 'input']);
    const byKind = new Map(usage.rows.map((r) => [r.kind, r.tokens]));
    expect(byKind.get('base')).toBe(BASE_PROMPT_TOKENS);
    const history =
      estimateTokens('把箱子挪到原点') +
      estimateTokens('先读文件') +
      estimateTokens(JSON.stringify({ path: 'Content/Scripts/box.rx' })) +
      estimateTokens(toolResult) +
      estimateTokens('已完成');
    expect(byKind.get('history')).toBe(history);
    expect(byKind.get('input')).toBe(estimateTokens('再加个碰撞体') + estimateTokens('scene-greybox') + 2);
    expect(usage.used).toBe(BASE_PROMPT_TOKENS + history + (byKind.get('input') ?? 0));
    expect(usage.messageCount).toBe(2);
    expect(usage.calibrated).toBe(false);
  });

  it('有实测:以实测为准,按静态基线切出系统行、其余归对话历史;窗口取调用方给的值', () => {
    const usage = computeContextUsage([userMsg('u1', '你好')], {
      skills: [],
      draft: '',
      contextWindow: 65536,
      measured: 20000,
    });
    expect(usage.calibrated).toBe(true);
    expect(usage.rows.find((r) => r.kind === 'base')?.tokens).toBe(BASE_PROMPT_TOKENS);
    expect(usage.rows.find((r) => r.kind === 'history')?.tokens).toBe(20000 - BASE_PROMPT_TOKENS);
    expect(usage.used).toBe(20000);
    expect(usage.percent).toBe(Math.round((20000 / 65536) * 100));
  });

  it('超窗:ratio 封顶 1 而 percent 如实 > 100', () => {
    const huge = 'x'.repeat(400000); // ≈ 120k tokens > 64k 窗口
    const usage = computeContextUsage([userMsg('u1', huge)], {
      skills: [],
      draft: '',
      contextWindow: 65536,
      measured: 0,
    });
    expect(usage.ratio).toBe(1);
    expect(usage.percent).toBeGreaterThan(100);
  });

  it('压缩分隔点:只估算其后的消息,再加摘要体量', () => {
    const messages = [userMsg('u1', 'x'.repeat(3000)), assistantWith([{ kind: 'text', text: '好', final: true }]), userMsg('u2', '继续')];
    expect(messagesAfter(messages, undefined)).toHaveLength(3);
    expect(messagesAfter(messages, 'a1').map((m) => m.id)).toEqual(['u2']);
    expect(messagesAfter(messages, 'gone')).toHaveLength(3);
    const usage = computeContextUsage(messagesAfter(messages, 'a1'), {
      skills: [],
      draft: '',
      contextWindow: 65536,
      measured: 0,
      summaryTokens: 300,
      compacted: true,
    });
    expect(usage.rows.find((r) => r.kind === 'history')?.tokens).toBe(estimateTokens('继续') + 300);
    expect(usage.messageCount).toBe(1);
    expect(usage.compacted).toBe(true);
  });
});

describe('chatStore:用量与压缩事件', () => {
  it('Codex 用量取最近一次请求(last)而非线程累计,并带回模型窗口', () => {
    useChatStore.getState().applyEvent(
      event('agent.usage', {
        promptTokens: 900_000,
        last: { promptTokens: 42_000, completionTokens: 300 },
        modelContextWindow: 272_000,
      }),
    );
    expect(useChatStore.getState().lastPromptTokens).toBe(42_000);
    expect(useChatStore.getState().usageContextWindow).toBe(272_000);
    // 本地引擎:无 last,沿用 promptTokens;窗口保持上次的值
    useChatStore.getState().applyEvent(event('agent.usage', { promptTokens: 12_000 }, 2));
    expect(useChatStore.getState().lastPromptTokens).toBe(12_000);
    expect(useChatStore.getState().usageContextWindow).toBe(272_000);
  });

  it('context.compacted:在最后一条消息后记分隔点,实测作废改走估算', () => {
    useChatStore.setState({ messages: [userMsg('u1', '你好'), assistantWith([{ kind: 'text', text: '在', final: true }])], lastPromptTokens: 30_000 });
    useChatStore.getState().applyEvent(event('context.compacted', { engine: 'local', turns: 1, tokensAfter: 120 }));
    const st = useChatStore.getState();
    expect(st.compactions).toEqual([{ afterMessageId: 'a1', ts: '2026-10-07T10:00:00Z', engine: 'local', summaryTokens: 120 }]);
    expect(st.lastPromptTokens).toBe(0);
  });
});

describe('<Composer /> 上下文计量环与面板', () => {
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

  it('点击在胶囊上方展开面板;X 关闭;再点环也关闭', () => {
    render(<Composer />);
    expect(screen.queryByTestId('composer-context-panel')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-context'));
    const panel = screen.getByTestId('composer-context-panel');
    // 落位:胶囊之前(DOM 顺序 = 视觉上方)
    expect(panel.compareDocumentPosition(screen.getByTestId('composer-capsule'))).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(within(panel).getByText('上下文')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-context-close'));
    expect(screen.queryByTestId('composer-context-panel')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-context'));
    fireEvent.click(screen.getByTestId('composer-context'));
    expect(screen.queryByTestId('composer-context-panel')).not.toBeInTheDocument();
  });

  it('面板只列三行分项,不再按文件 / 工具逐条列出', () => {
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
    render(<Composer />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '继续' } });
    fireEvent.click(screen.getByTestId('composer-context'));
    const panel = screen.getByTestId('composer-context-panel');
    expect(within(panel).getByTestId('context-row-base')).toHaveTextContent('系统提示与工具');
    expect(within(panel).getByTestId('context-row-history')).toHaveTextContent('对话历史');
    expect(within(panel).getByTestId('context-row-input')).toHaveTextContent('待发送');
    expect(within(panel).queryByText('a.rx')).toBeNull();
  });

  it('没有可压缩的对话时压缩钮禁用并说明原因', () => {
    useChatStore.setState({ currentSessionId: 'sess_1' });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-context'));
    expect(screen.getByTestId('composer-context-compact')).toBeDisabled();
    expect(screen.getByTestId('composer-context-hint')).toHaveTextContent('还没有可压缩的对话');
  });

  it('Agent 运行中压缩钮禁用', () => {
    useChatStore.setState({ currentSessionId: 'sess_1', activeRunId: 'run_9', messages: [userMsg('u1', '你好')] });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-context'));
    expect(screen.getByTestId('composer-context-compact')).toBeDisabled();
    expect(screen.getByTestId('composer-context-hint')).toHaveTextContent('Agent 正在运行');
  });

  it('点压缩:POST /compact,在途时暂停发送,成功 toast;分隔线随 context.compacted 出现', async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const calls: Array<{ url: string; method?: string }> = [];
    const base = mockForgeBackend({}, { '/api/forge/skills/list': { skills: [] } });
    vi.stubGlobal('fetch', async (url: unknown, init?: { method?: string; body?: string }) => {
      if (String(url).endsWith('/compact')) {
        calls.push({ url: String(url), method: init?.method });
        await gate;
        return { ok: true, status: 200, json: async () => ({ engine: 'local', manual: true, turns: 2, tokensBefore: 900, tokensAfter: 120 }) } as Response;
      }
      return base(url, init);
    });
    useChatStore.setState({
      currentSessionId: 'sess_1',
      messages: [userMsg('u1', '你好'), assistantWith([{ kind: 'text', text: '在', final: true }])],
    });
    render(<Composer />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '下一步' } });
    fireEvent.click(screen.getByTestId('composer-context'));
    fireEvent.click(screen.getByTestId('composer-context-compact'));
    await waitFor(() => expect(screen.getByTestId('composer-context-compact')).toHaveTextContent('压缩中'));
    expect(calls).toEqual([{ url: '/api/forge/sessions/sess_1/compact', method: 'POST' }]);
    expect(screen.getByTestId('composer-send')).toBeDisabled();
    await act(async () => {
      release();
      await gate;
    });
    await waitFor(() => expect(useChatStore.getState().compactingSessionId).toBeNull());
    expect(useToastStore.getState().items.some((t) => t.title.includes('2 轮对话已总结为摘要'))).toBe(true);
    expect(screen.getByTestId('composer-send')).not.toBeDisabled();

    cleanup();
    act(() => {
      useChatStore.getState().applyEvent(event('context.compacted', { engine: 'local', turns: 2, tokensAfter: 120 }));
    });
    render(<MessageList />);
    const divider = screen.getByTestId('context-compacted-divider');
    expect(divider).toHaveTextContent('上下文已压缩');
    const list = screen.getByTestId('message-list-content');
    expect(list.lastElementChild).toBe(divider);
  });

  it('没有新对话可压缩(409 AGENT_COMPACT_NOTHING)只提示,不报错', async () => {
    const base = mockForgeBackend({}, { '/api/forge/skills/list': { skills: [] } });
    vi.stubGlobal('fetch', async (url: unknown, init?: { method?: string; body?: string }) => {
      if (String(url).endsWith('/compact')) {
        return {
          ok: false,
          status: 409,
          json: async () => ({ error: { code: 'AGENT_COMPACT_NOTHING', message: '还没有可压缩的对话(或刚压缩过)' } }),
        } as Response;
      }
      return base(url, init);
    });
    useChatStore.setState({ currentSessionId: 'sess_1', messages: [userMsg('u1', '你好')] });
    await act(async () => {
      await useChatStore.getState().compactContext();
    });
    const toasts = useToastStore.getState().items;
    expect(toasts.map((t) => t.kind)).toEqual(['info']);
    expect(toasts[0].title).toBe('没有需要压缩的新对话');
  });
});
