//! 手动压缩上下文:`POST /api/forge/sessions/{id}/compact`(Composer 上下文面板的「压缩上下文」)。
//!
//! - 本地引擎:用会话当前模型把摘要锚点之后的全部已结束轮次连同旧摘要合并成新摘要
//!   ([`crate::history::compact_now`]),与自动压缩共用 summaries.json;此后各轮从摘要续。
//! - Codex 引擎:对已绑定线程发原生 `thread/compact/start`,等压缩轮结束
//!   ([`crate::codex::turn::compact_thread`])。
//!
//! 压缩期间占住会话运行锁(与起轮互斥,抢发的一方拿到 SESSION_BUSY;Stop 走 runs 取消);
//! 成功后落一条 `context.compacted` 事件:时间线据此画分隔线,上下文计量面从分隔线之后重新估算。

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::agent::CancelToken;
use crate::events::EventDraft;
use crate::history::{CompactError, CompactReport};
use crate::sessions::DebugSession;
use crate::AppState;

pub async fn compact_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let Some(session) = state.sessions.get(&id) else {
        return error(
            StatusCode::NOT_FOUND,
            "SESSION_NOT_FOUND",
            &format!("会话不存在: {id}"),
        );
    };
    let (run, token) = state.runs.begin(&id, "context_compact");
    if let Err(busy) = state.sessions.claim_active_run(&id, &run.id) {
        state.runs.finish(&run.id, "failed");
        return error(
            StatusCode::CONFLICT,
            "SESSION_BUSY",
            &format!("会话正在运行({busy}),等这一轮结束再压缩"),
        );
    }
    let engine = if session.is_codex() { "codex" } else { "local" };
    let result = if session.is_codex() {
        crate::codex::turn::compact_thread(&state, &session, &token)
            .await
            .map(|()| None)
    } else {
        compact_local(&state, &session, &token).await.map(Some)
    };
    let status = match &result {
        Ok(_) => "completed",
        Err(CompactError::Cancelled) => "cancelled",
        Err(_) => "failed",
    };
    state.runs.finish(&run.id, status);
    state.sessions.release_active_run(&id, &run.id);
    match result {
        Ok(report) => {
            let mut payload = json!({ "engine": engine, "manual": true });
            if let Some(r) = report {
                payload["turns"] = json!(r.turns);
                payload["tokensBefore"] = json!(r.tokens_before);
                payload["tokensAfter"] = json!(r.tokens_after);
            }
            state
                .events
                .emit(EventDraft::new(&id, "context.compacted", "agent").payload(payload.clone()));
            (StatusCode::OK, Json(payload)).into_response()
        }
        Err(CompactError::Nothing) => error(
            StatusCode::CONFLICT,
            "AGENT_COMPACT_NOTHING",
            "还没有可压缩的对话(或刚压缩过)",
        ),
        Err(CompactError::Cancelled) => {
            error(StatusCode::CONFLICT, "AGENT_COMPACT_CANCELLED", "压缩已取消")
        }
        Err(CompactError::Failed(message)) => {
            error(StatusCode::BAD_GATEWAY, "AGENT_COMPACT_FAILED", &message)
        }
    }
}

/// 本地引擎:用会话此刻选中的模型(同 ask:execute 的渠道与规格解析)做一次摘要调用。
async fn compact_local(
    state: &Arc<AppState>,
    session: &DebugSession,
    token: &CancelToken,
) -> Result<CompactReport, CompactError> {
    let provider =
        crate::llm::bind_chat_session(crate::agent::provider_for_session(session), &session.id);
    if matches!(provider, crate::llm::Provider::Mock) {
        return Err(CompactError::Failed(
            "mock 渠道没有真实模型,无法生成摘要".to_string(),
        ));
    }
    let resolved = crate::agent::resolved_request_spec(session, &provider);
    let spec = crate::llm::RequestSpec {
        model: resolved.model,
        reasoning_effort: resolved.reasoning_effort,
        thinking_enabled: resolved.thinking_enabled,
    };
    let step = crate::llm::step_for_provider(&provider, &spec);
    let events = state.events.persisted(&session.id);
    let summaries_path = state.sessions.path().with_file_name("summaries.json");
    let work = crate::history::compact_now(&session.id, &events, &summaries_path, step.as_ref());
    tokio::pin!(work);
    // 摘要调用本身不可中断:Stop 时放弃等待,迟到的结果随 future 一起丢弃,不落盘。
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if token.is_cancelled() {
                    return Err(CompactError::Cancelled);
                }
            }
        }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}
