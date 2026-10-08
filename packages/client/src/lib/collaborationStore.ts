import { create } from 'zustand';
import type { AgentMessage, AgentParticipant, SendAgentMessage, TeamAction, TeamState } from '@forge/protocol';
import { changeTeamState, getAgentMessages, getAgentParticipants, getSessionTeam, sendAgentMessage } from './forgeApi';

export const MESSAGE_STATUS_LABEL: Record<AgentMessage['status'], string> = {
  queued: '已排队', leased: '正在投递', injected: '已送达', recoveryRequired: '待恢复确认', failed: '投递失败',
};

interface CollaborationEvent {
  sessionId: string;
  type: string;
  seq: number;
  payload?: Record<string, unknown>;
}

interface CollaborationState {
  sessionId: string | null;
  agents: AgentParticipant[];
  messages: AgentMessage[];
  team: TeamState | null;
  loading: boolean;
  supported: boolean;
  error: string | null;
  refresh: (sessionId: string) => Promise<void>;
  loadMessages: (agentId: string) => Promise<void>;
  send: (agentId: string, message: SendAgentMessage) => Promise<AgentMessage>;
  control: (action: TeamAction) => Promise<void>;
  applyEvent: (event: CollaborationEvent) => void;
  reset: (sessionId?: string | null) => void;
}

function isAgent(value: unknown): value is AgentParticipant {
  const item = value as Partial<AgentParticipant> | null;
  return !!item && typeof item.id === 'string' && typeof item.sessionId === 'string' && typeof item.name === 'string' && typeof item.role === 'string';
}
function isMessage(value: unknown): value is AgentMessage {
  const item = value as Partial<AgentMessage> | null;
  return !!item && typeof item.id === 'string' && typeof item.sessionId === 'string' && typeof item.toAgentId === 'string' && typeof item.text === 'string' && typeof item.createdAt === 'string' && typeof item.status === 'string' && item.status in MESSAGE_STATUS_LABEL;
}
function isTeam(value: unknown): value is TeamState {
  const item = value as Partial<TeamState> | null;
  return !!item && typeof item.id === 'string' && typeof item.revision === 'number' && Array.isArray(item.tasks);
}

/** Immutable upsert; an older HTTP acknowledgement must not roll back a delivered SSE message. */
export function mergeAgentMessages(current: AgentMessage[], incoming: AgentMessage[]): AgentMessage[] {
  // A definitely rejected lease can return to queued; only durable terminal states dominate.
  const rank: Record<AgentMessage['status'], number> = { queued: 0, leased: 0, recoveryRequired: 2, injected: 3, failed: 3 };
  const result = new Map(current.map((message) => [message.id, message]));
  for (const message of incoming) {
    const prior = result.get(message.id);
    if (!prior || rank[message.status] >= rank[prior.status]) result.set(message.id, message);
  }
  return [...result.values()].sort((a, b) => a.createdAt.localeCompare(b.createdAt));
}

let generation = 0;
let eventVersion = 0;
let pendingRefresh: { sessionId: string; promise: Promise<void> } | null = null;

export const useCollaborationStore = create<CollaborationState>((set, get) => ({
  sessionId: null, agents: [], messages: [], team: null, loading: false, supported: false, error: null,
  reset: (sessionId = null) => {
    generation += 1;
    eventVersion = 0;
    pendingRefresh = null;
    set({ sessionId, agents: [], messages: [], team: null, loading: false, supported: false, error: null });
  },
  refresh: (sessionId) => {
    if (get().sessionId !== sessionId) get().reset(sessionId);
    if (pendingRefresh?.sessionId === sessionId) return pendingRefresh.promise;
    const token = generation;
    const version = eventVersion;
    set({ loading: true });
    let promise: Promise<void>;
    promise = (async () => {
      try {
        const [participants, team] = await Promise.all([getAgentParticipants(sessionId), getSessionTeam(sessionId)]);
        if (token !== generation || get().sessionId !== sessionId) return;
        const validAgents = Array.isArray(participants.agents) ? participants.agents.filter((agent) => isAgent(agent) && agent.sessionId === sessionId) : [];
        const current = get();
        // If SSE raced this read, live values win; retain older entries only to fill missing identities.
        const agents = new Map(validAgents.map((agent) => [agent.id, agent]));
        if (eventVersion !== version) for (const agent of current.agents) agents.set(agent.id, agent);
        const nextTeam = isTeam(team.team) && team.team.sessionId === sessionId ? team.team : null;
        const newerCurrent = current.team && nextTeam && current.team.id === nextTeam.id && current.team.revision > nextTeam.revision;
        set({ agents: [...agents.values()], team: newerCurrent || (eventVersion !== version && current.team) ? current.team : nextTeam, supported: Array.isArray(participants.agents), error: null });
      } catch (error) {
        if (token === generation) set({ error: error instanceof Error ? error.message : '无法读取协作状态' });
      } finally {
        if (token === generation) set({ loading: false });
        if (pendingRefresh?.sessionId === sessionId && token === generation) pendingRefresh = null;
      }
    })();
    pendingRefresh = { sessionId, promise };
    return promise;
  },
  loadMessages: async (agentId) => {
    const sessionId = get().sessionId;
    if (!sessionId) return;
    const token = generation;
    const result = await getAgentMessages(sessionId, agentId);
    if (generation !== token) return;
    set((state) => ({ messages: mergeAgentMessages(state.messages, (result.messages ?? []).filter((message) => isMessage(message) && message.sessionId === sessionId)) }));
  },
  send: async (agentId, input) => {
    const sessionId = get().sessionId;
    if (!sessionId) throw new Error('请先选择会话');
    const token = generation;
    const { message } = await sendAgentMessage(sessionId, agentId, input);
    if (!isMessage(message) || message.sessionId !== sessionId || message.toAgentId !== agentId) throw new Error('消息响应格式错误');
    if (generation === token) set((state) => ({ messages: mergeAgentMessages(state.messages, [message]) }));
    return message;
  },
  control: async (action) => {
    const state = get();
    if (!state.team) return;
    const token = generation;
    const { team } = await changeTeamState(state.team.sessionId, state.team.id, action);
    if (token === generation && isTeam(team) && team.id === get().team?.id && team.revision >= (get().team?.revision ?? 0)) set({ team });
  },
  applyEvent: (event) => {
    if (event.sessionId !== get().sessionId) return;
    const payload = event.payload;
    if (event.type === 'agent.participant.updated' && isAgent(payload) && payload.sessionId === event.sessionId) {
      eventVersion += 1;
      set((state) => ({ supported: true, agents: [...state.agents.filter((agent) => agent.id !== payload.id), payload] }));
    } else if (event.type.startsWith('agent.message.') && isMessage(payload) && payload.sessionId === event.sessionId) {
      eventVersion += 1;
      set((state) => ({ messages: mergeAgentMessages(state.messages, [payload]) }));
    } else if (event.type === 'team.updated' && isTeam(payload?.team) && payload.team.sessionId === event.sessionId) {
      eventVersion += 1;
      const team = payload.team;
      if (team.id !== get().team?.id || team.revision >= (get().team?.revision ?? 0)) set({ team });
    }
  },
}));
