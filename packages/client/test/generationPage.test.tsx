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

it('cloud model list: vendor tiles with window/capabilities, search + vendor filter, raw ids only in tooltips', async () => {
  useAccountStore.setState({ status: makeAccountStatus({ loggedIn: true }) });
  vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
    currency: 'USD',
    defaultModel: 'gpt-x',
    models: [
      { id: 'gpt-x', displayName: 'GPT X', platform: 'openai', available: true, capabilities: { vision: true, contextWindow: 1050000, maxOutput: 128000, reasoningEfforts: ['low', 'high'], tools: true }, pricing: { inputPer1M: 0, outputPer1M: 0 } },
      { id: 'claude-y', displayName: 'Claude Y', platform: 'anthropic', available: true, capabilities: { vision: false, contextWindow: 1048576, maxOutput: 65536, reasoningEfforts: [], tools: true }, pricing: { inputPer1M: 3_000_000, outputPer1M: 15_000_000 } },
      { id: 'gemini-z', displayName: 'Gemini Z', platform: 'openai', available: false },
    ],
  })));
  render(<CloudModelsSection />);
  const list = await screen.findByTestId('cloud-models-list');
  expect(screen.getByTestId('cloud-models-summary')).toHaveTextContent('2/3 可用 · 默认 GPT X');
  const gpt = screen.getByTestId('cloud-model-gpt-x');
  expect(gpt).toHaveTextContent('OpenAI');
  expect(gpt).toHaveTextContent('1.05M 上下文 · 128K 输出');
  expect(gpt).toHaveTextContent('默认');
  expect(gpt).toHaveTextContent('视觉');
  expect(gpt).toHaveTextContent('推理');
  expect(gpt).not.toHaveTextContent('USD');
  // Gemini 经 openai 协议上游,厂商按模型族归类而不是 platform。
  expect(screen.getByTestId('cloud-model-gemini-z')).toHaveTextContent('Google');
  expect(screen.getByTestId('cloud-model-gemini-z')).toHaveTextContent('暂不可用');
  expect(screen.getByTestId('cloud-model-use-gemini-z')).toBeDisabled();
  const claude = screen.getByTestId('cloud-model-claude-y');
  expect(claude).toHaveTextContent('1M 上下文 · 64K 输出');
  expect(claude).toHaveTextContent('输入 3.00 / 输出 15.00 USD 每百万 tokens');
  expect(claude).toHaveAttribute('title', 'Claude Y(claude-y)');
  expect(list).not.toHaveTextContent('claude-y');
  fireEvent.change(screen.getByTestId('cloud-models-search'), { target: { value: 'claude' } });
  expect(screen.queryByTestId('cloud-model-gpt-x')).not.toBeInTheDocument();
  expect(screen.getByTestId('cloud-model-claude-y')).toBeInTheDocument();
  fireEvent.change(screen.getByTestId('cloud-models-search'), { target: { value: '' } });
  fireEvent.click(screen.getByTestId('cloud-models-vendor-Google'));
  expect(screen.getByTestId('cloud-model-gemini-z')).toBeInTheDocument();
  expect(screen.queryByTestId('cloud-model-claude-y')).not.toBeInTheDocument();
  fireEvent.change(screen.getByTestId('cloud-models-search'), { target: { value: 'nothing-here' } });
  expect(screen.getByTestId('cloud-models-no-match')).toBeInTheDocument();
});
