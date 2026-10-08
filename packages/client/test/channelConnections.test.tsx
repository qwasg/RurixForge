import { StrictMode } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ChannelConnections, { OfficialChannelCard } from '@/components/settings/ChannelConnections';
import { channelDefaults, type ChannelStatus } from '@/lib/channelApi';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';

const api = vi.hoisted(() => ({ status: vi.fn(), codexStatus: vi.fn(), login: vi.fn(), codexLogin: vi.fn(), cancel: vi.fn(), codexCancel: vi.fn(), bind: vi.fn() }));
vi.mock('@/lib/channelApi', async (original) => ({ ...await original<typeof import('@/lib/channelApi')>(),
  getOfficialChannel: api.status, codexChannelStatus: api.codexStatus, loginOfficialChannel: api.login,
  cancelOfficialLogin: api.cancel, bindGlmKey: api.bind }));
vi.mock('@/lib/forgeApi', async (original) => ({ ...await original<typeof import('@/lib/forgeApi')>(),
  postCodexLogin: api.codexLogin, postCodexLoginCancel: api.codexCancel }));
const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();

beforeEach(() => {
  localStorage.removeItem('forge:channelConnectionsExpanded');
  vi.clearAllMocks(); vi.stubGlobal('forgeAPI', undefined);
  api.status.mockImplementation(async (id: 'antigravity' | 'kimi' | 'glm') => ({ ...channelDefaults[id], installed: true, login: { state: 'idle' } }));
  api.codexStatus.mockResolvedValue({ ...channelDefaults.codex, installed: true, modelId: 'codex:gpt-test' });
  api.cancel.mockResolvedValue({ ok: true }); api.codexCancel.mockResolvedValue({ ok: true });
  useSessionStore.setState({ ...initialSessions, activeSessionId: null, draftAgentEngine: 'local' }, true);
  useChatStore.setState({ ...initialChat, selectedModelId: null, ensureModels: vi.fn().mockResolvedValue(undefined) }, true);
  vi.spyOn(useToastStore.getState(), 'push');
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); useChatStore.setState(initialChat, true); useSessionStore.setState(initialSessions, true); });

async function loginButton(id: string) {
  const button = screen.getByTestId(`channel-login-${id}`);
  await waitFor(() => expect(button).not.toBeDisabled());
  return button;
}

describe('official subscription cards', () => {
  it('collapses the subscription grid without discarding an in-progress form and remembers the preference', async () => {
    const view = render(<ChannelConnections />);
    await loginButton('glm');
    fireEvent.click(screen.getByRole('button', { name: '已有套餐 Key' }));
    const input = screen.getByLabelText('Coding Plan Key');
    fireEvent.change(input, { target: { value: 'unfinished-test-key' } });
    const toggle = screen.getByTestId('channel-connections-toggle');
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(input).not.toBeVisible();
    fireEvent.click(toggle);
    expect(screen.getByLabelText('Coding Plan Key')).toBe(input);
    expect(input).toHaveValue('unfinished-test-key');
    fireEvent.click(toggle);
    view.unmount();
    render(<ChannelConnections />);
    expect(screen.getByTestId('channel-connections-toggle')).toHaveAttribute('aria-expanded', 'false');
    localStorage.removeItem('forge:channelConnectionsExpanded');
  });
  it('starts Google OAuth from the primary Antigravity button and cancels the pending login', async () => {
    const replace = vi.fn();
    vi.spyOn(window, 'open').mockReturnValue({ closed: false, opener: null, document: { body: { style: {} } }, location: { replace }, close: vi.fn() } as never);
    const url = 'https://accounts.google.com/o/oauth2/v2/auth?state=google-test';
    api.login.mockResolvedValue({ state: 'pending', authUrl: url, loginId: 'google-test' });
    render(<OfficialChannelCard channel="antigravity" />);
    fireEvent.click(await loginButton('antigravity'));
    await waitFor(() => expect(replace).toHaveBeenCalledWith(url));
    expect(api.login).toHaveBeenCalledWith('antigravity');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: /继续官方授权/ })).toHaveAttribute('href', url);
    fireEvent.click(screen.getByRole('button', { name: '取消 Antigravity 登录' }));
    await waitFor(() => expect(api.cancel).toHaveBeenCalledWith('antigravity'));
  });
  it('cancels an invalid Google origin without navigating the authorization window', async () => {
    const replace = vi.fn();
    vi.spyOn(window, 'open').mockReturnValue({ closed: false, opener: null, document: { body: { style: {} } }, location: { replace }, close: vi.fn() } as never);
    api.login.mockResolvedValue({ state: 'pending', authUrl: 'https://accounts.google.com.evil.test/auth' });
    render(<OfficialChannelCard channel="antigravity" />);
    fireEvent.click(await loginButton('antigravity'));
    expect(await screen.findByRole('alert')).toHaveTextContent('官方授权地址校验失败');
    expect(api.cancel).toHaveBeenCalledWith('antigravity');
    expect(replace).not.toHaveBeenCalled();
  });
  it('uses the authorized Google model and shows only its official quota', async () => {
    api.status.mockResolvedValue({ ...channelDefaults.antigravity, configured: true, authMode: 'googleOAuth',
      account: { email: 'person@example.test' }, modelId: 'antigravity:gemini-flash', model: 'Gemini Flash',
      models: [{ id: 'antigravity:gemini-flash', label: 'Gemini Flash' }, { id: 'antigravity:claude', label: 'Claude' }],
      quota: { state: 'available', windows: [{ id: 'gemini-flash', label: 'Gemini Flash', usedPercent: 30 }, { id: 'claude', label: 'Claude', usedPercent: 100 }] } });
    render(<OfficialChannelCard channel="antigravity" />);
    expect(await screen.findByText('person@example.test')).toBeInTheDocument();
    expect(screen.getAllByRole('progressbar')).toHaveLength(1);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '70');
    fireEvent.change(screen.getByLabelText('Antigravity 调用模型'), { target: { value: 'antigravity:claude' } });
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '0');
    fireEvent.click(screen.getByRole('button', { name: '使用模型' }));
    await waitFor(() => expect(useChatStore.getState().selectedModelId).toBe('antigravity:claude'));
    expect(useSessionStore.getState().draftAgentEngine).toBe('local');
  });
  it('loads real state in StrictMode despite the discarded first mount request', async () => {
    const connected: ChannelStatus = { ...channelDefaults.kimi, installed: true, configured: true,
      quota: { state: 'available', windows: [{ id: '5h', label: '5 小时', usedPercent: 25 }] } };
    api.status.mockResolvedValue(connected);
    render(<StrictMode><OfficialChannelCard channel="kimi" /></StrictMode>);
    expect(await screen.findByText('已授权')).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '75');
    expect(api.status).toHaveBeenCalledTimes(2);
  });

  it('opens the click popup before the delayed Codex request and offers the official fallback link', async () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    let resolve!: (value: object) => void;
    api.codexLogin.mockImplementation(() => new Promise((done) => { resolve = done; }));
    render(<OfficialChannelCard channel="codex" />);
    fireEvent.click(await loginButton('codex'));
    await waitFor(() => expect(api.codexLogin).toHaveBeenCalled());
    expect(open.mock.invocationCallOrder[0]).toBeLessThan(api.codexLogin.mock.invocationCallOrder[0]);
    await act(async () => resolve({ loginId: 'official-login', authUrl: 'https://auth.openai.com/oauth/authorize?code=demo' }));
    const fallback = await screen.findByRole('link', { name: /继续官方授权/ });
    expect(fallback).toHaveAttribute('href', 'https://auth.openai.com/oauth/authorize?code=demo');
    expect(fallback).toHaveAttribute('rel', 'noopener noreferrer');
    fireEvent.click(await screen.findByRole('button', { name: '取消 Codex 登录' }));
    await waitFor(() => expect(api.codexCancel).toHaveBeenCalledWith('official-login'));
  });

  it('cancels an unsafe Kimi authorization response and never opens its target', async () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    api.login.mockResolvedValue({ state: 'pending', authUrl: 'https://auth.kimi.com.evil.test/device' });
    render(<OfficialChannelCard channel="kimi" />);
    fireEvent.click(await loginButton('kimi'));
    expect(await screen.findByRole('alert')).toHaveTextContent('官方授权地址校验失败');
    expect(api.cancel).toHaveBeenCalledWith('kimi');
    expect(open).toHaveBeenCalledOnce();
    expect(open).toHaveBeenCalledWith('', expect.stringMatching(/^forge-kimi-login-/), expect.any(String));
    expect(screen.queryByRole('link', { name: /继续官方授权/ })).not.toBeInTheDocument();
  });

  it('never displays a fabricated quota and binds a GLM key without retaining it in the form', async () => {
    vi.spyOn(window, 'open').mockReturnValue(null);
    api.login.mockResolvedValue({ state: 'key_required', authUrl: channelDefaults.glm.quotaUrl });
    api.bind.mockResolvedValue({ ...channelDefaults.glm, configured: true });
    render(<OfficialChannelCard channel="glm" />);
    fireEvent.click(await loginButton('glm'));
    const key = await screen.findByLabelText('Coding Plan Key');
    expect(key).toHaveAttribute('type', 'password');
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    fireEvent.change(key, { target: { value: ' test-subscription-secret ' } });
    fireEvent.change(screen.getByLabelText('模型 ID'), { target: { value: 'glm-5.3' } });
    api.status.mockResolvedValue({ ...channelDefaults.glm, configured: true });
    fireEvent.click(screen.getByRole('button', { name: '绑定订阅' }));
    await waitFor(() => expect(api.bind).toHaveBeenCalledWith('test-subscription-secret', 'glm-5.3'));
    await waitFor(() => expect(screen.queryByLabelText('Coding Plan Key')).not.toBeInTheDocument());
    expect(screen.queryByText('test-subscription-secret')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '使用模型' }));
    await waitFor(() => expect(useChatStore.getState().selectedModelId).toBe('glm-coding'));
    expect(useSessionStore.getState().draftAgentEngine).toBe('local');
  });

  it('reports a rejected engine switch instead of claiming the model is in use', async () => {
    api.codexStatus.mockResolvedValue({ ...channelDefaults.codex, installed: true, configured: true, modelId: 'codex:gpt-test' });
    useSessionStore.setState({ activeSessionId: 'session', sessions: [{ id: 'session', agentEngine: 'local' } as never], setAgentEngine: vi.fn().mockResolvedValue(undefined) });
    render(<OfficialChannelCard channel="codex" />);
    fireEvent.click(await screen.findByRole('button', { name: '使用模型' }));
    await waitFor(() => expect(within(screen.getByTestId('channel-card-codex')).getByRole('alert')).toHaveTextContent('切换 Agent 引擎失败'));
    expect(useToastStore.getState().push).not.toHaveBeenCalledWith('success', expect.anything());
    expect(useChatStore.getState().selectedModelId).toBeNull();
  });
});
