import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import AuthScreen from '@/components/account/AuthScreen';
import CloudErrorGuidance, { cloudGuidance } from '@/components/chat/CloudErrorGuidance';
import AssistantMessage from '@/components/chat/AssistantMessage';
import ModelPicker from '@/components/chat/ModelPicker';
import SettingsOverlay from '@/components/settings/SettingsOverlay';
import { gateForced, gateVisible, useAccountStore } from '@/lib/accountStore';
import { useChatStore, type ChatMsg, type SnapshotModel } from '@/lib/chatStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore } from '@/lib/settingsStore';
import {
  applyRemoteSetting,
  pullCloudSettings,
  resetSettingsSync,
  SETTINGS_PUSH_DEBOUNCE_MS,
  startSettingsSync,
} from '@/lib/settingsSync';
import { defaultAppearance, useThemeStore } from '@/lib/themeStore';
import { makeAccountStatus, jsonResponse, tryAccountStatusFetch } from './accountTestHelpers';

const initialAccount = useAccountStore.getState();
const initialOverlay = useOverlayStore.getState();
const initialSettings = useSettingsStore.getState();
const initialTheme = useThemeStore.getState();
const initialChat = useChatStore.getState();

function resetAccount(over: Parameters<typeof makeAccountStatus>[0] = {}) {
  useAccountStore.setState(
    {
      ...initialAccount,
      status: makeAccountStatus(over),
      authConfig: null,
      loading: false,
      error: null,
      authOpen: false,
      dismissed: false,
    },
    true,
  );
}

beforeEach(() => {
  resetAccount();
  useOverlayStore.setState(initialOverlay, true);
  useSettingsStore.setState(initialSettings, true);
  useThemeStore.setState(initialTheme, true);
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  resetSettingsSync();
  globalThis.localStorage?.clear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('登录门 gateVisible / gateForced', () => {
  it('status 未知(null)时不弹门', () => {
    useAccountStore.setState({ status: null, authOpen: false, dismissed: false });
    expect(gateVisible(useAccountStore.getState())).toBe(false);
  });

  it('未登录且无 BYO/devMock 时强制门', () => {
    resetAccount({ loggedIn: false, byoConfigured: false, devMock: false });
    expect(gateVisible(useAccountStore.getState())).toBe(true);
    expect(gateForced(useAccountStore.getState())).toBe(true);
  });

  it('byoConfigured 或 devMock 时不自动弹门', () => {
    resetAccount({ byoConfigured: true });
    expect(gateVisible(useAccountStore.getState())).toBe(false);
    resetAccount({ devMock: true });
    expect(gateVisible(useAccountStore.getState())).toBe(false);
  });

  it('authOpen 手动打开且已登录时仍显示', () => {
    resetAccount({ loggedIn: true, user: { id: 1, email: 'a@b.c', nickname: 'n', role: 'user', status: 'active', hasAvatar: false, avatarVersion: 0, balanceMicros: 0 } });
    useAccountStore.setState({ authOpen: true });
    expect(gateVisible(useAccountStore.getState())).toBe(true);
    expect(gateForced(useAccountStore.getState())).toBe(false);
  });

  it('dismissGate 后会话内不再强制门', () => {
    resetAccount();
    useAccountStore.getState().dismissGateForSession();
    expect(gateVisible(useAccountStore.getState())).toBe(false);
    expect(useAccountStore.getState().dismissed).toBe(true);
  });
});

describe('<AuthScreen /> 注册字段', () => {
  it('closed 模式显示关闭提示', () => {
    resetAccount();
    useAccountStore.setState({ authConfig: { registrationMode: 'closed', requireEmailVerify: false, smtpEnabled: false, siteName: 'Test Cloud', currency: 'USD' } });
    render(<AuthScreen />);
    fireEvent.click(screen.getByTestId('auth-tab-register'));
    expect(screen.getByTestId('auth-register-closed')).toBeInTheDocument();
  });

  it('invite 模式显示邀请码;邮箱验证+SMTP 时显示验证码', () => {
    resetAccount();
    useAccountStore.setState({
      authConfig: {
        registrationMode: 'invite',
        requireEmailVerify: true,
        smtpEnabled: true,
        siteName: 'Test Cloud',
        currency: 'USD',
      },
    });
    render(<AuthScreen />);
    fireEvent.click(screen.getByTestId('auth-tab-register'));
    expect(screen.getByTestId('auth-invite')).toBeInTheDocument();
    expect(screen.getByTestId('auth-email-code')).toBeInTheDocument();
    expect(screen.getByTestId('auth-send-code')).toBeInTheDocument();
  });

  it('byoAllowed=false 时不显示自带密钥入口', () => {
    resetAccount({ byoAllowed: false });
    render(<AuthScreen />);
    expect(screen.queryByTestId('auth-byo')).not.toBeInTheDocument();
    expect(screen.queryByTestId('auth-official-channels')).not.toBeInTheDocument();
  });

  it('官方订阅入口直接打开渠道卡片，无需先登录 Forge 云', () => {
    resetAccount({ loggedIn: false, byoConfigured: false });
    render(<AuthScreen />);
    fireEvent.click(screen.getByTestId('auth-official-channels'));
    expect(useAccountStore.getState().dismissed).toBe(true);
    expect(useSettingsStore.getState().page).toBe('models');
    expect(useOverlayStore.getState().settings).toBe(true);
    expect(screen.queryByTestId('auth-screen')).not.toBeInTheDocument();
  });

  it('可编辑并保存服务器地址', async () => {
    const posts: unknown[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const path = String(url).split('?')[0];
        if (path === '/api/forge/account/config' && init?.method === 'POST') {
          posts.push(JSON.parse(init.body ?? '{}'));
          return jsonResponse({ serverUrl: 'http://cloud.example:8110' });
        }
        if (path === '/api/forge/account/status') {
          return jsonResponse(makeAccountStatus({ serverUrl: 'http://cloud.example:8110' }));
        }
        if (path === '/api/forge/account/auth-config') {
          return jsonResponse({ registrationMode: 'open', requireEmailVerify: false, smtpEnabled: false, siteName: 'X', currency: 'USD' });
        }
        throw new Error(`未 mock: ${String(url)}`);
      }),
    );
    resetAccount();
    render(<AuthScreen />);
    fireEvent.click(screen.getByTestId('auth-server-toggle'));
    fireEvent.change(screen.getByTestId('auth-server-input'), { target: { value: 'http://cloud.example:8110/' } });
    fireEvent.click(screen.getByTestId('auth-server-save'));
    await act(async () => Promise.resolve());
    expect(posts).toEqual([{ serverUrl: 'http://cloud.example:8110' }]);
    expect(await screen.findByTestId('auth-server-message')).toHaveTextContent('已保存');
  });
});

describe('<AuthScreen /> 版式(D-046)', () => {
  it('衬线标题随页签切换;品牌面板纯装饰', () => {
    resetAccount();
    render(<AuthScreen />);
    expect(screen.getByTestId('auth-title')).toHaveTextContent('欢迎回来');
    expect(screen.getByTestId('auth-brand-panel')).toHaveAttribute('aria-hidden', 'true');
    fireEvent.click(screen.getByTestId('auth-tab-register'));
    expect(screen.getByTestId('auth-title')).toHaveTextContent('创建你的账号');
    expect(screen.getByTestId('auth-tab-register')).toHaveAttribute('aria-selected', 'true');
  });

  it('强制门不给关闭钮;手动打开的门可关闭', () => {
    resetAccount();
    const { unmount } = render(<AuthScreen />);
    expect(screen.getByTestId('auth-screen')).toHaveAttribute('data-forced', '1');
    expect(screen.queryByTestId('auth-close')).not.toBeInTheDocument();
    unmount();
    resetAccount({ loggedIn: true });
    useAccountStore.setState({ authOpen: true });
    render(<AuthScreen />);
    fireEvent.click(screen.getByTestId('auth-close'));
    expect(useAccountStore.getState().authOpen).toBe(false);
  });
});

describe('账户页 · 未登录空状态(D-046)', () => {
  it('「登录或注册」打开登录门', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string }) => tryAccountStatusFetch(url, init) ?? jsonResponse({})),
    );
    resetAccount();
    useAccountStore.setState({ dismissed: true });
    useOverlayStore.getState().open('settings');
    useSettingsStore.getState().setPage('account');
    render(<SettingsOverlay />);
    const login = await screen.findByTestId('account-login');
    expect(screen.getByTestId('account-logged-out')).toHaveTextContent('登录 RurixForge 云');
    fireEvent.click(login);
    expect(useAccountStore.getState().authOpen).toBe(true);
  });
});

describe('cloudGuidance / AssistantMessage', () => {
  it('§8.5 失败码映射', () => {
    expect(cloudGuidance('INSUFFICIENT_BALANCE')?.action?.kind).toBe('recharge');
    expect(cloudGuidance('CLOUD_LOGIN_REQUIRED')?.action?.kind).toBe('login');
    expect(cloudGuidance('MODEL_NOT_CONFIGURED')?.action?.kind).toBe('pickModel');
    expect(cloudGuidance('RATE_LIMITED')?.action).toBeUndefined();
    expect(cloudGuidance('UPSTREAM_ERROR')?.text).toContain('稍后');
    expect(cloudGuidance('UNKNOWN')).toBeNull();
  });

  it('agent.failed 带 code 时渲染引导', () => {
    const msg: ChatMsg = {
      id: 'a1',
      role: 'assistant',
      text: '',
      blocks: [],
      status: 'failed',
      error: '余额不足',
      errorCode: 'INSUFFICIENT_BALANCE',
      mode: 'build',
      time: '12:00',
      runId: 'r1',
    };
    render(<AssistantMessage msg={msg} />);
    expect(screen.getByTestId('assistant-error-guidance')).toHaveAttribute('data-code', 'INSUFFICIENT_BALANCE');
    expect(screen.getByTestId('assistant-error-action-recharge')).toBeInTheDocument();
  });

  it('登录引导打开 AuthScreen', () => {
    resetAccount({ loggedIn: true });
    render(
      <>
        <AuthScreen />
        <CloudErrorGuidance code="CLOUD_UNAUTHORIZED" error="会话失效" />
      </>,
    );
    expect(screen.queryByTestId('auth-screen')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('assistant-error-action-login'));
    expect(screen.getByTestId('auth-screen')).toBeInTheDocument();
  });
});

describe('账户页 · 兑换', () => {
  it('兑换成功刷新余额', async () => {
    let balance = 1_000_000;
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const path = String(url).split('?')[0];
        const method = init?.method ?? 'GET';
        if (path === '/api/forge/account/status' && method === 'GET') {
          return jsonResponse(
            makeAccountStatus({
              loggedIn: true,
              balanceMicros: balance,
              user: {
                id: 1,
                email: 'u@test.com',
                nickname: 'U',
                role: 'user',
                status: 'active',
                hasAvatar: false,
                avatarVersion: 0,
                balanceMicros: balance,
              },
            }),
          );
        }
        if (path === '/api/forge/account/redeem' && method === 'POST') {
          balance += 5_000_000;
          return jsonResponse({ kind: 'balance', valueMicros: 5_000_000, plan: null, subscription: null, balanceMicros: balance });
        }
        if (path === '/api/forge/account/api-keys' && method === 'GET') {
          return jsonResponse({ items: [] });
        }
        if (path === '/api/forge/account/api-keys' && method === 'POST') {
          return jsonResponse({
            apiKey: { id: 9, name: 'cli', kind: 'user', prefix: 'sk-rf-ab', status: 'active', quotaMicros: 0, usedMicros: 0, expiresAt: null, lastUsedAt: null, createdAt: '2026-01-01T00:00:00Z' },
            key: 'sk-rf-secret-once',
          });
        }
        if (path === '/api/forge/account/devices') return jsonResponse({ items: [] });
        if (path === '/api/forge/account/usage/daily') return jsonResponse({ items: [] });
        if (path === '/api/forge/account/usage') return jsonResponse({ items: [], total: 0, summary: { requests: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0, costMicros: 0 } });
        if (path === '/api/forge/account/balance') return jsonResponse({ balanceMicros: balance, currency: 'USD', subscriptions: [] });
        throw new Error(`未 mock: ${path} ${method}`);
      }),
    );
    resetAccount({
      loggedIn: true,
      balanceMicros: balance,
      user: { id: 1, email: 'u@test.com', nickname: 'U', role: 'user', status: 'active', hasAvatar: false, avatarVersion: 0, balanceMicros: balance },
    });
    useOverlayStore.getState().open('settings');
    useSettingsStore.getState().setPage('account');
    render(<SettingsOverlay />);
    fireEvent.change(await screen.findByTestId('account-redeem-input'), { target: { value: 'GIFT-1' } });
    fireEvent.click(screen.getByTestId('account-redeem-submit'));
    await act(async () => Promise.resolve());
    expect(await screen.findByTestId('account-redeem-result')).toBeInTheDocument();

  });
});

describe('记忆页 CRUD', () => {
  it('新建 / 编辑 / 删除记忆', async () => {
    const store = new Map<string, Record<string, unknown>>();
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const path = String(url).split('?')[0];
        const method = init?.method ?? 'GET';
        const hit = tryAccountStatusFetch(url, init, { loggedIn: true });
        if (hit) return hit;
        if (path === '/api/forge/memory' && method === 'GET') {
          return jsonResponse({ items: [...store.values()], sync: { enabled: true, lastSyncAt: null } });
        }
        if (path === '/api/forge/memory' && method === 'POST') {
          const body = JSON.parse(init?.body ?? '{}') as Record<string, unknown>;
          const id = 'mem-1';
          const row = { id, scope: 'global', ...body, createdAt: '2026-01-01T00:00:00Z', updatedAt: '2026-01-01T00:00:00Z', source: 'user' };
          store.set(id, row);
          return jsonResponse(row);
        }
        if (path.startsWith('/api/forge/memory/') && method === 'PATCH') {
          const id = path.split('/').pop()!;
          const body = JSON.parse(init?.body ?? '{}');
          const prev = store.get(id)!;
          const row = { ...prev, ...body, updatedAt: '2026-01-02T00:00:00Z' };
          store.set(id, row);
          return jsonResponse(row);
        }
        if (path.startsWith('/api/forge/memory/') && method === 'DELETE') {
          const id = path.split('/').pop()!;
          store.delete(id);
          return jsonResponse({ ok: true });
        }
        throw new Error(`未 mock: ${path}`);
      }),
    );
    resetAccount({ loggedIn: true });
    useOverlayStore.getState().open('settings');
    useSettingsStore.getState().setPage('memory');
    render(<SettingsOverlay />);
    fireEvent.click(await screen.findByTestId('memory-create'));
    fireEvent.change(screen.getByTestId('memory-content'), { target: { value: '偏好暗色主题' } });
    fireEvent.click(screen.getByTestId('memory-save'));
    expect(await screen.findByTestId('memory-row-mem-1')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('memory-edit-mem-1'));
    fireEvent.change(screen.getByTestId('memory-content'), { target: { value: '偏好暗色主题 v2' } });
    fireEvent.click(screen.getByTestId('memory-save'));
    await act(async () => Promise.resolve());
    expect(await screen.findByTestId('memory-content-mem-1')).toHaveTextContent('v2');

    fireEvent.click(screen.getByTestId('memory-delete-mem-1'));
    fireEvent.click(screen.getByTestId('memory-delete-confirm-mem-1'));
    await act(async () => Promise.resolve());
    expect(screen.queryByTestId('memory-row-mem-1')).not.toBeInTheDocument();
  });
});

describe('设置同步 appearance / composer', () => {
  it('登录后 pull;本地改动 debounce 1s 推送;远端下发不 echo', async () => {
    vi.useFakeTimers();
    const puts: Array<{ ns: string; value: unknown }> = [];
    let remoteAppearance = { ...defaultAppearance(), mode: 'dark' as const };
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const path = String(url).split('?')[0];
        const method = init?.method ?? 'GET';
        if (path === '/api/forge/account/settings' && method === 'GET') {
          return jsonResponse({
            items: {
              appearance: { value: remoteAppearance, version: 1, updatedAt: '2026-01-01T00:00:00Z' },
              composer: { value: { submitCtrlEnter: true }, version: 1, updatedAt: '2026-01-01T00:00:00Z' },
            },
            source: 'cloud',
          });
        }
        if (path.startsWith('/api/forge/account/settings/') && method === 'PUT') {
          const ns = decodeURIComponent(path.split('/').pop()!);
          const body = JSON.parse(init?.body ?? '{}') as { value: unknown };
          puts.push({ ns, value: body.value });
          return jsonResponse({ namespace: ns, value: body.value, version: 2, updatedAt: '2026-01-02T00:00:00Z', pending: false });
        }
        return jsonResponse({});
      }),
    );
    resetAccount({ loggedIn: true });
    const stop = startSettingsSync();
    await pullCloudSettings();
    expect(useThemeStore.getState().mode).toBe('dark');
    expect(useSettingsStore.getState().submitCtrlEnter).toBe(true);

    useThemeStore.getState().setMode('light');
    await act(async () => {
      vi.advanceTimersByTime(SETTINGS_PUSH_DEBOUNCE_MS - 1);
    });
    expect(puts.length).toBe(0);
    await act(async () => {
      vi.advanceTimersByTime(1);
    });
    expect(puts.some((p) => p.ns === 'appearance' && (p.value as { mode: string }).mode === 'light')).toBe(true);

    applyRemoteSetting('appearance', remoteAppearance);
    expect(puts.filter((p) => p.ns === 'appearance').length).toBe(1);

    useSettingsStore.getState().setPage('agent');
    await act(async () => {
      vi.advanceTimersByTime(SETTINGS_PUSH_DEBOUNCE_MS);
    });
    expect(puts.some((p) => p.ns === 'composer')).toBe(false);

    stop();
  });
});

describe('BYO 折叠区', () => {
  it('byoAllowed=false 时模型页不渲染自带密钥折叠', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string }) => {
        const hit = tryAccountStatusFetch(url, init, { byoAllowed: false });
        if (hit) return hit;
        const u = String(url);
        if (u === '/api/forge/design-snapshot') return jsonResponse({ models: { models: [] } });
        if (u === '/api/forge/gen/backends') return jsonResponse({ backends: [] });
        throw new Error(`未 mock: ${u}`);
      }),
    );
    resetAccount({ byoAllowed: false });
    useOverlayStore.getState().open('settings');
    useSettingsStore.getState().setPage('models');
    render(<SettingsOverlay />);
    expect(await screen.findByTestId('settings-page-models')).toBeInTheDocument();
    expect(screen.queryByTestId('byo-advanced')).not.toBeInTheDocument();
    expect(screen.queryByTestId('deepseek-key-toggle')).not.toBeInTheDocument();
  });
});

describe('SettingsAccountChip', () => {
  it('已登录时展示昵称与余额', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string }) => {
        const hit = tryAccountStatusFetch(url, init, {
          loggedIn: true,
          balanceMicros: 2_500_000,
          user: {
            id: 1,
            email: 'nick@test.com',
            nickname: 'Nick',
            role: 'user',
            status: 'active',
            hasAvatar: false,
            avatarVersion: 0,
            balanceMicros: 2_500_000,
          },
        });
        if (hit) return hit;
        throw new Error(`未 mock: ${String(url)}`);
      }),
    );
    resetAccount({
      loggedIn: true,
      balanceMicros: 2_500_000,
      user: {
        id: 1,
        email: 'nick@test.com',
        nickname: 'Nick',
        role: 'user',
        status: 'active',
        hasAvatar: false,
        avatarVersion: 0,
        balanceMicros: 2_500_000,
      },
    });
    useOverlayStore.getState().open('settings');
    render(<SettingsOverlay />);
    expect(await screen.findByTestId('settings-account-name')).toHaveTextContent('Nick');
    expect(screen.getByTestId('settings-account-balance')).toHaveTextContent('2.50 USD');
  });
});

describe('ModelPicker 云端模型', () => {
  const CLOUD: SnapshotModel = {
    id: 'cloud:gpt-5.5',
    label: 'GPT-5.5',
    provider: 'cloud',
    availability: 'available',
    group: 'RurixForge 云',
    vision: true,
    pricing: { inputPer1M: 1_250_000, outputPer1M: 10_000_000, cacheReadPer1M: 125_000, cacheWritePer1M: 0 },
    currency: 'USD',
    contextOptions: [{ id: 'native', label: '400K', tokens: 400000 }],
    defaultContext: 'native',
  };

  it('展示单价提示与视觉徽标', async () => {
    useChatStore.setState({
      models: [CLOUD],
      selectedModelId: 'cloud:gpt-5.5',
      defaultModelId: 'cloud:gpt-5.5',
      thinkingEnabled: false,
    });
    render(<ModelPicker />);
    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.click(screen.getByTestId('spec-row-model'));
    expect(await screen.findByTestId('model-vision-cloud:gpt-5.5')).toBeInTheDocument();
    expect(screen.getByTestId('model-item-cloud:gpt-5.5')).toHaveTextContent('1.25');
  });
});
