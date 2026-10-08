import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentMessage, AgentParticipant, TeamState } from '@forge/protocol';
import { useCollaborationStore } from '@/lib/collaborationStore';
import { useChatStore, type ForgeEventWire } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { usePlanStore } from '@/lib/planStore';
import { useUltraPlanStore } from '@/lib/ultraPlanStore';
import { useSettingsStore } from '@/lib/settingsStore';
import Composer from '@/components/chat/Composer';
import AgentMailbox from '@/components/chat/AgentMailbox';
import SubagentOverlay from '@/components/chat/SubagentOverlay';
import TeamBoard from '@/components/chat/TeamBoard';
import MessageList from '@/components/chat/MessageList';
import SubagentRow from '@/components/chat/SubagentRow';
import { changeTeamState } from '@/lib/forgeApi';

const now = '2026-10-03T12:00:00Z';
const root: AgentParticipant = { id: 'root', sessionId: 's', name: '主 agent', role: 'root', status: 'running', engine: 'local', activeRunId: 'run-root' };
const member: AgentParticipant = { id: 'member', sessionId: 's', name: '接口工程师', role: 'member', status: 'idle', engine: 'local', parentAgentId: 'root' };
const team: TeamState = {
  id: 'team-1', sessionId: 's', name: '实现协作', status: 'active', leaderAgentId: 'root', memberAgentIds: ['member'],
  revision: 1, maxParallel: 2, maxFixRounds: 3, fixRounds: 0, createdAt: now, updatedAt: now,
  tasks: [
    { id: 'api', teamId: 'team-1', title: '定义接口', prompt: '实现接口', deps: [], status: 'completed', ownerAgentId: 'member', attempts: 1, createdAt: now, updatedAt: now },
    { id: 'ui', teamId: 'team-1', title: '实现界面', prompt: '实现界面', deps: ['api'], status: 'blocked', attempts: 0, result: '等待设计资源', createdAt: now, updatedAt: now },
  ],
};
const message = (patch: Partial<AgentMessage> = {}): AgentMessage => ({
  id: 'm1', sessionId: 's', fromAgentId: null, toAgentId: 'root', source: 'user', text: '先确认接口',
  clientMessageId: 'client-1', status: 'queued', createdAt: now, ...patch,
});
let seq = 0;
const event = (type: string, payload: object): ForgeEventWire => ({ id: `e${++seq}`, sessionId: 's', seq, type, ts: now, payload: { ...payload } });
const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialCollaboration = useCollaborationStore.getState();
const initialUltra = useUltraPlanStore.getState();
const initialSettings = useSettingsStore.getState();
const response = (body: unknown, status = 200) => ({ ok: status >= 200 && status < 300, status, json: async () => body }) as Response;

beforeEach(() => {
  seq = 0;
  useChatStore.setState(initialChat, true);
  useCollaborationStore.setState(initialCollaboration, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useSessionStore.setState({ activeSessionId: 's' });
  useUltraPlanStore.setState(initialUltra, true);
  useSettingsStore.setState(initialSettings, true);
  useChatStore.setState({ currentSessionId: 's', activeRunId: 'run-root' });
  useCollaborationStore.getState().reset('s');
  useCollaborationStore.setState({ agents: [root, member], supported: true });
  vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
    const path = String(url);
    if (path.endsWith('/agents')) return response({ agents: [root, member] });
    if (path.endsWith('/team')) return response({ team });
    if (path.endsWith('/messages')) return response({ messages: [] });
    return response({});
  }));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe('durable collaboration state', () => {
  it('ignores inherited fork payloads that still belong to the original session', () => {
    const store = useCollaborationStore.getState();
    store.reset('fork');
    for (const evt of [event('agent.participant.updated', root), event('agent.message.queued', message()), event('team.updated', { team })]) {
      store.applyEvent({ ...evt, sessionId: 'fork' });
    }
    expect(useCollaborationStore.getState()).toMatchObject({ agents: [], messages: [], team: null });
  });

  it('accepts a newly created team with a fresh revision sequence', () => {
    useCollaborationStore.setState({ team: { ...team, revision: 90 } });
    useCollaborationStore.getState().applyEvent(event('team.updated', { team: { ...team, id: 'new-team', revision: 1 } }));
    expect(useCollaborationStore.getState().team?.id).toBe('new-team');
  });

  it('deduplicates mailbox replay and does not downgrade delivered SSE after an older HTTP acknowledgement', async () => {
    let accept!: (value: Response) => void;
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>((resolve) => { accept = resolve; })));
    const pending = useCollaborationStore.getState().send('root', { text: '先确认接口', clientMessageId: 'client-1' });
    useCollaborationStore.getState().applyEvent(event('agent.message.injected', message({ status: 'injected', runId: 'run-root', injectedAt: now })));
    accept(response({ message: message() }));
    await pending;
    useCollaborationStore.getState().applyEvent(event('agent.message.queued', message()));
    expect(useCollaborationStore.getState().messages).toHaveLength(1);
    expect(useCollaborationStore.getState().messages[0].status).toBe('injected');
  });

  it('does not let old session reads or sends overwrite the selected session', async () => {
    let release!: (value: Response) => void;
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>((resolve) => { release = resolve; })));
    const pending = useCollaborationStore.getState().loadMessages('root');
    useCollaborationStore.getState().reset('other');
    release(response({ messages: [message()] }));
    await pending;
    expect(useCollaborationStore.getState().sessionId).toBe('other');
    expect(useCollaborationStore.getState().messages).toEqual([]);
    useCollaborationStore.getState().applyEvent(event('agent.participant.updated', root));
    expect(useCollaborationStore.getState().agents).toEqual([]);
  });

  it('keeps newer participant and team SSE values when a snapshot request resolves late', async () => {
    let agents!: (value: Response) => void;
    let currentTeam!: (value: Response) => void;
    vi.stubGlobal('fetch', vi.fn((url: unknown) => new Promise<Response>((resolve) => {
      if (String(url).endsWith('/agents')) agents = resolve;
      else currentTeam = resolve;
    })));
    const pending = useCollaborationStore.getState().refresh('s');
    useCollaborationStore.getState().applyEvent(event('agent.participant.updated', { ...member, status: 'running', activeRunId: 'run-member' }));
    useCollaborationStore.getState().applyEvent(event('team.updated', { team: { ...team, status: 'paused', revision: 2 } }));
    agents(response({ agents: [root, member] }));
    currentTeam(response({ team }));
    await pending;
    expect(useCollaborationStore.getState().agents.find((agent) => agent.id === 'member')?.status).toBe('running');
    expect(useCollaborationStore.getState().team?.status).toBe('paused');
  });

  it('keeps separate same-run steering messages after the initial assistant card', () => {
    const chat = useChatStore.getState();
    chat.applyEvent(event('composer.user.message', { runId: 'run-root', text: '开始' }));
    chat.applyEvent(event('agent.started', { runId: 'run-root', agentId: 'root' }));
    chat.applyEvent(event('agent.message.queued', message({ runId: 'run-root' })));
    chat.applyEvent(event('agent.message.queued', message({ id: 'm2', clientMessageId: 'client-2', runId: 'run-root' })));
    chat.applyEvent(event('agent.message.injected', message({ status: 'injected', runId: 'run-root' })));
    const history = useChatStore.getState().messages;
    expect(history.map((item) => item.role)).toEqual(['user', 'assistant', 'user', 'user']);
    expect(history.slice(2).map((item) => item.messageId)).toEqual(['m1', 'm2']);
    expect(history[2].messageStatus).toBe('injected');
  });

  it('child started, usage and completion preserve root context and planning state', () => {
    usePlanStore.getState().setPlanning(true);
    useChatStore.setState({ lastPromptTokens: 1200 });
    const chat = useChatStore.getState();
    chat.applyEvent(event('agent.started', { runId: 'run-member', agentId: 'member', agentRole: 'member' }));
    chat.applyEvent(event('agent.usage', { runId: 'run-member', agentId: 'member', promptTokens: 25, totalTokens: 30 }));
    chat.applyEvent(event('agent.completed', { runId: 'run-member', agentId: 'member', text: '子任务已完成' }));
    expect(useChatStore.getState().activeRunId).toBe('run-root');
    expect(useChatStore.getState().lastPromptTokens).toBe(1200);
    expect(usePlanStore.getState().planning).toBe(true);
    render(<MessageList />);
    expect(screen.queryByText('子任务已完成')).not.toBeInTheDocument();
  });

  it('routes stable child identity into its legacy parent card without settling the root run', () => {
    const chat = useChatStore.getState();
    chat.applyEvent(event('agent.started', { runId: 'run-root', agentId: 'root' }));
    chat.applyEvent(event('subagent.started', { parentRunId: 'run-root', subRunId: 'child-card', agentId: 'member', agentRole: 'member', agentRunId: 'actual-child-run', description: '接口工程师' }));
    chat.applyEvent(event('agent.reasoning', { runId: 'run-root', agentId: 'member', parentAgentId: 'root', agentRunId: 'actual-child-run', text: '只在子任务中显示' }));
    chat.applyEvent(event('permission.requested', { runId: 'run-root', agentId: 'member', agentRunId: 'actual-child-run', id: 'child-approval', tool: 'shell' }));
    expect(useChatStore.getState().pendingPermissions[0].runId).toBe('actual-child-run');
    const rootMessage = useChatStore.getState().messages[0];
    expect(rootMessage.blocks).toHaveLength(1);
    expect(rootMessage.blocks[0]).toMatchObject({ kind: 'subagent', work: [{ kind: 'reasoning', text: '只在子任务中显示' }, { kind: 'approval', id: 'child-approval' }] });
    chat.applyEvent(event('agent.completed', { runId: 'run-root', agentId: 'member', agentRunId: 'actual-child-run', text: '接口完成' }));
    expect(useChatStore.getState().activeRunId).toBe('run-root');
    expect(useChatStore.getState().messages[0].status).toBe('streaming');
    expect(useChatStore.getState().pendingPermissions).toHaveLength(0);
  });

  it('keeps grandchildren nested when their own events arrive', () => {
    const chat = useChatStore.getState();
    chat.applyEvent(event('agent.started', { runId: 'run-root', agentId: 'root' }));
    chat.applyEvent(event('subagent.started', { parentRunId: 'run-root', subRunId: 'child', agentId: 'member' }));
    chat.applyEvent(event('agent.tool.invoked', { runId: 'run-root', parentToolCallId: 'child', toolCallId: 'grandchild', name: 'task', args: { prompt: '继续分析' } }));
    chat.applyEvent(event('subagent.started', { parentRunId: 'run-root', subRunId: 'grandchild', agentId: 'grandchild-agent' }));
    chat.applyEvent(event('agent.reasoning', { runId: 'run-root', parentToolCallId: 'grandchild', text: '深入分析' }));
    const blocks = useChatStore.getState().messages[0].blocks;
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ kind: 'subagent', id: 'child', work: [{ kind: 'subagent', id: 'grandchild', work: [{ kind: 'reasoning', text: '深入分析' }] }] });
  });

  it.each(['agent.completed', 'agent.failed', 'agent.cancelled'])('isolates %s when participant metadata arrives before its legacy child card', (type) => {
    const chat = useChatStore.getState();
    chat.applyEvent(event('agent.started', { runId: 'run-root', agentId: 'root' }));
    const attribution = { runId: 'run-root', agentId: 'member', parentAgentId: 'root', agentRunId: 'actual-member-run' };
    chat.applyEvent(event('agent.tool.invoked', { ...attribution, name: 'read_file', toolCallId: 'read-child', args: { path: 'src/a.ts' } }));
    chat.applyEvent(event(type, { ...attribution, text: '成员终态', error: '子任务失败' }));
    expect(useChatStore.getState().activeRunId).toBe('run-root');
    const rootMessage = useChatStore.getState().messages[0];
    expect(rootMessage.status).toBe('streaming');
    expect(rootMessage.blocks).toHaveLength(1);
    expect(rootMessage.blocks[0]).toMatchObject({ kind: 'subagent', agentId: 'member', agentRunId: 'actual-member-run', work: [expect.objectContaining({ kind: 'tool', toolCallId: 'read-child' }), ...(type === 'agent.completed' ? [expect.objectContaining({ kind: 'text', text: '成员终态' })] : [])] });
    chat.applyEvent(event('subagent.started', { ...attribution, subRunId: 'legacy-member-card', description: '接口工程师' }));
    expect(useChatStore.getState().messages[0].blocks).toHaveLength(1);
    expect(useChatStore.getState().messages[0].blocks[0]).toMatchObject({ id: 'legacy-member-card', status: type === 'agent.completed' ? 'done' : 'error' });
  });

  it('queues approvals across members and resolves only the matching request', () => {
    const chat = useChatStore.getState();
    chat.applyEvent(event('permission.requested', { id: 'p1', agentId: 'member', agentName: '接口工程师', runId: 'child-1', tool: 'shell' }));
    chat.applyEvent(event('permission.requested', { id: 'p2', agentId: 'other-member', runId: 'child-2', tool: 'write_file' }));
    expect(useChatStore.getState().pendingPermissions.map((item) => item.id)).toEqual(['p1', 'p2']);
    expect(useChatStore.getState().pendingPermission?.id).toBe('p1');
    chat.applyEvent(event('permission.resolved', { id: 'p1', runId: 'child-1', allowed: true }));
    expect(useChatStore.getState().pendingPermission?.id).toBe('p2');
    expect(useChatStore.getState().pendingPermissions).toHaveLength(1);
  });
});

describe('collaboration controls', () => {
  it('uses the session-scoped Team endpoint and encoded identifiers', async () => {
    const fetcher = vi.fn(async () => response({ team }));
    vi.stubGlobal('fetch', fetcher);
    await changeTeamState('session one', 'team/two', 'pause');
    expect(fetcher).toHaveBeenCalledWith('/api/forge/sessions/session%20one/teams/team%2Ftwo', expect.objectContaining({ method: 'PATCH', body: '{"action":"pause"}' }));
  });

  it('shows accepted root steering from its HTTP acknowledgement even before SSE arrives', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => response({ message: message() })));
    expect(await useChatStore.getState().steerAgent('先确认接口', undefined, 'client-1')).toBe(true);
    expect(useChatStore.getState().messages).toHaveLength(1);
    useChatStore.getState().applyEvent(event('agent.message.injected', message({ status: 'injected' })));
    expect(useChatStore.getState().messages).toHaveLength(1);
    expect(useChatStore.getState().messages[0]).toMatchObject({ messageId: 'm1', messageStatus: 'injected' });
  });

  it('uses the independently cancellable member run while keeping legacy cancellation intact', () => {
    const cancelSubagent = vi.fn(async () => undefined);
    const cancelRun = vi.fn(async () => undefined);
    useChatStore.setState({ cancelSubagent, cancelRun });
    const { rerender } = render(<SubagentRow block={{ kind: 'subagent', id: 'tool', agentId: 'member', agentRunId: 'real-child-run', label: '工作', status: 'running', work: [] }} />);
    fireEvent.click(screen.getByTestId('subagent-stop'));
    expect(cancelSubagent).toHaveBeenCalledWith('real-child-run');
    expect(cancelRun).not.toHaveBeenCalled();
    rerender(<SubagentRow block={{ kind: 'subagent', id: 'legacy', label: '旧任务', status: 'running', work: [] }} />);
    fireEvent.click(screen.getByTestId('subagent-stop'));
    expect(cancelRun).toHaveBeenCalledOnce();
  });

  it('allows live user steering beside Stop, preserves failed draft and reuses its idempotency key', async () => {
    const steer = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    useChatStore.setState({ steerAgent: steer });
    render(<Composer />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '调整接口' } });
    expect(screen.getByTestId('composer-abort')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-send'));
    await waitFor(() => expect(steer).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId('composer-input')).toHaveValue('调整接口');
    await waitFor(() => expect(screen.getByTestId('composer-send')).toBeEnabled());
    fireEvent.click(screen.getByTestId('composer-send'));
    await waitFor(() => expect(screen.getByTestId('composer-input')).toHaveValue(''));
    expect(steer.mock.calls[0][2]).toBe(steer.mock.calls[1][2]);
  });

  it('renders idle persistent member without a legacy card and posts directly to its mailbox', async () => {
    const fetcher = vi.fn(async (_url: unknown, init?: RequestInit) => init?.method === 'POST' ? response({ message: message({ toAgentId: 'member' }) }) : response({ messages: [] }));
    vi.stubGlobal('fetch', fetcher);
    useChatStore.getState().openSubagent('member');
    render(<SubagentOverlay />);
    expect(screen.getByTestId('subagent-overlay-status')).toHaveTextContent('空闲');
    fireEvent.change(screen.getByTestId('agent-message-input'), { target: { value: '请检查接口' } });
    fireEvent.click(screen.getByTestId('agent-message-send'));
    await waitFor(() => expect(screen.getByTestId('agent-message-input')).toHaveValue(''));
    const call = fetcher.mock.calls.find(([, init]) => init?.method === 'POST');
    expect(call?.[0]).toBe('/api/forge/sessions/s/agents/member/messages');
    expect(JSON.parse(String(call?.[1]?.body))).toMatchObject({ text: '请检查接口', clientMessageId: expect.any(String) });
    expect(JSON.parse(String(call?.[1]?.body))).not.toHaveProperty('fromAgentId');
  });

  it('keeps a failed child draft and shows recovery-required delivery independently', async () => {
    vi.stubGlobal('fetch', vi.fn(async (_url: unknown, init?: RequestInit) => init?.method === 'POST'
      ? response({ error: { code: 'AGENT_RUN_CHANGED', message: '运行已变化，请重试' } }, 409)
      : response({ messages: [message({ toAgentId: 'member', status: 'recoveryRequired' })] })));
    render(<AgentMailbox agent={member} />);
    await screen.findByText('待恢复确认');
    fireEvent.change(screen.getByTestId('agent-message-input'), { target: { value: '保持草稿' } });
    fireEvent.click(screen.getByTestId('agent-message-send'));
    await screen.findByRole('alert');
    expect(screen.getByTestId('agent-message-input')).toHaveValue('保持草稿');
  });

  it('shows ownership, dependencies and blocked reason, then pauses the actual team', async () => {
    useCollaborationStore.setState({ team });
    const control = vi.fn(async () => undefined);
    useCollaborationStore.setState({ control });
    render(<TeamBoard />);
    fireEvent.click(screen.getByTestId('team-toggle'));
    expect(screen.getByText('依赖：定义接口')).toBeInTheDocument();
    expect(screen.getByText('负责人：接口工程师')).toBeInTheDocument();
    expect(screen.getByText('等待设计资源')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('team-pause-resume'));
    await waitFor(() => expect(control).toHaveBeenCalledWith('pause'));
    act(() => useCollaborationStore.setState({ team: { ...team, status: 'paused', revision: 2 } }));
    await waitFor(() => expect(screen.getByTestId('team-pause-resume')).toBeEnabled());
    fireEvent.click(screen.getByTestId('team-pause-resume'));
    await waitFor(() => expect(control).toHaveBeenCalledWith('resume'));
  });

  it('keeps recovery-required team state visible and offers explicit resume and stop', () => {
    useCollaborationStore.setState({ team: { ...team, status: 'recoveryRequired' } });
    render(<TeamBoard />);
    expect(screen.getByTestId('team-toggle')).toHaveTextContent('需要恢复');
    expect(screen.getByTestId('team-pause-resume')).toHaveTextContent('恢复');
    expect(screen.getByTestId('team-stop')).toBeEnabled();
  });
});
