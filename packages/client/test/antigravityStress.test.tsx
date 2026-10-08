import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  AntigravityCard,
} from '@/components/settings/ModelsPage';
import ModelPicker from '@/components/chat/ModelPicker';
import { useChatStore, type SnapshotModel } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { mockForgeBackend } from './forgeMock';

const MOCK_ANTIGRAVITY_MODELS: SnapshotModel[] = [
  {
    id: 'antigravity/gemini-3.8-flash',
    label: 'gemini-3.8-flash',
    provider: 'antigravity',
    availability: 'available',
    group: 'Antigravity',
    supportsThinking: true,
    effortOptions: [
      { id: 'low', label: 'Low' },
      { id: 'medium', label: 'Medium' },
      { id: 'high', label: 'High' },
      { id: 'max', label: 'Max' },
    ],
    defaultEffort: 'medium',
    contextOptions: [
      { id: '128k', label: '128K', tokens: 131072 },
      { id: '1m', label: '1M', tokens: 1048576 },
    ],
    defaultContext: '1m',
  },
  {
    id: 'antigravity/gemini-3.8-pro-needs-config',
    label: 'gemini-3.8-pro (unconfigured)',
    provider: 'antigravity',
    availability: 'needs-config',
    group: 'Antigravity',
    supportsThinking: true,
    effortOptions: [{ id: 'medium', label: 'Medium' }],
    contextOptions: [{ id: '1m', label: '1M', tokens: 1048576 }],
    defaultContext: '1m',
  },
  {
    id: 'antigravity/gemini-3.8-pro-offline',
    label: 'gemini-3.8-pro (offline)',
    provider: 'antigravity',
    availability: 'offline',
    group: 'Antigravity',
    supportsThinking: true,
    effortOptions: [{ id: 'medium', label: 'Medium' }],
    contextOptions: [{ id: '1m', label: '1M', tokens: 1048576 }],
    defaultContext: '1m',
  },
  {
    id: 'antigravity/gemini-3.8-pro-disconnected',
    label: 'gemini-3.8-pro (disconnected)',
    provider: 'antigravity',
    availability: 'disconnected',
    group: 'Antigravity',
    supportsThinking: true,
    effortOptions: [{ id: 'medium', label: 'Medium' }],
    contextOptions: [{ id: '1m', label: '1M', tokens: 1048576 }],
    defaultContext: '1m',
  },
  {
    id: 'antigravity/gemini-no-effort',
    label: 'gemini-no-effort',
    provider: 'antigravity',
    availability: 'available',
    group: 'Antigravity',
    supportsThinking: true,
    effortOptions: [],
    contextOptions: [{ id: '1m', label: '1M', tokens: 1048576 }],
    defaultContext: '1m',
  },
  {
    id: 'antigravity/gemini-no-thinking',
    label: 'gemini-no-thinking',
    provider: 'antigravity',
    availability: 'available',
    group: 'Antigravity',
    supportsThinking: false,
    effortOptions: [],
    contextOptions: [{ id: '1m', label: '1M', tokens: 1048576 }],
    defaultContext: '1m',
  },
];

describe('Empirical Challenge: ModelPicker Antigravity Integration', () => {
  beforeEach(() => {
    useSessionStore.setState({ activeSessionId: 'sess_test_1' });
    useChatStore.setState({
      models: MOCK_ANTIGRAVITY_MODELS,
      defaultModelId: 'antigravity/gemini-3.8-flash',
      selectedModelId: 'antigravity/gemini-3.8-flash',
      thinkingEnabled: true,
      contextOptionId: '1m',
      reasoningEffort: 'high',
    });
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('Challenge 1: ModelPicker disables needs-config and offline models with exact tooltips and blocks selection', () => {
    const pickModel = vi.fn();
    useChatStore.setState({ pickModel });

    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.click(screen.getByTestId('spec-row-model'));

    // Verify needs-config model
    const unconfItem = screen.getByTestId('model-item-antigravity/gemini-3.8-pro-needs-config');
    expect(unconfItem).toBeDisabled();
    expect(unconfItem).toHaveAttribute('title', '未配置反代');
    expect(unconfItem).toHaveTextContent('antigravity · 未配置反代');
    fireEvent.click(unconfItem);
    expect(pickModel).not.toHaveBeenCalled();

    // Verify offline model
    const offlineItem = screen.getByTestId('model-item-antigravity/gemini-3.8-pro-offline');
    expect(offlineItem).toBeDisabled();
    expect(offlineItem).toHaveAttribute('title', '反代离线');
    expect(offlineItem).toHaveTextContent('antigravity · 反代离线');
    fireEvent.click(offlineItem);
    expect(pickModel).not.toHaveBeenCalled();

    // Verify disconnected model
    const disconnectedItem = screen.getByTestId('model-item-antigravity/gemini-3.8-pro-disconnected');
    expect(disconnectedItem).toBeDisabled();
    expect(disconnectedItem).toHaveAttribute('title', '反代离线');
    expect(disconnectedItem).toHaveTextContent('antigravity · 反代离线');
    fireEvent.click(disconnectedItem);
    expect(pickModel).not.toHaveBeenCalled();

    // Verify available model is clickable
    const availableItem = screen.getByTestId('model-item-antigravity/gemini-3.8-flash');
    expect(availableItem).not.toBeDisabled();
    expect(availableItem).not.toHaveAttribute('title');
    fireEvent.click(availableItem);
    expect(pickModel).toHaveBeenCalledWith('antigravity/gemini-3.8-flash');
  });

  it('Challenge 2: Thinking toggle, Reasoning Effort selector, and 1M context options work seamlessly', () => {
    const setThinking = vi.fn();
    const pickEffort = vi.fn();
    const pickContext = vi.fn();

    useChatStore.setState({
      setThinking,
      pickEffort,
      pickContext,
      thinkingEnabled: true,
      reasoningEffort: 'high',
      contextOptionId: '1m',
    });

    render(<ModelPicker />);

    // Chip should show suffix: 1M High
    expect(screen.getByTestId('composer-model')).toHaveTextContent('gemini-3.8-flash');
    expect(screen.getByTestId('composer-model-suffix')).toHaveTextContent('1M High');

    // Open main menu
    fireEvent.click(screen.getByTestId('composer-model'));

    // Check Thinking switch is active
    expect(screen.getByTestId('spec-thinking-switch')).toHaveAttribute('data-on', '1');
    fireEvent.click(screen.getByTestId('spec-row-thinking'));
    expect(setThinking).toHaveBeenCalledWith(false);

    // Check Effort submenu
    fireEvent.click(screen.getByTestId('spec-row-effort'));
    const effortSub = screen.getByTestId('spec-submenu-effort');
    expect(effortSub).toBeInTheDocument();
    expect(screen.getByTestId('effort-item-low')).toBeInTheDocument();
    expect(screen.getByTestId('effort-item-medium')).toBeInTheDocument();
    expect(screen.getByTestId('effort-item-high')).toBeInTheDocument();
    expect(screen.getByTestId('effort-item-max')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('effort-item-max'));
    expect(pickEffort).toHaveBeenCalledWith('max');

    // Check Context submenu (1M)
    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.click(screen.getByTestId('spec-row-context'));
    const contextSub = screen.getByTestId('spec-submenu-context');
    expect(contextSub).toBeInTheDocument();
    expect(screen.getByTestId('context-item-128k')).toBeInTheDocument();
    const oneMItem = screen.getByTestId('context-item-1m');
    expect(oneMItem).toBeInTheDocument();
    expect(oneMItem).toHaveTextContent('1M');

    fireEvent.click(oneMItem);
    expect(pickContext).toHaveBeenCalledWith('1m');
  });

  it('Challenge 3: Effort is disabled when thinking is disabled or model does not support effort', () => {
    // 1. Thinking disabled
    useChatStore.setState({ thinkingEnabled: false });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    const effortRowThinkingOff = screen.getByTestId('spec-row-effort');
    expect(effortRowThinkingOff).toBeDisabled();
    expect(effortRowThinkingOff).toHaveAttribute('title', '先开启 Thinking');
    cleanup();

    // 2. Model does not support effort
    useChatStore.setState({
      selectedModelId: 'antigravity/gemini-no-effort',
      thinkingEnabled: true,
    });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    const effortRowNoSupport = screen.getByTestId('spec-row-effort');
    expect(effortRowNoSupport).toBeDisabled();
    expect(effortRowNoSupport).toHaveAttribute('title', 'gemini-no-effort 不接受推理强度参数');
    cleanup();

    // 3. Model does not support thinking
    useChatStore.setState({
      selectedModelId: 'antigravity/gemini-no-thinking',
      thinkingEnabled: true,
    });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    const thinkingRowNoSupport = screen.getByTestId('spec-row-thinking');
    expect(thinkingRowNoSupport).toBeDisabled();
    expect(thinkingRowNoSupport).toHaveAttribute('title', 'gemini-no-thinking 不支持思考模式');
  });
});

describe('Empirical Challenge: Interactive Probing & Loading & Error Handling in AntigravityCard', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('Challenge 4: Probe button displays loading state while in-flight and prevents concurrent clicks', async () => {
    let resolveProbe: (val: unknown) => void = () => {};
    const probePromise = new Promise((res) => {
      resolveProbe = res;
    });

    let probeCallCount = 0;

    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === '/api/forge/llm/antigravity/status') {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            configured: false,
            baseUrl: 'http://127.0.0.1:8080',
            model: 'gemini-3.8-flash',
            keyConfigured: false,
          }),
        } as Response;
      }
      if (url === '/api/forge/llm/antigravity/probe') {
        probeCallCount++;
        const payload = await probePromise;
        return {
          ok: true,
          status: 200,
          json: async () => payload,
        } as Response;
      }
      throw new Error(`Unhandled URL: ${url}`);
    }));

    render(<AntigravityCard />);
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const probeBtn = screen.getByTestId('antigravity-probe-btn');
    expect(probeBtn).toHaveTextContent('测试连接');
    expect(probeBtn).not.toBeDisabled();

    // Click probe button to start in-flight request
    fireEvent.click(probeBtn);

    // During flight:
    await waitFor(() => {
      expect(probeBtn).toHaveTextContent('测试中…');
      expect(probeBtn).toBeDisabled();
    });
    expect(probeCallCount).toBe(1);

    // Attempt second click while probing
    fireEvent.click(probeBtn);
    expect(probeCallCount).toBe(1);

    // Complete the probe
    resolveProbe({
      ok: true,
      latencyMs: 120,
      status: 'available',
    });

    await waitFor(() => {
      expect(screen.getByTestId('antigravity-probe-feedback')).toHaveTextContent('✓ 连接正常 (120ms)');
    });

    // After flight: button returns to idle state and is enabled
    expect(probeBtn).toHaveTextContent('测试连接');
    expect(probeBtn).not.toBeDisabled();
  });

  it('Challenge 5: Probe handles success cleanly with latency badge and no latencyMs fallback', async () => {
    useToastStore.getState().clear();

    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': {
            configured: true,
            baseUrl: 'http://127.0.0.1:8080',
            model: 'gemini-3.8-flash',
            keyConfigured: true,
            latencyMs: 95,
          },
          '/api/forge/llm/antigravity/probe': {
            ok: true,
            status: 'available',
            // no latencyMs returned
          },
        },
      ),
    );

    render(<AntigravityCard />);
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const probeBtn = screen.getByTestId('antigravity-probe-btn');
    fireEvent.click(probeBtn);

    await waitFor(() => {
      const feedback = screen.getByTestId('antigravity-probe-feedback');
      expect(feedback).toHaveTextContent('✓ 连接正常');
      // Should not contain "undefinedms"
      expect(feedback.textContent).not.toContain('undefined');
    });

    // Check toast pushed
    const toasts = useToastStore.getState().items;
    expect(toasts.some((t) => t.title.includes('Antigravity 反代连接成功'))).toBe(true);
  });

  it('Challenge 6: Probe handles upstream API error cleanly and recovers', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': {
            configured: false,
            baseUrl: 'http://127.0.0.1:8080',
            model: 'gemini-3.8-flash',
            keyConfigured: false,
          },
          '/api/forge/llm/antigravity/probe': {
            ok: false,
            error: 'Upstream 429 Quota Exceeded',
          },
        },
      ),
    );

    render(<AntigravityCard />);
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const probeBtn = screen.getByTestId('antigravity-probe-btn');
    fireEvent.click(probeBtn);

    await waitFor(() => {
      const feedback = screen.getByTestId('antigravity-probe-feedback');
      expect(feedback).toHaveTextContent('✕ Upstream 429 Quota Exceeded');
      expect(feedback).toHaveClass('text-warn');
    });

    // Button should be re-enabled for retrying
    expect(probeBtn).not.toBeDisabled();
    expect(probeBtn).toHaveTextContent('测试连接');
  });

  it('Challenge 7: Probe handles network exceptions / fetch rejection gracefully', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': {
            configured: false,
            baseUrl: 'http://127.0.0.1:8080',
            model: 'gemini-3.8-flash',
            keyConfigured: false,
          },
          '/api/forge/llm/antigravity/probe': () => {
            throw new Error('ECONNREFUSED 127.0.0.1:8080');
          },
        },
      ),
    );

    render(<AntigravityCard />);
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const probeBtn = screen.getByTestId('antigravity-probe-btn');
    fireEvent.click(probeBtn);

    await waitFor(() => {
      const feedback = screen.getByTestId('antigravity-probe-feedback');
      expect(feedback).toHaveTextContent('✕ ECONNREFUSED 127.0.0.1:8080');
      expect(feedback).toHaveClass('text-warn');
    });

    expect(probeBtn).not.toBeDisabled();
    expect(probeBtn).toHaveTextContent('测试连接');
  });

  it('Challenge 8: Redline R-5 - Password masking and no plaintext leakage in inputs or status line', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': {
            configured: true,
            baseUrl: 'https://proxy.ai',
            model: 'gemini-3.8-flash',
            keyConfigured: true,
            availability: 'available',
          },
        },
      ),
    );

    render(<AntigravityCard />);

    // Wait for async status fetch
    const statusLine = await screen.findByTestId('antigravity-status-line');
    expect(statusLine).toHaveTextContent('key 已配置');
    expect(statusLine).toHaveTextContent('https://proxy.ai');
    expect(statusLine.textContent).not.toContain('sk-');

    // Open form
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));
    const keyInput = screen.getByTestId('antigravity-key-input');
    expect(keyInput).toHaveAttribute('type', 'password');
    expect(keyInput).toHaveValue(''); // Never pre-filled with plaintext
    expect(keyInput).toHaveAttribute('placeholder', expect.stringContaining('已配置; 留空保留既有'));
  });
});
