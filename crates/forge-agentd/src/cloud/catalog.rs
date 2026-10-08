//! 模型目录(15 §3.3)的派生查询:缓存时效、默认模型、Codex 可用子集、embedding 模型。

use std::time::Duration;

use serde_json::{json, Value};

use super::{Catalog, CatalogModel};

/// 目录缓存时效。
pub(crate) const CATALOG_TTL: Duration = Duration::from_secs(5 * 60);

impl Catalog {
    /// 本地引擎缺省模型:目录 `defaultModel`(须在目录内)> 首个可用模型 > 首个模型。
    pub fn default_entry(&self) -> Option<&CatalogModel> {
        self.find(&self.default_model)
            .or_else(|| self.models.iter().find(|m| m.available))
            .or_else(|| self.models.first())
    }

    /// Codex 引擎可用子集(`capabilities.responses=true`)。
    pub fn responses_models(&self) -> impl Iterator<Item = &CatalogModel> {
        self.models.iter().filter(|m| m.capabilities.responses)
    }

    /// Codex 缺省模型:目录 `defaultModel`(须支持 responses)> 首个支持 responses 的可用模型。
    pub fn responses_default(&self) -> Option<&CatalogModel> {
        self.find(&self.default_model)
            .filter(|m| m.capabilities.responses)
            .or_else(|| self.responses_models().find(|m| m.available))
            .or_else(|| self.responses_models().next())
    }

    /// embedding 模型:env FORGE_CLOUD_EMBED_MODEL 指名 > id 含 "embed" 的首个可用模型。
    /// 目录的能力位里没有 embedding 一项,只能按命名约定认。
    pub fn embedding_model(&self) -> Option<String> {
        if let Ok(v) = std::env::var("FORGE_CLOUD_EMBED_MODEL") {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
        self.models
            .iter()
            .find(|m| m.available && m.id.to_ascii_lowercase().contains("embed"))
            .map(|m| m.id.clone())
    }
}

/// 上下文窗口 → 菜单标签(400000 → "400K",1000000 → "1M",65536 → "64K")。
/// design-snapshot 云端模型条目(15 §8.4)。
pub(crate) fn snapshot_cards(catalog: Option<&Catalog>, logged_in: bool) -> Vec<Value> {
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    catalog
        .models
        .iter()
        .map(|m| snapshot_card(m, logged_in, &catalog.currency))
        .collect()
}

fn snapshot_card(m: &CatalogModel, logged_in: bool, currency: &str) -> Value {
    let availability = if !logged_in {
        "needs-login"
    } else if m.available {
        "available"
    } else {
        "unavailable"
    };
    let ctx = m.capabilities.context_window;
    let ctx_id = if ctx >= 1_000_000 {
        "1m"
    } else if ctx >= 400_000 {
        "native"
    } else if ctx >= 200_000 {
        "200k"
    } else {
        "64k"
    };
    let efforts: Vec<Value> = m
        .capabilities
        .reasoning_efforts
        .iter()
        .map(|e| json!({ "id": e, "label": e }))
        .collect();
    let default_effort = m
        .capabilities
        .reasoning_efforts
        .first()
        .map(|s| json!(s))
        .unwrap_or(Value::Null);
    json!({
        "id": format!("cloud:{}", m.id),
        "label": m.display_name,
        "provider": "cloud",
        "availability": availability,
        "group": "RurixForge 云",
        "supportsThinking": !efforts.is_empty() || !m.capabilities.thinking_mode.is_empty(),
        "thinkingMode": m.capabilities.thinking_mode,
        "thinkingAlwaysOn": m.capabilities.thinking_always_on,
        "effortOptions": efforts,
        "defaultEffort": default_effort,
        "contextOptions": [{
            "id": ctx_id,
            "label": context_label(ctx),
            "tokens": ctx,
        }],
        "defaultContext": ctx_id,
        "vision": m.capabilities.vision,
        "pricing": {
            "inputPer1M": m.pricing.input_per_1m,
            "outputPer1M": m.pricing.output_per_1m,
            "cacheReadPer1M": m.pricing.cache_read_per_1m,
            "cacheWritePer1M": m.pricing.cache_write_per_1m,
        },
        "currency": currency,
    })
}

pub(crate) fn context_label(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let m = tokens as f64 / 1_000_000.0;
        let s = format!("{m:.1}");
        return format!("{}M", s.trim_end_matches('0').trim_end_matches('.'));
    }
    if tokens % 1000 == 0 {
        return format!("{}K", tokens / 1000);
    }
    if tokens % 1024 == 0 {
        return format!("{}K", tokens / 1024);
    }
    format!("{}K", (tokens + 500) / 1000)
}

#[cfg(test)]
mod tests {
    use super::super::{CatalogCapabilities, CatalogModel};
    use super::*;

    fn model(id: &str, available: bool, responses: bool) -> CatalogModel {
        CatalogModel {
            id: id.into(),
            available,
            capabilities: CatalogCapabilities {
                responses,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn defaults_and_subsets() {
        let c = Catalog {
            default_model: "claude".into(),
            models: vec![
                model("claude", true, false),
                model("gpt-5.5", false, true),
                model("gpt-5.5-mini", true, true),
                model("text-embedding-3-small", true, false),
            ],
            ..Default::default()
        };
        assert_eq!(c.default_entry().unwrap().id, "claude");
        assert_eq!(c.responses_default().unwrap().id, "gpt-5.5-mini");
        assert_eq!(c.responses_models().count(), 2);
        assert_eq!(
            c.embedding_model().as_deref(),
            Some("text-embedding-3-small")
        );
        let missing = Catalog {
            default_model: "gone".into(),
            models: vec![model("a", false, false), model("b", true, false)],
            ..Default::default()
        };
        assert_eq!(missing.default_entry().unwrap().id, "b");
        assert!(missing.responses_default().is_none());
    }

    #[test]
    fn context_labels() {
        assert_eq!(context_label(400_000), "400K");
        assert_eq!(context_label(1_000_000), "1M");
        assert_eq!(context_label(1_048_576), "1M");
        assert_eq!(context_label(1_500_000), "1.5M");
        assert_eq!(context_label(65_536), "64K");
        assert_eq!(context_label(128_000), "128K");
    }

    #[test]
    fn claude_always_on_thinking_is_in_snapshot() {
        let mut m = model("claude-opus-5-5", true, false);
        m.capabilities.thinking_mode = "adaptive".into();
        m.capabilities.thinking_always_on = true;
        m.capabilities.reasoning_efforts = vec!["medium".into(), "low".into(), "high".into()];
        let card = snapshot_card(&m, true, "CNY");
        assert_eq!(card["supportsThinking"], true);
        assert_eq!(card["thinkingAlwaysOn"], true);
        assert_eq!(card["thinkingMode"], "adaptive");
        assert_eq!(card["defaultEffort"], "medium");
    }
}
