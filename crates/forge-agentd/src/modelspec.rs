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

const CTX_FIXED_256K: &[ContextOption] = &[ContextOption { id: "256k", label: "256K", tokens: 262_144 }];

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
    ModelCard {
        id: "gemini-3.8-flash",
        label: "gemini-3.8-flash",
        provider: "antigravity",
        group: "Antigravity",
        supports_thinking: true,
        thinking_model: None,
        effort_options: EFFORTS_FULL,
        default_effort: Some("medium"),
        context_options: CTX_TIERS,
        default_context: "1m",
    },
    ModelCard {
        id: "gemini-3.8-pro",
        label: "gemini-3.8-pro",
        provider: "antigravity",
        group: "Antigravity",
        supports_thinking: true,
        thinking_model: None,
        effort_options: EFFORTS_FULL,
        default_effort: Some("high"),
        context_options: CTX_TIERS,
        default_context: "1m",
    },
    ModelCard {
        id: "kimi-code", label: "Kimi Code", provider: "kimi", group: "Kimi Code",
        supports_thinking: true, thinking_model: None, effort_options: EFFORTS_FULL,
        default_effort: Some("medium"), context_options: CTX_FIXED_256K, default_context: "256k",
    },
    ModelCard {
        id: "glm-coding", label: "GLM Coding Plan", provider: "glm", group: "GLM Coding Plan",
        supports_thinking: false, thinking_model: None, effort_options: EFFORTS_NONE,
        default_effort: None, context_options: CTX_TIERS, default_context: "1m",
    },
];

pub fn card(model_id: &str) -> Option<&'static ModelCard> {
    if let Some(c) = CATALOG.iter().find(|c| c.id == model_id) {
        return Some(c);
    }
    if model_id == "antigravity" || model_id == "antigravity/" || model_id == "antigravity:" {
        return CATALOG.iter().find(|c| c.provider == "antigravity");
    }
    if let Some(stripped) = model_id
        .strip_prefix("antigravity/")
        .or_else(|| model_id.strip_prefix("antigravity:"))
    {
        return CATALOG
            .iter()
            .find(|c| c.provider == "antigravity" && c.id == stripped);
    }
    None
}

/// PATCH 入参校验用:是否本仓已知的 effort 档(跨模型集合;当前模型是否支持交给 resolve 回落)。
pub fn is_known_effort(id: &str) -> bool {
    EFFORTS_FULL.iter().any(|o| o.id == id)
}

/// PATCH 入参校验用:是否本仓已知的 context 档(同上)。
pub fn is_known_context(id: &str) -> bool {
    CTX_TIERS.iter().chain(CTX_FIXED_256K).any(|o| o.id == id)
}

/// 会话选择解析后的实发规格(每轮现算,不落库)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSpec {
    /// 实发模型名覆盖(None = 用 provider 默认名)。
    pub model: Option<String>,
    /// 实发 reasoning_effort(None = 请求体不含该字段)。
    pub reasoning_effort: Option<String>,
    /// Claude 云渠道的显式思考选择；其他渠道不发送兼容扩展字段。
    pub thinking_enabled: Option<bool>,
    /// 上下文窗口声明(计量面;不进请求体)。
    pub context_tokens: u64,
}

impl Default for ResolvedSpec {
    fn default() -> Self {
        ResolvedSpec {
            model: None,
            reasoning_effort: None,
            thinking_enabled: None,
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
        thinking_enabled: None,
        context_tokens,
    }
}

/// reasoning_effort 取值的强度序(弱 → 强),即模块注里那份 chat.completions 现行取值集。
/// 本仓 UI 只暴露 low..max 五档,但云端目录的档位清单可能带 none/minimal,排序时一并认。
const EFFORT_RANK: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// 档位清单里最强的一档(D-044:UltraPlan 的 leader 取它)。不在强度序里的档位 id 不参与比较
/// ——排不了序的值宁可不选,也不猜它是强是弱;清单为空或全不认识 → None。
pub fn max_effort<S: AsRef<str>>(options: &[S]) -> Option<String> {
    options
        .iter()
        .filter_map(|o| {
            let id = o.as_ref().trim();
            EFFORT_RANK
                .iter()
                .position(|r| *r == id)
                .map(|rank| (rank, id))
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, id)| id.to_string())
}

fn cloud_model_id(model_id: Option<&str>) -> Option<&str> {
    model_id.and_then(|m| m.strip_prefix("cloud:"))
}

/// 云模型的规格来自动态目录，不能用静态 card 查找后回落为空规格。
/// 思考开启时尊重所选档位，失效选择回落目录默认档（首项）；关闭时传最低可用档，
/// 避免省略参数后继承上游默认思考强度。目录没有可识别档位时不发参数。
pub fn resolve_with_cloud(
    model_id: Option<&str>,
    thinking: bool,
    effort: Option<&str>,
    context: Option<&str>,
    cloud_efforts: Option<&[String]>,
) -> ResolvedSpec {
    let mut spec = resolve(model_id, thinking, effort, context);
    if cloud_model_id(model_id).is_none() {
        return spec;
    }
    let options: Vec<&str> = cloud_efforts
        .into_iter()
        .flatten()
        .map(|o| o.trim())
        .filter(|id| EFFORT_RANK.contains(id))
        .collect();
    spec.reasoning_effort = if thinking {
        effort
            .filter(|id| options.contains(id))
            .or_else(|| options.first().copied())
    } else {
        options.iter().copied().min_by_key(|id| {
            EFFORT_RANK.iter().position(|rank| rank == id).unwrap()
        })
    }
    .map(str::to_string);
    spec
}

/// 该模型有没有「思考」可开(D-044:没有时调用方发 THINKING_UNAVAILABLE 提示,不谎称深度规划)。
///
/// 静态目录模型看 card.supports_thinking(未选模型按 DEFAULT_MODEL_ID,与 resolve 同口径);
/// `cloud:<id>` 不在静态目录里,以调用方递进来的云端档位清单为准——有可排序的 effort 档即支持
/// (与 design-snapshot 云端卡片的 supportsThinking 同判据)。
pub fn supports_thinking(model_id: Option<&str>, cloud_efforts: Option<&[String]>) -> bool {
    if cloud_model_id(model_id).is_some() {
        return cloud_efforts.and_then(max_effort).is_some();
    }
    model_id
        .or(Some(DEFAULT_MODEL_ID))
        .and_then(card)
        .is_some_and(|c| c.supports_thinking)
}

/// D-044「深度规划」规格:思考强制开 + 该模型最强的 effort 档;context 档照会话选择。
/// 只给 UltraPlan 的 leader 步进用——子代理仍用会话自己的规格,不被连带抬到最高档。
///
/// 逐渠道的实际效果(与 [`resolve`] 对各渠道的处理一一对应):
/// - deepseek:思考靠换模型名表达 → 实发 deepseek-reasoner;该渠道不收 effort,不发;
/// - openai-compat:发 reasoning_effort = 目录里最强一档(现为 max);
/// - mock / 不支持思考的卡片:与「思考关」的 resolve 结果相同,什么都不强加;
/// - `cloud:<id>`:静态目录没有它的卡片,effort 取 `cloud_efforts` 里最强的一档
///   (清单缺失/为空/全不认识 → 不发 effort);窗口仍是 resolve 对未知模型的缺省;
/// - 未选模型:与 resolve 一样按 DEFAULT_MODEL_ID 的卡片算。注意这只是「卡片缺省」,
///   实际渠道若回落到了别家(如只配了 deepseek),调用方应传与实际渠道对应的模型 id。
pub fn resolve_deep(
    model_id: Option<&str>,
    context: Option<&str>,
    cloud_efforts: Option<&[String]>,
) -> ResolvedSpec {
    if cloud_model_id(model_id).is_some() {
        let mut spec = resolve(model_id, true, None, context);
        spec.reasoning_effort = cloud_efforts.and_then(max_effort);
        return spec;
    }
    let top = model_id
        .or(Some(DEFAULT_MODEL_ID))
        .and_then(card)
        .and_then(|c| {
            let ids: Vec<&str> = c.effort_options.iter().map(|o| o.id).collect();
            max_effort(&ids)
        });
    resolve(model_id, true, top.as_deref(), context)
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
    fn cloud_session_effort_uses_dynamic_capabilities() {
        let options = vec!["medium".into(), "low".into(), "high".into(), "max".into()];
        let resolve = |thinking, effort| {
            resolve_with_cloud(Some("cloud:gpt-6.1-sol"), thinking, effort, None, Some(&options))
        };
        assert_eq!(resolve(true, Some("low")).reasoning_effort.as_deref(), Some("low"));
        assert_eq!(resolve(true, Some("max")).reasoning_effort.as_deref(), Some("max"));
        assert_eq!(resolve(true, Some("xhigh")).reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(resolve(true, None).reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(resolve(false, Some("max")).reasoning_effort.as_deref(), Some("low"));
        assert_eq!(resolve(false, None).model, None, "provider keeps the actual cloud model ID");
    }

    #[test]
    fn cloud_thinking_off_uses_lowest_supported_effort() {
        for (options, want) in [
            (vec!["high", "none", "minimal", "low"], Some("none")),
            (vec!["medium", "minimal", "low"], Some("minimal")),
            (vec!["high", "low", "medium"], Some("low")),
            (vec!["unknown"], None),
            (vec![], None),
        ] {
            let options: Vec<String> = options.into_iter().map(str::to_string).collect();
            let spec = resolve_with_cloud(Some("cloud:gpt-x"), false, Some("max"), None, Some(&options));
            assert_eq!(spec.reasoning_effort.as_deref(), want);
        }
        assert_eq!(resolve_with_cloud(Some("cloud:gpt-x"), true, Some("low"), None, None), ResolvedSpec::default());
    }

    #[test]
    fn cloud_capabilities_do_not_change_local_channel_specs() {
        let options = vec!["low".into()];
        for model in [Some("openai-compat"), Some("deepseek-chat"), Some("mock"), None] {
            for thinking in [false, true] {
                assert_eq!(resolve_with_cloud(model, thinking, Some("high"), Some("300k"), Some(&options)),
                           resolve(model, thinking, Some("high"), Some("300k")));
            }
        }
    }

    #[test]
    fn official_model_capabilities_match_native_channel_options() {
        let kimi = resolve(Some("kimi-code"), true, Some("xhigh"), None);
        assert_eq!(kimi.reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(kimi.context_tokens, 262_144);
        let kimi_off = resolve(Some("kimi-code"), false, Some("high"), None);
        assert_eq!(kimi_off.reasoning_effort, None);
        let glm = resolve(Some("glm-coding"), true, Some("high"), None);
        assert_eq!(glm.reasoning_effort, None);
        assert_eq!(glm.model, None);
    }

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
        assert_eq!(
            on.reasoning_effort, None,
            "deepseek 官方不收 reasoning_effort"
        );
    }

    /// openai-compat 三腿:思考关不发 effort;开且选档发该档;开但档位越界回落默认档。
    #[test]
    fn openai_compat_effort_and_context_resolution() {
        let off = resolve(Some("openai-compat"), false, Some("max"), Some("1m"));
        assert_eq!(off.reasoning_effort, None);
        assert_eq!(
            off.model, None,
            "openai-compat 靠 effort 表达思考,不换模型名"
        );
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
        assert_eq!(
            resolve(Some("不存在的模型"), true, Some("max"), Some("1m")),
            ResolvedSpec::default()
        );
    }

    /// D-044:强度序取最强档;不认识的档位不参与,空清单 → None。
    #[test]
    fn max_effort_ranks_known_tiers_only() {
        assert_eq!(
            max_effort(&["low", "xhigh", "medium"]).as_deref(),
            Some("xhigh")
        );
        assert_eq!(max_effort(&["none", "minimal"]).as_deref(), Some("minimal"));
        assert_eq!(
            max_effort(&["high".to_string(), " max ".to_string()]).as_deref(),
            Some("max"),
            "String 清单同样可用,首尾空白不影响"
        );
        assert_eq!(
            max_effort(&["ludicrous", "medium"]).as_deref(),
            Some("medium")
        );
        assert_eq!(max_effort(&["ludicrous"]), None);
        assert_eq!(max_effort::<&str>(&[]), None);
        // 目录里每张卡的全部档位都排得了序(新增档位忘了进强度序会在这里露出来)。
        for c in CATALOG {
            for o in c.effort_options {
                assert!(
                    EFFORT_RANK.contains(&o.id),
                    "{} 的档位 {} 不在强度序里",
                    c.id,
                    o.id
                );
            }
        }
        let full: Vec<&str> = EFFORTS_FULL.iter().map(|o| o.id).collect();
        assert_eq!(max_effort(&full).as_deref(), Some("max"));
    }

    /// D-044 深度规划规格:思考强制开 + 最强 effort;各渠道的落点与 resolve 的渠道语义一致。
    #[test]
    fn resolve_deep_forces_thinking_and_top_effort() {
        // deepseek:换 reasoner 模型名;该渠道不收 effort。
        let ds = resolve_deep(Some("deepseek-chat"), None, None);
        assert_eq!(ds.model.as_deref(), Some("deepseek-reasoner"));
        assert_eq!(ds.reasoning_effort, None);
        assert_eq!(ds.context_tokens, 65_536);
        assert_eq!(ds, resolve(Some("deepseek-chat"), true, None, None));

        // openai-compat:不换名,effort 取最强档(会话原先选的档位不参与);context 照会话选择。
        let oai = resolve_deep(Some("openai-compat"), Some("300k"), None);
        assert_eq!(oai.model, None);
        assert_eq!(oai.reasoning_effort.as_deref(), Some("max"));
        assert_eq!(oai.context_tokens, 307_200);
        // 未选模型:与 resolve 同口径按 DEFAULT_MODEL_ID(openai-compat)。
        let absent = resolve_deep(None, None, None);
        assert_eq!(absent.reasoning_effort.as_deref(), Some("max"));
        assert_eq!(absent.context_tokens, 1_048_576);

        // mock:不支持思考 → 与思考关的 resolve 逐字段相同(恒绿 seam 不变)。
        let mock = resolve_deep(Some("mock"), Some("1m"), None);
        assert_eq!(mock, resolve(Some("mock"), false, None, Some("1m")));
        assert_eq!(mock, ResolvedSpec::default());
        // 目录外的未知 id:全默认,不强加任何东西。
        assert_eq!(
            resolve_deep(Some("不存在的模型"), None, None),
            ResolvedSpec::default()
        );

        // cloud:<id>:effort 取云端档位清单里最强的一档;清单缺失/为空/全不认识 → 不发 effort。
        let efforts = vec!["low".to_string(), "high".to_string(), "medium".to_string()];
        let cloud = resolve_deep(Some("cloud:gpt-x"), Some("1m"), Some(&efforts));
        assert_eq!(cloud.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(cloud.model, None, "云端模型名由 provider 决定,规格不覆盖");
        assert_eq!(cloud.context_tokens, DEFAULT_CONTEXT_TOKENS);
        let empty: Vec<String> = Vec::new();
        let unknown = vec!["ludicrous".to_string()];
        for list in [None, Some(empty.as_slice()), Some(unknown.as_slice())] {
            assert_eq!(
                resolve_deep(Some("cloud:gpt-x"), None, list),
                ResolvedSpec::default(),
                "{list:?}"
            );
        }
        // 静态目录模型不看云端清单。
        assert_eq!(
            resolve_deep(Some("deepseek-chat"), None, Some(&efforts)),
            ds
        );

        // supports_thinking:调用方据此决定发 THINKING_UNAVAILABLE 还是声称深度规划。
        assert!(supports_thinking(Some("deepseek-chat"), None));
        assert!(supports_thinking(Some("openai-compat"), None));
        assert!(supports_thinking(None, None), "未选模型按 DEFAULT_MODEL_ID");
        assert!(!supports_thinking(Some("mock"), None));
        assert!(!supports_thinking(Some("不存在的模型"), None));
        assert!(supports_thinking(Some("cloud:gpt-x"), Some(&efforts)));
        assert!(!supports_thinking(Some("cloud:gpt-x"), None));
        assert!(!supports_thinking(Some("cloud:gpt-x"), Some(&empty)));
        assert!(
            !supports_thinking(Some("mock"), Some(&efforts)),
            "云端清单只对 cloud: 模型有意义"
        );
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

    #[test]
    fn antigravity_modelspec_thinking_effort_context_resolution() {
        // 思考开启 + 指定 high effort + 1M 窗口
        let on = resolve(Some("gemini-3.8-flash"), true, Some("high"), Some("1m"));
        assert_eq!(on.model, None, "gemini 思考模式不换模型名");
        assert_eq!(on.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(on.context_tokens, 1_048_576, "默认 1M 上下文窗口");

        // 思考关闭 -> reasoning_effort 为 None
        let off = resolve(Some("gemini-3.8-flash"), false, Some("high"), Some("1m"));
        assert_eq!(off.reasoning_effort, None, "思考关闭时请求体不发 reasoning_effort");
        assert_eq!(off.context_tokens, 1_048_576);

        // 越界 effort 回落默认档 (flash 默认 medium, pro 默认 high)
        let fallback_flash = resolve(Some("gemini-3.8-flash"), true, Some("invalid_effort"), None);
        assert_eq!(fallback_flash.reasoning_effort.as_deref(), Some("medium"));

        let fallback_pro = resolve(Some("gemini-3.8-pro"), true, Some("invalid_effort"), None);
        assert_eq!(fallback_pro.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn antigravity_modelspec_prefix_and_aliasing() {
        // 1. card() 验证 antigravity/ 与 antigravity: 前缀解析
        assert!(card("antigravity/gemini-3.8-flash").is_some());
        assert_eq!(card("antigravity/gemini-3.8-flash").unwrap().id, "gemini-3.8-flash");
        assert_eq!(card("antigravity:gemini-3.8-flash").unwrap().id, "gemini-3.8-flash");

        assert!(card("antigravity/gemini-3.8-pro").is_some());
        assert_eq!(card("antigravity/gemini-3.8-pro").unwrap().id, "gemini-3.8-pro");
        assert_eq!(card("antigravity:gemini-3.8-pro").unwrap().id, "gemini-3.8-pro");

        // 裸 antigravity / antigravity: / antigravity/ 解析到默认 flash 卡片
        assert!(card("antigravity").is_some());
        assert_eq!(card("antigravity").unwrap().id, "gemini-3.8-flash");
        assert_eq!(card("antigravity:").unwrap().id, "gemini-3.8-flash");
        assert_eq!(card("antigravity/").unwrap().id, "gemini-3.8-flash");

        // 2. resolve() 验证带前缀的规格解析
        let res_slash = resolve(Some("antigravity/gemini-3.8-flash"), true, Some("high"), Some("1m"));
        assert_eq!(res_slash.context_tokens, 1_048_576);
        assert_eq!(res_slash.reasoning_effort.as_deref(), Some("high"));

        let res_colon = resolve(Some("antigravity:gemini-3.8-pro"), true, Some("medium"), Some("300k"));
        assert_eq!(res_colon.context_tokens, 307_200);
        assert_eq!(res_colon.reasoning_effort.as_deref(), Some("medium"));

        let res_bare = resolve(Some("antigravity"), true, None, None);
        assert_eq!(res_bare.context_tokens, 1_048_576);
        assert_eq!(res_bare.reasoning_effort.as_deref(), Some("medium"));

        // 3. supports_thinking() 验证带前缀识别
        assert!(supports_thinking(Some("antigravity/gemini-3.8-flash"), None));
        assert!(supports_thinking(Some("antigravity:gemini-3.8-pro"), None));
    }
}
