import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ModelsPage, {
  AntigravityCard,
  AntigravityRateLimitBar,
  formatResetTime,
  parseAntigravityLimits,
} from '@/components/settings/ModelsPage';
import ModelPicker from '@/components/chat/ModelPicker';
import { useChatStore, type SnapshotModel } from '@/lib/chatStore';
import {
  getAntigravityStatus,
  postAntigravityConfig,
  postAntigravityProbe,
  type AntigravityStatus,
} from '@/lib/forgeApi';
import { useSessionStore } from '@/lib/sessionStore';
import { mockForgeBackend } from './forgeMock';

describe('formatResetTime & parseAntigravityLimits', () => {
  it('formatResetTime 格式化秒级/毫秒级时间戳及非法输入', () => {
    expect(formatResetTime(undefined)).toBe('');
    expect(formatResetTime('')).toBe('');
    expect(formatResetTime('invalid-date')).toBe('');

    // 2026-10-06 20:00:00 UTC (1791316800 秒)
    const formatted = formatResetTime(1791316800);
    expect(formatted).toMatch(/^重置/);
  });

  it('parseAntigravityLimits 支持 usedPercent 与 remainingPercent 互转计算', () => {
    // 1. primary 给出 usedPercent
    // 2. secondary 给出 remainingPercent
    const buckets = parseAntigravityLimits({
      primary: { usedPercent: 35, resetsAt: 1791316800 },
      secondary: { remainingPercent: 80, windowDurationMins: 1440 },
    });

    expect(buckets).toHaveLength(2);
    expect(buckets[0]).toEqual({
      id: 'primary',
      label: '主要额度',
      remainingPercent: 65,
      resetsAt: 1791316800,
    });
    expect(buckets[1]).toEqual({
      id: 'secondary',
      label: '次要额度',
      remainingPercent: 80,
      resetsAt: undefined,
    });
  });

  it('parseAntigravityLimits 优雅处理空值及缺省桶', () => {
    expect(parseAntigravityLimits(null)).toEqual([]);
    expect(parseAntigravityLimits(undefined)).toEqual([]);
    expect(parseAntigravityLimits({})).toEqual([]);
  });
});

describe('AntigravityCard 渠道卡状态与交互', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('未配置态: 呈现 needs-config 徽标与默认模型', async () => {
    const statusData: AntigravityStatus = {
      configured: false,
      baseUrl: '',
      model: 'gemini-3.8-flash',
      keyConfigured: false,
      availability: 'needs-config',
    };

    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': statusData,
        },
      ),
    );

    render(<AntigravityCard />);

    expect(screen.getByText('Antigravity 订阅反代')).toBeInTheDocument();
    const badge = await screen.findByTestId('antigravity-availability');
    expect(badge).toHaveTextContent('needs-config');
    expect(screen.queryByTestId('antigravity-latency')).toBeNull();
  });

  it('已配置可用态: 显示 available 徽标、延迟指示与配置摘要', async () => {
    const statusData: AntigravityStatus = {
      configured: true,
      baseUrl: 'http://127.0.0.1:8080',
      model: 'gemini-3.8-flash',
      keyConfigured: true,
      availability: 'available',
      latencyMs: 88,
    };

    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': statusData,
        },
      ),
    );

    render(<AntigravityCard />);

    const badge = await screen.findByTestId('antigravity-availability');
    expect(badge).toHaveTextContent('available');

    const latency = await screen.findByTestId('antigravity-latency');
    expect(latency).toHaveTextContent('88ms');

    const statusLine = screen.getByTestId('antigravity-status-line');
    expect(statusLine).toHaveTextContent('http://127.0.0.1:8080');
    expect(statusLine).toHaveTextContent('gemini-3.8-flash');
    expect(statusLine).toHaveTextContent('key 已配置');
  });

  it('离线态: 显示 disconnected / offline 徽标', async () => {
    const statusData: AntigravityStatus = {
      configured: true,
      baseUrl: 'https://proxy.example.com',
      model: 'gemini-3.8-pro',
      keyConfigured: true,
      availability: 'disconnected',
    };

    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': statusData,
        },
      ),
    );

    render(<AntigravityCard />);

    const badge = await screen.findByTestId('antigravity-availability');
    expect(badge).toHaveTextContent('disconnected');
  });

  it('额度展示: 支持 rateLimits / quota 并在卡片内渲染主要/次要额度条', async () => {
    const statusData: AntigravityStatus = {
      configured: true,
      baseUrl: 'http://127.0.0.1:8080',
      model: 'gemini-3.8-flash',
      keyConfigured: true,
      availability: 'available',
      rateLimits: {
        primary: { usedPercent: 20, resetsAt: 1791316800 },
        secondary: { remainingPercent: 55 },
      },
    };

    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': statusData,
        },
      ),
    );

    render(<AntigravityCard />);

    const primaryBar = await screen.findByTestId('antigravity-limit-primary');
    expect(primaryBar).toHaveTextContent('主要额度');
    expect(primaryBar).toHaveTextContent('剩余 80%');

    const secondaryBar = await screen.findByTestId('antigravity-limit-secondary');
    expect(secondaryBar).toHaveTextContent('次要额度');
    expect(secondaryBar).toHaveTextContent('剩余 55%');
  });

  it('展开配置抽屉: 快捷预设填充、密码掩码输入、保存与测试连接交互', async () => {
    let savedPayload: Record<string, unknown> | null = null;
    let probedPayload: Record<string, unknown> | null = null;

    const initialStatus: AntigravityStatus = {
      configured: false,
      baseUrl: '',
      model: 'gemini-3.8-flash',
      keyConfigured: false,
      availability: 'needs-config',
    };

    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': initialStatus,
          '/api/forge/llm/antigravity/probe': (init?: { body?: string }) => {
            probedPayload = JSON.parse(init?.body ?? '{}');
            return {
              ok: true,
              status: 'available',
              latencyMs: 65,
            };
          },
          '/api/forge/llm/antigravity/config': (init?: { body?: string }) => {
            savedPayload = JSON.parse(init?.body ?? '{}');
            return {
              ok: true,
              configured: true,
              baseUrl: savedPayload?.baseUrl,
              model: savedPayload?.model,
              keyConfigured: true,
            };
          },
        },
      ),
    );

    render(<AntigravityCard />);

    // 1. 点击展开配置
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));
    expect(screen.getByTestId('antigravity-config-form')).toBeInTheDocument();

    const baseUrlInput = screen.getByTestId('antigravity-baseurl-input');
    const modelInput = screen.getByTestId('antigravity-model-input');
    const keyInput = screen.getByTestId('antigravity-key-input');

    // 2. 快捷预设按钮切换
    const presetPro = screen.getByTestId('antigravity-preset-gemini-3.8-pro');
    fireEvent.click(presetPro);
    expect(modelInput).toHaveValue('gemini-3.8-pro');

    const presetFlash = screen.getByTestId('antigravity-preset-gemini-3.8-flash');
    fireEvent.click(presetFlash);
    expect(modelInput).toHaveValue('gemini-3.8-flash');

    // 3. 输入配置值
    fireEvent.change(baseUrlInput, { target: { value: 'https://proxy.example.com/v1' } });
    fireEvent.change(keyInput, { target: { value: 'secret-token-12345' } });
    expect(keyInput).toHaveAttribute('type', 'password');

    // 4. 测试连接 (Probe)
    const probeBtn = screen.getByTestId('antigravity-probe-btn');
    fireEvent.click(probeBtn);

    await waitFor(() => {
      expect(screen.getByTestId('antigravity-probe-feedback')).toHaveTextContent('✓ 连接正常 (65ms)');
    });
    expect(probedPayload).toEqual({
      baseUrl: 'https://proxy.example.com/v1',
      model: 'gemini-3.8-flash',
      key: 'secret-token-12345',
    });

    // 5. 保存配置
    const saveBtn = screen.getByTestId('antigravity-config-save');
    fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(screen.queryByTestId('antigravity-config-form')).toBeNull();
    });
    expect(savedPayload).toEqual({
      baseUrl: 'https://proxy.example.com/v1',
      model: 'gemini-3.8-flash',
      key: 'secret-token-12345',
    });
  });

  it('探针失败时展示错误提示', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': {
            configured: false,
            baseUrl: 'http://localhost:9999',
            model: 'gemini-3.8-flash',
            keyConfigured: false,
          },
          '/api/forge/llm/antigravity/probe': {
            ok: false,
            status: 'disconnected',
            error: 'Connection refused (os error 111)',
          },
        },
      ),
    );

    render(<AntigravityCard />);
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const probeBtn = screen.getByTestId('antigravity-probe-btn');
    fireEvent.click(probeBtn);

    await waitFor(() => {
      expect(screen.getByTestId('antigravity-probe-feedback')).toHaveTextContent(
        '✕ Connection refused (os error 111)',
      );
    });
  });
});

describe('ModelsPage 完整页面集成', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('ModelsPage 正常挂载 AntigravityCard', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/design-snapshot': { models: { models: [] } },
          '/api/forge/llm/antigravity/status': {
            configured: true,
            baseUrl: 'http://127.0.0.1:8080',
            model: 'gemini-3.8-flash',
            keyConfigured: true,
            availability: 'available',
          },
          '/api/forge/llm/openai-compat/status': { configured: false, baseUrl: '', model: '', keyConfigured: false },
          '/api/forge/llm/embedding/status': { configured: false, baseUrl: '', model: '', keyConfigured: false },
          '/api/forge/gen/backends': { backends: [] },
          '/api/forge/ffmpeg/status': { available: false, version: null },
        },
      ),
    );

    render(<ModelsPage />);
    const card = await screen.findByTestId('channel-card-antigravity');
    expect(card).toBeInTheDocument();
    expect(card).toHaveTextContent('反重力 · Google AI 订阅');
    expect(screen.getByTestId('official-channel-connections').querySelectorAll('article')).toHaveLength(4);
    expect(screen.getByTestId('byo-advanced-toggle')).toHaveAttribute('aria-expanded', 'false');
  });
});

describe('BFF API Client 方法验证', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('getAntigravityStatus, postAntigravityConfig, postAntigravityProbe 发送正确的路径与载荷', async () => {
    let configSent: unknown = null;
    let probeSent: unknown = null;

    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': {
            ok: true,
            configured: true,
            baseUrl: 'http://test:8000',
            model: 'gemini-3.8-pro',
            keyConfigured: true,
          },
          '/api/forge/llm/antigravity/config': (init?: { body?: string }) => {
            configSent = JSON.parse(init?.body ?? '{}');
            return { ok: true, configured: true };
          },
          '/api/forge/llm/antigravity/probe': (init?: { body?: string }) => {
            probeSent = JSON.parse(init?.body ?? '{}');
            return { ok: true, latencyMs: 33, status: 'available' };
          },
        },
      ),
    );

    const status = await getAntigravityStatus();
    expect(status.configured).toBe(true);
    expect(status.model).toBe('gemini-3.8-pro');

    const configRes = await postAntigravityConfig({
      baseUrl: 'http://test:8000',
      model: 'gemini-3.8-pro',
      key: 'token-xyz',
    });
    expect(configRes.ok).toBe(true);
    expect(configSent).toEqual({
      baseUrl: 'http://test:8000',
      model: 'gemini-3.8-pro',
      key: 'token-xyz',
    });

    const probeRes = await postAntigravityProbe({
      baseUrl: 'http://test:8000',
      model: 'gemini-3.8-pro',
    });
    expect(probeRes.ok).toBe(true);
    expect(probeRes.latencyMs).toBe(33);
    expect(probeSent).toEqual({
      baseUrl: 'http://test:8000',
      model: 'gemini-3.8-pro',
    });
  });
});

describe('ModelPicker Antigravity 模型选择与能力档位', () => {
  const ANTIGRAVITY_MODELS: SnapshotModel[] = [
    {
      id: 'gemini-3.8-flash',
      label: 'gemini-3.8-flash',
      provider: 'antigravity',
      availability: 'available',
      group: 'Antigravity',
      supportsThinking: true,
      effortOptions: [
        { id: 'low', label: 'Low' },
        { id: 'medium', label: 'Medium' },
        { id: 'high', label: 'High' },
      ],
      defaultEffort: 'medium',
      contextOptions: [
        { id: '128k', label: '128K', tokens: 131072 },
        { id: '1m', label: '1M', tokens: 1048576 },
      ],
      defaultContext: '1m',
    },
    {
      id: 'gemini-3.8-pro',
      label: 'gemini-3.8-pro',
      provider: 'antigravity',
      availability: 'needs-config',
      group: 'Antigravity',
      supportsThinking: true,
      effortOptions: [],
      contextOptions: [{ id: '1m', label: '1M', tokens: 1048576 }],
      defaultContext: '1m',
    },
    {
      id: 'gemini-3.8-pro-offline',
      label: 'gemini-3.8-pro-offline',
      provider: 'antigravity',
      availability: 'offline',
      group: 'Antigravity',
      supportsThinking: true,
      effortOptions: [],
      contextOptions: [{ id: '1m', label: '1M', tokens: 1048576 }],
      defaultContext: '1m',
    },
  ];

  beforeEach(() => {
    useSessionStore.setState({ activeSessionId: 'sess_ag_1' });
    useChatStore.setState({
      models: ANTIGRAVITY_MODELS,
      defaultModelId: 'gemini-3.8-flash',
      selectedModelId: 'gemini-3.8-flash',
      thinkingEnabled: true,
      contextOptionId: '1m',
      reasoningEffort: 'medium',
    });
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('ModelSubMenu 正确渲染 Antigravity 分组, needs-config 与 offline 禁用并给出提示', () => {
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.click(screen.getByTestId('spec-row-model'));

    // 分组标题
    expect(screen.getByText('Antigravity')).toBeInTheDocument();

    // available 状态项可选
    const flashItem = screen.getByTestId('model-item-gemini-3.8-flash');
    expect(flashItem).not.toBeDisabled();

    // needs-config 状态项被禁用并提示未配置反代
    const proItem = screen.getByTestId('model-item-gemini-3.8-pro');
    expect(proItem).toBeDisabled();
    expect(proItem).toHaveTextContent('未配置反代');

    // offline 状态项被禁用并提示反代离线
    const offlineItem = screen.getByTestId('model-item-gemini-3.8-pro-offline');
    expect(offlineItem).toBeDisabled();
    expect(offlineItem).toHaveTextContent('反代离线');
  });

  it('选定 Antigravity 模型后 Thinking, Reasoning Effort 与 1M 上下文完整可用', () => {
    const pickContext = vi.fn();
    const pickEffort = vi.fn();
    const setThinking = vi.fn();

    useChatStore.setState({
      pickContext,
      pickEffort,
      setThinking,
      thinkingEnabled: true,
    });

    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));

    // 1. Thinking 开关
    const thinkingRow = screen.getByTestId('spec-row-thinking');
    expect(thinkingRow).not.toBeDisabled();
    fireEvent.click(thinkingRow);
    expect(setThinking).toHaveBeenCalledWith(false);

    // 2. Effort 档位
    fireEvent.click(screen.getByTestId('spec-row-effort'));
    const effortPanel = screen.getByTestId('spec-submenu-effort');
    expect(effortPanel).toBeInTheDocument();
    expect(screen.getByTestId('effort-item-low')).toBeInTheDocument();
    expect(screen.getByTestId('effort-item-high')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('effort-item-high'));
    expect(pickEffort).toHaveBeenCalledWith('high');

    // 3. 1M Context 档位
    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.click(screen.getByTestId('spec-row-context'));
    const contextPanel = screen.getByTestId('spec-submenu-context');
    expect(contextPanel).toBeInTheDocument();
    expect(screen.getByTestId('context-item-1m')).toHaveTextContent('1M');
    fireEvent.click(screen.getByTestId('context-item-1m'));
    expect(pickContext).toHaveBeenCalledWith('1m');
  });
});
