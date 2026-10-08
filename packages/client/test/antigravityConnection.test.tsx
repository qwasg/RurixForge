import { StrictMode } from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import AntigravityConnectionCard, { antigravityQuotaWindows } from '@/components/settings/AntigravityConnectionCard';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';

const api = vi.hoisted(() => ({ status: vi.fn(), save: vi.fn(), probe: vi.fn() }));
vi.mock('@/lib/forgeApi', async (original) => ({ ...await original<typeof import('@/lib/forgeApi')>(),
  getAntigravityStatus: api.status, postAntigravityConfig: api.save, postAntigravityProbe: api.probe }));
const empty = { configured: false, keyConfigured: false, baseUrl: '', model: 'gemini-3.8-flash', availability: 'needs-config' };
const connected = { ...empty, configured: true, keyConfigured: true, baseUrl: 'http://127.0.0.1:8080', availability: 'available' };
const chat = useChatStore.getState();
const sessions = useSessionStore.getState();
beforeEach(() => {
  vi.clearAllMocks(); api.status.mockResolvedValue(empty);
  useChatStore.setState({ ...chat, selectedModelId: null, ensureModels: vi.fn().mockResolvedValue(undefined) }, true);
  useSessionStore.setState({ ...sessions, activeSessionId: null, draftAgentEngine: 'codex' }, true);
  vi.spyOn(useToastStore.getState(), 'push');
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); useChatStore.setState(chat, true); useSessionStore.setState(sessions, true); });

describe('Antigravity primary subscription card', () => {
  it('does not fabricate full remaining quota for missing or invalid percentages', () => {
    expect(antigravityQuotaWindows({ primary: {}, secondary: { usedPercent: NaN } })).toEqual([]);
    expect(antigravityQuotaWindows({ primary: { remainingPercent: 72 }, secondary: { usedPercent: 120 } })).toMatchObject([{ usedPercent: 28 }, { usedPercent: 100 }]);
  });
  it('loads StrictMode state and displays real remaining quota', async () => {
    api.status.mockResolvedValue({ ...connected, rateLimits: { primary: { usedPercent: 30 } } });
    render(<StrictMode><AntigravityConnectionCard /></StrictMode>);
    expect(await screen.findByText('已连接')).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '70');
  });
  it('clears an unsubmitted connection key when closing and reopening the dialog', async () => {
    render(<AntigravityConnectionCard />);
    const button = screen.getByTestId('channel-login-antigravity');
    await waitFor(() => expect(button).not.toBeDisabled()); fireEvent.click(button);
    fireEvent.change(screen.getByLabelText('反代访问密钥'), { target: { value: 'private-connection-key' } });
    fireEvent.click(screen.getByRole('button', { name: '关闭反重力配置' })); fireEvent.click(button);
    expect(screen.getByLabelText('反代访问密钥')).toHaveValue('');
    expect(api.save).not.toHaveBeenCalled();
  });
  it('persists the actual connection, removes the key from the form and switches the engine and model', async () => {
    render(<AntigravityConnectionCard />);
    const button = screen.getByTestId('channel-login-antigravity');
    await waitFor(() => expect(button).not.toBeDisabled()); fireEvent.click(button);
    fireEvent.change(screen.getByLabelText('反代服务地址'), { target: { value: 'http://127.0.0.1:8080/' } });
    fireEvent.change(screen.getByLabelText('反代访问密钥'), { target: { value: ' proxy-key ' } });
    api.save.mockResolvedValue(connected); api.status.mockResolvedValue(connected);
    fireEvent.click(screen.getByRole('button', { name: '保存并连接' }));
    await waitFor(() => expect(api.save).toHaveBeenCalledWith({ baseUrl: 'http://127.0.0.1:8080', model: 'gemini-3.8-flash', key: 'proxy-key', enabled: true }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    fireEvent.click(await screen.findByRole('button', { name: '使用模型' }));
    await waitFor(() => expect(useChatStore.getState().selectedModelId).toBe('antigravity'));
    expect(useSessionStore.getState().draftAgentEngine).toBe('local');
  });
  it('synchronizes a failed probe and stops offering the disconnected model as ready', async () => {
    api.status.mockResolvedValue(connected); render(<AntigravityConnectionCard />);
    await screen.findByText('已连接');
    api.status.mockResolvedValue({ ...connected, availability: 'disconnected' });
    api.probe.mockResolvedValue({ ok: false, error: '上游连接失败' });
    fireEvent.click(screen.getByRole('button', { name: '刷新 Antigravity 额度' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('上游连接失败');
    expect(screen.getByText('连接异常')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '使用模型' })).not.toBeInTheDocument();
  });
});
