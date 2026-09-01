//! F7 wave.1 design-snapshot 聚合(D-F7-A;参考 api/handlers/snapshot.rs 的 wave.1 子集):
//! GET /api/forge/design-snapshot?sessionId= →
//! { sessions, activeSession, events(该会话持久化全量回放), todos, run, models, latestSeq, chatFolders }。
//! wave.1 裁剪留痕:参考全量还含 planBundle/diffs/proposals/metrics/contextWindow/swarm,
//! models 走本仓 deepseek+mock 双档
//! (key 判定复用 llm.rs 的 resolve_deepseek_key,R-5:只回 availability 布尔语义,密钥不出)。
//! sessionId 空/不存在 → activeSession:null、events:[]、latestSeq:0。

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::llm;
use crate::modelspec;
use crate::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    #[serde(default)]
    pub(crate) session_id: Option<String>,
}

/// models 面(F8 wave.2 抽出纯函数:availability 实测同源 + openai-compat 条目,便于确定单测)。
/// 模型规格波:条目本体改由 modelspec::CATALOG 生成(带 Thinking/Effort/Context 能力面),
/// 本函数只负责把「实测才知道」的两项注入——availability(密钥/配置齐否)与 openai-compat 的
/// 动态 label(已配 = 模型名,未配如实标注)。R-5 不变:只回布尔语义,密钥不出。
fn models_json() -> Value {
    let deepseek_availability = if llm::deepseek_key_available() {
        "available"
    } else {
        "needs-key"
    };
    // F8 wave.2:openai-compat 渠道条目(availability 同源 openai_compat_status;只布尔面不出 key)。
    let oai = llm::openai_compat_status();
    let oai_availability = if oai.configured {
        "available"
    } else {
        "needs-key"
    };
    // label:已配 model 用模型名,未配如实「openai-compatible(未配置)」。
    let oai_label = if oai.model.is_empty() {
        "openai-compatible(未配置)".to_string()
    } else {
        oai.model.clone()
    };
    let models: Vec<Value> = modelspec::CATALOG
        .iter()
        .map(|c| match c.id {
            "deepseek-chat" => modelspec::card_json(c, deepseek_availability, c.label),
            llm::OPENAI_COMPAT_MODEL_ID => {
                modelspec::card_json(c, oai_availability, &oai_label)
            }
            _ => modelspec::card_json(c, "available", c.label),
        })
        .collect();
    json!({
        "models": models,
        "defaultModelId": modelspec::DEFAULT_MODEL_ID,
    })
}

/// GET /api/forge/design-snapshot?sessionId=。
pub async fn design_snapshot(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SnapshotQuery>,
) -> Json<Value> {
    let sessions = state.sessions.list();
    let active = q
        .session_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .and_then(|sid| state.sessions.get(&sid));
    let (events, latest_seq) = match &active {
        Some(s) => (
            state
                .events
                .persisted(&s.id)
                .iter()
                .map(|e| e.to_wire())
                .collect::<Vec<Value>>(),
            state.events.latest_seq(&s.id),
        ),
        None => (Vec::new(), 0),
    };
    // F7 wave.2:todos/run 填真(todos=该会话列表;run=activeRunId 对应记录,丢失则 null 如实)。
    let todos: Vec<Value> = match &active {
        Some(s) => state
            .todos
            .list_by_session(&s.id)
            .iter()
            .map(|t| serde_json::to_value(t).expect("todo 序列化失败"))
            .collect(),
        None => Vec::new(),
    };
    let run: Value = active
        .as_ref()
        .and_then(|s| s.active_run_id.as_deref())
        .and_then(|rid| state.runs.get(rid))
        .map(|r| serde_json::to_value(r).expect("run 序列化失败"))
        .unwrap_or(Value::Null);
    Json(json!({
        "sessions": sessions,
        "activeSession": active,
        "events": events,
        "todos": todos,
        "run": run,
        "models": models_json(),
        "latestSeq": latest_seq,
        "chatFolders": state.folders.list(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F8 wave.2:models 面 openai-compat 条目(未配 needs-key / 配齐 available;响应面无 key)。
    #[test]
    fn models_openai_compat_availability_two_legs_redline() {
        let _g = llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!(
            "agentd-snap-oai-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        // 未配置腿:第三条目 needs-key + 如实 label;deepseek/mock 条目位置不变(主线测试索引 [0] 纪律)。
        let v = models_json();
        let arr = v["models"].as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["id"], "deepseek-chat");
        assert_eq!(arr[0]["availability"], "needs-key");
        assert_eq!(arr[1]["id"], "mock");
        assert_eq!(arr[2]["id"], "openai-compat");
        assert_eq!(arr[2]["provider"], "openai-compat");
        assert_eq!(arr[2]["availability"], "needs-key");
        assert_eq!(arr[2]["label"], "openai-compatible(未配置)");
        // 规格能力面随条目下发(client ModelPicker 的三个子菜单全靠它驱动):
        // deepseek 收不到 reasoning_effort → effortOptions 空 + 单档窗口;
        // openai-compat 是自配渠道 → 五档 effort + 多档窗口。
        assert_eq!(arr[0]["supportsThinking"], true);
        assert_eq!(arr[0]["effortOptions"].as_array().unwrap().len(), 0);
        assert_eq!(arr[0]["contextOptions"].as_array().unwrap().len(), 1);
        assert_eq!(arr[1]["supportsThinking"], false);
        assert_eq!(arr[2]["effortOptions"].as_array().unwrap().len(), 5);
        assert!(arr[2]["contextOptions"].as_array().unwrap().len() > 1);
        assert_eq!(v["defaultModelId"], "deepseek-chat");
        // 配齐腿:config JSON + keystore → available + label=model 名;全文无 key 子串。
        std::fs::write(
            dir.join("llm-openai-compat.json"),
            r#"{"base_url":"http://127.0.0.1:1","model":"qwen2.5-7b"}"#,
        )
        .unwrap();
        let secret = "sk-test-oai-REDLINE-snapshot";
        gend::keystore::set_key("openai-compat", secret).unwrap();
        let v2 = models_json();
        let arr2 = v2["models"].as_array().unwrap();
        assert_eq!(arr2[2]["availability"], "available");
        assert_eq!(arr2[2]["label"], "qwen2.5-7b");
        assert!(!v2.to_string().contains(secret), "models 面泄漏密钥(R-5)");
        assert!(!v2.to_string().contains("sk-"), "models 面含 sk- 串(R-5)");
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&dir).ok();
    }
}
