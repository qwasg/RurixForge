import type { EditorAnnotation } from './editor.js';
/** Stable agent identities and durable mailboxes shared by Forge and Codex. */
export interface AgentParticipant {
  id: string;
  sessionId: string;
  parentAgentId?: string | null;
  teamId?: string | null;
  name: string;
  role: 'root' | 'member' | 'subagent';
  status: 'idle' | 'running' | 'stopped' | 'recoveryRequired';
  engine: 'local' | 'codex';
  activeRunId?: string | null;
}

export type AgentMessageStatus = 'queued' | 'leased' | 'injected' | 'recoveryRequired' | 'failed';

export interface AgentMessage {
  annotations?: EditorAnnotation[];
  id: string;
  sessionId: string;
  fromAgentId?: string | null;
  toAgentId: string;
  source: 'user' | 'agent';
  kind?: 'message' | 'receipt';
  wake?: boolean;
  text: string;
  clientMessageId?: string | null;
  status: AgentMessageStatus;
  createdAt: string;
  injectedAt?: string | null;
  runId?: string | null;
  error?: string | null;
}

export interface SendAgentMessage {
  annotations?: EditorAnnotation[];
  text: string;
  clientMessageId: string;
  expectedRunId?: string;
}

export interface TeamTask {
  id: string;
  teamId: string;
  title: string;
  prompt: string;
  role?: string | null;
  stage?: string | null;
  deps: string[];
  ownerAgentId?: string | null;
  status: 'queued' | 'running' | 'completed' | 'failed' | 'blocked';
  result?: string | null;
  attempts: number;
  createdAt: string;
  updatedAt: string;
}

export interface TeamState {
  id: string;
  sessionId: string;
  name: string;
  status: 'active' | 'paused' | 'stopped' | 'completed' | 'recoveryRequired' | 'blocked';
  leaderAgentId: string;
  memberAgentIds: string[];
  revision: number;
  maxParallel: number;
  maxFixRounds: number;
  fixRounds: number;
  tasks: TeamTask[];
  createdAt: string;
  updatedAt: string;
}

export type TeamAction = 'pause' | 'resume' | 'stop' | 'complete';
