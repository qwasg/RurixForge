import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsOverlay from '@/components/settings/SettingsOverlay';
import CodexPage from '@/components/settings/CodexPage';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useSkillStore } from '@/lib/skillStore';
import { useThemeStore } from '@/lib/themeStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useChatStore } from '@/lib/chatStore';
import { useAccountStore } from '@/lib/accountStore';
import { makeAccountStatus, tryAccountStatusFetch } from './accountTestHelpers';

/**
 * F7 wave.5 设置体系:全屏 overlay + 左导航翻页 + Agent 页(Ctrl+Enter/权限信息行)
 * + 模型页(deepseek 卡/配置 key 展开保存/gen backends 平移)+ 技能页(toggle 全量写回)
 * + 关于页(health 实测)。
 */

const initialSettings = useSettingsStore.getState();
const initialOverlay = useOverlayStore.getState();
const initialTheme = useThemeStore.getState();
const initialSkill = useSkillStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialSessions = useSessionStore.getState();
const initialChat = useChatStore.getState();
const initialAccount = useAccountStore.getState();

function openSettings(
  page?: 'appearance' | 'account' | 'memory' | 'agent' | 'codex' | 'models' | 'generation' | 'skills' | 'about',
) {
  if (page) useSettingsStore.getState().setPage(page);
  useOverlayStore.getState().open('settings');
}

beforeEach(() => {
  useSettingsStore.setState(initialSettings, true);
  useOverlayStore.setState(initialOverlay, true);
  useThemeStore.setState(initialTheme, true);
  useSkillStore.setState(initialSkill, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useSessionStore.setState(initialSessions, true);
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useAccountStore.setState(
    {
      ...initialAccount,
      status: makeAccountStatus(),
      authConfig: null,
      loading: false,
      error: null,
      authOpen: false,
      dismissed: false,
    },
    true,
  );
  globalThis.localStorage?.clear();
});

async function openModelsWithByo() {
  act(() => openSettings('models'));
  const toggle = await screen.findByTestId('byo-advanced-toggle');
  if (toggle.getAttribute('aria-expanded') !== 'true') fireEvent.click(toggle);
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('设置 overlay 壳', () => {
  it('关态不渲染;开态 = 全屏 overlay + 五页导航;翻页持久化', async () => {
    render(<SettingsOverlay />);
    expect(screen.queryByTestId('settings-overlay')).not.toBeInTheDocument();
    act(() => openSettings());
    expect(await screen.findByTestId('settings-overlay')).toBeInTheDocument();
    // 八页导航行(含账户 / 记忆)
    for (const p of ['appearance', 'account', 'memory', 'agent', 'codex', 'models', 'skills', 'about']) {
      expect(screen.getByTestId(`settings-nav-${p}`)).toBeInTheDocument();
    }
    // 默认外观页(翻页之前)
    expect(screen.getByTestId('settings-page-appearance')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('settings-nav-account'));
    expect(screen.getByTestId('settings-page-account')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('settings-nav-memory'));
    expect(screen.getByTestId('settings-page-memory')).toBeInTheDocument();
    // 翻页 → 持久化
    fireEvent.click(screen.getByTestId('settings-nav-agent'));
    expect(screen.getByTestId('settings-page-agent')).toBeInTheDocument();
    expect(globalThis.localStorage?.getItem('forge:settingsPage')).toBe('agent');
    // 返回钮关闭
    fireEvent.click(screen.getByTestId('settings-back'));
    expect(screen.queryByTestId('settings-overlay')).not.toBeInTheDocument();
  });
});

describe('Agent 页', () => {
  it('Ctrl+Enter toggle 写 settingsStore;默认权限读取后台选项', async () => {
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const hit = tryAccountStatusFetch(url);
      if (hit) return hit;
      if (String(url) === '/api/forge/agent/config') return {
        ok: true, status: 200, json: async () => ({
          config: { exploreModel: '', defaultPermissionMode: 'bypass' },
          options: { exploreModels: [], permissionModes: [
            { id: 'plan', label: '只读', description: '只读查询' },
            { id: 'auto', label: '写入需批准', description: '写入先批准' },
            { id: 'bypass', label: '自动执行', description: '工具自动执行' },
          ] },
        }),
      } as Response;
      throw new Error(`未 mock: ${String(url)}`);
    }));
    render(<SettingsOverlay />);
    act(() => openSettings('agent'));
    const toggle = await screen.findByTestId('submit-ctrl-toggle');
    expect(toggle).toHaveAttribute('aria-checked', 'false');
    fireEvent.click(toggle);
    expect(useSettingsStore.getState().submitCtrlEnter).toBe(true);
    expect(globalThis.localStorage?.getItem('forge:submitCtrl')).toBe('1');
    await waitFor(() => expect(screen.getByTestId('agent-default-permission-trigger')).toHaveTextContent('自动执行'));
    fireEvent.click(screen.getByTestId('agent-default-permission-trigger'));
    expect(screen.getByTestId('agent-default-permission-item-plan')).toHaveTextContent('只读');
    expect(screen.getByTestId('agent-default-permission-item-auto')).toHaveTextContent('写入需批准');
    expect(screen.getByTestId('default-engine-card')).toBeInTheDocument();
    expect(screen.getByTestId('default-engine-local')).toHaveTextContent('本地');
    expect(screen.getByTestId('default-engine-codex')).toHaveTextContent('Codex');
  });
});

describe('Codex 页', () => {
  it.each(['pro', 'Plus'])('%s 账户按官方窗口显示 Codex 额度，不混入基础模型额度', async (planType) => {
    const codex = planType === 'pro'
      ? { primary: { usedPercent: 20, windowDurationMins: 10080 }, secondary: null }
      : { primary: { usedPercent: 25, windowDurationMins: 300 }, secondary: { usedPercent: 20, windowDurationMins: 10080 } };
    const rateLimits = { primary: { usedPercent: 0, windowDurationMins: 300 }, rateLimitsByLimitId: {
      base_model_inference: { primary: { usedPercent: 0, windowDurationMins: 10080 } }, codex,
    } };
    const account = { authMode: 'chatgpt', planType, rateLimits };
    const responses: Record<string, unknown> = {
      '/api/forge/codex/status': { installed: true, running: true, authSource: 'chatgpt',
        config: { defaultEngine: 'codex', autoRegisterMcp: true }, account,
        models: [{ id: 'gpt-test' }], install: { running: false }, mcp: { servers: [] } },
      '/api/forge/codex/account': { ok: true, ...account },
      '/api/forge/codex/rate-limits': { ok: true, rateLimits },
      '/api/forge/codex/mcp/status': { servers: [], live: false },
    };
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const hit = tryAccountStatusFetch(url);
      if (hit) return hit;
      const path = String(url).split('?')[0];
      if (path in responses) return { ok: true, status: 200, json: async () => responses[path] } as Response;
      throw new Error(`未 mock: ${path}`);
    }));
    render(<CodexPage />);
    const primary = await screen.findByTestId('codex-limit-codex-primary');
    const card = screen.getByTestId('codex-limits-card');
    expect(card).not.toHaveTextContent('base_model_inference');
    expect(card).not.toHaveTextContent('剩余 100%');
    if (planType === 'pro') {
      expect(primary).toHaveTextContent('周额度');
      expect(primary).toHaveTextContent('剩余 80%');
      expect(card).not.toHaveTextContent('5 小时');
      expect(screen.queryByTestId('codex-limit-codex-secondary')).not.toBeInTheDocument();
    } else {
      expect(primary).toHaveTextContent('5 小时');
      expect(primary).toHaveTextContent('剩余 75%');
      expect(screen.getByTestId('codex-limit-codex-secondary')).toHaveTextContent('周额度');
    }
  });

  it('呈现安装/账户/额度/模型/MCP，并保存 Computer Use 开关', async () => {
    const posts: Array<Record<string, unknown>> = [];
    const status = {
      installed: true,
      managedInstalled: true,
      computerUseInstalled: true,
      command: 'codex app-server',
      npmAvailable: true,
      running: true,
      config: {
        codexBin: '', codexHome: '', defaultEngine: 'codex', defaultModel: 'gpt-5',
        autoRegisterMcp: true, computerUse: false,
      },
      account: {
        authMode: 'chatgpt', planType: 'Plus', email: 'dev@example.com',
        rateLimits: { primary: { usedPercent: 25, resetsAt: 1_800_000_000 } },
      },
      models: [{ id: 'gpt-5', displayName: 'GPT-5' }],
      install: { running: false, log: '' },
      mcp: { servers: [{ name: 'engine-scene', command: 'forge-mcp-engine-scene', available: true }] },
    };
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
      const hit = tryAccountStatusFetch(url, init);
      if (hit) return hit;
      const path = String(url).split('?')[0];
      if (path === '/api/forge/codex/status') {
        return { ok: true, status: 200, json: async () => status } as Response;
      }
      if (path === '/api/forge/codex/mcp/status') {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            autoRegisterMcp: true,
            projectRoot: 'D:/RurixForge',
            servers: status.mcp.servers,
            live: true,
            runtime: {
              data: [
                { name: 'engine-scene', runtimeStatus: 'connected', authStatus: 'unsupported' },
                { name: 'remote-auth', runtimeStatus: 'authenticationRequired', authStatus: 'notLoggedIn' },
              ],
              nextCursor: null,
            },
          }),
        } as Response;
      }
      if (path === '/api/forge/codex/config' && init?.method === 'POST') {
        posts.push(JSON.parse(init.body ?? '{}'));
        const body = posts[posts.length - 1];
        return {
          ok: true,
          status: 200,
          json: async () => ({ ...status, config: { ...status.config, ...body } }),
        } as Response;
      }
      throw new Error(`未 mock: ${String(url)}`);
    }));
    render(<SettingsOverlay />);
    act(() => openSettings('codex'));
    expect(await screen.findByTestId('settings-page-codex')).toBeInTheDocument();
    expect(screen.getByTestId('codex-install-card')).toHaveTextContent('已安装 · 运行中');
    expect(screen.getByTestId('codex-account-card')).toHaveTextContent('dev@example.com');
    expect(screen.getByTestId('codex-account-card')).toHaveTextContent('Plus');
    expect(screen.getByTestId('codex-limit-primary')).toHaveTextContent('剩余 75%');
    expect(screen.queryByTestId('codex-mcp-engine-scene')).not.toBeInTheDocument();
    expect(screen.getByTestId('codex-mcp-summary')).toHaveTextContent('1 / 2 项需要处理');
    expect(screen.getByTestId('codex-mcp-remote-auth')).toHaveTextContent('authenticationRequired · notLoggedIn');
    fireEvent.click(screen.getByTestId('codex-default-model-trigger'));
    expect(screen.getByRole('menuitem', { name: '自动' })).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('codex-computer-use'));
    await act(async () => Promise.resolve());
    expect(posts).toContainEqual({ computerUse: true });
  });

  it('启动 app-server 后以 account/rate-limits 新鲜值覆盖首次 status 的空缓存', async () => {
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const hit = tryAccountStatusFetch(url);
      if (hit) return hit;
      const path = String(url).split('?')[0];
      if (path === '/api/forge/codex/status') {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            installed: true,
            managedInstalled: false,
            computerUseInstalled: false,
            npmAvailable: true,
            running: false,
            config: {
              codexBin: 'codex', codexHome: '', defaultEngine: 'local', defaultModel: '',
              autoRegisterMcp: true, computerUse: false,
            },
            account: { authMode: null, planType: null, rateLimits: null },
            models: [],
            install: { running: false, log: '' },
            mcp: { servers: [] },
          }),
        } as Response;
      }
      if (path === '/api/forge/codex/models') {
        return { ok: true, status: 200, json: async () => ({ ok: true, models: [{ id: 'gpt-5.6', displayName: 'GPT-5.6' }] }) } as Response;
      }
      if (path === '/api/forge/codex/account') {
        return { ok: true, status: 200, json: async () => ({ ok: true, authMode: 'chatgpt', planType: 'Plus', email: 'fresh@example.com' }) } as Response;
      }
      if (path === '/api/forge/codex/rate-limits') {
        return { ok: true, status: 200, json: async () => ({ ok: true, rateLimits: { primary: { usedPercent: 12 } } }) } as Response;
      }
      if (path === '/api/forge/codex/mcp/status') {
        return { ok: true, status: 200, json: async () => ({ servers: [], live: false }) } as Response;
      }
      throw new Error(`未 mock: ${String(url)}`);
    }));
    render(<SettingsOverlay />);
    act(() => openSettings('codex'));
    expect(await screen.findByText('fresh@example.com')).toBeInTheDocument();
    expect(screen.getByTestId('codex-account-card')).toHaveTextContent('Plus');
    expect(screen.getByTestId('codex-limit-primary')).toHaveTextContent('剩余 88%');
    expect(screen.getByTestId('codex-default-model')).toHaveTextContent('自动');
  });

  it('ChatGPT 登录拒绝非 HTTP(S) 跳转并取消对应登录请求', async () => {
    let cancelledLoginId: string | undefined;
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      const hit = tryAccountStatusFetch(url, init);
      if (hit) return hit;
      const path = String(url).split('?')[0];
      const method = init?.method ?? 'GET';
      if (path === '/api/forge/codex/status') {
        return { ok: true, status: 200, json: async () => ({
          installed: true, managedInstalled: false, computerUseInstalled: false, npmAvailable: true,
          running: false,
          config: { codexBin: 'codex', codexHome: '', defaultEngine: 'local', defaultModel: '', autoRegisterMcp: true, computerUse: false },
          account: { authMode: null }, models: [{ id: 'gpt-5.6' }], install: { running: false, log: '' }, mcp: { servers: [] },
        }) } as Response;
      }
      if (path === '/api/forge/codex/mcp/status') {
        return { ok: true, status: 200, json: async () => ({ servers: [], live: false }) } as Response;
      }
      if (path === '/api/forge/codex/account') {
        return { ok: true, status: 200, json: async () => ({ ok: true, authMode: null }) } as Response;
      }
      if (path === '/api/forge/codex/rate-limits') {
        return { ok: true, status: 200, json: async () => ({ ok: true, rateLimits: null }) } as Response;
      }
      if (path === '/api/forge/codex/login' && method === 'POST') {
        return { ok: true, status: 200, json: async () => ({ loginId: 'login-bad', authUrl: 'javascript:alert(1)' }) } as Response;
      }
      if (path === '/api/forge/codex/login/cancel' && method === 'POST') {
        cancelledLoginId = (JSON.parse(String(init?.body)) as { loginId?: string }).loginId;
        return { ok: true, status: 200, json: async () => ({ ok: true }) } as Response;
      }
      throw new Error(`未 mock: ${String(url)}`);
    }));
    render(<SettingsOverlay />);
    act(() => openSettings('codex'));
    fireEvent.click(await screen.findByTestId('codex-login-chatgpt'));
    expect(await screen.findByTestId('codex-error')).toHaveTextContent('不是安全的 HTTP(S) URL');
    expect(cancelledLoginId).toBe('login-bad');
    // An empty login window may be reserved before the request; the unsafe URL must never open.
    expect(open.mock.calls.every(([url]) => url === '')).toBe(true);
    open.mockRestore();
  });

  it('账户 lastError 结束等待态并显示原因；重试登录会清除旧错误', async () => {
    let accountCalls = 0;
    let loginCalls = 0;
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      const hit = tryAccountStatusFetch(url, init);
      if (hit) return hit;
      const path = String(url).split('?')[0];
      const method = init?.method ?? 'GET';
      if (path === '/api/forge/codex/status') {
        return { ok: true, status: 200, json: async () => ({
          installed: true, managedInstalled: false, computerUseInstalled: false, npmAvailable: true,
          running: true,
          config: { codexBin: 'codex', codexHome: '', defaultEngine: 'local', defaultModel: '', autoRegisterMcp: true, computerUse: false },
          account: { authMode: null, lastError: null }, models: [{ id: 'gpt-5.6' }],
          install: { running: false, log: '' }, mcp: { servers: [] },
        }) } as Response;
      }
      if (path === '/api/forge/codex/mcp/status') {
        return { ok: true, status: 200, json: async () => ({ servers: [], live: false }) } as Response;
      }
      if (path === '/api/forge/codex/account') {
        accountCalls += 1;
        return { ok: true, status: 200, json: async () => ({
          ok: true,
          authMode: null,
          lastError: accountCalls === 2 ? '设备登录已被拒绝' : null,
        }) } as Response;
      }
      if (path === '/api/forge/codex/rate-limits') {
        return { ok: true, status: 200, json: async () => ({ ok: true, rateLimits: null }) } as Response;
      }
      if (path === '/api/forge/codex/login' && method === 'POST') {
        loginCalls += 1;
        return { ok: true, status: 200, json: async () => ({
          loginId: `login-${loginCalls}`,
          userCode: `CODE-${loginCalls}`,
        }) } as Response;
      }
      throw new Error(`未 mock: ${String(url)}`);
    }));

    render(<SettingsOverlay />);
    act(() => openSettings('codex'));
    const login = await screen.findByTestId('codex-login-device');
    fireEvent.click(login);
    expect(await screen.findByTestId('codex-error')).toHaveTextContent('设备登录已被拒绝');
    expect(screen.queryByText('等待 Codex 完成登录…')).not.toBeInTheDocument();
    expect(screen.queryByText('CODE-1')).not.toBeInTheDocument();

    fireEvent.click(login);
    expect(await screen.findByText('CODE-2')).toBeInTheDocument();
    expect(screen.queryByTestId('codex-error')).not.toBeInTheDocument();
    expect(screen.getByText('等待 Codex 完成登录…')).toBeInTheDocument();
  });

  it('轮询耗时超过间隔时不并发饿饿，仍采用已完成的安装结果', async () => {
    vi.useFakeTimers();
    let statusCalls = 0;
    let releaseSlow!: (response: Response) => void;
    const slow = new Promise<Response>((resolve) => {
      releaseSlow = resolve;
    });
    const never = new Promise<Response>(() => undefined);
    const statusBody = (running: boolean, error: string | null = null) => ({
      installed: false,
      managedInstalled: false,
      computerUseInstalled: false,
      npmAvailable: true,
      running: false,
      config: {
        codexBin: '', codexHome: '', defaultEngine: 'local', defaultModel: '',
        autoRegisterMcp: true, computerUse: false,
      },
      account: { authMode: null },
      models: [{ id: 'gpt-test', displayName: 'GPT Test' }],
      install: { running, log: running ? '安装中' : '', error },
      mcp: { servers: [] },
    });
    const response = (body: unknown) => ({
      ok: true,
      status: 200,
      json: async () => body,
    }) as Response;
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const hit = tryAccountStatusFetch(url);
      if (hit) return hit;
      const path = String(url).split('?')[0];
      if (path === '/api/forge/codex/mcp/status') return response({ servers: [], live: false });
      if (path !== '/api/forge/codex/status') throw new Error(`未 mock: ${String(url)}`);
      statusCalls += 1;
      if (statusCalls === 1) return response(statusBody(true));
      if (statusCalls === 2) return slow;
      return never;
    }));

    render(<CodexPage />);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByTestId('codex-install-card')).toHaveTextContent('安装中…');

    await act(async () => {
      vi.advanceTimersByTime(4_000);
      await Promise.resolve();
    });
    expect(statusCalls).toBe(2);

    await act(async () => {
      releaseSlow(response(statusBody(false, 'npm 安装失败')));
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(statusCalls).toBe(3);
    expect(screen.getByTestId('codex-error')).toHaveTextContent('npm 安装失败');
  });
});

describe('模型页', () => {
  it('deepseek 内置卡 availability(design-snapshot 实测)+ 配置展开保存(POST llm/key)', async () => {
    const posts: Array<{ url: string; body: string }> = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const hit = tryAccountStatusFetch(url, init);
        if (hit) return hit;
        const u = String(url);
        if (u === '/api/forge/design-snapshot') {
          return {
            ok: true,
            status: 200,
            json: async () => ({
              models: {
                models: [
                  { id: 'deepseek-chat', label: 'deepseek-chat', provider: 'deepseek', availability: 'needs-key' },
                  { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
                ],
              },
            }),
          } as Response;
        }
        if (u === '/api/forge/llm/key' && init?.method === 'POST') {
          posts.push({ url: u, body: init.body ?? '' });
          return { ok: true, status: 200, json: async () => ({ ok: true, configured: true }) } as Response;
        }
        if (u === '/api/forge/llm/custom-models') {
          return { ok: true, status: 200, json: async () => ({ models: [], defaultId: null }) } as Response;
        }
        if (u === '/api/forge/gen/backends') {
          return { ok: true, status: 200, json: async () => ({ backends: [] }) } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    await openModelsWithByo();
    // needs-key 徽标
    expect(await screen.findByTestId('deepseek-availability')).toHaveTextContent('needs-key');
    // 展开配置 → password 输入 → 保存
    fireEvent.click(screen.getByTestId('deepseek-key-toggle'));
    const input = await screen.findByTestId('deepseek-key-input');
    expect(input).toHaveAttribute('type', 'password');
    fireEvent.change(input, { target: { value: 'sk-test-w5' } });
    fireEvent.click(screen.getByTestId('deepseek-key-save'));
    await act(async () => {
      await Promise.resolve();
    });
    expect(posts.length).toBe(1);
    expect(JSON.parse(posts[0].body)).toEqual({ apiKey: 'sk-test-w5' });
  });

  it('gen backends 平移:清单卡 + configure POST(endpoint/apiKey 不回显)', async () => {
    const posts: Array<{ url: string; body: string }> = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const hit = tryAccountStatusFetch(url, init);
        if (hit) return hit;
        const u = String(url);
        if (u === '/api/forge/design-snapshot') {
          return { ok: true, status: 200, json: async () => ({ models: { models: [] } }) } as Response;
        }
        if (u === '/api/forge/gen/backends/configure' && init?.method === 'POST') {
          posts.push({ url: u, body: init.body ?? '' });
          return { ok: true, status: 200, json: async () => ({ ok: true, id: 'remote-openai-compatible', configured: true }) } as Response;
        }
        if (u === '/api/forge/gen/backends') {
          return {
            ok: true,
            status: 200,
            json: async () => ({
              backends: [
                { id: 'local-mock', kind: 'local', configured: true, endpointSet: false },
                { id: 'remote-openai-compatible', kind: 'remote', configured: false, endpointSet: false },
              ],
            }),
          } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    act(() => openSettings('generation'));
    // 清单两卡(configured 徽标)
    expect(await screen.findByTestId('gen-configured-local-mock')).toHaveTextContent('已就绪');
    expect(screen.getByTestId('gen-configured-remote-openai-compatible')).toHaveTextContent('未启用');
    // remote 展开:endpoint+apiKey 表单 → 保存 POST
    fireEvent.click(screen.getByTestId('gen-configure-remote-openai-compatible'));
    fireEvent.change(await screen.findByTestId('gen-endpoint-remote-openai-compatible'), {
      target: { value: 'https://api.example.com' },
    });
    fireEvent.change(screen.getByTestId('gen-apikey-remote-openai-compatible'), { target: { value: 'sk-gen' } });
    fireEvent.click(screen.getByTestId('gen-save-remote-openai-compatible'));
    await act(async () => {
      await Promise.resolve();
    });
    expect(posts.length).toBe(1);
    const body = JSON.parse(posts[0].body);
    expect(body.id).toBe('remote-openai-compatible');
    expect(body.kind).toBe('remote');
    expect(body.endpoint).toBe('https://api.example.com');
    expect(body.apiKey).toBe('sk-gen');
  });

  it('停用态据实预填:只改 key 的保存不得把 enabled 悄悄翻回 true', async () => {
    const posts: Array<{ url: string; body: string }> = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const hit = tryAccountStatusFetch(url, init);
        if (hit) return hit;
        const u = String(url);
        if (u === '/api/forge/design-snapshot') {
          return { ok: true, status: 200, json: async () => ({ models: { models: [] } }) } as Response;
        }
        if (u === '/api/forge/gen/backends/configure' && init?.method === 'POST') {
          posts.push({ url: u, body: init.body ?? '' });
          return { ok: true, status: 200, json: async () => ({ ok: true, configured: false }) } as Response;
        }
        if (u === '/api/forge/gen/backends') {
          return {
            ok: true,
            status: 200,
            json: async () => ({
              backends: [
                {
                  id: 'remote-video-compatible',
                  kind: 'remote',
                  configured: false,
                  enabled: false,
                  endpointSet: true,
                  keyConfigured: true,
                  model: 'vgen-1',
                  capabilities: { kinds: ['text2video'] },
                },
              ],
            }),
          } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    act(() => openSettings('generation'));
    // 卡片行如实标停用 + 已配置 endpoint + 当前 model
    const row = await screen.findByTestId('gen-backend-row-remote-video-compatible');
    expect(row).toHaveTextContent('已停用');
    expect(row).toHaveTextContent('vgen-1');
    // configured=false 只因停用,key 事实独立成面,不得被误报成「未配置」
    // 展开:toggle 预填 false(不是硬编码 true),model 预填既有值
    fireEvent.click(screen.getByTestId('gen-configure-remote-video-compatible'));
    expect(await screen.findByTestId('gen-enabled-remote-video-compatible')).toHaveAttribute(
      'aria-checked',
      'false',
    );
    expect(screen.getByTestId('gen-model-remote-video-compatible')).toHaveValue('vgen-1');
    expect(
      screen.getByTestId('gen-apikey-remote-video-compatible').getAttribute('placeholder'),
    ).toContain('已配置');
    // 只换 key 后保存 → enabled 保持 false,model 原样带回
    fireEvent.change(screen.getByTestId('gen-apikey-remote-video-compatible'), {
      target: { value: 'sk-only-key' },
    });
    fireEvent.click(screen.getByTestId('gen-save-remote-video-compatible'));
    await act(async () => {
      await Promise.resolve();
    });
    expect(posts.length).toBe(1);
    const body = JSON.parse(posts[0].body);
    expect(body.enabled).toBe(false);
    expect(body.model).toBe('vgen-1');
    expect(body.apiKey).toBe('sk-only-key');
  });

  it('agentd 不可达:离线态如实 + 重试钮复拉清单(不是死胡同)', async () => {
    let backendsUp = false;
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        const u = String(url);
        if (u === '/api/forge/design-snapshot') {
          return { ok: true, status: 200, json: async () => ({ models: { models: [] } }) } as Response;
        }
        if (u === '/api/forge/gen/backends') {
          if (!backendsUp) {
            return {
              ok: false,
              status: 502,
              json: async () => ({
                error: { code: 'UPSTREAM_UNREACHABLE', message: 'agentd 不可达: http://127.0.0.1:8103' },
              }),
            } as Response;
          }
          return {
            ok: true,
            status: 200,
            json: async () => ({
              backends: [{ id: 'local-mock', kind: 'local', configured: true, enabled: true, endpointSet: false }],
            }),
          } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    act(() => openSettings('generation'));
    // 502 → 离线态措辞(与「清单出错」分开)+ 原始信息保留,不吞
    const err = await screen.findByTestId('gen-backends-error');
    expect(err).toHaveTextContent('生成后端服务未连接');
    expect(err).toHaveTextContent('agentd 不可达: http://127.0.0.1:8103');
    expect(screen.queryByTestId('gen-backend-local-mock')).not.toBeInTheDocument();
    // agentd 起来后点重试 → 清单上屏,错误卡消失
    backendsUp = true;
    fireEvent.click(screen.getByTestId('gen-backends-retry'));
    expect(await screen.findByTestId('gen-backend-local-mock')).toBeInTheDocument();
    expect(screen.queryByTestId('gen-backends-error')).not.toBeInTheDocument();
  });
});

describe('模型页 · 自定义模型卡片页(D-049)', () => {
  type Call = { method: string; url: string; body?: Record<string, unknown> };

  const relay = {
    id: 'custom:0a1b2c3d',
    name: 'Claude 中转',
    label: 'Claude 中转',
    apiFormat: 'anthropic',
    baseUrl: 'https://relay.example.com/v1',
    model: 'claude-sonnet-5-5',
    endpoint: 'https://relay.example.com/v1/messages',
    contextWindow: 204800,
    contextWindowSet: true,
    maxOutputTokens: null,
    vision: true,
    keyConfigured: true,
    configured: true,
    isDefault: true,
  };
  const legacy = {
    id: 'openai-compat',
    name: '',
    label: 'qwen2.5-7b',
    apiFormat: 'openai',
    baseUrl: 'http://127.0.0.1:8000',
    model: 'qwen2.5-7b',
    endpoint: 'http://127.0.0.1:8000/v1/chat/completions',
    contextWindow: 1048576,
    contextWindowSet: false,
    maxOutputTokens: null,
    vision: false,
    keyConfigured: false,
    configured: false,
    isDefault: false,
  };

  function reply(body: unknown, status = 200): Response {
    return {
      ok: status < 400,
      status,
      json: async () => body,
      text: async () => (body === undefined ? '' : JSON.stringify(body)),
    } as Response;
  }

  /** 目录 GET 每次取 list() 的当前值;POST/DELETE/test 记入 calls 并按 handler 回包。 */
  function stubCatalog(
    list: () => unknown[],
    calls: Call[],
    handlers: { save?: (b: Record<string, unknown>) => unknown; test?: unknown } = {},
  ) {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const hit = tryAccountStatusFetch(url, init);
        if (hit) return hit;
        const u = String(url);
        const method = init?.method ?? 'GET';
        const body = init?.body ? (JSON.parse(init.body) as Record<string, unknown>) : undefined;
        if (u === '/api/forge/design-snapshot') return reply({ models: { models: [] } });
        if (u === '/api/forge/llm/custom-models' && method === 'GET') return reply({ models: list(), defaultId: null });
        if (u === '/api/forge/llm/custom-models' && method === 'POST') {
          calls.push({ method, url: u, body });
          return reply({ ok: true, model: handlers.save?.(body ?? {}) ?? { ...relay, ...body } });
        }
        if (u === '/api/forge/llm/custom-models/test' && method === 'POST') {
          calls.push({ method, url: u, body });
          return reply(handlers.test ?? { ok: true, latencyMs: 321, endpoint: relay.endpoint, reply: 'pong' });
        }
        if (u.startsWith('/api/forge/llm/custom-models/') && method === 'DELETE') {
          calls.push({ method, url: u });
          return reply({ ok: true });
        }
        if (u === '/api/forge/gen/backends') return reply({ backends: [] });
        throw new Error(`未 mock: ${method} ${u}`);
      }),
    );
  }

  async function flush() {
    await act(async () => {
      await Promise.resolve();
    });
  }

  it('卡片如实呈现格式/地址/窗口/输出/图片输入与状态;整页不含密钥', async () => {
    stubCatalog(() => [relay, legacy], []);
    render(<SettingsOverlay />);
    await openModelsWithByo();
    const card = await screen.findByTestId('custom-model-custom:0a1b2c3d');
    expect(card).toHaveTextContent('Claude 中转');
    expect(card).toHaveTextContent('Anthropic 兼容');
    expect(card).toHaveTextContent('https://relay.example.com/v1/messages');
    expect(card).toHaveTextContent('200K');
    expect(card).toHaveTextContent('32K · 默认');
    expect(card).toHaveTextContent('支持');
    expect(screen.getByTestId('custom-model-status-custom:0a1b2c3d')).toHaveTextContent('可用');
    expect(screen.getByTestId('custom-model-default-custom:0a1b2c3d')).toBeInTheDocument();
    // 旧单槽迁移来的条目:未声明窗口标「默认」,缺 key 不可用。
    const old = screen.getByTestId('custom-model-openai-compat');
    expect(old).toHaveTextContent('1M · 默认');
    expect(old).toHaveTextContent('自动');
    expect(screen.getByTestId('custom-model-status-openai-compat')).toHaveTextContent('缺少密钥');
    expect(screen.getByTestId('custom-model-use-openai-compat')).toBeDisabled();
    expect(screen.getByTestId('custom-models-count')).toHaveTextContent('1/2 可用');
    // DeepSeek 官方作为内置卡片同列;RAG/Embedding 与 Mock 行已撤下。
    expect(screen.getByTestId('channel-deepseek')).toBeInTheDocument();
    expect(screen.queryByTestId('channel-embedding')).not.toBeInTheDocument();
    expect(screen.queryByTestId('models-mock-row')).not.toBeInTheDocument();
    expect(screen.queryByTestId('models-open-generation')).not.toBeInTheDocument();
    expect(screen.getByTestId('settings-page-models').textContent ?? '').not.toContain('sk-');
  });

  it('添加:双格式切换实时预览请求地址;窗口/输出/图片输入随保存上送', async () => {
    const calls: Call[] = [];
    let saved = false;
    stubCatalog(() => (saved ? [relay] : []), calls, {
      save: () => {
        saved = true;
        return relay;
      },
    });
    render(<SettingsOverlay />);
    await openModelsWithByo();
    expect(await screen.findByTestId('custom-models-empty')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('custom-models-add'));
    const form = await screen.findByTestId('cm-form-new');
    const save = screen.getByTestId('cm-save');
    expect(save).toBeDisabled();
    // 默认 OpenAI 格式 → /v1/chat/completions;切 Anthropic → /v1/messages(版本段不重复)。
    fireEvent.change(screen.getByTestId('cm-baseurl-input'), { target: { value: 'https://relay.example.com/v1/' } });
    expect(screen.getByTestId('cm-endpoint-preview')).toHaveTextContent('https://relay.example.com/v1/chat/completions');
    fireEvent.click(screen.getByTestId('cm-format-anthropic'));
    expect(screen.getByTestId('cm-format-anthropic')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('cm-endpoint-preview')).toHaveTextContent('https://relay.example.com/v1/messages');
    expect(form).toHaveTextContent('选「自动」时按 32K 发送');
    fireEvent.change(screen.getByTestId('cm-name-input'), { target: { value: ' Claude 中转 ' } });
    fireEvent.change(screen.getByTestId('cm-model-input'), { target: { value: 'claude-sonnet-5-5' } });
    // 新建必须填 key。
    expect(save).toBeDisabled();
    expect(screen.getByTestId('cm-problem')).toHaveTextContent('API Key');
    fireEvent.change(screen.getByTestId('cm-key-input'), { target: { value: 'sk-test-custom-card' } });
    fireEvent.click(screen.getByTestId('cm-context-200K'));
    fireEvent.click(screen.getByTestId('cm-output-64K'));
    fireEvent.click(screen.getByTestId('cm-vision-toggle'));
    expect(save).toBeEnabled();
    fireEvent.click(save);
    await flush();
    expect(calls).toHaveLength(1);
    expect(calls[0].body).toEqual({
      name: 'Claude 中转',
      apiFormat: 'anthropic',
      baseUrl: 'https://relay.example.com/v1/',
      model: 'claude-sonnet-5-5',
      key: 'sk-test-custom-card',
      contextWindow: 204800,
      maxOutputTokens: 65536,
      vision: true,
    });
    // 保存后表单收起、清单重拉出新卡;密钥不回显。
    expect(await screen.findByTestId('custom-model-custom:0a1b2c3d')).toBeInTheDocument();
    expect(screen.queryByTestId('cm-form-new')).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue('sk-test-custom-card')).not.toBeInTheDocument();
  });

  it('编辑:预填既有值,key 留空则省略(保留原密钥);自定义窗口按 K 换算;输出超窗口拦截', async () => {
    const calls: Call[] = [];
    stubCatalog(() => [relay], calls);
    render(<SettingsOverlay />);
    await openModelsWithByo();
    fireEvent.click(await screen.findByTestId('custom-model-edit-custom:0a1b2c3d'));
    await screen.findByTestId('cm-form-custom:0a1b2c3d');
    expect(screen.getByTestId('cm-model-input')).toHaveValue('claude-sonnet-5-5');
    expect(screen.getByTestId('cm-baseurl-input')).toHaveValue('https://relay.example.com/v1');
    expect(screen.getByTestId('cm-key-input')).toHaveValue('');
    expect(screen.getByTestId('cm-key-input').getAttribute('placeholder')).toContain('已配置');
    expect(screen.getByTestId('cm-format-anthropic')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('cm-context-200K')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('cm-output-auto')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('cm-vision-toggle')).toHaveAttribute('aria-checked', 'true');
    // 自定义 96K 窗口 + 128K 输出 → 超窗口,保存钮禁用并说明原因。
    fireEvent.change(screen.getByTestId('cm-context-custom'), { target: { value: '96' } });
    fireEvent.click(screen.getByTestId('cm-output-128K'));
    expect(screen.getByTestId('cm-save')).toBeDisabled();
    expect(screen.getByTestId('cm-problem')).toHaveTextContent('最大输出不能超过上下文窗口');
    fireEvent.click(screen.getByTestId('cm-output-8K'));
    fireEvent.click(screen.getByTestId('cm-save'));
    await flush();
    expect(calls).toHaveLength(1);
    const body = calls[0].body ?? {};
    expect(body.id).toBe('custom:0a1b2c3d');
    expect('key' in body).toBe(false);
    expect(body.contextWindow).toBe(96 * 1024);
    expect(body.maxOutputTokens).toBe(8192);
    expect(body.apiFormat).toBe('anthropic');
  });

  it('卡片测试连接带 id(后端取已存密钥)并如实回显结果;删除需二次确认', async () => {
    const calls: Call[] = [];
    let list: unknown[] = [relay];
    stubCatalog(() => list, calls, { test: { ok: false, latencyMs: 88, endpoint: relay.endpoint, error: 'anthropic-compat HTTP 401: invalid x-api-key' } });
    render(<SettingsOverlay />);
    await openModelsWithByo();
    fireEvent.click(await screen.findByTestId('custom-model-test-custom:0a1b2c3d'));
    await flush();
    expect(calls[0].url).toBe('/api/forge/llm/custom-models/test');
    expect(calls[0].body).toMatchObject({ id: 'custom:0a1b2c3d', apiFormat: 'anthropic', model: 'claude-sonnet-5-5' });
    expect('key' in (calls[0].body ?? {})).toBe(false);
    expect(await screen.findByTestId('custom-model-test-result-custom:0a1b2c3d')).toHaveTextContent('invalid x-api-key');
    // 删除:第一次点只出确认条,确认后才发 DELETE(id 做 URL 编码)。
    fireEvent.click(screen.getByTestId('custom-model-delete-custom:0a1b2c3d'));
    expect(calls.filter((c) => c.method === 'DELETE')).toHaveLength(0);
    list = [];
    fireEvent.click(screen.getByTestId('custom-model-delete-confirm-custom:0a1b2c3d'));
    await flush();
    expect(calls.find((c) => c.method === 'DELETE')?.url).toBe('/api/forge/llm/custom-models/custom%3A0a1b2c3d');
    expect(await screen.findByTestId('custom-models-empty')).toBeInTheDocument();
  });

  it('旧版 agentd(无目录路由 404)如实提示重启,不是空白死胡同', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string }) => {
        const hit = tryAccountStatusFetch(url, init);
        if (hit) return hit;
        const u = String(url);
        if (u === '/api/forge/design-snapshot') return reply({ models: { models: [] } });
        if (u === '/api/forge/llm/custom-models') {
          return { ok: false, status: 404, json: async () => { throw new SyntaxError('Unexpected end of JSON input'); } } as unknown as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    await openModelsWithByo();
    expect(await screen.findByTestId('custom-models-error')).toHaveTextContent('重启 agentd');
    expect(screen.getByTestId('custom-models-add')).toBeDisabled();
    // DeepSeek 内置卡不依赖目录接口,照常可配。
    expect(screen.getByTestId('deepseek-key-toggle')).toBeInTheDocument();
  });
});

/**
 * F11 wave.5(D-F11-E):技能清单移交工作台「Skill 管理」tab,设置页只留
 * 跳转入口 + 技能目录(extraDirs)配置——避免清单两处事实源。
 */
describe('技能页', () => {
  interface RestCall {
    url: string;
    method: string;
    body: Record<string, unknown> | null;
  }

  function stubSkillsRest(): RestCall[] {
    const calls: RestCall[] = [];
    let extraDirs: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const hit = tryAccountStatusFetch(url, init);
        if (hit) return hit;
        const u = String(url);
        const method = init?.method ?? 'GET';
        const body = init?.body ? (JSON.parse(init.body) as Record<string, unknown>) : null;
        calls.push({ url: u, method, body });
        if (u === '/api/forge/skills/list') {
          return {
            ok: true,
            status: 200,
            json: async () => ({
              skills: [
                {
                  name: 'asset-cleanup',
                  description: '整理资产',
                  enabled: true,
                  tags: [],
                  allowedTools: [],
                  builtin: true,
                  dir: 'skills/asset-cleanup',
                },
              ],
            }),
          } as Response;
        }
        if (u === '/api/forge/skills/config/write' && method === 'POST') {
          if (Array.isArray(body?.extraDirs)) extraDirs = body.extraDirs as string[];
          return {
            ok: true,
            status: 200,
            json: async () => ({ written: true, disabled: [], extraDirs }),
          } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    return calls;
  }

  it('不再重复渲染技能清单;入口钮开 Skill 管理 tab 并关掉设置浮层', async () => {
    stubSkillsRest();
    render(<SettingsOverlay />);
    act(() => openSettings('skills'));
    expect(await screen.findByTestId('settings-page-skills')).toBeInTheDocument();
    expect(screen.queryByTestId('skill-row-asset-cleanup')).not.toBeInTheDocument();
    expect(screen.queryByTestId('skill-toggle-asset-cleanup')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('skills-open-manager'));
    expect(useWorkbenchStore.getState().tabs.some((t) => t.kind === 'skills')).toBe(true);
    expect(useWorkbenchStore.getState().activeTabId).toBe('skills');
    expect(useOverlayStore.getState().settings).toBe(false);
    expect(screen.queryByTestId('settings-overlay')).not.toBeInTheDocument();
  });

  it('技能目录卡:添加目录 → config/write {extraDirs}', async () => {
    const calls = stubSkillsRest();
    render(<SettingsOverlay />);
    act(() => openSettings('skills'));
    expect(await screen.findByTestId('skill-dirs-empty')).toBeInTheDocument();
    fireEvent.change(screen.getByTestId('skill-dir-input'), { target: { value: 'vendor/skills' } });
    fireEvent.click(screen.getByTestId('skill-dir-add'));
    expect(await screen.findByTestId('skill-dir-vendor/skills')).toBeInTheDocument();
    const write = calls.find(
      (c) => c.url === '/api/forge/skills/config/write' && Array.isArray(c.body?.extraDirs),
    );
    expect(write?.body?.extraDirs).toEqual(['vendor/skills']);
  });
});

describe('关于页', () => {
  it('host/agentd/Codex 版本与运行时长实测呈现;无内部代号', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        const u = String(url).split('?')[0];
        const ok = (body: unknown) => ({ ok: true, status: 200, json: async () => body }) as Response;
        if (u === '/api/forge/health') {
          return ok({
            status: 'ok',
            version: '0.1.0',
            uptimeSec: 3,
            platform: 'win32',
            user: { name: 'wcj' },
            agentd: { ok: true, version: '0.2.0', uptimeSec: 7200 },
          });
        }
        if (u === '/api/forge/design-snapshot') return ok({});
        if (u === '/api/forge/codex/status') return ok({ installed: true, version: '0.44.0' });
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    act(() => openSettings('about'));
    expect(await screen.findByTestId('about-host-health')).toHaveTextContent('在线');
    expect(screen.getByTestId('about-host-health')).toHaveTextContent('v0.1.0');
    expect(await screen.findByText(/可达 · v0\.2\.0 · 已运行 2 小时/)).toBeInTheDocument();
    expect(await screen.findByText('v0.44.0')).toBeInTheDocument();
    expect(screen.getByTestId('about-platform')).toHaveTextContent('win32 · wcj');
    expect(screen.getByTestId('about-runtime')).toHaveTextContent('浏览器模式');
    const page = screen.getByTestId('settings-page-about');
    expect(page.textContent).not.toMatch(/wave|Moonlit/);
  });
});
