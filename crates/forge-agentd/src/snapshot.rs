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
    /// D-040:`events=0`(或 false)跳过事件回放——状态栏 5s 轮询只要模型/待办/项目面,
    /// 长会话的全量事件动辄上百 KB;latestSeq 照常给出。
    #[serde(default)]
    pub(crate) events: Option<String>,
}

/// models 面(F8 wave.2 抽出纯函数:availability 实测同源 + openai-compat 条目,便于确定单测)。
/// 模型规格波:条目本体改由 modelspec::CATALOG 生成(带 Thinking/Effort/Context 能力面),
/// 本函数只负责把「实测才知道」的两项注入——availability(密钥/配置齐否)与 openai-compat 的
/// 动态 label(已配 = 模型名,未配如实标注)。R-5 不变:只回布尔语义,密钥不出。
pub(crate) fn models_json(cloud: &crate::cloud::CloudService) -> Value {
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
    let ag_availability = crate::antigravity::availability();
    let ag_oauth = crate::antigravity_oauth::ready();
    let dev_mock = crate::cloud::dev_mock_enabled();
    let logged_in = cloud.is_logged_in();
    let mut models: Vec<Value> = modelspec::CATALOG
        .iter()
        .filter(|c| c.id != "mock" || dev_mock)
        .filter(|c| c.provider != "antigravity" || !ag_oauth)
        .map(|c| match c.id {
            "deepseek-chat" => modelspec::card_json(c, deepseek_availability, c.label),
            llm::OPENAI_COMPAT_MODEL_ID => modelspec::card_json(c, oai_availability, &oai_label),
            _ if c.provider == "antigravity" => {
                modelspec::card_json(c, ag_availability, c.label)
            }
            _ if matches!(c.provider, "kimi" | "glm") => modelspec::card_json(c,
                if crate::channels::ready(c.provider) { "available" } else { "needs-config" },
                &crate::channels::model_label(c.provider)),
            _ => modelspec::card_json(c, "available", c.label),
        })
        .collect();
    for model in crate::antigravity_oauth::models() {
        models.push(json!({"id":model["id"],"label":model["label"],"provider":"antigravity","availability":"available","group":"Google AI","supportsThinking":false,"effortOptions":[],"contextOptions":[],"defaultEffort":null,"defaultContext":null}));
    }
    models.extend(crate::cloud::snapshot_cards(
        cloud.last_catalog().as_ref(),
        logged_in,
    ));
    let default_model_id = if logged_in {
        cloud
            .catalog_cached()
            .or_else(|| cloud.last_catalog())
            .map(|c| format!("cloud:{}", c.default_model))
            .unwrap_or_else(|| modelspec::DEFAULT_MODEL_ID.to_string())
    } else {
        modelspec::DEFAULT_MODEL_ID.to_string()
    };
    json!({
        "models": models,
        "defaultModelId": default_model_id,
    })
}

/// GET /api/forge/design-snapshot?sessionId=。
pub async fn design_snapshot(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SnapshotQuery>,
) -> Json<Value> {
    let sessions = state.sessions.list();
    let with_events = !matches!(q.events.as_deref().map(str::trim), Some("0" | "false"));
    let active = q
        .session_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .and_then(|sid| state.sessions.get(&sid));
    let (events, latest_seq) = match &active {
        Some(s) => (
            if with_events {
                state
                    .events
                    .persisted(&s.id)
                    .iter()
                    .map(|e| e.to_wire())
                    .collect::<Vec<Value>>()
            } else {
                Vec::new()
            },
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
    // F-GAME-3:当前项目面(2D/3D 模式徽标 + 客户端视口切换的事实源;
    // 无会话时按默认工作区解析,徽标恒有定义)。
    let project: Value = {
        let sp = crate::scope::project_of(
            &state,
            active.as_ref().and_then(|s| s.workspace_id.as_deref()),
        );
        json!({
            "name": sp.name,
            "mode": sp.game_mode.as_str(),
            "root": sp.project_root.to_string_lossy(),
        })
    };
    state.cloud.refresh_catalog_soon();
    let mut models = models_json(&state.cloud);
    if let Some(arr) = models.get_mut("models").and_then(Value::as_array_mut) {
        arr.extend(state.codex.model_cards());
    }
    let goal = active
        .as_ref()
        .and_then(|s| state.goals.get(&s.id))
        .map(|g| {
            crate::goals::goal_json(
                &g,
                active
                    .as_ref()
                    .map(|s| s.agent_engine.as_str())
                    .unwrap_or("local"),
            )
        })
        .unwrap_or(Value::Null);
    Json(json!({
        "sessions": sessions,
        "activeSession": active,
        "events": events,
        "todos": todos,
        "run": run,
        "models": models,
        "latestSeq": latest_seq,
        "chatFolders": state.folders.list(),
        "project": project,
        "agents": state.codex.agents_json(),
        "goal": goal,
        "account": state.cloud.account_summary(),
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
        std::env::set_var("FORGE_AGENT_DEV_MOCK", "1");
        let v = models_json(&crate::cloud::CloudService::new());
        let arr = v["models"].as_array().unwrap();
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[0]["id"], "deepseek-chat");
        assert_eq!(arr[0]["availability"], "needs-key");
        assert_eq!(arr[1]["id"], "mock");
        assert_eq!(arr[2]["id"], "openai-compat");
        assert_eq!(arr[2]["provider"], "openai-compat");
        assert_eq!(arr[2]["availability"], "needs-key");
        assert_eq!(arr[2]["label"], "openai-compatible(未配置)");
        assert_eq!(arr[3]["id"], "gemini-3.8-flash");
        assert_eq!(arr[3]["provider"], "antigravity");
        assert_eq!(arr[3]["availability"], "needs-config");
        assert_eq!(arr[4]["id"], "gemini-3.8-pro");
        assert_eq!(arr[4]["provider"], "antigravity");
        assert_eq!(arr[4]["availability"], "needs-config");
        // 规格能力面随条目下发(client ModelPicker 的三个子菜单全靠它驱动):
        // deepseek 收不到 reasoning_effort → effortOptions 空 + 单档窗口;
        // openai-compat 是自配渠道 → 五档 effort + 多档窗口。
        assert_eq!(arr[0]["supportsThinking"], true);
        assert_eq!(arr[0]["effortOptions"].as_array().unwrap().len(), 0);
        assert_eq!(arr[0]["contextOptions"].as_array().unwrap().len(), 1);
        assert_eq!(arr[1]["supportsThinking"], false);
        assert_eq!(arr[2]["effortOptions"].as_array().unwrap().len(), 5);
        assert!(arr[2]["contextOptions"].as_array().unwrap().len() > 1);
        assert_eq!(arr[3]["supportsThinking"], true);
        assert_eq!(arr[3]["effortOptions"].as_array().unwrap().len(), 5);
        assert_eq!(v["defaultModelId"], "openai-compat");
        // 配齐腿:config JSON + keystore → available + label=model 名;全文无 key 子串。
        std::fs::write(
            dir.join("llm-openai-compat.json"),
            r#"{"base_url":"http://127.0.0.1:1","model":"qwen2.5-7b"}"#,
        )
        .unwrap();
        let secret = "sk-test-oai-REDLINE-snapshot";
        gend::keystore::set_key("openai-compat", secret).unwrap();
        let v2 = models_json(&crate::cloud::CloudService::new());
        let arr2 = v2["models"].as_array().unwrap();
        assert_eq!(arr2[2]["availability"], "available");
        assert_eq!(arr2[2]["label"], "qwen2.5-7b");
        assert!(!v2.to_string().contains(secret), "models 面泄漏密钥(R-5)");
        assert!(!v2.to_string().contains("sk-"), "models 面含 sk- 串(R-5)");
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::env::remove_var("FORGE_AGENT_DEV_MOCK");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Antigravity 渠道可用性注入与 Redline R-5 密钥不出
    #[test]
    fn models_antigravity_availability_propagation() {
        let _g = llm::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::remove_var("FORGE_ANTIGRAVITY_API_KEY");
        let dir = std::env::temp_dir().join(format!(
            "agentd-snap-ag-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        crate::antigravity::clear_probe_cache();

        // 1. 未配置态 -> needs-config
        let v1 = models_json(&crate::cloud::CloudService::new());
        let arr1 = v1["models"].as_array().unwrap();
        let flash1 = arr1.iter().find(|m| m["id"] == "gemini-3.8-flash").unwrap();
        assert_eq!(flash1["availability"], "needs-config");

        // 2. 配置齐备态 -> available (R-5: 密钥永不进入 snapshot)
        let secret = "sk-antigravity-snapshot-secret-998877";
        std::fs::write(
            dir.join("llm-antigravity.json"),
            r#"{"baseUrl":"https://proxy.example.com","model":"gemini-3.8-flash","enabled":true}"#,
        ).unwrap();
        gend::keystore::set_key("antigravity", secret).unwrap();

        let v2 = models_json(&crate::cloud::CloudService::new());
        let arr2 = v2["models"].as_array().unwrap();
        let flash2 = arr2.iter().find(|m| m["id"] == "gemini-3.8-flash").unwrap();
        assert_eq!(flash2["availability"], "available");
        assert!(!v2.to_string().contains(secret), "models snapshot 泄露密钥 (R-5 违规)");

        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&dir).ok();
        crate::antigravity::clear_probe_cache();
    }
}
