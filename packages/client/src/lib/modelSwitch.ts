import { useChatStore } from './chatStore';
import { useSessionStore } from './sessionStore';

/**
 * 设置·模型页的「使用」按钮:把当前会话(没有会话则为新会话草稿)切到本地引擎并选中该模型。
 * 云端模型(`cloud:<id>`)与自定义模型(`openai-compat` / `custom:<hex>`)都跑在本地引擎上;
 * 流程与官方订阅卡的「使用模型」一致,任何一步没生效都如实抛错,不假装切换成功。
 */
export async function switchToLocalModel(modelId: string): Promise<void> {
  const sessions = useSessionStore.getState();
  if (sessions.activeSessionId) await sessions.setAgentEngine(sessions.activeSessionId, 'local');
  else sessions.setDraftAgentEngine('local');
  const updated = useSessionStore.getState();
  const engine = updated.activeSessionId
    ? updated.sessions.find((s) => s.id === updated.activeSessionId)?.agentEngine
    : updated.draftAgentEngine;
  if (engine !== 'local') throw new Error('切换到本地引擎失败,请重试');
  await useChatStore.getState().ensureModels(true);
  await useChatStore.getState().pickModel(modelId);
  if (useChatStore.getState().selectedModelId !== modelId) throw new Error('切换调用模型失败,请重试');
}

/** 当前会话(或草稿)在本地引擎上选中的模型 id;Codex 引擎或未选模型时为 null。 */
export function useCurrentLocalModelId(): string | null {
  const selected = useChatStore((s) => s.selectedModelId);
  const sessionId = useSessionStore((s) => s.activeSessionId);
  const draftEngine = useSessionStore((s) => s.draftAgentEngine);
  const activeEngine = useSessionStore(
    (s) => s.sessions.find((session) => session.id === s.activeSessionId)?.agentEngine,
  );
  const engine = sessionId ? activeEngine : draftEngine;
  return engine === 'local' ? selected : null;
}
