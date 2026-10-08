import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import GenerationPage from '@/components/settings/GenerationPage';
import { mockForgeBackend } from './forgeMock';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('本地 H3 设置', () => {
  it('显示本地服务地址与启用状态,保存无需密钥且可停用', async () => {
    const posts: Record<string, unknown>[] = [];
    const id = 'comfyui-minimax-h3';
    vi.stubGlobal('fetch', mockForgeBackend({}, {
      '/api/forge/design-snapshot': { models: { models: [] } },
      '/api/forge/gen/backends': { backends: [{
        id, kind: 'local', enabled: true, configured: true, endpointSet: true, keyConfigured: false,
        capabilities: { kinds: ['text2video', 'image2video'], defaultEndpoint: 'http://127.0.0.1:8188' },
      }] },
      '/api/forge/gen/backends/configure': (init?: { body?: string }) => {
        posts.push(JSON.parse(init?.body ?? '{}'));
        return { ok: true, configured: false };
      },
    }));
    render(<GenerationPage />);
    const row = await screen.findByTestId(`gen-backend-row-${id}`);
    expect(row).toHaveTextContent('MiniMax H3 · 本地');
    expect(row).toHaveTextContent('无需 API Key');
    expect(row).toHaveTextContent('352p / 2 秒预览');
    fireEvent.click(screen.getByTestId(`gen-configure-${id}`));
    expect(screen.getByTestId(`gen-endpoint-${id}`)).toHaveValue('');
    expect(screen.getByTestId(`gen-endpoint-${id}`)).toHaveAttribute('placeholder', expect.stringContaining('留空保留既有'));
    expect(screen.queryByTestId(`gen-apikey-${id}`)).toBeNull();
    expect(screen.queryByTestId(`gen-model-${id}`)).toBeNull();
    fireEvent.change(screen.getByTestId(`gen-endpoint-${id}`), { target: { value: ' http://127.0.0.1:8288 ' } });
    fireEvent.click(screen.getByTestId(`gen-enabled-${id}`));
    fireEvent.click(screen.getByTestId(`gen-save-${id}`));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]).toEqual({ id, kind: 'local', enabled: false, endpoint: 'http://127.0.0.1:8288' });
  });

  it.each([false, true])('仅启用并保存时,endpointSet=%s 决定预填默认地址或保留既有地址', async (endpointSet) => {
    const posts: Record<string, unknown>[] = [];
    const id = 'comfyui-minimax-h3';
    const defaultEndpoint = 'http://127.0.0.1:8188';
    vi.stubGlobal('fetch', mockForgeBackend({}, {
      '/api/forge/design-snapshot': { models: { models: [] } },
      '/api/forge/gen/backends': { backends: [{
        id, kind: 'local', enabled: false, configured: false, endpointSet, keyConfigured: false,
        capabilities: { kinds: ['text2video', 'image2video'], defaultEndpoint },
      }] },
      '/api/forge/gen/backends/configure': (init?: { body?: string }) => {
        posts.push(JSON.parse(init?.body ?? '{}'));
        return { ok: true, configured: true };
      },
    }));
    render(<GenerationPage />);
    fireEvent.click(await screen.findByTestId(`gen-configure-${id}`));
    expect(screen.getByTestId(`gen-endpoint-${id}`)).toHaveValue(endpointSet ? '' : defaultEndpoint);
    expect(screen.queryByTestId(`gen-apikey-${id}`)).toBeNull();
    fireEvent.click(screen.getByTestId(`gen-enabled-${id}`));
    fireEvent.click(screen.getByTestId(`gen-save-${id}`));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]).toEqual({ id, kind: 'local', enabled: true, ...(!endpointSet ? { endpoint: defaultEndpoint } : {}) });
  });
});
