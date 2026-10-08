import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ModelPicker from '@/components/chat/ModelPicker';
import { useChatStore, type SnapshotModel } from '@/lib/chatStore';
import { resolveModelSpec, DEFAULT_CONTEXT_WINDOW } from '@/lib/modelSpec';
import { useSessionStore } from '@/lib/sessionStore';

/**
 * 模型规格选择器(Cursor 式 Thinking·Context·Effort·Model 四行菜单)。
 * 覆盖:chip 规格后缀 / 四行主菜单与子菜单 / 三档能力驱动的禁用态 / 四个选择的 PATCH 出口 /
 * 与后端 modelspec::resolve 同规则的客户端归一(越界档位回落)。
 */

/** 与 agentd modelspec.rs CATALOG 同形的三条(能力面即测试夹具)。 */
const MODELS: SnapshotModel[] = [
  {
    id: 'deepseek-chat',
    label: 'deepseek-chat',
    provider: 'deepseek',
    availability: 'needs-key',
    group: 'DeepSeek',
    supportsThinking: true,
    effortOptions: [],
    defaultEffort: null,
    contextOptions: [{ id: '64k', label: '64K', tokens: 65536 }],
    defaultContext: '64k',
  },
  {
    id: 'mock',
    label: 'Mock provider',
    provider: 'mock',
    availability: 'available',
    group: '本地',
    supportsThinking: false,
    effortOptions: [],
    contextOptions: [{ id: '64k', label: '64K', tokens: 65536 }],
    defaultContext: '64k',
  },
  {
    id: 'openai-compat',
    label: 'qwen2.5-7b',
    provider: 'openai-compat',
    availability: 'available',
    group: '自定义渠道',
    supportsThinking: true,
    effortOptions: [
      { id: 'low', label: 'Low' },
      { id: 'medium', label: 'Medium' },
      { id: 'high', label: 'High' },
      { id: 'xhigh', label: 'Extra High' },
      { id: 'max', label: 'Max' },
    ],
    defaultEffort: 'medium',
    contextOptions: [
      { id: '64k', label: '64K', tokens: 65536 },
      { id: '300k', label: '300K', tokens: 307200 },
      { id: '1m', label: '1M', tokens: 1048576 },
    ],
    defaultContext: '64k',
  },
];

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useSessionStore.setState({ activeSessionId: 'sess_1' });
  useChatStore.setState({
    models: MODELS,
    defaultModelId: 'deepseek-chat',
    selectedModelId: 'openai-compat',
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

/** 开主菜单并进到指定子菜单。 */
function openSub(row: 'context' | 'effort' | 'model') {
  fireEvent.click(screen.getByTestId('composer-model'));
  fireEvent.click(screen.getByTestId(`spec-row-${row}`));
}

describe('resolveModelSpec 归一(与后端 modelspec::resolve 同规则)', () => {
  it('思考开 + 选档 → 该档生效;后缀为「窗口 强度」', () => {
    const s = resolveModelSpec(MODELS, 'openai-compat', true, 'xhigh', '1m');
    expect(s.thinking).toBe(true);
    expect(s.effort?.id).toBe('xhigh');
    expect(s.contextTokens).toBe(1048576);
    expect(s.suffix).toBe('1M Extra High');
  });

  it('思考关 → Effort 不生效,后缀只剩窗口档', () => {
    const s = resolveModelSpec(MODELS, 'openai-compat', false, 'xhigh', '1m');
    expect(s.thinking).toBe(false);
    expect(s.effortSupported).toBe(true);
    expect(s.suffix).toBe('1M');
  });

  it('越界档位回落该模型默认档,不沿用上一模型的选择', () => {
    const s = resolveModelSpec(MODELS, 'openai-compat', true, '不存在', '9m');
    expect(s.effort?.id).toBe('medium');
    expect(s.context?.id).toBe('64k');
  });

  it('模型不支持思考 → 开关值被忽略;Effort 整体不可用', () => {
    const s = resolveModelSpec(MODELS, 'mock', true, 'max', '64k');
    expect(s.thinking).toBe(false);
    expect(s.thinkingSupported).toBe(false);
    expect(s.effortSupported).toBe(false);
  });

  it('deepseek 支持思考但不收 reasoning_effort → Effort 档为空', () => {
    const s = resolveModelSpec(MODELS, 'deepseek-chat', true, 'max', null);
    expect(s.thinking).toBe(true);
    expect(s.effortSupported).toBe(false);
    expect(s.suffix).toBe('64K');
  });

  it('模型不在清单里 → 全档回落,窗口走默认', () => {
    const s = resolveModelSpec(MODELS, 'gpt-9', true, 'max', '1m');
    expect(s.modelLabel).toBe('gpt-9');
    expect(s.contextTokens).toBe(DEFAULT_CONTEXT_WINDOW);
    expect(s.suffix).toBe('');
  });
});

describe('<ModelPicker /> chip', () => {
  it('显示模型 label + 淡色规格后缀', () => {
    useChatStore.setState({ thinkingEnabled: true, reasoningEffort: 'max', contextOptionId: '1m' });
    render(<ModelPicker />);
    expect(screen.getByTestId('composer-model')).toHaveTextContent('qwen2.5-7b');
    expect(screen.getByTestId('composer-model-suffix')).toHaveTextContent('1M Max');
  });

  it('无可显示档位时不出后缀', () => {
    useChatStore.setState({ selectedModelId: 'gpt-9' });
    render(<ModelPicker />);
    expect(screen.queryByTestId('composer-model-suffix')).not.toBeInTheDocument();
  });

  it('Codex 冷启动会先预热动态模型，未显式选择时如实显示自动', async () => {
    useChatStore.setState({
      models: MODELS,
      defaultModelId: 'openai-compat',
      selectedModelId: null,
    });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      const body = url.startsWith('/api/forge/codex/models')
        ? { ok: true, models: [{ id: 'gpt-5.6-sol', displayName: 'GPT-5.6-Sol' }] }
        : url === '/api/forge/design-snapshot'
          ? {
              models: {
                models: [
                  ...MODELS,
                  {
                    id: 'codex:gpt-5.6-sol',
                    label: 'GPT-5.6-Sol',
                    provider: 'codex',
                    availability: 'available',
                  },
                ],
                defaultModelId: 'openai-compat',
              },
            }
          : null;
      if (body === null) throw new Error(`未 mock: ${url}`);
      return { ok: true, status: 200, json: async () => body } as Response;
    }));

    render(<ModelPicker provider="codex" />);
    expect(screen.getByTestId('composer-model')).toHaveTextContent('自动');
    await waitFor(() => {
      expect(useChatStore.getState().models.some((model) => model.provider === 'codex')).toBe(true);
    });
    expect(vi.mocked(fetch).mock.calls.some(([url]) => String(url).startsWith('/api/forge/codex/models'))).toBe(true);

    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.click(screen.getByTestId('spec-row-model'));
    expect(screen.getByTestId('model-item-auto')).toHaveTextContent('使用 Codex 默认模型');
    expect(screen.getByTestId('model-item-auto')).toHaveTextContent('自动');
    expect(screen.getByTestId('model-item-codex:gpt-5.6-sol')).toBeInTheDocument();
  });

  it('运行期禁用模型选择，不打开菜单', () => {
    render(<ModelPicker disabled />);
    const trigger = screen.getByTestId('composer-model');
    expect(trigger).toBeDisabled();
    fireEvent.click(trigger);
    expect(screen.queryByTestId('composer-model-menu')).not.toBeInTheDocument();
  });
});

describe('<ModelPicker /> 主菜单四行', () => {
  it('始终思考的云模型显示开启，保留强度选择，不发送关闭请求', () => {
    const setThinking = vi.fn();
    const model: SnapshotModel = {
      ...MODELS[2], id: 'cloud:claude-opus-5-5', label: 'Claude Opus 5.5',
      provider: 'cloud', thinkingMode: 'adaptive', thinkingAlwaysOn: true,
    };
    useChatStore.setState({ models: [model], selectedModelId: model.id, thinkingEnabled: false, setThinking });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    expect(screen.getByTestId('spec-row-thinking-value')).toHaveTextContent('始终开启');
    expect(screen.getByTestId('spec-thinking-switch')).toHaveAttribute('data-on');
    expect(screen.getByTestId('spec-row-thinking')).toBeDisabled();
    expect(screen.getByTestId('spec-row-effort')).not.toBeDisabled();
    fireEvent.click(screen.getByTestId('spec-row-thinking'));
    expect(setThinking).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId('spec-row-effort'));
    expect(screen.getByTestId('spec-submenu-effort')).toBeInTheDocument();
  });

  it('四行齐备,前三行显示当前值,Model 行显示模型名', () => {
    useChatStore.setState({ thinkingEnabled: true, contextOptionId: '300k' });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    expect(screen.getByTestId('spec-row-thinking')).toBeInTheDocument();
    expect(screen.getByTestId('spec-row-context-value')).toHaveTextContent('300K');
    expect(screen.getByTestId('spec-row-effort-value')).toHaveTextContent('Medium');
    expect(screen.getByTestId('spec-row-model-value')).toHaveTextContent('qwen2.5-7b');
  });

  it('Thinking:开关反映状态,点击调 setThinking 取反', () => {
    const setThinking = vi.fn();
    useChatStore.setState({ setThinking, thinkingEnabled: false });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    expect(screen.getByTestId('spec-thinking-switch')).not.toHaveAttribute('data-on');
    fireEvent.click(screen.getByTestId('spec-row-thinking'));
    expect(setThinking).toHaveBeenCalledWith(true);
  });

  it('Thinking:模型不支持时整行禁用并给出原因', () => {
    useChatStore.setState({ selectedModelId: 'mock' });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    const row = screen.getByTestId('spec-row-thinking');
    expect(row).toBeDisabled();
    expect(row).toHaveAttribute('title', 'Mock provider 不支持思考模式');
  });

  it('Effort:思考关着时禁用并提示先开 Thinking;deepseek 则如实说不收该参数', () => {
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    expect(screen.getByTestId('spec-row-effort')).toHaveAttribute('title', '先开启 Thinking');
    cleanup();

    useChatStore.setState({ selectedModelId: 'deepseek-chat', thinkingEnabled: true });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    expect(screen.getByTestId('spec-row-effort')).toHaveAttribute(
      'title',
      'deepseek-chat 不接受推理强度参数',
    );
  });
});

describe('<ModelPicker /> 子菜单', () => {
  it('Context:列出档位、标注当前档、点击 PATCH 并关菜单', () => {
    const pickContext = vi.fn();
    useChatStore.setState({ pickContext, contextOptionId: '300k' });
    render(<ModelPicker />);
    openSub('context');
    const panel = screen.getByTestId('spec-submenu-context');
    expect(panel).toHaveTextContent('64K');
    expect(panel).toHaveTextContent('1M');
    fireEvent.click(screen.getByTestId('context-item-1m'));
    expect(pickContext).toHaveBeenCalledWith('1m');
    expect(screen.queryByTestId('composer-model-menu')).not.toBeInTheDocument();
  });

  it('Effort:思考开着时五档可选,点击 PATCH', () => {
    const pickEffort = vi.fn();
    useChatStore.setState({ pickEffort, thinkingEnabled: true });
    render(<ModelPicker />);
    openSub('effort');
    for (const id of ['low', 'medium', 'high', 'xhigh', 'max']) {
      expect(screen.getByTestId(`effort-item-${id}`)).toBeInTheDocument();
    }
    expect(screen.getByTestId('effort-item-xhigh')).toHaveTextContent('Extra High');
    fireEvent.click(screen.getByTestId('effort-item-max'));
    expect(pickEffort).toHaveBeenCalledWith('max');
  });

  it('Model:按 group 分组,搜索过滤,needs-key 禁用', () => {
    render(<ModelPicker />);
    openSub('model');
    expect(screen.getByText('DeepSeek')).toBeInTheDocument();
    expect(screen.getByText('自定义渠道')).toBeInTheDocument();
    expect(screen.getByTestId('model-item-deepseek-chat')).toBeDisabled();
    // 搜索按 id/label/provider 命中
    fireEvent.change(screen.getByTestId('model-search'), { target: { value: 'qwen' } });
    expect(screen.getByTestId('model-item-openai-compat')).toBeInTheDocument();
    expect(screen.queryByTestId('model-item-mock')).not.toBeInTheDocument();
    fireEvent.change(screen.getByTestId('model-search'), { target: { value: '找不到' } });
    expect(screen.getByText('无匹配模型')).toBeInTheDocument();
  });

  it('hover 展开;hover 后再 click 不会把子菜单收回去', () => {
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.mouseEnter(screen.getByTestId('spec-row-model'));
    expect(screen.getByTestId('spec-submenu-model')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('spec-row-model'));
    expect(screen.getByTestId('spec-submenu-model')).toBeInTheDocument();
    // hover 到另一行 → 换页;hover 到 Thinking 行 → 收起
    fireEvent.mouseEnter(screen.getByTestId('spec-row-context'));
    expect(screen.getByTestId('spec-submenu-context')).toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByTestId('spec-row-thinking'));
    expect(screen.queryByTestId('spec-submenu-context')).not.toBeInTheDocument();
  });

  it('Esc 先收子菜单再关主菜单', () => {
    render(<ModelPicker />);
    openSub('model');
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByTestId('spec-submenu-model')).not.toBeInTheDocument();
    expect(screen.getByTestId('composer-model-menu')).toBeInTheDocument();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByTestId('composer-model-menu')).not.toBeInTheDocument();
  });
});
