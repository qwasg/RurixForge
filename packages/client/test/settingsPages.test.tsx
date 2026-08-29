import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsOverlay from '@/components/settings/SettingsOverlay';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useSkillStore } from '@/lib/skillStore';
import { useThemeStore } from '@/lib/themeStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

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

function openSettings(page?: 'appearance' | 'agent' | 'models' | 'skills' | 'about') {
  if (page) useSettingsStore.getState().setPage(page);
  useOverlayStore.getState().open('settings');
}

beforeEach(() => {
  useSettingsStore.setState(initialSettings, true);
  useOverlayStore.setState(initialOverlay, true);
  useThemeStore.setState(initialTheme, true);
  useSkillStore.setState(initialSkill, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  globalThis.localStorage?.clear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('设置 overlay 壳', () => {
  it('关态不渲染;开态 = 全屏 overlay + 五页导航;翻页持久化', async () => {
    render(<SettingsOverlay />);
    expect(screen.queryByTestId('settings-overlay')).not.toBeInTheDocument();
    act(() => openSettings());
    expect(await screen.findByTestId('settings-overlay')).toBeInTheDocument();
    // 五页导航行
    for (const p of ['appearance', 'agent', 'models', 'skills', 'about']) {
      expect(screen.getByTestId(`settings-nav-${p}`)).toBeInTheDocument();
    }
    // 默认外观页
    expect(screen.getByTestId('settings-page-appearance')).toBeInTheDocument();
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
  it('Ctrl+Enter toggle 写 settingsStore;权限信息行如实静态', async () => {
    render(<SettingsOverlay />);
    act(() => openSettings('agent'));
    const toggle = await screen.findByTestId('submit-ctrl-toggle');
    expect(toggle).toHaveAttribute('aria-checked', 'false');
    fireEvent.click(toggle);
    expect(useSettingsStore.getState().submitCtrlEnter).toBe(true);
    expect(globalThis.localStorage?.getItem('forge:submitCtrl')).toBe('1');
    expect(screen.getByTestId('permission-mode-value')).toHaveTextContent('bypass · 工具全部自动执行');
    expect(screen.getByTestId('permission-mode-bypass')).toBeInTheDocument();
    expect(screen.getByTestId('permission-mode-plan')).toBeInTheDocument();
    expect(screen.getByTestId('permission-mode-auto')).toBeInTheDocument();
  });
});

describe('模型页', () => {
  it('deepseek 卡 availability(design-snapshot 实测)+ 配置展开保存(POST llm/key)', async () => {
    const posts: Array<{ url: string; body: string }> = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
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
        if (u === '/api/forge/gen/backends') {
          return { ok: true, status: 200, json: async () => ({ backends: [] }) } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    act(() => openSettings('models'));
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
    act(() => openSettings('models'));
    // 清单两卡(configured 徽标)
    expect(await screen.findByTestId('gen-configured-local-mock')).toHaveTextContent('configured');
    expect(screen.getByTestId('gen-configured-remote-openai-compatible')).toHaveTextContent('未配置');
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
    act(() => openSettings('models'));
    // 卡片行如实标停用 + 已配置 endpoint + 当前 model
    const row = await screen.findByTestId('gen-backend-row-remote-video-compatible');
    expect(row).toHaveTextContent('已停用');
    expect(row).toHaveTextContent('endpoint 已配置');
    expect(row).toHaveTextContent('model=vgen-1');
    // configured=false 只因停用,key 事实独立成面,不得被误报成「未配置」
    expect(row).toHaveTextContent('key 已配置');
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
    act(() => openSettings('models'));
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

describe('模型页 · OpenAI-Compatible 渠道卡(F8 wave.2)', () => {
  /** 三端点公共 mock 面(design-snapshot=deepseek 卡;status=本卡;gen/backends=生成后端区)。 */
  function stubBase(statusBody: unknown, posts: Array<{ url: string; body: string }>) {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const u = String(url);
        if (u === '/api/forge/design-snapshot') {
          return { ok: true, status: 200, json: async () => ({ models: { models: [] } }) } as Response;
        }
        if (u === '/api/forge/llm/openai-compat/status') {
          return { ok: true, status: 200, json: async () => statusBody } as Response;
        }
        if (u === '/api/forge/llm/openai-compat/config' && init?.method === 'POST') {
          posts.push({ url: u, body: init.body ?? '' });
          return {
            ok: true,
            status: 200,
            json: async () => ({ ok: true, configured: true, baseUrl: 'http://127.0.0.1:8000', model: 'qwen2.5-7b', keyConfigured: true }),
          } as Response;
        }
        if (u === '/api/forge/gen/backends') {
          return { ok: true, status: 200, json: async () => ({ backends: [] }) } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
  }

  it('未配置腿:needs-key 徽标 + 无配置行', async () => {
    stubBase({ configured: false, baseUrl: '', model: '', keyConfigured: false }, []);
    render(<SettingsOverlay />);
    act(() => openSettings('models'));
    expect(await screen.findByTestId('channel-openai-compat')).toBeInTheDocument();
    expect(screen.getByTestId('oai-availability')).toHaveTextContent('needs-key');
    expect(screen.queryByTestId('oai-status-line')).not.toBeInTheDocument();
  });

  it('已配置腿:available 徽标 + 状态行显示 baseUrl/model/key 已配置(无 key 串)', async () => {
    stubBase(
      { configured: true, baseUrl: 'http://127.0.0.1:8000', model: 'qwen2.5-7b', keyConfigured: true },
      [],
    );
    render(<SettingsOverlay />);
    act(() => openSettings('models'));
    expect(await screen.findByTestId('oai-availability')).toHaveTextContent('available');
    const line = await screen.findByTestId('oai-status-line');
    expect(line).toHaveTextContent('http://127.0.0.1:8000');
    expect(line).toHaveTextContent('qwen2.5-7b');
    expect(line).toHaveTextContent('key 已配置');
    // R-5:整卡无 sk- 串。
    expect(screen.getByTestId('channel-openai-compat').textContent ?? '').not.toContain('sk-');
  });

  it('配置展开:预填 baseUrl/model + 保存 POST(key 进 body 不回显;key 留空则省略)', async () => {
    const posts: Array<{ url: string; body: string }> = [];
    stubBase(
      { configured: true, baseUrl: 'http://127.0.0.1:8000', model: 'qwen2.5-7b', keyConfigured: true },
      posts,
    );
    render(<SettingsOverlay />);
    act(() => openSettings('models'));
    // 展开:预填已配置 baseUrl/model,key 空(password 不回显)。
    fireEvent.click(await screen.findByTestId('oai-config-toggle'));
    const buInput = await screen.findByTestId('oai-baseurl-input');
    expect(buInput).toHaveValue('http://127.0.0.1:8000');
    expect(screen.getByTestId('oai-model-input')).toHaveValue('qwen2.5-7b');
    const keyInput = screen.getByTestId('oai-key-input');
    expect(keyInput).toHaveAttribute('type', 'password');
    expect(keyInput).toHaveValue('');
    // 改 model + 填 key → 保存 → POST 全量三联。
    fireEvent.change(screen.getByTestId('oai-model-input'), { target: { value: 'glm-4-air' } });
    fireEvent.change(keyInput, { target: { value: 'sk-test-oai-card' } });
    fireEvent.click(screen.getByTestId('oai-config-save'));
    await act(async () => {
      await Promise.resolve();
    });
    expect(posts.length).toBe(1);
    // vision 恒随体上送(不同于 key 的「留空即省略」):它是开关不是密钥,
    // 省略会被后端当成「保留既有」,那样用户关不掉它。
    expect(JSON.parse(posts[0].body)).toEqual({
      baseUrl: 'http://127.0.0.1:8000',
      model: 'glm-4-air',
      key: 'sk-test-oai-card',
      vision: false,
    });
    // 保存后收起 + 状态刷新(第二轮 status 仍为 stub 值)。
    expect(screen.queryByTestId('oai-config-form')).not.toBeInTheDocument();
    // key 留空保存 → body 省略 key 域(只改 baseUrl/model)。
    fireEvent.click(screen.getByTestId('oai-config-toggle'));
    fireEvent.click(await screen.findByTestId('oai-config-save'));
    await act(async () => {
      await Promise.resolve();
    });
    expect(posts.length).toBe(2);
    const body2 = JSON.parse(posts[1].body) as Record<string, unknown>;
    expect(body2.baseUrl).toBe('http://127.0.0.1:8000');
    expect(body2.model).toBe('qwen2.5-7b');
    expect('key' in body2).toBe(false);
    expect(body2.vision).toBe(false);
  });

  it('图片输入开关:从 status 预填并随保存上送', async () => {
    const posts: Array<{ url: string; body: string }> = [];
    stubBase(
      {
        configured: true,
        baseUrl: 'http://127.0.0.1:8000',
        model: 'gpt-4o-mini',
        keyConfigured: true,
        vision: true,
      },
      posts,
    );
    render(<SettingsOverlay />);
    act(() => openSettings('models'));
    fireEvent.click(await screen.findByTestId('oai-config-toggle'));
    // 已开启态须按 status 预填,否则「只改 model」的保存会把它悄悄关掉。
    const toggle = await screen.findByTestId('oai-vision-toggle');
    expect(toggle).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(toggle);
    fireEvent.click(screen.getByTestId('oai-config-save'));
    await act(async () => {
      await Promise.resolve();
    });
    expect((JSON.parse(posts[0].body) as Record<string, unknown>).vision).toBe(false);
  });

  it('空 baseUrl/model 保存钮禁用(400 防线前置)', async () => {
    stubBase({ configured: false, baseUrl: '', model: '', keyConfigured: false }, []);
    render(<SettingsOverlay />);
    act(() => openSettings('models'));
    fireEvent.click(await screen.findByTestId('oai-config-toggle'));
    expect(await screen.findByTestId('oai-config-save')).toBeDisabled();
    fireEvent.change(screen.getByTestId('oai-baseurl-input'), { target: { value: 'http://x' } });
    expect(screen.getByTestId('oai-config-save')).toBeDisabled();
    fireEvent.change(screen.getByTestId('oai-model-input'), { target: { value: 'm' } });
    expect(screen.getByTestId('oai-config-save')).toBeEnabled();
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
  it('host/agentd health 实测呈现', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        const u = String(url);
        if (u === '/api/forge/health') {
          return { ok: true, status: 200, json: async () => ({ status: 'ok', version: '0.1.0', uptimeSec: 3 }) } as Response;
        }
        if (u === '/api/forge/design-snapshot') {
          return { ok: true, status: 200, json: async () => ({}) } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    act(() => openSettings('about'));
    expect(await screen.findByTestId('about-host-health')).toHaveTextContent('在线');
    expect(await screen.findByTestId('about-agentd-health')).toHaveTextContent('可达');
  });
});
