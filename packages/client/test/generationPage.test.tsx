import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import GenerationPage from '@/components/settings/GenerationPage';
import CloudModelsSection from '@/components/settings/CloudModelsSection';
import { useAccountStore } from '@/lib/accountStore';
import { makeAccountStatus, jsonResponse } from './accountTestHelpers';

const initialAccount = useAccountStore.getState();
afterEach(() => { cleanup(); vi.unstubAllGlobals(); useAccountStore.setState(initialAccount, true); });

describe('generation connections', () => {
  it('creates an independently named connection and reloads the persisted catalog', async () => {
    const saved: Record<string, unknown>[] = [];
    const backends = [{ id: 'remote-openai-compatible', kind: 'remote', configured: false, enabled: false, endpointSet: false, capabilities: { kinds: ['text2img'] } }];
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      if (String(url).endsWith('/configure')) {
        const body = JSON.parse(String(init?.body));
        saved.push(body);
        backends.push({ ...body, configured: true, endpointSet: true });
        return jsonResponse({ ok: true, configured: true });
      }
      return jsonResponse({ backends });
    }));
    render(<GenerationPage />);
    await waitFor(() => expect(screen.getByTestId('gen-add')).toBeEnabled());
    fireEvent.click(screen.getByTestId('gen-add'));
    const form = screen.getByTestId('gen-new-connection');
    fireEvent.change(within(form).getByLabelText('连接名称'), { target: { value: '我的图像服务' } });
    fireEvent.change(within(form).getByLabelText('服务地址'), { target: { value: ' https://images.example.test ' } });
    fireEvent.change(within(form).getByLabelText('模型'), { target: { value: 'my-image' } });
    fireEvent.change(within(form).getByLabelText('API Key'), { target: { value: 'test-only-key' } });
    fireEvent.click(within(form).getByRole('button', { name: '保存' }));
    await waitFor(() => expect(saved).toHaveLength(1));
    expect(saved[0]).toEqual({ id: expect.stringMatching(/^custom-/), adapter: 'remote-openai-compatible', label: '我的图像服务', kind: 'remote', enabled: true, endpoint: 'https://images.example.test', model: 'my-image', apiKey: 'test-only-key' });
    expect(await screen.findByText('我的图像服务')).toBeVisible();
    expect(screen.queryByTestId('gen-new-connection')).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue('test-only-key')).not.toBeInTheDocument();
  });

  it('renames a disabled custom connection while preserving its endpoint and secret', async () => {
    const saved: Record<string, unknown>[] = [];
    const id = 'custom-video';
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      if (String(url).endsWith('/configure')) { saved.push(JSON.parse(String(init?.body))); return jsonResponse({ ok: true }); }
      return jsonResponse({ backends: [{ id, label: '视频服务', adapter: 'remote-video-compatible', kind: 'remote', enabled: false, configured: false, endpointSet: true, keyConfigured: true, model: 'video-1' }] });
    }));
    render(<GenerationPage />);
    fireEvent.click(await screen.findByTestId(`gen-configure-${id}`));
    expect(screen.getByTestId(`gen-enabled-${id}`)).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByTestId(`gen-endpoint-${id}`)).toHaveValue('');
    expect(screen.getByTestId(`gen-apikey-${id}`)).toHaveValue('');
    fireEvent.change(screen.getByTestId(`gen-name-${id}`), { target: { value: '改名视频服务' } });
    fireEvent.click(screen.getByTestId(`gen-save-${id}`));
    await waitFor(() => expect(saved).toHaveLength(1));
    expect(saved[0]).toEqual({ id, label: '改名视频服务', adapter: 'remote-video-compatible', kind: 'remote', enabled: false, model: 'video-1' });
  });
});

it('shows a compact model catalog with names only', async () => {
  useAccountStore.setState({ status: makeAccountStatus({ loggedIn: true }) });
  vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ currency: 'USD', defaultModel: 'model-a', models: [{ id: 'model-a', displayName: 'Model A', platform: 'openai', available: true, capabilities: { vision: true, contextWindow: 1000000, reasoningEfforts: ['high'] }, pricing: { inputPer1M: 123, outputPer1M: 456 } }, { id: 'model-b', displayName: 'Model B', available: false }] })));
  render(<CloudModelsSection />);
  const catalog = await screen.findByTestId('cloud-models-names');
  expect(catalog).toHaveTextContent('Model AModel B');
  expect(catalog).not.toHaveTextContent('model-a');
  expect(catalog).not.toHaveTextContent('USD');
  expect(catalog).not.toHaveTextContent('上下文');
  expect(catalog).not.toHaveTextContent('视觉');
  expect(screen.getByTestId('cloud-model-model-b')).toHaveAttribute('title', 'Model B · 暂不可用');
});
