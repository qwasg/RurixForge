//! F7 wave.1 SSE 事件流(D-F7-A;参考 api/sse.rs 语义级移植):
//! GET /api/forge/sessions/{id}/events/stream?fromSeq=N —— 先发 replay 段(fromSeq 起,
//! 超窗先补一帧合成 stream.gap),再桥接 broadcast live 流;keep-alive 15s 注释帧。
//!
//! 帧格式:id=seq、event=type、data=wire JSON(与参考 to_sse 一致;gap 帧无 id)。
//! 背压:先订阅后回放,消除「回放→订阅」窗口期的丢失;live 段按 seq>已发上限 去重,
//! broadcast Lagged(慢消费者丢帧)→ 合成 stream.gap(subscriber-lagged)如实上报。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::{stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;

use crate::events::DebugEvent;
use crate::AppState;

fn to_sse(ev: &DebugEvent) -> Event {
    let data = serde_json::to_string(&ev.to_wire()).unwrap_or_default();
    Event::default()
        .id(ev.seq.to_string())
        .event(ev.event_type.clone())
        .data(data)
}

/// 合成 gap 帧(无 id:不推进客户端 lastSeq;参考同形态)。
fn gap_event(session_id: &str, reason: &str) -> Event {
    Event::default().event("stream.gap").data(
        json!({
            "sessionId": session_id,
            "type": "stream.gap",
            "channel": "logs",
            "payload": { "gap": true, "reason": reason },
        })
        .to_string(),
    )
}

#[derive(Deserialize)]
pub struct StreamQuery {
    #[serde(default, rename = "fromSeq")]
    from_seq: i64,
}

/// GET /api/forge/sessions/{id}/events/stream?fromSeq=N。
pub async fn session_event_stream(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<StreamQuery>,
) -> Response {
    if state.sessions.get(&id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "SESSION_NOT_FOUND", "message": format!("会话不存在: {id}") } })),
        )
            .into_response();
    }
    // 先订阅后回放:回放期间到达的事件进 rx 缓冲,live 段按 seq 去重,不丢不重。
    let rx = state.events.subscribe(&id);
    let (backlog, gap) = state.events.replay_since(&id, q.from_seq);
    let mut last = q.from_seq;
    let mut head: Vec<Result<Event, Infallible>> = Vec::with_capacity(backlog.len() + 1);
    if gap {
        head.push(Ok(gap_event(&id, "replay-window-exceeded")));
    }
    for ev in backlog {
        last = last.max(ev.seq);
        head.push(Ok(to_sse(&ev)));
    }
    let backlog_stream = stream::iter(head);
    let live = stream::unfold((rx, id, last), |(mut rx, sid, last)| async move {
        let mut last = last;
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if ev.seq <= last {
                        continue; // 回放段已覆盖
                    }
                    last = ev.seq;
                    return Some((Ok::<Event, Infallible>(to_sse(&ev)), (rx, sid, last)));
                }
                // 慢消费者丢帧:如实合成 gap,不静默跳过(参考同语义)。
                Err(RecvError::Lagged(_n)) => {
                    return Some((
                        Ok::<Event, Infallible>(gap_event(&sid, "subscriber-lagged")),
                        (rx, sid, last),
                    ));
                }
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(backlog_stream.chain(live))
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response()
}
