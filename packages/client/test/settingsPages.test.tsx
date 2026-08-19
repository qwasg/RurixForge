import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsOverlay from '@/components/settings/SettingsOverlay';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useThemeStore } from '@/lib/themeStore';

/**
 * F7 wave.5 设置体系:全屏 overlay + 左导航翻页 + Agent 页(Ctrl+Enter/权限信息行)
 * + 模型页(deepseek 卡/配置 key 展开保存/gen backends 平移)+ 技能页(toggle 全量写回)
 * + 关于页(health 实测)。
 */

const initialSettings = useSettingsStore.getState();
const initialOverlay = useOverlayStore.getState();
const initialTheme = useThemeStore.getState();

function openSettings(page?: 'appearance' | 'agent' | 'models' | 'skills' | 'about') {
  if (page) useSettingsStore.getState().setPage(page);
  useOverlayStore.getState().open('settings');
}

beforeEach(() => {
  useSettingsStore.setState(initialSettings, true);
  useOverlayStore.setState(initialOverlay, true);
  useThemeStore.setState(initialTheme, true);
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
});

describe('技能页', () => {
  it('清单渲染 + toggle → config/write {disabled} 全量写回', async () => {
    const writes: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
        const u = String(url);
        if (u === '/api/forge/skills/list') {
          return {
            ok: true,
            status: 200,
            json: async () => ({
              skills: [
                { name: 'asset-cleanup', description: '整理资产', enabled: true },
                { name: 'scene-greybox', description: '灰盒', enabled: true },
              ],
            }),
          } as Response;
        }
        if (u === '/api/forge/skills/config/write' && init?.method === 'POST') {
          writes.push(init.body ?? '');
          return { ok: true, status: 200, json: async () => ({ written: true }) } as Response;
        }
        throw new Error(`未 mock: ${u}`);
      }),
    );
    render(<SettingsOverlay />);
    act(() => openSettings('skills'));
    const toggle = await screen.findByTestId('skill-toggle-asset-cleanup');
    expect(toggle).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(toggle);
    await act(async () => {
      await Promise.resolve();
    });
    expect(writes.length).toBe(1);
    expect(JSON.parse(writes[0])).toEqual({ disabled: ['asset-cleanup'] });
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
