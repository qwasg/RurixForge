//! 模型规格面(Thinking / Effort / Context 三档)的单一事实源。
//!
//! 三档语义(与 client ModelPicker 的三个子菜单逐项对应):
//! - Thinking:思考总开关。deepseek 官方以「模型名」区分思考模式,故本仓 thinking 开
//!   → 实发 deepseek-reasoner,关 → deepseek-chat;openai-compat 走 chat.completions 的
//!   reasoning_effort 字段(关 = 请求体不含该字段)。mock 不支持,该行禁用。
//! - Effort:reasoning_effort 强度。档位 id 即实发 API 值,对齐 OpenAI chat.completions
//!   现行取值集(none/minimal/low/medium/high/xhigh/max),本仓 UI 暴露 low..max 五档。
//!   deepseek 官方接口不收该字段 → effort_options 为空,UI 该行禁用,请求体也不发。
//! - Context:上下文窗口声明。chat.completions 没有这个参数,它只作用于「客户端计量环的
//!   分母」与本仓落库——补上 client contextUsage.ts 头注留痕的数据缺口(此前是写死的 64K
//!   静态表)。固定窗口模型只给一档(UI 呈现但不可切);openai-compat 是用户自配渠道,
//!   窗口只有人知道 → 给全档由人选。
//!
//! 会话侧只存「选择」(thinkingEnabled / reasoningEffort / contextOptionId),实发参数一律由
//! resolve() 现算:换模型后旧选择不适用时如实回落该模型默认档,不静默沿用上一模型的档位。

use serde_json::{json, Value};

/// 上下文窗口档位(tokens 即计量环分母)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextOption {
    pub id: &'static str,
    pub label: &'static str,
    pub tokens: u64,
}

/// reasoning_effort 档位(id 即实发 API 值,label 为菜单显示名)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffortOption {
    pub id: &'static str,
    pub label: &'static str,
}

/// 一条模型的能力面(availability/label 属实测面,由 snapshot 侧注入,不进本表)。
#[derive(Debug, Clone, Copy)]
pub struct ModelCard {
    pub id: &'static str,
    pub label: &'static str,
    pub provider: &'static str,
    /// 菜单分组标题。
    pub group: &'static str,
    pub supports_thinking: bool,
    /// thinking 开启时实发的模型名(None = 不换名,靠 reasoning_effort 表达)。
    pub thinking_model: Option<&'static str>,
    pub effort_options: &'static [EffortOption],
    pub default_effort: Option<&'static str>,
    pub context_options: &'static [ContextOption],
    /// 默认档 id(恒在 context_options 内,见 default_context_in_options 测试)。
    pub default_context: &'static str,
}

/// OpenAI chat.completions reasoning_effort 现行取值集里本仓暴露的五档。
const EFFORTS_FULL: &[EffortOption] = &[
    EffortOption {
        id: "low",
        label: "Low",
    },
    EffortOption {
        id: "medium",
        label: "Medium",
    },
    EffortOption {
        id: "high",
        label: "High",
    },
    EffortOption {
        id: "xhigh",
        label: "Extra High",
    },
    EffortOption {
        id: "max",
        label: "Max",
    },
];

/// 不收 reasoning_effort 的渠道(deepseek/mock)。
const EFFORTS_NONE: &[EffortOption] = &[];

/// 固定 64K 窗口单档(deepseek 官方口径;mock 本地回声无真实窗口,同档占位)。
const CTX_FIXED_64K: &[ContextOption] = &[ContextOption {
    id: "64k",
    label: "64K",
    tokens: 65_536,
}];

/// 用户自配渠道的可选窗口档(接什么模型只有人知道,如实交给人选)。
const CTX_TIERS: &[ContextOption] = &[
    ContextOption {
        id: "64k",
        label: "64K",
        tokens: 65_536,
    },
    ContextOption {
        id: "128k",
        label: "128K",
        tokens: 131_072,
    },
    ContextOption {
        id: "200k",
        label: "200K",
        tokens: 204_800,
    },
    ContextOption {
        id: "300k",
        label: "300K",
        tokens: 307_200,
    },
    ContextOption {
        id: "1m",
        label: "1M",
        tokens: 1_048_576,
    },
];

/// 未知模型的回落窗口(与 client DEFAULT_CONTEXT_WINDOW 同值)。
pub const DEFAULT_CONTEXT_TOKENS: u64 = 65_536;

/// 会话未选模型时的默认(design-snapshot defaultModelId 同源)。
/// D-F8-C 延伸:默认 = openai-compat 渠道(resolve_provider 同纪律:配齐即默认,
/// 未配齐时 llm 层按优先级回落 deepseek/mock,此处只声明菜单默认选中项)。
pub const DEFAULT_MODEL_ID: &str = "openai-compat";

/// 模型能力目录(条目顺序即菜单顺序;label 对 openai-compat 由 snapshot 侧按实配覆盖)。
pub const CATALOG: &[ModelCard] = &[
    ModelCard {
        id: "deepseek-chat",
        label: "deepseek-chat",
        provider: "deepseek",
        group: "DeepSeek",
        supports_thinking: true,
        thinking_model: Some("deepseek-reasoner"),
        effort_options: EFFORTS_NONE,
        default_effort: None,
        context_options: CTX_FIXED_64K,
        default_context: "64k",
    },
    ModelCard {
        id: "mock",
        label: "Mock provider",
        provider: "mock",
        group: "本地",
        supports_thinking: false,
        thinking_model: None,
        effort_options: EFFORTS_NONE,
        default_effort: None,
        context_options: CTX_FIXED_64K,
        default_context: "64k",
    },
    ModelCard {
        id: "openai-compat",
        label: "openai-compatible",
        provider: "openai-compat",
        group: "自定义渠道",
        supports_thinking: true,
        thinking_model: None,
        effort_options: EFFORTS_FULL,
        default_effort: Some("medium"),
        context_options: CTX_TIERS,
        // 用户拍板(2026-08-29):k3 渠道按 1M 上下文使用,默认档直接给 1m。
        default_context: "1m",
    },
];

pub fn card(model_id: &str) -> Option<&'static ModelCard> {
    CATALOG.iter().find(|c| c.id == model_id)
}

/// PATCH 入参校验用:是否本仓已知的 effort 档(跨模型集合;当前模型是否支持交给 resolve 回落)。
pub fn is_known_effort(id: &str) -> bool {
    EFFORTS_FULL.iter().any(|o| o.id == id)
}

/// PATCH 入参校验用:是否本仓已知的 context 档(同上)。
pub fn is_known_context(id: &str) -> bool {
    CTX_TIERS.iter().any(|o| o.id == id)
}

/// 会话选择解析后的实发规格(每轮现算,不落库)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSpec {
    /// 实发模型名覆盖(None = 用 provider 默认名)。
    pub model: Option<String>,
    /// 实发 reasoning_effort(None = 请求体不含该字段)。
    pub reasoning_effort: Option<String>,
    /// 上下文窗口声明(计量面;不进请求体)。
    pub context_tokens: u64,
}

impl Default for ResolvedSpec {
    fn default() -> Self {
        ResolvedSpec {
            model: None,
            reasoning_effort: None,
            context_tokens: DEFAULT_CONTEXT_TOKENS,
        }
    }
}

/// 会话三档选择 → 实发规格。未知模型/不支持的档位一律回落该模型默认,不报错
/// (换模型是常态操作,旧档位不适用时静默失效比 400 更符合交互预期;非法值在 PATCH 侧已拦)。
pub fn resolve(
    model_id: Option<&str>,
    thinking: bool,
    effort: Option<&str>,
    context: Option<&str>,
) -> ResolvedSpec {
    let Some(card) = model_id.or(Some(DEFAULT_MODEL_ID)).and_then(card) else {
        return ResolvedSpec::default();
    };
    let thinking = thinking && card.supports_thinking;
    let model = if thinking {
        card.thinking_model.map(str::to_string)
    } else {
        None
    };
    // effort 仅在「思考开 + 该渠道收 reasoning_effort」时实发;选择不在档内回落模型默认档。
    let reasoning_effort = if thinking && !card.effort_options.is_empty() {
        effort
            .filter(|e| card.effort_options.iter().any(|o| o.id == *e))
            .or(card.default_effort)
            .map(str::to_string)
    } else {
        None
    };
    let context_tokens = context
        .and_then(|c| card.context_options.iter().find(|o| o.id == c))
        .or_else(|| {
            card.context_options
                .iter()
                .find(|o| o.id == card.default_context)
        })
        .map(|o| o.tokens)
        .unwrap_or(DEFAULT_CONTEXT_TOKENS);
    ResolvedSpec {
        model,
        reasoning_effort,
        context_tokens,
    }
}

/// 单条模型的 wire(design-snapshot models[];availability/label 由调用方按实测注入)。
pub fn card_json(card: &ModelCard, availability: &str, label: &str) -> Value {
    json!({
        "id": card.id,
        "label": label,
        "provider": card.provider,
        "availability": availability,
        "group": card.group,
        "supportsThinking": card.supports_thinking,
        "effortOptions": card.effort_options.iter()
            .map(|o| json!({ "id": o.id, "label": o.label }))
            .collect::<Vec<Value>>(),
        "defaultEffort": card.default_effort,
        "contextOptions": card.context_options.iter()
            .map(|o| json!({ "id": o.id, "label": o.label, "tokens": o.tokens }))
            .collect::<Vec<Value>>(),
        "defaultContext": card.default_context,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_in_options_and_ids_unique() {
        for c in CATALOG {
            assert!(
                c.context_options.iter().any(|o| o.id == c.default_context),
                "{} 的 default_context 不在 context_options 内",
                c.id
            );
            if let Some(d) = c.default_effort {
                assert!(
                    c.effort_options.iter().any(|o| o.id == d),
                    "{} 的 default_effort 不在 effort_options 内",
                    c.id
                );
            }
            // 不支持思考的渠道不该带思考模型名,也不该有 effort 档(否则 UI 出可点却不生效的行)。
            if !c.supports_thinking {
                assert!(c.thinking_model.is_none());
                assert!(c.effort_options.is_empty());
            }
        }
        let mut ids: Vec<&str> = CATALOG.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "CATALOG 存在重复 id");
    }

    /// deepseek 三腿:思考关 = 不换名不发 effort;思考开 = 换 reasoner;effort 选了也不发。
    #[test]
    fn deepseek_thinking_switches_model_name_only() {
        let off = resolve(Some("deepseek-chat"), false, Some("high"), None);
        assert_eq!(off.model, None);
        assert_eq!(off.reasoning_effort, None);
        assert_eq!(off.context_tokens, 65_536);

        let on = resolve(Some("deepseek-chat"), true, None, None);
        assert_eq!(on.model.as_deref(), Some("deepseek-reasoner"));
        assert_eq!(on.reasoning_effort, None, "deepseek 官方不收 reasoning_effort");
    }

    /// openai-compat 三腿:思考关不发 effort;开且选档发该档;开但档位越界回落默认档。
    #[test]
    fn openai_compat_effort_and_context_resolution() {
        let off = resolve(Some("openai-compat"), false, Some("max"), Some("1m"));
        assert_eq!(off.reasoning_effort, None);
        assert_eq!(off.model, None, "openai-compat 靠 effort 表达思考,不换模型名");
        assert_eq!(off.context_tokens, 1_048_576);

        let on = resolve(Some("openai-compat"), true, Some("xhigh"), Some("300k"));
        assert_eq!(on.reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(on.context_tokens, 307_200);

        // 越界 effort(deepseek 才有的空档)与越界 context → 回落该模型默认档。
        let fallback = resolve(Some("openai-compat"), true, Some("ludicrous"), Some("9m"));
        assert_eq!(fallback.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(fallback.context_tokens, 1_048_576);
    }

    /// mock 不支持思考:开关打开也不换名不发 effort(恒绿 seam 行为不变)。
    #[test]
    fn mock_ignores_thinking_and_effort() {
        let r = resolve(Some("mock"), true, Some("max"), Some("1m"));
        assert_eq!(r, ResolvedSpec::default());
    }

    /// 未选模型 = 走 defaultModelId(openai-compat)能力;完全未知 id 走全默认。
    #[test]
    fn unknown_and_absent_model_fall_back() {
        let r = resolve(None, true, None, None);
        assert_eq!(
            r.model, None,
            "未选模型应按 defaultModelId(openai-compat)解析:思考开但无思考换名"
        );
        assert_eq!(r.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(r.context_tokens, 1_048_576);
        assert_eq!(resolve(Some("不存在的模型"), true, Some("max"), Some("1m")), ResolvedSpec::default());
    }

    #[test]
    fn card_json_carries_capability_face() {
        let c = card("openai-compat").unwrap();
        let v = card_json(c, "available", "qwen2.5-7b");
        assert_eq!(v["label"], "qwen2.5-7b");
        assert_eq!(v["availability"], "available");
        assert_eq!(v["supportsThinking"], true);
        assert_eq!(v["effortOptions"].as_array().unwrap().len(), 5);
        assert_eq!(v["effortOptions"][3]["id"], "xhigh");
        assert_eq!(v["effortOptions"][3]["label"], "Extra High");
        assert_eq!(v["contextOptions"][4]["id"], "1m");
        assert_eq!(v["contextOptions"][4]["tokens"], 1_048_576);
        assert_eq!(v["defaultContext"], "1m");
    }
}
