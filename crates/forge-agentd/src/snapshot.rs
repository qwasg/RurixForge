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
use crate::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    #[serde(default)]
    pub(crate) session_id: Option<String>,
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
    let deepseek_availability = if llm::deepseek_key_available() {
        "available"
    } else {
        "needs-key"
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
        "models": {
            "models": [
                { "id": "deepseek-chat", "label": "deepseek-chat", "provider": "deepseek", "availability": deepseek_availability },
                { "id": "mock", "label": "Mock provider", "provider": "mock", "availability": "available" },
            ],
            "defaultModelId": "deepseek-chat",
        },
        "latestSeq": latest_seq,
        "chatFolders": state.folders.list(),
    }))
}
