//! Durable, engine-neutral team membership, task board and addressed inboxes.
//! The host owns execution and wakeups; model JSON never supplies sender identity.

use crate::{
    events::{new_id, now_rfc3339, EventDraft},
    AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

const MAX_MESSAGE_CHARS: usize = 16_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentParticipant {
    pub id: String,
    pub session_id: String,
    pub parent_agent_id: Option<String>,
    pub team_id: Option<String>,
    pub name: String,
    pub role: String,
    pub engine: String,
    pub status: String,
    pub active_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct AgentRegistration {
    pub id: String,
    pub session_id: String,
    pub parent_agent_id: Option<String>,
    pub team_id: Option<String>,
    pub name: String,
    pub role: String,
    pub engine: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub annotations: Vec<crate::editor::Annotation>,
    pub id: String,
    pub session_id: String,
    pub from_agent_id: Option<String>,
    pub to_agent_id: String,
    pub source: String,
    #[serde(default = "default_message_kind")]
    pub kind: String,
    #[serde(default = "default_message_wake")]
    pub wake: bool,
    pub text: String,
    pub client_message_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub injected_at: Option<String>,
    pub run_id: Option<String>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
}
fn default_message_kind() -> String {
    "message".into()
}
fn default_message_wake() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMessageRequest {
    #[serde(default)]
    pub annotations: Vec<crate::editor::Annotation>,
    pub text: String,
    pub client_message_id: Option<String>,
    pub expected_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageLease {
    pub id: String,
    pub messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamTask {
    pub id: String,
    pub team_id: String,
    pub title: String,
    pub prompt: String,
    pub role: Option<String>,
    pub stage: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
    pub owner_agent_id: Option<String>,
    pub status: String,
    pub result: Option<String>,
    pub attempts: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamState {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub status: String,
    pub leader_agent_id: String,
    pub member_agent_ids: Vec<String>,
    pub revision: u64,
    pub max_parallel: usize,
    pub max_fix_rounds: u32,
    pub fix_rounds: u32,
    pub tasks: Vec<TeamTask>,
    pub created_at: String,
    pub updated_at: String,
}

fn default_parallel() -> usize {
    4
}
fn default_fix_rounds() -> u32 {
    3
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTeamRequest {
    pub name: String,
    #[serde(default = "default_parallel")]
    pub max_parallel: usize,
    #[serde(default = "default_fix_rounds")]
    pub max_fix_rounds: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollaborationError {
    pub code: String,
    pub message: String,
}
impl std::fmt::Display for CollaborationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for CollaborationError {}
impl IntoResponse for CollaborationError {
    fn into_response(self) -> Response {
        let status = if self.code.ends_with("NOT_FOUND") {
            StatusCode::NOT_FOUND
        } else if self.code == "AGENT_UNAUTHORIZED" {
            StatusCode::UNAUTHORIZED
        } else if self.code.ends_with("FORBIDDEN") {
            StatusCode::FORBIDDEN
        } else if self.code == "COLLABORATION_IO" {
            StatusCode::INTERNAL_SERVER_ERROR
        } else if matches!(self.code.as_str(), "INVALID_INPUT" | "TEAM_INVALID_GRAPH") {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::CONFLICT
        };
        (status, Json(json!({"error": self}))).into_response()
    }
}
fn error(code: &str, message: impl Into<String>) -> CollaborationError {
    CollaborationError {
        code: code.into(),
        message: message.into(),
    }
}
type Result<T> = std::result::Result<T, CollaborationError>;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[serde(default)]
    participants: Vec<AgentParticipant>,
    #[serde(default)]
    messages: Vec<AgentMessage>,
    #[serde(default)]
    teams: Vec<TeamState>,
    #[serde(default)]
    histories: HashMap<String, Vec<Value>>,
    #[serde(default)]
    member_configs: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct AgentAuth {
    pub session_id: String,
    pub agent_id: String,
    pub run_id: String,
}
#[derive(Clone)]
struct Capability {
    session_id: String,
    agent_id: String,
    run_id: Option<String>,
}
#[derive(Default)]
struct Runtime {
    endpoint: Option<String>,
    capabilities: HashMap<String, Capability>,
}

pub struct CollaborationStore {
    path: PathBuf,
    inner: Mutex<Document>,
    runtime: Mutex<Runtime>,
    notify: Arc<Notify>,
    load_error: Option<String>,
}

pub fn root_id(session_id: &str) -> String {
    format!("agent-root-{session_id}")
}

impl CollaborationStore {
    pub fn load(path: PathBuf) -> Self {
        let (mut doc, load_error) = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Document>(&bytes) {
                Ok(d) => (d, None),
                Err(e) => (
                    Document::default(),
                    Some(format!("协作状态损坏，保留原文件: {e}")),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Document::default(), None),
            Err(e) => (Document::default(), Some(e.to_string())),
        };
        // A lease records delivery intent, not proof that the model consumed it.
        for message in &mut doc.messages {
            if message.status == "leased" {
                message.status = "recoveryRequired".into();
                message.error =
                    Some("进程重启时消息正在注入，送达状态未知，请核实后重新发送".into());
            }
        }
        for agent in &mut doc.participants {
            if agent.active_run_id.take().is_some() && agent.status != "stopped" {
                agent.status = "recoveryRequired".into();
            }
        }
        for team in &mut doc.teams {
            let interrupted_member = doc.participants.iter().any(|p| {
                p.team_id.as_deref() == Some(team.id.as_str()) && p.status == "recoveryRequired"
            });
            if !["completed", "stopped"].contains(&team.status.as_str())
                && (team.status == "active"
                    || interrupted_member
                    || team.tasks.iter().any(|t| t.status == "running"))
            {
                team.status = "recoveryRequired".into();
            }
        }
        Self {
            path,
            inner: Mutex::new(doc),
            runtime: Mutex::new(Runtime::default()),
            notify: Arc::new(Notify::new()),
            load_error,
        }
    }

    fn change<T>(&self, change: impl FnOnce(&mut Document) -> Result<T>) -> Result<T> {
        if let Some(e) = &self.load_error {
            return Err(error("COLLABORATION_IO", e));
        }
        let mut current = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = current.clone();
        let result = change(&mut next)?;
        if serde_json::to_value(&next).ok() == serde_json::to_value(&*current).ok() {
            return Ok(result);
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| error("COLLABORATION_IO", e.to_string()))?;
        }
        let bytes = serde_json::to_vec_pretty(&next)
            .map_err(|e| error("COLLABORATION_IO", e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)
            .and_then(|_| std::fs::rename(&tmp, &self.path))
            .map_err(|e| error("COLLABORATION_IO", e.to_string()))?;
        *current = next;
        drop(current);
        self.notify.notify_waiters();
        self.notify.notify_one();
        Ok(result)
    }

    pub fn notifier(&self) -> Arc<Notify> {
        self.notify.clone()
    }
    pub fn set_endpoint(&self, endpoint: String) {
        self.runtime.lock().unwrap().endpoint = Some(endpoint);
    }
    pub fn endpoint(&self) -> Option<String> {
        self.runtime.lock().unwrap().endpoint.clone()
    }
    pub fn participant(&self, id: &str) -> Option<AgentParticipant> {
        self.inner
            .lock()
            .unwrap()
            .participants
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }
    pub fn agents(&self, sid: &str) -> Vec<AgentParticipant> {
        self.inner
            .lock()
            .unwrap()
            .participants
            .iter()
            .filter(|p| p.session_id == sid)
            .cloned()
            .collect()
    }
    pub fn messages(&self, agent_id: &str) -> Vec<AgentMessage> {
        self.inner
            .lock()
            .unwrap()
            .messages
            .iter()
            .filter(|m| m.to_agent_id == agent_id || m.from_agent_id.as_deref() == Some(agent_id))
            .cloned()
            .collect()
    }
    pub fn has_queued_messages(&self, agent_id: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.to_agent_id == agent_id && m.status == "queued")
    }
    /// Only explicit user/agent messages schedule an idle recipient. Automatic
    /// receipts remain available to an already-running turn without ping-pong.
    pub fn has_wake_messages(&self, agent_id: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.to_agent_id == agent_id && m.status == "queued" && m.wake)
    }
    pub fn history(&self, agent_id: &str) -> Vec<Value> {
        self.inner
            .lock()
            .unwrap()
            .histories
            .get(agent_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn member_config(&self, agent_id: &str) -> Option<Value> {
        self.inner
            .lock()
            .unwrap()
            .member_configs
            .get(agent_id)
            .cloned()
    }

    pub fn save_member_config(&self, agent_id: &str, config: Value) -> Result<()> {
        self.change(|d| {
            let agent = require_agent(d, agent_id)?;
            if agent.role != "member" {
                return Err(error("TEAM_MEMBER_FORBIDDEN", "只有持久团队成员有配置"));
            }
            // Repeated spawn of a named member keeps its original identity and
            // instructions. New guidance belongs in the addressed mailbox.
            d.member_configs.entry(agent_id.into()).or_insert(config);
            Ok(())
        })
    }
    pub fn save_history(&self, agent_id: &str, messages: Vec<Value>) -> Result<()> {
        self.change(|d| {
            require_agent(d, agent_id)?;
            d.histories.insert(agent_id.into(), messages);
            Ok(())
        })
    }

    pub fn register(&self, registration: AgentRegistration) -> Result<AgentParticipant> {
        self.change(|d| {
            let r = &registration;
            if r.id.trim().is_empty()
                || r.session_id.trim().is_empty()
                || r.name.trim().is_empty()
                || !["root", "member", "subagent"].contains(&r.role.as_str())
                || !["local", "codex"].contains(&r.engine.as_str())
            {
                return Err(error("INVALID_INPUT", "无效 agent 身份"));
            }
            if r.role == "root"
                && (r.id != root_id(&r.session_id)
                    || r.parent_agent_id.is_some()
                    || r.team_id.is_some())
            {
                return Err(error("INVALID_INPUT", "root 身份须使用会话稳定 ID"));
            }
            if let Some(parent) = &r.parent_agent_id {
                if require_agent(d, parent)?.session_id != r.session_id {
                    return Err(error("AGENT_SCOPE_FORBIDDEN", "父 agent 不在同一会话"));
                }
            }
            if let Some(team_id) = &r.team_id {
                let team = require_team(d, team_id)?;
                if team.session_id != r.session_id
                    || !["active", "paused", "recoveryRequired"].contains(&team.status.as_str())
                {
                    return Err(error("TEAM_CLOSED", "团队已关闭或不在当前会话"));
                }
                if d.participants.iter().any(|p| {
                    p.team_id.as_deref() == Some(team_id) && p.id != r.id && p.name == r.name
                }) {
                    return Err(error("AGENT_NAME_CONFLICT", "团队内成员名称不可重复"));
                }
            }
            if let Some(old) = d.participants.iter_mut().find(|p| p.id == r.id) {
                if old.session_id != r.session_id
                    || old.role != r.role
                    || old.parent_agent_id != r.parent_agent_id
                    || old.team_id != r.team_id
                {
                    return Err(error(
                        "AGENT_IDENTITY_CONFLICT",
                        "不能改变已注册 agent 的所属关系",
                    ));
                }
                if old.active_run_id.is_some() && old.engine != r.engine {
                    return Err(error("AGENT_BUSY", "运行时不能切换 agent 引擎"));
                }
                old.engine = r.engine.clone();
                old.name = r.name.clone();
                old.updated_at = now_rfc3339();
                return Ok(old.clone());
            }
            let at = now_rfc3339();
            let participant = AgentParticipant {
                id: r.id.clone(),
                session_id: r.session_id.clone(),
                parent_agent_id: r.parent_agent_id.clone(),
                team_id: r.team_id.clone(),
                name: r.name.clone(),
                role: r.role.clone(),
                engine: r.engine.clone(),
                status: "idle".into(),
                active_run_id: None,
                created_at: at.clone(),
                updated_at: at,
            };
            d.participants.push(participant.clone());
            if let Some(id) = &r.team_id {
                let team = team_mut(d, id)?;
                team.member_agent_ids.push(r.id.clone());
                touch(team);
            }
            Ok(participant)
        })
    }

    pub fn begin_run(&self, agent_id: &str, run_id: &str) -> Result<AgentParticipant> {
        self.change(|d| {
            let agent = require_agent(d, agent_id)?;
            if agent.status == "stopped" {
                return Err(error("AGENT_CLOSED", "agent 已关闭"));
            }
            if run_id.trim().is_empty() {
                return Err(error("INVALID_INPUT", "runId 不可空"));
            }
            if let Some(team_id) = &agent.team_id {
                let team = require_team(d, team_id)?;
                if team.status != "active" {
                    return Err(error("TEAM_NOT_ACTIVE", "团队未运行"));
                }
                let running = d
                    .participants
                    .iter()
                    .filter(|p| {
                        p.team_id.as_deref() == Some(team_id)
                            && p.active_run_id.is_some()
                            && p.id != agent_id
                    })
                    .count();
                if running >= team.max_parallel {
                    return Err(error("TEAM_PARALLEL_LIMIT", "团队并发名额已满"));
                }
            }
            let p = agent_mut(d, agent_id)?;
            if p.active_run_id.as_deref().is_some_and(|r| r != run_id) {
                return Err(error("AGENT_BUSY", "agent 已有运行轮次"));
            }
            p.active_run_id = Some(run_id.into());
            p.status = "running".into();
            p.updated_at = now_rfc3339();
            Ok(p.clone())
        })
    }

    pub fn end_run(&self, agent_id: &str, run_id: &str) -> Result<AgentParticipant> {
        self.change(|d| {
            let p = agent_mut(d, agent_id)?;
            if p.active_run_id.as_deref() != Some(run_id) {
                return Err(error("AGENT_RUN_CONFLICT", "agent 当前轮次已改变"));
            }
            p.active_run_id = None;
            if p.status != "stopped" {
                p.status = "idle".into();
            }
            p.updated_at = now_rfc3339();
            let participant = p.clone();
            for message in d.messages.iter_mut().filter(|m| {
                m.to_agent_id == agent_id
                    && m.run_id.as_deref() == Some(run_id)
                    && m.status == "leased"
            }) {
                message.status = "recoveryRequired".into();
                message.error = Some(
                    "接收轮次已结束，但没有确认模型接受消息；送达状态未知，请核实后重新发送".into(),
                );
            }
            Ok(participant)
        })
    }

    pub fn stop_agent(&self, agent_id: &str) -> Result<AgentParticipant> {
        self.change(|d| {
            let p = agent_mut(d, agent_id)?;
            p.status = "stopped".into();
            p.updated_at = now_rfc3339();
            let p = p.clone();
            fail_messages(d, agent_id, "目标 agent 已关闭");
            Ok(p)
        })
    }

    pub fn enqueue(
        &self,
        session_id: &str,
        sender_agent_id: Option<&str>,
        recipient_agent_id: &str,
        req: &SendMessageRequest,
    ) -> Result<AgentMessage> {
        self.enqueue_with_policy(
            session_id,
            sender_agent_id,
            recipient_agent_id,
            req,
            "message",
            true,
        )
    }

    /// Host-generated task/activation receipt. Never exposed as model-supplied
    /// metadata, and never by itself wakes an idle participant.
    pub fn enqueue_receipt(
        &self,
        session_id: &str,
        sender_agent_id: &str,
        recipient_agent_id: &str,
        req: &SendMessageRequest,
    ) -> Result<AgentMessage> {
        self.enqueue_with_policy(
            session_id,
            Some(sender_agent_id),
            recipient_agent_id,
            req,
            "receipt",
            false,
        )
    }

    fn enqueue_with_policy(
        &self,
        session_id: &str,
        sender_agent_id: Option<&str>,
        recipient_agent_id: &str,
        req: &SendMessageRequest,
        kind: &str,
        wake: bool,
    ) -> Result<AgentMessage> {
        self.change(|d| {
            let target = require_agent(d, recipient_agent_id)?;
            if target.session_id != session_id {
                return Err(error("AGENT_SCOPE_FORBIDDEN", "消息目标不在当前会话"));
            }
            if let Some(sender) = sender_agent_id {
                let sender = require_agent(d, sender)?;
                if sender.session_id != session_id
                    || (sender.role != "root"
                        && target.role != "root"
                        && (sender.team_id != target.team_id
                            || (sender.team_id.is_none()
                                && sender.parent_agent_id != target.parent_agent_id)))
                {
                    return Err(error(
                        "AGENT_SCOPE_FORBIDDEN",
                        "只允许向同一协作范围内的 agent 发消息",
                    ));
                }
                if sender.status == "stopped" && kind != "receipt" {
                    return Err(error("AGENT_CLOSED", "发送方 agent 已关闭"));
                }
                if sender.team_id.as_deref().is_some_and(|id| {
                    require_team(d, id)
                        .is_ok_and(|team| ["stopped", "completed"].contains(&team.status.as_str()))
                }) {
                    return Err(error("TEAM_CLOSED", "消息所属团队已结束"));
                }
            }
            if let Some(key) = req
                .client_message_id
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                if let Some(prior) = d.messages.iter().find(|m| {
                    m.session_id == session_id
                        && m.from_agent_id.as_deref() == sender_agent_id
                        && m.client_message_id.as_deref() == Some(key)
                }) {
                    if prior.to_agent_id != recipient_agent_id
                        || prior.text != req.text.trim()
                        || prior.annotations != req.annotations
                        || prior.kind != kind
                    {
                        return Err(error(
                            "MESSAGE_ID_CONFLICT",
                            "clientMessageId 已用于另一条消息",
                        ));
                    }
                    return Ok(prior.clone());
                }
            }
            if target.status == "stopped" {
                return Err(error("AGENT_CLOSED", "目标 agent 已关闭"));
            }
            if let Some(team_id) = &target.team_id {
                if ["stopped", "completed"].contains(&require_team(d, team_id)?.status.as_str()) {
                    return Err(error("TEAM_CLOSED", "目标团队已关闭"));
                }
            }
            if let Some(expected) = &req.expected_run_id {
                if target.active_run_id.as_ref() != Some(expected) {
                    return Err(error("AGENT_RUN_CONFLICT", "目标轮次已改变，请刷新后重发"));
                }
            }
            if (req.text.trim().is_empty() && req.annotations.is_empty()) || req.text.chars().count() > MAX_MESSAGE_CHARS {
                return Err(error("INVALID_INPUT", "消息须为 1 至 16000 字符"));
            }
            let message = AgentMessage {
                annotations: req.annotations.clone(),
                id: new_id("amsg"),
                session_id: session_id.into(),
                from_agent_id: sender_agent_id.map(str::to_owned),
                to_agent_id: recipient_agent_id.into(),
                source: if sender_agent_id.is_some() {
                    "agent"
                } else {
                    "user"
                }
                .into(),
                kind: kind.into(),
                wake,
                text: req.text.trim().into(),
                client_message_id: req
                    .client_message_id
                    .clone()
                    .filter(|s| !s.trim().is_empty()),
                status: "queued".into(),
                created_at: now_rfc3339(),
                injected_at: None,
                run_id: None,
                error: None,
                lease_id: None,
            };
            d.messages.push(message.clone());
            Ok(message)
        })
    }

    pub fn lease_messages(
        &self,
        agent_id: &str,
        run_id: &str,
        max_chars: usize,
    ) -> Result<MessageLease> {
        // Native Codex polls at short intervals; an empty inbox must be read-only.
        {
            let d = self.inner.lock().unwrap();
            let p = require_agent(&d, agent_id)?;
            if p.active_run_id.as_deref() != Some(run_id) || p.status != "running" {
                return Err(error("AGENT_RUN_CONFLICT", "消息只能注入当前运行轮次"));
            }
            if !d.messages.iter().any(|m| {
                m.to_agent_id == agent_id && ["queued", "leased"].contains(&m.status.as_str())
            }) {
                return Ok(MessageLease {
                    id: String::new(),
                    messages: vec![],
                });
            }
        }
        self.change(|d| {
            let p = require_agent(d, agent_id)?;
            if p.active_run_id.as_deref() != Some(run_id) || p.status != "running" {
                return Err(error("AGENT_RUN_CONFLICT", "消息只能注入当前运行轮次"));
            }
            if let Some(message) = d
                .messages
                .iter()
                .find(|m| m.to_agent_id == agent_id && m.status == "leased")
            {
                return Err(error(
                    "MESSAGE_LEASE_CONFLICT",
                    format!(
                        "仍有未确认租约 {}",
                        message.lease_id.as_deref().unwrap_or_default()
                    ),
                ));
            }
            let id = new_id("lease");
            let mut messages = Vec::new();
            let mut used = 0;
            for message in d
                .messages
                .iter_mut()
                .filter(|m| m.to_agent_id == agent_id && m.status == "queued")
            {
                let size = message.text.chars().count();
                // Deliver one oversize message whole; truncating steering loses intent.
                if !messages.is_empty() && used + size > max_chars {
                    break;
                }
                if max_chars == 0 {
                    break;
                }
                used += size;
                message.status = "leased".into();
                message.lease_id = Some(id.clone());
                message.run_id = Some(run_id.into());
                messages.push(message.clone());
            }
            Ok(MessageLease { id, messages })
        })
    }

    pub fn commit_lease(&self, lease_id: &str) -> Result<Vec<AgentMessage>> {
        self.change(|d| {
            let at = now_rfc3339();
            let mut committed = Vec::new();
            for message in &mut d.messages {
                if message.lease_id.as_deref() == Some(lease_id) && message.status == "leased" {
                    message.status = "injected".into();
                    message.injected_at = Some(at.clone());
                    committed.push(message.clone());
                }
            }
            Ok(committed)
        })
    }
    pub fn release_lease(&self, lease_id: &str) -> Result<()> {
        self.change(|d| {
            for message in &mut d.messages {
                if message.lease_id.as_deref() == Some(lease_id) && message.status == "leased" {
                    message.status = "queued".into();
                    message.lease_id = None;
                    message.run_id = None;
                }
            }
            Ok(())
        })
    }
    /// Transport loss after submission cannot distinguish accepted from rejected input.
    /// Keep this visible and never automatically replay a possibly accepted instruction.
    pub fn fail_lease(&self, lease_id: &str, reason: &str) -> Result<Vec<AgentMessage>> {
        self.change(|d| {
            let mut failed = Vec::new();
            for message in &mut d.messages {
                if message.lease_id.as_deref() == Some(lease_id) && message.status == "leased" {
                    message.status = "recoveryRequired".into();
                    message.error = Some(reason.into());
                    failed.push(message.clone());
                }
            }
            Ok(failed)
        })
    }

    pub fn issue_token(&self, sid: &str, agent_id: &str, run_id: &str) -> Result<String> {
        self.issue_capability(sid, agent_id, Some(run_id))
    }
    pub fn issue_root_token(&self, sid: &str, agent_id: &str) -> Result<String> {
        if self.participant(agent_id).is_none_or(|p| p.role != "root") {
            return Err(error(
                "AGENT_UNAUTHORIZED",
                "仅 root 可获得会话级 capability",
            ));
        }
        self.issue_capability(sid, agent_id, None)
    }
    fn issue_capability(&self, sid: &str, agent_id: &str, run_id: Option<&str>) -> Result<String> {
        let p = self
            .participant(agent_id)
            .ok_or_else(|| error("AGENT_NOT_FOUND", "agent 不存在"))?;
        if p.session_id != sid
            || p.status != "running"
            || p.active_run_id.is_none()
            || run_id.is_some_and(|r| p.active_run_id.as_deref() != Some(r))
        {
            return Err(error("AGENT_UNAUTHORIZED", "capability 须绑定活动 agent"));
        }
        let mut runtime = self.runtime.lock().unwrap();
        if let Some((token, _)) = runtime.capabilities.iter().find(|(_, c)| {
            c.session_id == sid && c.agent_id == agent_id && c.run_id.as_deref() == run_id
        }) {
            return Ok(token.clone());
        }
        let material = (0..4).map(|_| new_id("cap")).collect::<Vec<_>>().join(":");
        let token = forge_util::hashutil::sha256_hex(material.as_bytes());
        runtime.capabilities.insert(
            token.clone(),
            Capability {
                session_id: sid.into(),
                agent_id: agent_id.into(),
                run_id: run_id.map(str::to_owned),
            },
        );
        Ok(token)
    }
    pub fn authenticate_token(&self, token: &str) -> Result<AgentAuth> {
        let capability = self
            .runtime
            .lock()
            .unwrap()
            .capabilities
            .get(token)
            .cloned()
            .ok_or_else(|| error("AGENT_UNAUTHORIZED", "无效或已失效 capability"))?;
        let p = self
            .participant(&capability.agent_id)
            .ok_or_else(|| error("AGENT_UNAUTHORIZED", "agent 不存在"))?;
        if p.session_id != capability.session_id
            || p.status != "running"
            || p.active_run_id.is_none()
            || capability
                .run_id
                .as_ref()
                .is_some_and(|r| p.active_run_id.as_ref() != Some(r))
        {
            return Err(error("AGENT_UNAUTHORIZED", "agent 当前没有匹配的活动轮次"));
        }
        Ok(AgentAuth {
            session_id: capability.session_id,
            agent_id: capability.agent_id,
            run_id: p.active_run_id.unwrap(),
        })
    }

    pub fn team(&self, id: &str) -> Option<TeamState> {
        self.inner
            .lock()
            .unwrap()
            .teams
            .iter()
            .find(|t| t.id == id)
            .cloned()
    }
    pub fn latest_team(&self, sid: &str) -> Option<TeamState> {
        self.inner
            .lock()
            .unwrap()
            .teams
            .iter()
            .rev()
            .find(|t| t.session_id == sid)
            .cloned()
    }
    /// Serialize session setting changes against team creation. The callback
    /// must not call back into CollaborationStore while this guard is held.
    pub fn with_inactive_team<T>(&self, sid: &str, change: impl FnOnce() -> T) -> Result<T> {
        self.with_inactive_teams(|| vec![sid.into()], change)
    }

    /// Resolve the affected sessions under the collaboration lock so workspace
    /// reassignment and team creation cannot invalidate a previously read list.
    /// Neither callback may call back into CollaborationStore.
    pub fn with_inactive_teams<T>(
        &self,
        session_ids: impl FnOnce() -> Vec<String>,
        change: impl FnOnce() -> T,
    ) -> Result<T> {
        let doc = self.inner.lock().unwrap();
        let session_ids = session_ids();
        if doc.teams.iter().any(|t| {
            session_ids.contains(&t.session_id)
                && !["stopped", "completed"].contains(&t.status.as_str())
        }) {
            return Err(error(
                "TEAM_SESSION_LOCKED",
                "会话仍有未结束团队，请先停止或完成团队",
            ));
        }
        Ok(change())
    }

    /// Discard undelivered coordination when the user truncates chat history.
    /// Keep actor identities and closed member histories for audit/reuse rules.
    /// The caller must reserve the idle session until event truncation finishes.
    pub fn reset_after_revert(&self, sid: &str) -> Result<()> {
        self.change(|d| {
            if d.teams.iter().any(|t| {
                t.session_id == sid && !["stopped", "completed"].contains(&t.status.as_str())
            }) {
                return Err(error("TEAM_REVERT_BLOCKED", "团队尚未结束，不能回退会话"));
            }
            let root = root_id(sid);
            if d.participants
                .iter()
                .any(|p| p.session_id == sid && p.active_run_id.is_some())
            {
                return Err(error(
                    "SESSION_BUSY",
                    "主 agent 或后台子 agent 仍在执行，不能回退会话",
                ));
            }
            for message in d.messages.iter_mut().filter(|m| {
                m.session_id == sid && ["queued", "leased"].contains(&m.status.as_str())
            }) {
                message.status = "failed".into();
                message.error = Some("会话已回退，旧上下文中的待投递消息已取消".into());
            }
            d.histories.remove(&root);
            Ok(())
        })
    }
    pub fn has_open_team(&self, sid: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .teams
            .iter()
            .any(|t| t.session_id == sid && !["stopped", "completed"].contains(&t.status.as_str()))
    }
    /// Session deletion stops durable actors and visibly closes undelivered mail.
    /// The caller cancels physical runs before removing the session/event log.
    pub fn close_session(&self, sid: &str) -> Result<()> {
        self.change(|d| {
            let ids: Vec<String> = d
                .participants
                .iter()
                .filter(|p| p.session_id == sid)
                .map(|p| p.id.clone())
                .collect();
            for id in ids {
                let p = agent_mut(d, &id)?;
                p.status = "stopped".into();
                p.updated_at = now_rfc3339();
                fail_messages(d, &id, "会话已删除");
            }
            for team in d.teams.iter_mut().filter(|t| {
                t.session_id == sid && !["completed", "stopped"].contains(&t.status.as_str())
            }) {
                team.status = "stopped".into();
                touch(team);
            }
            Ok(())
        })
    }
    pub fn create_team(
        &self,
        sid: &str,
        leader: &str,
        req: &CreateTeamRequest,
    ) -> Result<TeamState> {
        self.change(|d| {
            let p = require_agent(d, leader)?;
            if p.session_id != sid || p.role != "root" {
                return Err(error(
                    "TEAM_LEADER_FORBIDDEN",
                    "只有当前会话主 agent 可以创建团队",
                ));
            }
            if req.name.trim().is_empty()
                || !(1..=16).contains(&req.max_parallel)
                || req.max_fix_rounds > 20
            {
                return Err(error(
                    "INVALID_INPUT",
                    "团队名称不可空，并发须为 1..16，修复上限为 0..20",
                ));
            }
            if let Some(index) = d.teams.iter().position(|t| {
                t.session_id == sid && !["stopped", "completed"].contains(&t.status.as_str())
            }) {
                let existing = &d.teams[index];
                if existing.leader_agent_id != leader {
                    return Err(error(
                        "TEAM_ALREADY_ACTIVE",
                        "当前会话已有其他 leader 的未结束团队",
                    ));
                }
                if existing.name == req.name.trim()
                    && existing.max_parallel == req.max_parallel
                    && existing.max_fix_rounds == req.max_fix_rounds
                {
                    return Ok(existing.clone());
                }
                if existing.fix_rounds != 0
                    || existing.tasks.iter().any(|t| t.attempts != 0)
                    || d.participants.iter().any(|p| {
                        p.team_id.as_deref() == Some(existing.id.as_str())
                            && p.active_run_id.is_some()
                    })
                {
                    return Err(error(
                        "TEAM_ALREADY_ACTIVE",
                        "团队已经开始执行，不能改写并发或返修约束",
                    ));
                }
                let existing = &mut d.teams[index];
                existing.name = req.name.trim().into();
                existing.max_parallel = req.max_parallel;
                existing.max_fix_rounds = req.max_fix_rounds;
                touch(existing);
                return Ok(existing.clone());
            }
            let at = now_rfc3339();
            let team = TeamState {
                id: new_id("team"),
                session_id: sid.into(),
                name: req.name.trim().into(),
                status: "active".into(),
                leader_agent_id: leader.into(),
                member_agent_ids: vec![],
                revision: 1,
                max_parallel: req.max_parallel,
                max_fix_rounds: req.max_fix_rounds,
                fix_rounds: 0,
                tasks: vec![],
                created_at: at.clone(),
                updated_at: at,
            };
            d.teams.push(team.clone());
            Ok(team)
        })
    }

    pub fn update_plan(
        &self,
        team_id: &str,
        leader_id: &str,
        expected_revision: Option<u64>,
        tasks: &Value,
    ) -> Result<TeamState> {
        self.change(|d| {
            let old = require_team(d, team_id)?;
            if old.leader_agent_id != leader_id {
                return Err(error("TEAM_LEADER_FORBIDDEN", "只有团队 leader 可修改计划"));
            }
            if !["active", "paused", "blocked", "recoveryRequired"].contains(&old.status.as_str()) {
                return Err(error("TEAM_CLOSED", "团队当前不可修改计划"));
            }
            if expected_revision.is_some_and(|r| r != old.revision) {
                return Err(error("TEAM_REVISION_CONFLICT", "计划版本已改变"));
            }
            let rows = tasks
                .as_array()
                .or_else(|| tasks.get("todos").and_then(Value::as_array))
                .ok_or_else(|| error("INVALID_INPUT", "计划须提供 todos 数组"))?;
            let members: HashSet<String> = old.member_agent_ids.iter().cloned().collect();
            let recovering = old.status == "recoveryRequired";
            let team = team_mut(d, team_id)?;
            let mut retried = false;
            for row in rows {
                let title = row["title"].as_str().unwrap_or_default().trim();
                if title.is_empty() {
                    return Err(error("INVALID_INPUT", "任务 title 不可空"));
                }
                let supplied_id = row["id"].as_str().filter(|s| !s.trim().is_empty());
                let existing = team
                    .tasks
                    .iter()
                    .position(|t| supplied_id.map_or(t.title == title, |id| t.id == id));
                let at = now_rfc3339();
                let task = match existing {
                    Some(index) => {
                        if team.tasks[index].status == "running"
                            && !(recovering && row["status"] == "queued")
                        {
                            return Err(error("TEAM_TASK_BUSY", "不能修改正在执行的任务"));
                        }
                        &mut team.tasks[index]
                    }
                    None => {
                        team.tasks.push(TeamTask {
                            id: supplied_id
                                .map(str::to_owned)
                                .unwrap_or_else(|| new_id("ttask")),
                            team_id: team_id.into(),
                            title: title.into(),
                            prompt: title.into(),
                            role: None,
                            stage: None,
                            deps: vec![],
                            owner_agent_id: None,
                            status: "queued".into(),
                            result: None,
                            attempts: 0,
                            created_at: at.clone(),
                            updated_at: at.clone(),
                        });
                        team.tasks.last_mut().unwrap()
                    }
                };
                task.title = title.into();
                if let Some(p) = row["prompt"].as_str() {
                    task.prompt = p.into();
                }
                for (field, dest) in [
                    ("role", &mut task.role),
                    ("stage", &mut task.stage),
                    ("ownerAgentId", &mut task.owner_agent_id),
                ] {
                    if let Some(v) = row.get(field) {
                        *dest = v
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_owned);
                    }
                }
                if let Some(owner) = &task.owner_agent_id {
                    if !members.contains(owner) {
                        return Err(error("TEAM_MEMBER_FORBIDDEN", "任务 owner 须为本团队成员"));
                    }
                }
                if let Some(deps) = row.get("deps") {
                    task.deps = serde_json::from_value(deps.clone())
                        .map_err(|_| error("TEAM_INVALID_GRAPH", "deps 须为字符串数组"))?;
                }
                if let Some(status) = row["status"].as_str() {
                    if status != "queued" {
                        return Err(error("INVALID_INPUT", "计划编辑只能把终态任务显式重新排队"));
                    }
                    if ["completed", "failed", "blocked", "running"].contains(&task.status.as_str())
                    {
                        retried = true;
                        task.status = "queued".into();
                        task.result = None;
                    }
                }
                task.updated_at = at;
            }
            if retried {
                if team.fix_rounds >= team.max_fix_rounds {
                    return Err(error("TEAM_FIX_LIMIT", "修复轮数已达上限"));
                }
                team.fix_rounds += 1;
            }
            validate_graph(&mut team.tasks)?;
            touch(team);
            Ok(team.clone())
        })
    }

    pub fn claim_task(
        &self,
        team_id: &str,
        agent_id: &str,
        task_id: Option<&str>,
    ) -> Result<TeamTask> {
        self.change(|d| {
            let p = require_agent(d, agent_id)?;
            if p.team_id.as_deref() != Some(team_id) || p.status == "stopped" {
                return Err(error(
                    "TEAM_MEMBER_FORBIDDEN",
                    "只有本团队活动成员可领取任务",
                ));
            }
            let team = team_mut(d, team_id)?;
            if team.status != "active" {
                return Err(error("TEAM_NOT_ACTIVE", "团队未运行"));
            }
            if let Some(task) = team
                .tasks
                .iter()
                .find(|t| t.status == "running" && t.owner_agent_id.as_deref() == Some(agent_id))
            {
                if task_id.is_none_or(|id| id == task.id) {
                    return Ok(task.clone());
                }
                return Err(error("TEAM_MEMBER_BUSY", "成员须先完成或报告当前任务"));
            }
            if team.tasks.iter().filter(|t| t.status == "running").count() >= team.max_parallel {
                return Err(error("TEAM_PARALLEL_LIMIT", "团队任务并发已满"));
            }
            let ready = ready_task_ids(team);
            let task = team
                .tasks
                .iter_mut()
                .find(|t| {
                    task_id.is_none_or(|id| t.id == id)
                        && ready.contains(&t.id)
                        && t.owner_agent_id
                            .as_deref()
                            .is_none_or(|owner| owner == agent_id)
                })
                .ok_or_else(|| {
                    error(
                        "TEAM_TASK_NOT_READY",
                        "没有满足依赖、阶段及分配条件的可领取任务",
                    )
                })?;
            task.owner_agent_id = Some(agent_id.into());
            task.status = "running".into();
            task.attempts += 1;
            task.updated_at = now_rfc3339();
            let out = task.clone();
            touch(team);
            Ok(out)
        })
    }

    /// Host-only rollback when a claimed worker never began execution. This
    /// keeps a pause or occupied slot from counting as a failed task attempt.
    pub fn defer_claim(&self, team_id: &str, agent_id: &str, task_id: &str) -> Result<TeamTask> {
        self.change(|d| {
            let participant = require_agent(d, agent_id)?;
            if participant.team_id.as_deref() != Some(team_id) {
                return Err(error(
                    "TEAM_MEMBER_FORBIDDEN",
                    "只能退回本团队成员领取的任务",
                ));
            }
            if participant.active_run_id.is_some() {
                return Err(error("TEAM_MEMBER_BUSY", "成员已经开始执行，不能退回领取"));
            }
            let team = team_mut(d, team_id)?;
            if ["stopped", "completed"].contains(&team.status.as_str()) {
                return Err(error("TEAM_CLOSED", "团队已经结束"));
            }
            let task = team
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or_else(|| error("TEAM_TASK_NOT_FOUND", "任务不存在"))?;
            if task.owner_agent_id.as_deref() != Some(agent_id) {
                return Err(error("TEAM_MEMBER_FORBIDDEN", "只能退回自己领取的任务"));
            }
            if task.status != "running" {
                return Err(error("TEAM_TASK_NOT_RUNNING", "任务当前不在执行"));
            }
            task.status = "queued".into();
            task.attempts = task.attempts.saturating_sub(1);
            task.updated_at = now_rfc3339();
            let result = task.clone();
            touch(team);
            Ok(result)
        })
    }

    pub fn report_task(
        &self,
        team_id: &str,
        agent_id: &str,
        task_id: &str,
        status: &str,
        result: &str,
    ) -> Result<TeamTask> {
        self.change(|d| {
            let team = team_mut(d, team_id)?;
            if !["active", "paused", "blocked"].contains(&team.status.as_str()) {
                return Err(error("TEAM_NOT_ACTIVE", "团队不接受任务结果"));
            }
            if !["completed", "failed"].contains(&status) {
                return Err(error("INVALID_INPUT", "结果状态只能为 completed 或 failed"));
            }
            let task = team
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or_else(|| error("TEAM_TASK_NOT_FOUND", "任务不存在"))?;
            if task.owner_agent_id.as_deref() != Some(agent_id) {
                return Err(error("TEAM_MEMBER_FORBIDDEN", "只能报告自己领取的任务"));
            }
            if task.status != "running" {
                return Err(error("TEAM_TASK_NOT_RUNNING", "任务当前不在执行"));
            }
            task.status = status.into();
            task.result = Some(result.into());
            task.updated_at = now_rfc3339();
            let out = task.clone();
            propagate_failures(&mut team.tasks);
            touch(team);
            Ok(out)
        })
    }

    pub fn control_team(&self, team_id: &str, action: &str) -> Result<TeamState> {
        self.change(|d| {
            if action == "complete" {
                let team = require_team(d, team_id)?;
                if d.participants
                    .iter()
                    .any(|p| p.team_id.as_deref() == Some(team_id) && p.active_run_id.is_some())
                {
                    return Err(error("TEAM_BUSY", "团队成员仍在执行，不能提前完成团队"));
                }
                if d.messages.iter().any(|m| {
                    (m.to_agent_id == team.leader_agent_id
                        || team.member_agent_ids.contains(&m.to_agent_id))
                        && ["queued", "leased"].contains(&m.status.as_str())
                }) {
                    return Err(error(
                        "TEAM_MESSAGES_PENDING",
                        "团队仍有待处理或未确认的消息",
                    ));
                }
            }
            let team = team_mut(d, team_id)?;
            if ["stopped", "completed"].contains(&team.status.as_str()) {
                return Err(error("TEAM_CLOSED", "团队已经结束"));
            }
            match action {
                "pause" => team.status = "paused".into(),
                "resume" => {
                    if team.status == "recoveryRequired"
                        && team.tasks.iter().any(|t| t.status == "running")
                    {
                        return Err(error(
                            "TEAM_RECOVERY_REQUIRED",
                            "上次运行任务的结果未知，须显式重新排队后恢复",
                        ));
                    }
                    team.status = "active".into();
                }
                "stop" => team.status = "stopped".into(),
                "block" => team.status = "blocked".into(),
                "recover" => team.status = "recoveryRequired".into(),
                "complete" => {
                    if team.tasks.is_empty() || team.tasks.iter().any(|t| t.status != "completed") {
                        return Err(error("TEAM_INCOMPLETE", "仍有未完成任务"));
                    }
                    team.status = "completed".into();
                }
                _ => return Err(error("INVALID_INPUT", "未知团队操作")),
            }
            touch(team);
            let out = team.clone();
            if action == "resume" {
                for participant in d.participants.iter_mut().filter(|p| {
                    p.team_id.as_deref() == Some(team_id)
                        && p.status == "recoveryRequired"
                        && p.active_run_id.is_none()
                }) {
                    participant.status = "idle".into();
                    participant.updated_at = now_rfc3339();
                }
            }
            if ["stopped", "completed"].contains(&out.status.as_str()) {
                for id in &out.member_agent_ids {
                    if let Some(p) = d.participants.iter_mut().find(|p| p.id == *id) {
                        p.status = "stopped".into();
                        p.updated_at = now_rfc3339();
                    }
                    fail_messages(d, id, "团队已结束");
                }
                // Closing a team also closes its queued outbound coordination;
                // preserve unrelated messages and user steering addressed to root.
                for message in d.messages.iter_mut().filter(|m| {
                    m.from_agent_id
                        .as_ref()
                        .is_some_and(|id| out.member_agent_ids.contains(id))
                        && ["queued", "leased"].contains(&m.status.as_str())
                }) {
                    let unknown = message.status == "leased";
                    message.status = if unknown {
                        "recoveryRequired"
                    } else {
                        "failed"
                    }
                    .into();
                    message.error = Some(
                        if unknown {
                            "团队结束时消息正在注入，送达状态未知"
                        } else {
                            "消息所属团队已结束"
                        }
                        .into(),
                    );
                }
            }
            Ok(out)
        })
    }

    pub fn set_team_status(&self, team_id: &str, status: &str) -> Result<TeamState> {
        let action = match status {
            "active" => "resume",
            "paused" => "pause",
            "stopped" => "stop",
            "completed" => "complete",
            "blocked" => "block",
            "recoveryRequired" => "recover",
            _ => return Err(error("INVALID_INPUT", "未知团队状态")),
        };
        self.control_team(team_id, action)
    }

    pub fn dispatch_tool(
        &self,
        sid: &str,
        agent_id: &str,
        name: &str,
        args: &Value,
    ) -> Result<Value> {
        let agent = self
            .participant(agent_id)
            .ok_or_else(|| error("AGENT_NOT_FOUND", "agent 不存在"))?;
        if agent.session_id != sid || agent.status == "stopped" {
            return Err(error("AGENT_SCOPE_FORBIDDEN", "agent 不属于当前活动会话"));
        }
        if name == "agent_list" {
            return Ok(
                json!({"agents":self.agents(sid).into_iter().filter(|p| agent.role == "root" || p.role == "root" || (p.team_id == agent.team_id && (agent.team_id.is_some() || p.parent_agent_id == agent.parent_agent_id))).collect::<Vec<_>>()}),
            );
        }
        if name == "send_message" {
            let recipient = args["toAgentId"]
                .as_str()
                .or_else(|| args["recipient"].as_str())
                .ok_or_else(|| error("INVALID_INPUT", "toAgentId required"))?;
            let req = SendMessageRequest {
                annotations: serde_json::from_value(args.get("annotations").cloned().unwrap_or(json!([]))).map_err(|e|error("INVALID_INPUT",e.to_string()))?,
                text: args["text"].as_str().unwrap_or_default().into(),
                client_message_id: args["clientMessageId"].as_str().map(str::to_owned),
                expected_run_id: args["expectedRunId"].as_str().map(str::to_owned),
            };
            return Ok(json!({"message": self.enqueue(sid,Some(agent_id),recipient,&req)?}));
        }
        if name == "team_create" {
            let req = serde_json::from_value::<CreateTeamRequest>(args.clone())
                .map_err(|e| error("INVALID_INPUT", e.to_string()))?;
            return Ok(json!({"team":self.create_team(sid,agent_id,&req)?}));
        }
        let team_id = args["teamId"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| agent.team_id.clone())
            .or_else(|| self.latest_team(sid).map(|t| t.id))
            .ok_or_else(|| error("TEAM_NOT_FOUND", "当前会话没有团队"))?;
        let team = self
            .team(&team_id)
            .ok_or_else(|| error("TEAM_NOT_FOUND", "团队不存在"))?;
        if team.session_id != sid
            || (agent.role != "root" && agent.team_id.as_deref() != Some(&team_id))
        {
            return Err(error("TEAM_MEMBER_FORBIDDEN", "团队不在当前 agent 范围内"));
        }
        match name {
            "team_get" => Ok(json!({"team":team})),
            "team_task_list" => Ok(json!({"tasks":team.tasks,"revision":team.revision})),
            "team_task_claim" => {
                Ok(json!({"task":self.claim_task(&team_id,agent_id,args["taskId"].as_str())?}))
            }
            "team_task_report" => Ok(
                json!({"task":self.report_task(&team_id,agent_id,args["taskId"].as_str().unwrap_or_default(),args["status"].as_str().unwrap_or_default(),args["result"].as_str().unwrap_or_default())?}),
            ),
            "team_task_create" | "team_task_update" => {
                let mut row = args.clone();
                if name == "team_task_update" {
                    let id = args["taskId"]
                        .as_str()
                        .ok_or_else(|| error("INVALID_INPUT", "taskId required"))?;
                    let existing = team
                        .tasks
                        .iter()
                        .find(|t| t.id == id)
                        .ok_or_else(|| error("TEAM_TASK_NOT_FOUND", "任务不存在"))?;
                    row["id"] = json!(id);
                    if row.get("title").is_none() {
                        row["title"] = json!(existing.title);
                    }
                }
                Ok(
                    json!({"team":self.update_plan(&team_id,agent_id,args["expectedRevision"].as_u64(),&json!([row]))?}),
                )
            }
            "team_control" => {
                if team.leader_agent_id != agent_id {
                    return Err(error("TEAM_LEADER_FORBIDDEN", "只有 leader 可以控制团队"));
                }
                Ok(
                    json!({"team":self.control_team(&team_id,args["action"].as_str().unwrap_or_default())?}),
                )
            }
            _ => Err(error("INVALID_INPUT", format!("未知协作工具 {name}"))),
        }
    }
}

fn require_agent<'a>(d: &'a Document, id: &str) -> Result<&'a AgentParticipant> {
    d.participants
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| error("AGENT_NOT_FOUND", "agent 不存在"))
}
fn agent_mut<'a>(d: &'a mut Document, id: &str) -> Result<&'a mut AgentParticipant> {
    d.participants
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| error("AGENT_NOT_FOUND", "agent 不存在"))
}
fn require_team<'a>(d: &'a Document, id: &str) -> Result<&'a TeamState> {
    d.teams
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| error("TEAM_NOT_FOUND", "团队不存在"))
}
fn team_mut<'a>(d: &'a mut Document, id: &str) -> Result<&'a mut TeamState> {
    d.teams
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| error("TEAM_NOT_FOUND", "团队不存在"))
}
fn touch(team: &mut TeamState) {
    team.revision += 1;
    team.updated_at = now_rfc3339();
}
fn fail_messages(d: &mut Document, agent_id: &str, reason: &str) {
    for m in &mut d.messages {
        if m.to_agent_id == agent_id && ["queued", "leased"].contains(&m.status.as_str()) {
            let unknown = m.status == "leased";
            m.status = if unknown {
                "recoveryRequired"
            } else {
                "failed"
            }
            .into();
            m.error = Some(if unknown {
                format!("{reason}；关闭时消息正在注入，送达状态未知")
            } else {
                reason.into()
            });
        }
    }
}

fn validate_graph(tasks: &mut [TeamTask]) -> Result<()> {
    let ids: HashSet<String> = tasks.iter().map(|t| t.id.clone()).collect();
    if ids.len() != tasks.len() {
        return Err(error("TEAM_INVALID_GRAPH", "任务 ID 重复"));
    }
    let mut titles: HashMap<String, Vec<String>> = HashMap::new();
    for task in tasks.iter() {
        titles
            .entry(task.title.clone())
            .or_default()
            .push(task.id.clone());
    }
    for task in tasks.iter_mut() {
        for dep in &mut task.deps {
            if !ids.contains(dep) {
                let resolved = titles.get(dep).filter(|v| v.len() == 1).ok_or_else(|| {
                    error(
                        "TEAM_INVALID_GRAPH",
                        format!("依赖不存在或名称不唯一: {dep}"),
                    )
                })?;
                *dep = resolved[0].clone();
            }
            if dep == &task.id {
                return Err(error("TEAM_INVALID_GRAPH", "任务不能依赖自身"));
            }
        }
    }
    let mut done = HashSet::<String>::new();
    loop {
        let before = done.len();
        for task in tasks.iter() {
            if task.deps.iter().all(|d| done.contains(d)) {
                done.insert(task.id.clone());
            }
        }
        if done.len() == tasks.len() {
            return Ok(());
        }
        if done.len() == before {
            return Err(error("TEAM_INVALID_GRAPH", "任务依赖存在循环"));
        }
    }
}

/// Explicit stages follow first appearance order. Every task in earlier named
/// stages must complete; tasks without a stage are constrained by deps only.
pub fn ready_task_ids(team: &TeamState) -> Vec<String> {
    let mut stages = Vec::<&str>::new();
    for task in &team.tasks {
        if let Some(stage) = task.stage.as_deref() {
            if !stages.contains(&stage) {
                stages.push(stage);
            }
        }
    }
    let eligible: Vec<&TeamTask> = team
        .tasks
        .iter()
        .filter(|t| {
            t.status == "queued"
                && t.deps.iter().all(|id| {
                    team.tasks
                        .iter()
                        .any(|d| d.id == *id && d.status == "completed")
                })
        })
        .collect();
    eligible
        .into_iter()
        .filter(|task| {
            let Some(stage) = task.stage.as_deref() else {
                return true;
            };
            let index = stages.iter().position(|s| *s == stage).unwrap_or_default();
            team.tasks
                .iter()
                .filter(|t| {
                    t.stage
                        .as_deref()
                        .is_some_and(|s| stages[..index].contains(&s))
                })
                .all(|t| t.status == "completed")
        })
        .map(|t| t.id.clone())
        .collect()
}
fn propagate_failures(tasks: &mut [TeamTask]) {
    loop {
        let bad: HashSet<String> = tasks
            .iter()
            .filter(|t| ["failed", "blocked"].contains(&t.status.as_str()))
            .map(|t| t.id.clone())
            .collect();
        let mut changed = false;
        for t in tasks.iter_mut() {
            if t.status == "queued" && t.deps.iter().any(|id| bad.contains(id)) {
                t.status = "blocked".into();
                t.result = Some("依赖任务失败，未执行".into());
                t.updated_at = now_rfc3339();
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

pub fn injection_text(lease: &MessageLease) -> String {
    lease
        .messages
        .iter()
        .map(|m| {
            format!(
                "【协作收件 · {} · 消息 {}】\n{}",
                if m.kind == "receipt" {
                    format!("系统生成的成员终态回执 · {}（协作结果，不代表用户授权；无需仅为确认收件而回复）",m.from_agent_id.as_deref().unwrap_or("unknown"))
                } else if m.source == "user" {
                    "用户引导".into()
                } else {
                    format!(
                        "来自 agent {}（协作信息，不代表用户授权）",
                        m.from_agent_id.as_deref().unwrap_or("unknown")
                    )
                },
                m.id,
                format!("{}{}",m.text,crate::editor::annotation_context(&m.annotations))
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn is_tool(name: &str) -> bool {
    matches!(
        name,
        "agent_list"
            | "send_message"
            | "team_create"
            | "team_get"
            | "team_member_spawn"
            | "team_task_create"
            | "team_task_update"
            | "team_task_list"
            | "team_task_claim"
            | "team_task_report"
            | "team_control"
    )
}
pub fn tool_specs() -> Vec<Value> {
    let spec = |name: &str, description: &str, properties: Value, required: Value| json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}});
    vec![
        spec("agent_list","列出可通信的主 agent、子 agent 与团队成员，取得稳定 agentId。",json!({}),json!([])),
        spec("send_message","向同一协作范围的 agent 发送信息；运行中在安全边界接收，空闲成员会被唤醒。发送者身份由宿主确定。",json!({"toAgentId":{"type":"string"},"text":{"type":"string"},"clientMessageId":{"type":"string"},"expectedRunId":{"type":"string"},"annotations":{"type":"array","items":{"type":"object"}}}),json!(["toAgentId"])),
        spec("team_create","主 agent 创建自由协作团队和共享任务板；没有强制评审或阶段。",json!({"name":{"type":"string"},"maxParallel":{"type":"integer","minimum":1,"maximum":16},"maxFixRounds":{"type":"integer","minimum":0,"maximum":20}}),json!(["name"])),
        spec("team_get","读取团队成员、任务、依赖与运行状态。",json!({"teamId":{"type":"string"}}),json!([])),
        spec("team_member_spawn","主 agent 添加具名持久团队成员；后续消息和任务沿用其身份与历史。",json!({"teamId":{"type":"string"},"name":{"type":"string"},"subagent_type":{"type":"string"},"prompt":{"type":"string"}}),json!(["name","prompt"])),
        spec("team_task_create","主 agent 添加可领取的团队任务，可指定成员、依赖与阶段。",json!({"teamId":{"type":"string"},"title":{"type":"string"},"prompt":{"type":"string"},"role":{"type":"string"},"stage":{"type":"string"},"deps":{"type":"array","items":{"type":"string"}},"ownerAgentId":{"type":"string"},"expectedRevision":{"type":"integer"}}),json!(["title"])),
        spec("team_task_update","主 agent 修改未运行任务；status=queued 显式重试并消耗修复预算。",json!({"teamId":{"type":"string"},"taskId":{"type":"string"},"title":{"type":"string"},"prompt":{"type":"string"},"role":{"type":"string"},"stage":{"type":"string"},"deps":{"type":"array","items":{"type":"string"}},"ownerAgentId":{"type":"string"},"status":{"type":"string","enum":["queued"]},"expectedRevision":{"type":"integer"}}),json!(["taskId"])),
        spec("team_task_list","列出共享任务板及版本；成员仅领取依赖已完成的可用任务。",json!({"teamId":{"type":"string"}}),json!([])),
        spec("team_task_claim","原子领取指定或第一个可执行任务；不绕过依赖、成员分配和并发限制。",json!({"teamId":{"type":"string"},"taskId":{"type":"string"}}),json!([])),
        spec("team_task_report","报告自己领取任务的结果。失败应保留原因；不得把未完成任务标为 completed。",json!({"teamId":{"type":"string"},"taskId":{"type":"string"},"status":{"type":"string","enum":["completed","failed"]},"result":{"type":"string"}}),json!(["taskId","status","result"])),
        spec("team_control","主 agent 暂停、恢复、停止团队或在全部任务完成后结束团队。",json!({"teamId":{"type":"string"},"action":{"type":"string","enum":["pause","resume","stop","complete"]}}),json!(["action"])),
    ]
}

pub fn emit_participant(state: &AppState, participant: &AgentParticipant) {
    state.events.emit(
        EventDraft::new(
            &participant.session_id,
            "agent.participant.updated",
            "agent",
        )
        .payload(json!(participant)),
    );
}
pub fn emit_message(state: &AppState, message: &AgentMessage) {
    let kind = match message.status.as_str() {
        "injected" => "agent.message.injected",
        "failed" | "recoveryRequired" => "agent.message.failed",
        _ => "agent.message.queued",
    };
    state
        .events
        .emit(EventDraft::new(&message.session_id, kind, "agent").payload(json!(message)));
}
pub fn emit_team(state: &AppState, team: &TeamState) {
    state.events.emit(
        EventDraft::new(&team.session_id, "team.updated", "team").payload(json!({"team":team})),
    );
}

pub async fn list_agents(State(state): State<Arc<AppState>>, Path(sid): Path<String>) -> Response {
    if state.sessions.get(&sid).is_none() {
        return error("SESSION_NOT_FOUND", "会话不存在").into_response();
    }
    Json(json!({"agents":state.collaboration.agents(&sid)})).into_response()
}
pub async fn list_messages(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    if state.collaboration.participant(&id).is_none() {
        return error("AGENT_NOT_FOUND", "agent 不存在").into_response();
    }
    Json(json!({"messages":state.collaboration.messages(&id)})).into_response()
}
pub async fn send_user_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Response {
    let Some(agent) = state.collaboration.participant(&id) else {
        return error("AGENT_NOT_FOUND", "agent 不存在").into_response();
    };
    let session = state.sessions.get(&agent.session_id);
    let scope = state.team_runtime.last_native_context(&agent.session_id).map(|(_,scope)|scope).unwrap_or_else(||crate::scope::resolve(&state,session.as_ref().and_then(|s|s.workspace_id.as_deref()),&[],true));
    if let Err(e)=crate::editor::validate_annotations(&req.annotations,&scope){return error("INVALID_INPUT",e).into_response();}
    match state
        .collaboration
        .enqueue(&agent.session_id, None, &id, &req)
    {
        Ok(message) => {
            emit_message(&state, &message);
            Json(json!({"message":message})).into_response()
        }
        Err(e) => e.into_response(),
    }
}
pub async fn get_team(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match state.collaboration.team(&id) {
        Some(team) => Json(json!({"team":team})).into_response(),
        None => error("TEAM_NOT_FOUND", "团队不存在").into_response(),
    }
}
pub async fn latest_team(State(state): State<Arc<AppState>>, Path(sid): Path<String>) -> Response {
    if state.sessions.get(&sid).is_none() {
        return error("SESSION_NOT_FOUND", "会话不存在").into_response();
    }
    Json(json!({"team":state.collaboration.latest_team(&sid)})).into_response()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamActionRequest {
    pub action: String,
}
pub async fn patch_team(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<TeamActionRequest>,
) -> Response {
    if !["pause", "resume", "stop", "complete"].contains(&req.action.as_str()) {
        return error("INVALID_INPUT", "未知团队操作").into_response();
    }
    let runs: Vec<String> = state
        .collaboration
        .team(&id)
        .map(|t| {
            t.member_agent_ids
                .into_iter()
                .chain(std::iter::once(t.leader_agent_id))
                .filter_map(|id| {
                    state
                        .collaboration
                        .participant(&id)
                        .and_then(|p| p.active_run_id)
                })
                .collect()
        })
        .unwrap_or_default();
    match state.collaboration.control_team(&id, &req.action) {
        Ok(team) => {
            if req.action == "stop" {
                for run in runs {
                    state.runs.cancel(&run);
                    state.permissions.abandon_run(&run);
                }
            }
            emit_team(&state, &team);
            for id in &team.member_agent_ids {
                if let Some(p) = state.collaboration.participant(id) {
                    emit_participant(&state, &p);
                }
                for m in state
                    .collaboration
                    .messages(id)
                    .iter()
                    .filter(|m| m.status == "failed")
                {
                    emit_message(&state, m);
                }
            }
            Json(json!({"team":team})).into_response()
        }
        Err(e) => e.into_response(),
    }
}

async fn scoped_messages(
    State(state): State<Arc<AppState>>,
    Path((sid, id)): Path<(String, String)>,
) -> Response {
    if state
        .collaboration
        .participant(&id)
        .is_none_or(|p| p.session_id != sid)
    {
        return error("AGENT_NOT_FOUND", "当前会话中不存在此 agent").into_response();
    }
    list_messages(State(state), Path(id)).await
}
async fn scoped_send_message(
    State(state): State<Arc<AppState>>,
    Path((sid, id)): Path<(String, String)>,
    Json(req): Json<SendMessageRequest>,
) -> Response {
    if state
        .collaboration
        .participant(&id)
        .is_none_or(|p| p.session_id != sid)
    {
        return error("AGENT_NOT_FOUND", "当前会话中不存在此 agent").into_response();
    }
    send_user_message(State(state), Path(id), Json(req)).await
}
async fn scoped_team(
    State(state): State<Arc<AppState>>,
    Path((sid, id)): Path<(String, String)>,
) -> Response {
    if state
        .collaboration
        .team(&id)
        .is_none_or(|t| t.session_id != sid)
    {
        return error("TEAM_NOT_FOUND", "当前会话中不存在此团队").into_response();
    }
    get_team(State(state), Path(id)).await
}
async fn scoped_patch_team(
    State(state): State<Arc<AppState>>,
    Path((sid, id)): Path<(String, String)>,
    Json(req): Json<TeamActionRequest>,
) -> Response {
    if state
        .collaboration
        .team(&id)
        .is_none_or(|t| t.session_id != sid)
    {
        return error("TEAM_NOT_FOUND", "当前会话中不存在此团队").into_response();
    }
    patch_team(State(state), Path(id), Json(req)).await
}
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/forge/sessions/{sid}/agents", get(list_agents))
        .route(
            "/api/forge/sessions/{sid}/agents/{id}/messages",
            get(scoped_messages).post(scoped_send_message),
        )
        .route("/api/forge/sessions/{sid}/team", get(latest_team))
        .route(
            "/api/forge/sessions/{sid}/teams/{id}",
            get(scoped_team).merge(patch(scoped_patch_team)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (CollaborationStore, PathBuf, String) {
        let dir = std::env::temp_dir().join(new_id("forge-collab-test"));
        let store = CollaborationStore::load(dir.join("state.json"));
        let root = root_id("s");
        store
            .register(AgentRegistration {
                id: root.clone(),
                session_id: "s".into(),
                parent_agent_id: None,
                team_id: None,
                name: "leader".into(),
                role: "root".into(),
                engine: "local".into(),
            })
            .unwrap();
        (store, dir, root)
    }
    fn member(store: &CollaborationStore, root: &str, id: &str, team: Option<&str>) {
        store
            .register(AgentRegistration {
                id: id.into(),
                session_id: "s".into(),
                parent_agent_id: Some(root.into()),
                team_id: team.map(str::to_owned),
                name: id.into(),
                role: if team.is_some() { "member" } else { "subagent" }.into(),
                engine: "local".into(),
            })
            .unwrap();
    }
    fn team(store: &CollaborationStore, root: &str) -> TeamState {
        store
            .create_team(
                "s",
                root,
                &CreateTeamRequest {
                    name: "test".into(),
                    max_parallel: 4,
                    max_fix_rounds: 3,
                },
            )
            .unwrap()
    }
    #[test]
    fn mailbox_scopes_dedup_leases_and_restart_unknown() {
        let (s, dir, r) = setup();
        member(&s, &r, "a", None);
        member(&s, &r, "b", None);
        s.begin_run("b", "run").unwrap();
        let req = SendMessageRequest {
                annotations: Vec::new(),
            text: "请先确认接口".into(),
            client_message_id: Some("m1".into()),
            expected_run_id: Some("run".into()),
        };
        let first = s.enqueue("s", Some("a"), "b", &req).unwrap();
        assert_eq!(s.enqueue("s", Some("a"), "b", &req).unwrap().id, first.id);
        assert!(s.enqueue("other", Some("a"), "b", &req).is_err());
        let lease = s.lease_messages("b", "run", 100).unwrap();
        assert_eq!(lease.messages.len(), 1);
        assert!(injection_text(&lease).contains("不代表用户授权"));
        drop(s);
        let s = CollaborationStore::load(dir.join("state.json"));
        assert_eq!(s.messages("b")[0].status, "recoveryRequired");
        assert_eq!(s.participant("b").unwrap().status, "recoveryRequired");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn closed_target_fails_pending_and_stale_run_cannot_receive() {
        let (s, dir, r) = setup();
        member(&s, &r, "a", None);
        s.begin_run("a", "r1").unwrap();
        let req = SendMessageRequest {
                annotations: Vec::new(),
            text: "hello".into(),
            ..Default::default()
        };
        s.enqueue("s", None, "a", &req).unwrap();
        assert!(s.lease_messages("a", "r2", 100).is_err());
        s.stop_agent("a").unwrap();
        assert_eq!(s.messages("a")[0].status, "failed");
        assert!(s.enqueue("s", None, "a", &req).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn claim_is_exclusive_and_dependency_failure_propagates() {
        let (s, dir, r) = setup();
        let t = team(&s, &r);
        member(&s, &r, "a", Some(&t.id));
        member(&s, &r, "b", Some(&t.id));
        s.update_plan(
            &t.id,
            &r,
            None,
            &json!([{"id":"one","title":"one"},{"id":"two","title":"two","deps":["one"]}]),
        )
        .unwrap();
        let claimed = s.claim_task(&t.id, "a", Some("one")).unwrap();
        assert_eq!(claimed.owner_agent_id.as_deref(), Some("a"));
        assert!(s.claim_task(&t.id, "b", Some("one")).is_err());
        assert!(s.claim_task(&t.id, "b", Some("two")).is_err());
        s.report_task(&t.id, "a", "one", "failed", "missing input")
            .unwrap();
        assert_eq!(s.team(&t.id).unwrap().tasks[1].status, "blocked");
        assert!(s.control_team(&t.id, "complete").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn invalid_graph_rolls_back_and_revision_protects_updates() {
        let (s, dir, r) = setup();
        let t = team(&s, &r);
        assert!(s
            .update_plan(
                &t.id,
                &r,
                None,
                &json!([{"id":"a","title":"A","deps":["B"]},{"id":"b","title":"B","deps":["A"]}])
            )
            .is_err());
        assert!(s.team(&t.id).unwrap().tasks.is_empty());
        let t = s
            .update_plan(
                &t.id,
                &r,
                Some(t.revision),
                &json!([{"id":"a","title":"A"},{"id":"b","title":"B","deps":["A"]}]),
            )
            .unwrap();
        assert_eq!(t.tasks[1].deps, vec!["a"]);
        assert!(s.update_plan(&t.id, &r, Some(1), &json!([])).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn root_capability_survives_turns_but_not_epoch_or_idle() {
        let (s, dir, r) = setup();
        s.begin_run(&r, "r1").unwrap();
        let token = s.issue_root_token("s", &r).unwrap();
        assert_eq!(s.authenticate_token(&token).unwrap().run_id, "r1");
        s.end_run(&r, "r1").unwrap();
        assert!(s.authenticate_token(&token).is_err());
        s.begin_run(&r, "r2").unwrap();
        assert_eq!(s.authenticate_token(&token).unwrap().run_id, "r2");
        assert_eq!(s.issue_root_token("s", &r).unwrap(), token);
        let next = CollaborationStore::load(dir.join("state.json"));
        assert!(next.authenticate_token(&token).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn individual_histories_and_message_ack_are_isolated() {
        let (s, dir, r) = setup();
        member(&s, &r, "a", None);
        member(&s, &r, "b", None);
        s.save_history("a", vec![json!({"role":"assistant","content":"only-a"})])
            .unwrap();
        assert!(s.history("b").is_empty());
        s.begin_run("a", "run").unwrap();
        s.enqueue(
            "s",
            None,
            "a",
            &SendMessageRequest {
                annotations: Vec::new(),
                text: "more".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let l = s.lease_messages("a", "run", 10).unwrap();
        assert_eq!(s.commit_lease(&l.id).unwrap().len(), 1);
        assert!(s.commit_lease(&l.id).unwrap().is_empty());
        assert!(s
            .lease_messages("a", "run", 10)
            .unwrap()
            .messages
            .is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn concurrent_claim_has_exactly_one_winner() {
        let (s, dir, r) = setup();
        let t = team(&s, &r);
        member(&s, &r, "a", Some(&t.id));
        member(&s, &r, "b", Some(&t.id));
        s.update_plan(&t.id, &r, None, &json!([{"id":"one","title":"one"}]))
            .unwrap();
        let barrier = std::sync::Barrier::new(2);
        let wins = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                barrier.wait();
                s.claim_task(&t.id, "a", Some("one")).is_ok()
            });
            let b = scope.spawn(|| {
                barrier.wait();
                s.claim_task(&t.id, "b", Some("one")).is_ok()
            });
            usize::from(a.join().unwrap()) + usize::from(b.join().unwrap())
        });
        assert_eq!(wins, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn pause_and_parallel_limit_gate_task_claims() {
        let (s, dir, r) = setup();
        let t = s
            .create_team(
                "s",
                &r,
                &CreateTeamRequest {
                    name: "one slot".into(),
                    max_parallel: 1,
                    max_fix_rounds: 1,
                },
            )
            .unwrap();
        member(&s, &r, "a", Some(&t.id));
        member(&s, &r, "b", Some(&t.id));
        s.update_plan(
            &t.id,
            &r,
            None,
            &json!([{"id":"one","title":"one"},{"id":"two","title":"two"}]),
        )
        .unwrap();
        s.control_team(&t.id, "pause").unwrap();
        assert!(s.claim_task(&t.id, "a", Some("one")).is_err());
        s.control_team(&t.id, "resume").unwrap();
        s.claim_task(&t.id, "a", Some("one")).unwrap();
        assert_eq!(
            s.claim_task(&t.id, "b", Some("two")).unwrap_err().code,
            "TEAM_PARALLEL_LIMIT"
        );
        s.report_task(&t.id, "a", "one", "failed", "failed")
            .unwrap();
        s.update_plan(
            &t.id,
            &r,
            None,
            &json!([{"id":"one","title":"one","status":"queued"}]),
        )
        .unwrap();
        s.claim_task(&t.id, "a", Some("one")).unwrap();
        s.report_task(&t.id, "a", "one", "failed", "failed again")
            .unwrap();
        assert_eq!(
            s.update_plan(
                &t.id,
                &r,
                None,
                &json!([{"id":"one","title":"one","status":"queued"}])
            )
            .unwrap_err()
            .code,
            "TEAM_FIX_LIMIT"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn unknown_delivery_does_not_replay_and_failed_write_does_not_accept() {
        let (s, dir, r) = setup();
        s.begin_run(&r, "r").unwrap();
        s.enqueue(
            "s",
            None,
            &r,
            &SendMessageRequest {
                annotations: Vec::new(),
                text: "once".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let l = s.lease_messages(&r, "r", 100).unwrap();
        s.fail_lease(&l.id, "RPC timeout").unwrap();
        assert!(s.lease_messages(&r, "r", 100).unwrap().messages.is_empty());
        assert_eq!(s.messages(&r)[0].status, "recoveryRequired");
        let invalid = dir.join("invalid.json");
        std::fs::write(&invalid, b"invalid").unwrap();
        let broken = CollaborationStore::load(invalid.clone());
        assert!(broken
            .register(AgentRegistration {
                id: root_id("x"),
                session_id: "x".into(),
                parent_agent_id: None,
                team_id: None,
                name: "root".into(),
                role: "root".into(),
                engine: "local".into()
            })
            .is_err());
        assert_eq!(std::fs::read(invalid).unwrap(), b"invalid");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn message_only_activations_respect_parallel_limit_and_root_is_exempt() {
        let (store, dir, root) = setup();
        let team = store
            .create_team(
                "s",
                &root,
                &CreateTeamRequest {
                    name: "one slot".into(),
                    max_parallel: 1,
                    max_fix_rounds: 3,
                },
            )
            .unwrap();
        member(&store, &root, "a", Some(&team.id));
        member(&store, &root, "b", Some(&team.id));
        store.begin_run("a", "a1").unwrap();
        assert_eq!(
            store.begin_run("b", "b1").unwrap_err().code,
            "TEAM_PARALLEL_LIMIT"
        );
        store.begin_run(&root, "root1").unwrap();
        store.end_run("a", "a1").unwrap();
        store.begin_run("b", "b1").unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn member_config_survives_restart_and_same_identity_keeps_original_instructions() {
        let (store, dir, root) = setup();
        let team = team(&store, &root);
        member(&store, &root, "named-member", Some(&team.id));
        let config = json!({"profile":"explore","prompt":"original specialty"});
        store
            .save_member_config("named-member", config.clone())
            .unwrap();
        member(&store, &root, "named-member", Some(&team.id));
        store
            .save_member_config(
                "named-member",
                json!({"profile":"qa-tester","prompt":"must not overwrite"}),
            )
            .unwrap();
        assert_eq!(store.member_config("named-member"), Some(config.clone()));
        assert_eq!(
            store.team(&team.id).unwrap().member_agent_ids,
            vec!["named-member"]
        );
        drop(store);
        let reloaded = CollaborationStore::load(dir.join("state.json"));
        assert_eq!(reloaded.member_config("named-member"), Some(config));
        assert_eq!(
            reloaded.participant("named-member").unwrap().name,
            "named-member"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn team_create_configures_unstarted_team_and_is_idempotent_after_execution() {
        let (store, dir, root) = setup();
        let team = team(&store, &root);
        member(&store, &root, "a", Some(&team.id));
        let requested = CreateTeamRequest {
            name: "bounded work".into(),
            max_parallel: 1,
            max_fix_rounds: 2,
        };
        let configured = store.create_team("s", &root, &requested).unwrap();
        assert_eq!(configured.id, team.id);
        assert_eq!(
            (
                configured.max_parallel,
                configured.max_fix_rounds,
                configured.fix_rounds
            ),
            (1, 2, 0)
        );
        assert_eq!(configured.member_agent_ids, vec!["a"]);
        store
            .update_plan(&team.id, &root, None, &json!([{"id":"one","title":"one"}]))
            .unwrap();
        store.claim_task(&team.id, "a", Some("one")).unwrap();
        let before = store.team(&team.id).unwrap();
        let repeated = store.create_team("s", &root, &requested).unwrap();
        assert_eq!(
            repeated.revision, before.revision,
            "an exact replay is a no-op"
        );
        assert_eq!(
            store
                .create_team(
                    "s",
                    &root,
                    &CreateTeamRequest {
                        name: requested.name,
                        max_parallel: 2,
                        max_fix_rounds: 3
                    }
                )
                .unwrap_err()
                .code,
            "TEAM_ALREADY_ACTIVE"
        );
        assert_eq!(store.team(&team.id).unwrap().tasks[0].attempts, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn revert_reset_closes_pending_mail_and_keeps_root_and_closed_member_audit() {
        let (store, dir, root) = setup();
        let team = team(&store, &root);
        member(&store, &root, "closed-member", Some(&team.id));
        store
            .save_history(
                &root,
                vec![json!({"role":"user","content":"removed branch"})],
            )
            .unwrap();
        store
            .save_history(
                "closed-member",
                vec![json!({"role":"assistant","content":"archived member result"})],
            )
            .unwrap();
        assert_eq!(
            store.reset_after_revert("s").unwrap_err().code,
            "TEAM_REVERT_BLOCKED"
        );
        store.control_team(&team.id, "stop").unwrap();
        store
            .enqueue(
                "s",
                None,
                &root,
                &SendMessageRequest {
                annotations: Vec::new(),
                    text: "old queued instruction".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store.begin_run(&root, "active").unwrap();
        assert_eq!(
            store.reset_after_revert("s").unwrap_err().code,
            "SESSION_BUSY"
        );
        assert_eq!(store.messages(&root)[0].status, "queued");
        assert!(!store.history(&root).is_empty());
        store.end_run(&root, "active").unwrap();
        store.reset_after_revert("s").unwrap();
        assert_eq!(store.messages(&root)[0].status, "failed");
        assert!(store.messages(&root)[0]
            .error
            .as_ref()
            .unwrap()
            .contains("回退"));
        assert!(store.history(&root).is_empty());
        assert_eq!(store.history("closed-member").len(), 1);
        assert_eq!(
            store.participant("closed-member").unwrap().status,
            "stopped"
        );
        assert_eq!(
            store.begin_run(&root, "after-revert").unwrap().status,
            "running"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn passive_receipts_do_not_wake_and_unaccepted_run_leases_become_unknown() {
        let (store, dir, root) = setup();
        member(&store, &root, "a", None);
        // Detached actors stop before their host posts the terminal receipt.
        store.stop_agent("a").unwrap();
        store
            .enqueue_receipt(
                "s",
                "a",
                &root,
                &SendMessageRequest {
                annotations: Vec::new(),
                    text: "finished".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(store.has_queued_messages(&root));
        assert!(!store.has_wake_messages(&root));
        store.begin_run(&root, "run").unwrap();
        let lease = store.lease_messages(&root, "run", 100).unwrap();
        assert_eq!(lease.messages[0].kind, "receipt");
        assert!(injection_text(&lease).contains("无需仅为确认收件"));
        store.end_run(&root, "run").unwrap();
        let message = &store.messages(&root)[0];
        assert_eq!(message.status, "recoveryRequired");
        assert_eq!(message.run_id.as_deref(), Some("run"));
        assert_eq!(message.lease_id.as_deref(), Some(lease.id.as_str()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn closed_team_drops_outbound_coordination_but_keeps_user_steering() {
        let (store, dir, root) = setup();
        let team = team(&store, &root);
        member(&store, &root, "a", Some(&team.id));
        store
            .enqueue(
                "s",
                Some("a"),
                &root,
                &SendMessageRequest {
                annotations: Vec::new(),
                    text: "old team".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .enqueue(
                "s",
                None,
                &root,
                &SendMessageRequest {
                annotations: Vec::new(),
                    text: "new user direction".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store.control_team(&team.id, "stop").unwrap();
        let messages = store.messages(&root);
        assert_eq!(messages[0].status, "failed");
        assert_eq!(messages[1].status, "queued");
        assert!(store.has_wake_messages(&root));
        assert_eq!(
            store
                .enqueue_receipt(
                    "s",
                    "a",
                    &root,
                    &SendMessageRequest {
                annotations: Vec::new(),
                        text: "late receipt from stopped team".into(),
                        ..Default::default()
                    }
                )
                .unwrap_err()
                .code,
            "TEAM_CLOSED"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn completion_requires_members_idle_and_mail_fully_accepted() {
        let (store, dir, root) = setup();
        let team = team(&store, &root);
        member(&store, &root, "a", Some(&team.id));
        store
            .update_plan(&team.id, &root, None, &json!([{"id":"one","title":"one"}]))
            .unwrap();
        store.begin_run(&root, "root-run").unwrap();
        store.begin_run("a", "member-run").unwrap();
        store.claim_task(&team.id, "a", Some("one")).unwrap();
        store
            .report_task(&team.id, "a", "one", "completed", "done")
            .unwrap();
        assert_eq!(
            store.control_team(&team.id, "complete").unwrap_err().code,
            "TEAM_BUSY"
        );
        store.end_run("a", "member-run").unwrap();
        store
            .enqueue_receipt(
                "s",
                "a",
                &root,
                &SendMessageRequest {
                annotations: Vec::new(),
                    text: "done".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            store.control_team(&team.id, "complete").unwrap_err().code,
            "TEAM_MESSAGES_PENDING"
        );
        let lease = store.lease_messages(&root, "root-run", 100).unwrap();
        assert_eq!(
            store.control_team(&team.id, "complete").unwrap_err().code,
            "TEAM_MESSAGES_PENDING"
        );
        store.commit_lease(&lease.id).unwrap();
        assert_eq!(
            store.control_team(&team.id, "complete").unwrap().status,
            "completed"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explicit_stage_barrier_does_not_skip_failed_earlier_stage() {
        let (store, dir, root) = setup();
        let team = team(&store, &root);
        member(&store, &root, "a", Some(&team.id));
        store.update_plan(&team.id,&root,None,&json!([{"id":"a","title":"A","stage":"first"},{"id":"b","title":"B","stage":"second"},{"id":"free","title":"free"}])).unwrap();
        assert_eq!(
            ready_task_ids(&store.team(&team.id).unwrap()),
            vec!["a", "free"]
        );
        store.claim_task(&team.id, "a", Some("a")).unwrap();
        store
            .report_task(&team.id, "a", "a", "failed", "failed")
            .unwrap();
        assert_eq!(ready_task_ids(&store.team(&team.id).unwrap()), vec!["free"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn paused_claim_defers_without_attempt_or_message_loss_and_blocked_work_can_report() {
        let (store, dir, root) = setup();
        let team = team(&store, &root);
        member(&store, &root, "a", Some(&team.id));
        store
            .update_plan(&team.id, &root, None, &json!([{"id":"one","title":"one"}]))
            .unwrap();
        store
            .enqueue(
                "s",
                None,
                "a",
                &SendMessageRequest {
                annotations: Vec::new(),
                    text: "keep this steering".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store.claim_task(&team.id, "a", Some("one")).unwrap();
        store.control_team(&team.id, "pause").unwrap();
        assert_eq!(
            store.begin_run("a", "not-started").unwrap_err().code,
            "TEAM_NOT_ACTIVE"
        );
        let task = store.defer_claim(&team.id, "a", "one").unwrap();
        assert_eq!(task.status, "queued");
        assert_eq!(task.attempts, 0);
        assert_eq!(store.messages("a")[0].status, "queued");
        store.control_team(&team.id, "resume").unwrap();
        store.claim_task(&team.id, "a", Some("one")).unwrap();
        store.begin_run("a", "started").unwrap();
        assert_eq!(
            store.defer_claim(&team.id, "a", "one").unwrap_err().code,
            "TEAM_MEMBER_BUSY"
        );
        store.control_team(&team.id, "block").unwrap();
        assert_eq!(
            store
                .report_task(&team.id, "a", "one", "completed", "finished while blocked")
                .unwrap()
                .status,
            "completed"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explicit_recovery_requeues_unknown_task_then_restores_idle_member() {
        let (store, dir, root) = setup();
        let team = team(&store, &root);
        member(&store, &root, "a", Some(&team.id));
        store
            .update_plan(&team.id, &root, None, &json!([{"id":"one","title":"one"}]))
            .unwrap();
        store.begin_run("a", "lost-run").unwrap();
        store.claim_task(&team.id, "a", Some("one")).unwrap();
        // Pausing permits current work to finish; a crash during that work is
        // still an unknown result, not an ordinary resumable paused task.
        store.control_team(&team.id, "pause").unwrap();
        drop(store);
        let store = CollaborationStore::load(dir.join("state.json"));
        assert_eq!(store.team(&team.id).unwrap().status, "recoveryRequired");
        assert_eq!(
            store.control_team(&team.id, "resume").unwrap_err().code,
            "TEAM_RECOVERY_REQUIRED"
        );
        store
            .update_plan(
                &team.id,
                &root,
                None,
                &json!([{"id":"one","title":"one","status":"queued"}]),
            )
            .unwrap();
        store.control_team(&team.id, "resume").unwrap();
        assert_eq!(store.participant("a").unwrap().status, "idle");
        assert_eq!(store.team(&team.id).unwrap().tasks[0].status, "queued");
        assert_eq!(store.team(&team.id).unwrap().tasks[0].attempts, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn team_stop_cancels_leader_and_member_but_pause_preserves_current_runs() {
        let (state, dir) = crate::test_app_state("collaboration-stop");
        let session = state.sessions.create("chat", "coding", None, true, None);
        let root = root_id(&session.id);
        state
            .collaboration
            .register(AgentRegistration {
                id: root.clone(),
                session_id: session.id.clone(),
                parent_agent_id: None,
                team_id: None,
                name: "root".into(),
                role: "root".into(),
                engine: "local".into(),
            })
            .unwrap();
        let team = state
            .collaboration
            .create_team(
                &session.id,
                &root,
                &CreateTeamRequest {
                    name: "test".into(),
                    max_parallel: 4,
                    max_fix_rounds: 3,
                },
            )
            .unwrap();
        state
            .collaboration
            .register(AgentRegistration {
                id: "worker".into(),
                session_id: session.id.clone(),
                parent_agent_id: Some(root.clone()),
                team_id: Some(team.id.clone()),
                name: "worker".into(),
                role: "member".into(),
                engine: "local".into(),
            })
            .unwrap();
        let (leader_run, leader_cancel) = state.runs.begin(&session.id, "team-leader");
        let (worker_run, worker_cancel) = state.runs.begin(&session.id, "team-worker");
        state
            .collaboration
            .begin_run(&root, &leader_run.id)
            .unwrap();
        state
            .collaboration
            .begin_run("worker", &worker_run.id)
            .unwrap();
        let response = patch_team(
            State(state.clone()),
            Path(team.id.clone()),
            Json(TeamActionRequest {
                action: "pause".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!leader_cancel.is_cancelled() && !worker_cancel.is_cancelled());
        let response = patch_team(
            State(state.clone()),
            Path(team.id),
            Json(TeamActionRequest {
                action: "stop".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(leader_cancel.is_cancelled() && worker_cancel.is_cancelled());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn scoped_message_routes_deduplicate_and_reject_stale_runs() {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let (state, dir) = crate::test_app_state("collaboration-routes");
        let session = state.sessions.create("chat", "coding", None, true, None);
        let root = root_id(&session.id);
        state
            .collaboration
            .register(AgentRegistration {
                id: root.clone(),
                session_id: session.id.clone(),
                parent_agent_id: None,
                team_id: None,
                name: "root".into(),
                role: "root".into(),
                engine: "local".into(),
            })
            .unwrap();
        state.collaboration.begin_run(&root, "run1").unwrap();
        let app = routes().with_state(state.clone());
        let uri = format!("/api/forge/sessions/{}/agents/{root}/messages", session.id);
        let request = |path: &str, run: &str| {
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"text":"steer","clientMessageId":"once","expectedRunId":run})
                        .to_string(),
                ))
                .unwrap()
        };
        let first = app.clone().oneshot(request(&uri, "run1")).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let first: Value =
            serde_json::from_slice(&first.into_body().collect().await.unwrap().to_bytes()).unwrap();
        let again = app.clone().oneshot(request(&uri, "run1")).await.unwrap();
        let again: Value =
            serde_json::from_slice(&again.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(first["message"]["id"], again["message"]["id"]);
        let wrong = app
            .clone()
            .oneshot(request(
                &format!("/api/forge/sessions/other/agents/{root}/messages"),
                "run1",
            ))
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::NOT_FOUND);
        let stale = Request::builder()
            .method("POST")
            .uri(&uri)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"text":"new steering","clientMessageId":"new","expectedRunId":"old"})
                    .to_string(),
            ))
            .unwrap();
        assert_eq!(
            app.oneshot(stale).await.unwrap().status(),
            StatusCode::CONFLICT
        );
        assert_eq!(state.collaboration.messages(&root).len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
