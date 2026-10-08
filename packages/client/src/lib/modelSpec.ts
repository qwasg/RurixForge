import { useChatStore, type ModelContextOption, type ModelEffortOption, type SnapshotModel } from './chatStore';

/**
 * 模型规格三档(Thinking / Effort / Context)的客户端归一。
 *
 * 能力面由后端下发(agentd modelspec.rs CATALOG 随 design-snapshot models[] 一起来),
 * 会话侧只存「选择」。本模块把「选择 + 能力面」归一成「当前实际生效的规格」,规则与后端
 * modelspec::resolve 一一对应,避免两侧显示与实发不一致:
 * - 模型不在清单里 → 全档回落(菜单三行禁用);
 * - Thinking 开关只在 supportsThinking 时生效;
 * - Effort 仅在「思考开 + 该渠道收 reasoning_effort」时生效,选择越界回落 defaultEffort;
 * - Context 选择越界回落 defaultContext,再无则回落 DEFAULT_CONTEXT_WINDOW。
 *
 * 唯一的两侧差异是刻意的:Context 档不进请求体,它只作计量环的分母(见 contextUsage.ts)。
 */

/** 后端未下发任何 contextOptions 时的回落窗口(与 agentd DEFAULT_CONTEXT_TOKENS 同值)。 */
export const DEFAULT_CONTEXT_WINDOW = 65536;

export interface EffectiveSpec {
  model: SnapshotModel | null;
  /** chip 主标题(模型 label,无模型时如实回落 id / "default")。 */
  modelLabel: string;
  thinking: boolean;
  thinkingSupported: boolean;
  thinkingAlwaysOn: boolean;
  effort: ModelEffortOption | null;
  effortOptions: ModelEffortOption[];
  effortSupported: boolean;
  context: ModelContextOption | null;
  contextOptions: ModelContextOption[];
  /** 计量环分母。 */
  contextTokens: number;
  /** chip 上模型名之后的淡色规格后缀(如 "1M Max");无可显示档位时为空串。 */
  suffix: string;
}

export function resolveModelSpec(
  models: SnapshotModel[],
  modelId: string | null,
  thinkingEnabled: boolean,
  effortId: string | null,
  contextId: string | null,
): EffectiveSpec {
  const model = models.find((m) => m.id === modelId) ?? null;
  const effortOptions = model?.effortOptions ?? [];
  const contextOptions = model?.contextOptions ?? [];
  const thinkingSupported = model?.supportsThinking === true;
  const thinkingAlwaysOn = thinkingSupported && model?.thinkingAlwaysOn === true;
  const thinking = (thinkingEnabled || thinkingAlwaysOn) && thinkingSupported;
  const effortSupported = thinkingSupported && effortOptions.length > 0;

  const effort = effortSupported
    ? (effortOptions.find((o) => o.id === effortId) ??
      effortOptions.find((o) => o.id === model?.defaultEffort) ??
      null)
    : null;
  const context =
    contextOptions.find((o) => o.id === contextId) ??
    contextOptions.find((o) => o.id === model?.defaultContext) ??
    null;

  // 后缀照 Cursor 口径:窗口档常显,强度档只在思考开着时才有意义。
  const parts = [context?.label, thinking ? effort?.label : undefined].filter(
    (s): s is string => typeof s === 'string' && s !== '',
  );

  return {
    model,
    modelLabel: model?.label || modelId || 'default',
    thinking,
    thinkingSupported,
    thinkingAlwaysOn,
    effort,
    effortOptions,
    effortSupported,
    context,
    contextOptions,
    contextTokens: context?.tokens ?? DEFAULT_CONTEXT_WINDOW,
    suffix: parts.join(' '),
  };
}

/** 订阅式:当前会话生效中的规格(Composer chip、模型菜单、上下文计量环共用)。 */
export function useEffectiveSpec(): EffectiveSpec {
  const models = useChatStore((st) => st.models);
  const selectedModelId = useChatStore((st) => st.selectedModelId);
  const defaultModelId = useChatStore((st) => st.defaultModelId);
  const thinkingEnabled = useChatStore((st) => st.thinkingEnabled);
  const reasoningEffort = useChatStore((st) => st.reasoningEffort);
  const contextOptionId = useChatStore((st) => st.contextOptionId);
  return resolveModelSpec(
    models,
    selectedModelId ?? defaultModelId,
    thinkingEnabled,
    reasoningEffort,
    contextOptionId,
  );
}
