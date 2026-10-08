import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useUltraPlanStore, type UltraPlanState } from '@/lib/ultraPlanStore';
import { modesForKind } from '../composerModes';

/**
 * D-044:UltraPlan 卡片 / 状态条 / Composer 共用的「当前会话的流程上下文」。
 *
 * - flow:ultraPlanStore 里的阶段机,**且**它属于当前选中的会话;store 里挂着别的会话的
 *   state(切会话的瞬间)一律当作没有流程,免得在 B 会话里对 A 的流程发动作。
 * - unsupportedReason:UltraPlan 对本地 / Codex 的 coding 代理开放(与 modesForKind 同一判据)。
 *   流程中途把代理切到通用或文档时,卡片与状态条只读并给出原因。
 */
export interface FlowContext {
  activeSessionId: string | null;
  flow: UltraPlanState | null;
  activeRunId: string | null;
  unsupportedReason: string | null;
}

export function useFlowContext(): FlowContext {
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const agentKind = useSessionStore(
    (st) => st.sessions.find((x) => x.id === st.activeSessionId)?.agentKind ?? 'coding',
  );
  const agentEngine = useSessionStore(
    (st) => st.sessions.find((x) => x.id === st.activeSessionId)?.agentEngine ?? st.draftAgentEngine,
  );
  const state = useUltraPlanStore((st) => st.state);
  const flowSessionId = useUltraPlanStore((st) => st.sessionId);
  const activeRunId = useChatStore((st) => st.activeRunId);

  const flow = state !== null && flowSessionId === activeSessionId ? state : null;
  const supported = modesForKind(agentKind, agentEngine).some((m) => m.id === 'ultraplan');
  const unsupportedReason = supported
    ? null
    : '当前代理类型不支持 UltraPlan,切回「编码」代理后可继续';

  return { activeSessionId, flow, activeRunId, unsupportedReason };
}
