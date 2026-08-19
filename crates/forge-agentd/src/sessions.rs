//! F7 wave.1 会话/聊天文件夹持久化与 REST(D-F7-A/E;参考 I:\agent-debug-frontend-backend-copy-20260530
//! gateway-go/backend-rs agent-core/session.rs + api/handlers/{sessions,folders}.rs 语义级移植,不 fork 代码)。
//!
//! 数据面(D-F7-E):
//! - data/agent-sessions/sessions.json:`{"sessions":[DebugSession...]}`(读-改-写,Mutex 串行,
//!   serde_json pretty,tmp+rename 原子落盘;重启 load 恢复)。
//! - data/agent-sessions/chat-folders.json:`{"folders":[ChatFolder...]}`。
//! 事件面:create/patch/fork/revert 发持久事件(session.created/updated/forked/reverted);delete 清空事件文件。
//! 路由形态差异留痕:参考为 `/api/forge/sessions/{id}:fork|：revert`(单段内冒号动作);axum 0.8
//! (matchit)不支持段内冒号参数,本仓用 `{id}/fork`、`{id}/revert`(动作语义不变,仅路径形态差异)。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};

use crate::events::{new_id, now_rfc3339, EventDraft};
use crate::AppState;

fn default_status() -> String {
    "idle".to_string()
}
fn default_agent_kind() -> String {
    "coding".to_string()
}
fn default_true() -> bool {
    true
}

/// 会话本体(wire 对齐参考 DebugSession;workspaceRoot/mode/activePlanId 属参考全量字段,
/// wave.1 不落地,如实省略——参考默认值即本仓行为)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugSession {
    pub id: String,
    pub title: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default = "default_agent_kind")]
    pub agent_kind: String,
    #[serde(default)]
    pub selected_model_id: Option<String>,
    #[serde(default = "default_true")]
    pub web_search_enabled: bool,
    #[serde(default)]
    pub active_run_id: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub title_manually_set: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
}

impl DebugSession {
    fn new(title: &str, agent_kind: &str, model_id: Option<String>, web_search: bool) -> Self {
        let ts = now_rfc3339();
        DebugSession {
            id: new_id("sess"),
            title: title.to_string(),
            status: default_status(),
            agent_kind: agent_kind.to_string(),
            selected_model_id: model_id,
            web_search_enabled: web_search,
            active_run_id: None,
            created_at: ts.clone(),
            updated_at: ts,
            pinned: false,
            title_manually_set: false,
            folder_id: None,
        }
    }

    /// F7 wave.2:pub(crate)(agent.rs activeRunId/自动命名写回用)。
    pub(crate) fn touch(&mut self) {
        self.updated_at = now_rfc3339();
    }
}

/// 聊天文件夹(Cursor 式侧栏分组)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatFolder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug)]
pub enum PatchError {
    NotFound,
    InvalidTitle,
}

#[derive(Debug)]
pub enum FolderError {
    NotFound,
    InvalidName,
}

/// PATCH 面(folderId 三态:缺省不变 / null 清除 / 字符串设置)。
/// F7 wave.2:字段 pub(crate)(agent.rs 单测构造手动命名用)。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSessionRequest {
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) pinned: Option<bool>,
    #[serde(default)]
    pub(crate) folder_id: Option<Option<String>>,
    #[serde(default)]
    pub(crate) agent_kind: Option<String>,
    #[serde(default)]
    pub(crate) web_search_enabled: Option<bool>,
    /// F7 wave.4:模型选择(三态同 folderId:缺省不变 / null 清除 / 字符串设置;
    /// "mock" 在 ask:execute 侧强制 Mock provider)。
    #[serde(default)]
    pub(crate) selected_model_id: Option<Option<String>>,
}

/// 会话存贮:内存 HashMap + sessions.json 整文件读-改-写(Mutex 串行化并发写)。
pub struct SessionStore {
    path: PathBuf,
    inner: Mutex<HashMap<String, DebugSession>>,
}

impl SessionStore {
    pub fn load(path: PathBuf) -> Self {
        let map = read_sessions_file(&path);
        SessionStore {
            path,
            inner: Mutex::new(map),
        }
    }

    fn persist_locked(&self, inner: &HashMap<String, DebugSession>) {
        let mut v: Vec<&DebugSession> = inner.values().collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let doc = json!({ "sessions": v });
        let text = serde_json::to_string_pretty(&doc).expect("sessions 序列化失败");
        if let Err(e) = write_atomic(&self.path, &text) {
            eprintln!("sessions.json 写盘失败({}): {e}", self.path.display());
        }
    }

    /// updatedAt 倒序(同刻 tie-break:id 倒序,确定性)。
    pub fn list(&self) -> Vec<DebugSession> {
        let inner = self.inner.lock().unwrap();
        let mut v: Vec<DebugSession> = inner.values().cloned().collect();
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
        v
    }

    pub fn get(&self, id: &str) -> Option<DebugSession> {
        self.inner.lock().unwrap().get(id).cloned()
    }

    pub fn create(
        &self,
        title: &str,
        agent_kind: &str,
        model_id: Option<String>,
        web_search: bool,
    ) -> DebugSession {
        let session = DebugSession::new(title, agent_kind, model_id, web_search);
        let mut inner = self.inner.lock().unwrap();
        inner.insert(session.id.clone(), session.clone());
        self.persist_locked(&inner);
        session
    }

    /// 整体覆写(fork 继承 folderId / revert 清 activeRunId 用;调用方负责 touch)。
    pub fn save(&self, session: &DebugSession) {
        let mut inner = self.inner.lock().unwrap();
        inner.insert(session.id.clone(), session.clone());
        self.persist_locked(&inner);
    }

    pub fn patch(&self, id: &str, req: &PatchSessionRequest) -> Result<DebugSession, PatchError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(session) = inner.get_mut(id) else {
            return Err(PatchError::NotFound);
        };
        if let Some(title) = &req.title {
            if title.trim().is_empty() {
                return Err(PatchError::InvalidTitle);
            }
            session.title = title.clone();
            session.title_manually_set = true;
        }
        if let Some(pinned) = req.pinned {
            session.pinned = pinned;
        }
        if let Some(folder) = &req.folder_id {
            session.folder_id = folder
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if let Some(kind) = &req.agent_kind {
            if !kind.trim().is_empty() {
                session.agent_kind = kind.trim().to_string();
            }
        }
        if let Some(web) = req.web_search_enabled {
            session.web_search_enabled = web;
        }
        if let Some(model) = &req.selected_model_id {
            session.selected_model_id = model
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        session.touch();
        let out = session.clone();
        self.persist_locked(&inner);
        Ok(out)
    }

    pub fn delete(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let existed = inner.remove(id).is_some();
        if existed {
            self.persist_locked(&inner);
        }
        existed
    }

    /// 文件夹删除级联:清引用该 folderId 的会话;返回清理数。
    pub fn clear_folder(&self, folder_id: &str) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let mut n = 0;
        for s in inner.values_mut() {
            if s.folder_id.as_deref() == Some(folder_id) {
                s.folder_id = None;
                s.touch();
                n += 1;
            }
        }
        if n > 0 {
            self.persist_locked(&inner);
        }
        n
    }
}

/// 聊天文件夹存贮(Vec 保创建序)。
pub struct ChatFolderStore {
    path: PathBuf,
    inner: Mutex<Vec<ChatFolder>>,
}

impl ChatFolderStore {
    pub fn load(path: PathBuf) -> Self {
        let folders = read_folders_file(&path);
        ChatFolderStore {
            path,
            inner: Mutex::new(folders),
        }
    }

    fn persist_locked(&self, inner: &[ChatFolder]) {
        let doc = json!({ "folders": inner });
        let text = serde_json::to_string_pretty(&doc).expect("folders 序列化失败");
        if let Err(e) = write_atomic(&self.path, &text) {
            eprintln!("chat-folders.json 写盘失败({}): {e}", self.path.display());
        }
    }

    pub fn list(&self) -> Vec<ChatFolder> {
        self.inner.lock().unwrap().clone()
    }

    pub fn create(&self, name: &str) -> Result<ChatFolder, FolderError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(FolderError::InvalidName);
        }
        let ts = now_rfc3339();
        let folder = ChatFolder {
            id: new_id("fld"),
            name: name.to_string(),
            created_at: ts.clone(),
            updated_at: ts,
        };
        let mut inner = self.inner.lock().unwrap();
        inner.push(folder.clone());
        self.persist_locked(&inner);
        Ok(folder)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<ChatFolder, FolderError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(FolderError::InvalidName);
        }
        let mut inner = self.inner.lock().unwrap();
        let Some(f) = inner.iter_mut().find(|f| f.id == id) else {
            return Err(FolderError::NotFound);
        };
        f.name = name.to_string();
        f.updated_at = now_rfc3339();
        let out = f.clone();
        self.persist_locked(&inner);
        Ok(out)
    }

    pub fn delete(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.len();
        inner.retain(|f| f.id != id);
        let existed = inner.len() != before;
        if existed {
            self.persist_locked(&inner);
        }
        existed
    }
}

fn read_sessions_file(path: &FsPath) -> HashMap<String, DebugSession> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sessions.json 解析失败({}): {e},按空处理", path.display());
            return HashMap::new();
        }
    };
    // 兼容 {"sessions":[...]} 与裸数组两种形态。
    let arr = v
        .get("sessions")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| v.as_array().cloned())
        .unwrap_or_default();
    let mut map = HashMap::new();
    for item in arr {
        match serde_json::from_value::<DebugSession>(item) {
            Ok(s) => {
                map.insert(s.id.clone(), s);
            }
            Err(e) => eprintln!("sessions.json 条目解析失败: {e}"),
        }
    }
    map
}

fn read_folders_file(path: &FsPath) -> Vec<ChatFolder> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("chat-folders.json 解析失败({}): {e},按空处理", path.display());
            return Vec::new();
        }
    };
    v.get("folders")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            serde_json::from_value::<ChatFolder>(item)
                .map_err(|e| eprintln!("chat-folders.json 条目解析失败: {e}"))
                .ok()
        })
        .collect()
}

/// 原子写:tmp 全量写 + rename(与 events.rs write_jsonl_atomic 同纪律)。
fn write_atomic(path: &FsPath, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

// ---------- REST handlers(路由注册见 main.rs build_app) ----------

fn not_found(code: &str, message: String) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn bad_request(code: &str, message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

/// GET /api/forge/sessions → {sessions}(updatedAt 倒序)。
pub async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "sessions": state.sessions.list() }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    agent_kind: Option<String>,
    #[serde(default)]
    selected_model_id: Option<String>,
    #[serde(default)]
    web_search_enabled: Option<bool>,
}

/// POST /api/forge/sessions → {session};发持久事件 session.created。
pub async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Json<Value> {
    let title = req
        .title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "新会话".to_string());
    let kind = req
        .agent_kind
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "coding".to_string());
    let session = state.sessions.create(
        &title,
        &kind,
        req.selected_model_id,
        req.web_search_enabled.unwrap_or(true),
    );
    state.events.emit(
        EventDraft::new(&session.id, "session.created", "session")
            .payload(json!({ "sessionId": session.id, "title": session.title })),
    );
    Json(json!({ "session": session }))
}

/// GET /api/forge/sessions/{id} → {session}(404 SESSION_NOT_FOUND)。
pub async fn get_session(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match state.sessions.get(&id) {
        Some(s) => Json(json!({ "session": s })).into_response(),
        None => not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}")),
    }
}

/// PATCH /api/forge/sessions/{id}:title/pinned/folderId/agentKind/webSearchEnabled;
/// 改 title 时 titleManuallySet=true;发 session.updated。
pub async fn patch_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchSessionRequest>,
) -> Response {
    match state.sessions.patch(&id, &req) {
        Ok(s) => {
            state.events.emit(
                EventDraft::new(&id, "session.updated", "session")
                    .payload(json!({ "sessionId": id })),
            );
            Json(json!({ "session": s })).into_response()
        }
        Err(PatchError::NotFound) => not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}")),
        Err(PatchError::InvalidTitle) => bad_request("INVALID_TITLE", "title 不可为空"),
    }
}

/// DELETE /api/forge/sessions/{id} → {ok:true}(删元信息 + 事件文件)。
pub async fn delete_session(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    if !state.sessions.delete(&id) {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    }
    state.events.purge_session(&id);
    Json(json!({ "ok": true })).into_response()
}

/// POST /api/forge/sessions/{id}/fork:克隆会话 + 拷贝事件文件(seq 保持单调),
/// 标题「分支 · {原题}」;发 session.forked(目标会话)→ {session}。
/// (参考为 {id}:fork 单段冒号形态;axum 0.8 不支持段内冒号参数,路径形态差异如实留痕。)
pub async fn fork_session(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let Some(src) = state.sessions.get(&id) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    let mut forked = state.sessions.create(
        &format!("分支 · {}", src.title),
        &src.agent_kind,
        src.selected_model_id.clone(),
        src.web_search_enabled,
    );
    forked.folder_id = src.folder_id.clone();
    forked.touch();
    state.sessions.save(&forked);
    // 事件流克隆(磁盘全文,sessionId 换新,seq/id/ts 保持 → 单调)。
    state.events.fork_events(&id, &forked.id);
    state.events.emit(
        EventDraft::new(&forked.id, "session.forked", "session").payload(json!({
            "sessionId": forked.id,
            "sourceSessionId": id,
            "title": forked.title,
        })),
    );
    Json(json!({ "session": forked })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertRequest {
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

/// POST /api/forge/sessions/{id}/revert {messageId?, mode?}:
/// mode="before" 截断事件流到该事件 id 之前(排他),否则截到含该事件;
/// 重写 JSONL + 内存缓冲 + latestSeq;无 messageId 仅清 activeRunId;发 session.reverted。
/// (参考 truncate_before_event 还会回卷同 run 前导事件——wave.1 无 run,纯 seq 截断,留痕。)
pub async fn revert_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RevertRequest>,
) -> Response {
    let Some(mut session) = state.sessions.get(&id) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    let message_id = req.message_id.clone().filter(|m| !m.trim().is_empty());
    if let Some(message_id) = &message_id {
        let Some(target_seq) = state.events.seq_of_event(&id, message_id) else {
            return not_found("EVENT_NOT_FOUND", format!("事件不存在: {message_id}"));
        };
        let cutoff = if req.mode.as_deref() == Some("before") {
            target_seq - 1
        } else {
            target_seq
        };
        state.events.truncate_to_seq(&id, cutoff);
        session.active_run_id = None;
        session.status = default_status();
    } else {
        session.active_run_id = None;
    }
    session.touch();
    state.sessions.save(&session);
    state.events.emit(
        EventDraft::new(&id, "session.reverted", "session").payload(json!({
            "sessionId": id,
            "messageId": req.message_id,
            "mode": req.mode,
        })),
    );
    Json(json!({ "session": session })).into_response()
}

/// GET /api/forge/chat-folders → {folders}。
pub async fn list_chat_folders(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "folders": state.folders.list() }))
}

#[derive(Deserialize)]
pub struct FolderNameRequest {
    #[serde(default)]
    name: String,
}

/// POST /api/forge/chat-folders {name} → {folder}(空名 400 INVALID_NAME)。
pub async fn create_chat_folder(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FolderNameRequest>,
) -> Response {
    match state.folders.create(&req.name) {
        Ok(f) => Json(json!({ "folder": f })).into_response(),
        Err(FolderError::InvalidName) => bad_request("INVALID_NAME", "folder name 不可为空"),
        Err(FolderError::NotFound) => unreachable!("create 不产生 NotFound"),
    }
}

/// PATCH /api/forge/chat-folders/{id} {name} → {folder}。
pub async fn patch_chat_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<FolderNameRequest>,
) -> Response {
    match state.folders.rename(&id, &req.name) {
        Ok(f) => Json(json!({ "folder": f })).into_response(),
        Err(FolderError::NotFound) => not_found("FOLDER_NOT_FOUND", format!("文件夹不存在: {id}")),
        Err(FolderError::InvalidName) => bad_request("INVALID_NAME", "folder name 不可为空"),
    }
}

/// DELETE /api/forge/chat-folders/{id} → {ok:true}(级联清会话 folderId)。
pub async fn delete_chat_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    if !state.folders.delete(&id) {
        return not_found("FOLDER_NOT_FOUND", format!("文件夹不存在: {id}"));
    }
    let cleared = state.sessions.clear_folder(&id);
    Json(json!({ "ok": true, "clearedSessions": cleared })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-sessions-{tag}-{}-{}",
            std::process::id(),
            new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn store(dir: &FsPath) -> SessionStore {
        SessionStore::load(dir.join("sessions.json"))
    }

    #[test]
    fn crud_sort_and_reload() {
        let dir = temp_dir("crud");
        let s = store(&dir);
        let a = s.create("甲", "coding", None, true);
        std::thread::sleep(std::time::Duration::from_millis(3));
        let b = s.create("乙", "coding", Some("deepseek-chat".to_string()), false);
        // 排序:updatedAt 倒序(乙新)。
        let list = s.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, b.id);
        assert_eq!(list[1].id, a.id);
        // 默认字段面。
        assert_eq!(a.status, "idle");
        assert_eq!(a.agent_kind, "coding");
        assert!(a.web_search_enabled);
        assert!(!a.pinned);
        assert!(!a.title_manually_set);
        assert!(a.active_run_id.is_none());
        assert!(a.id.starts_with("sess_"));
        // patch 甲 → 甲冒头。
        std::thread::sleep(std::time::Duration::from_millis(3));
        let pa = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    pinned: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(pa.pinned);
        assert_eq!(s.list()[0].id, a.id);
        // get / delete。
        assert_eq!(s.get(&b.id).unwrap().title, "乙");
        assert!(s.delete(&b.id));
        assert!(s.get(&b.id).is_none());
        assert!(!s.delete(&b.id), "二次删除 false");
        // 重启恢复(重新 load 同一路径)。
        let s2 = store(&dir);
        let list2 = s2.list();
        assert_eq!(list2.len(), 1);
        assert_eq!(list2[0].id, a.id);
        assert!(list2[0].pinned, "持久化字段恢复");
        // 文件形态 {"sessions":[...]}。
        let text = std::fs::read_to_string(dir.join("sessions.json")).unwrap();
        let doc: Value = serde_json::from_str(&text).unwrap();
        assert!(doc["sessions"].is_array());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn patch_title_sets_manual_flag_and_validates() {
        let dir = temp_dir("title");
        let s = store(&dir);
        let a = s.create("原题", "coding", None, true);
        assert!(!a.title_manually_set);
        let p = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    title: Some("新题".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p.title, "新题");
        assert!(p.title_manually_set, "改 title 须置 titleManuallySet");
        // 空 title → InvalidTitle。
        let bad = s.patch(
            &a.id,
            &PatchSessionRequest {
                title: Some("  ".to_string()),
                ..Default::default()
            },
        );
        assert!(matches!(bad, Err(PatchError::InvalidTitle)));
        // 不存在 → NotFound。
        let missing = s.patch("sess_none", &PatchSessionRequest::default());
        assert!(matches!(missing, Err(PatchError::NotFound)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn patch_folder_id_three_states() {
        let dir = temp_dir("fold3");
        let s = store(&dir);
        let a = s.create("x", "coding", None, true);
        // 设置。
        let p1 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    folder_id: Some(Some("fld_1".to_string())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p1.folder_id.as_deref(), Some("fld_1"));
        // 缺省不变。
        let p2 = s.patch(&a.id, &PatchSessionRequest::default()).unwrap();
        assert_eq!(p2.folder_id.as_deref(), Some("fld_1"));
        // null 清除。
        let p3 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    folder_id: Some(None),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p3.folder_id.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn patch_selected_model_id_three_states() {
        // F7 wave.4:selectedModelId 三态(设置/缺省不变/null 清除),同 folderId 口径。
        let dir = temp_dir("model3");
        let s = store(&dir);
        let a = s.create("x", "coding", None, true);
        assert!(a.selected_model_id.is_none());
        let p1 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    selected_model_id: Some(Some("mock".to_string())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p1.selected_model_id.as_deref(), Some("mock"));
        let p2 = s.patch(&a.id, &PatchSessionRequest::default()).unwrap();
        assert_eq!(p2.selected_model_id.as_deref(), Some("mock"), "缺省不变");
        let p3 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    selected_model_id: Some(None),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p3.selected_model_id.is_none(), "null 清除");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn folders_crud_and_cascade_clear() {
        let dir = temp_dir("folders");
        let s = store(&dir);
        let f = ChatFolderStore::load(dir.join("chat-folders.json"));
        // 空名 400 语义。
        assert!(matches!(f.create("  "), Err(FolderError::InvalidName)));
        let fld = f.create("工作").unwrap();
        assert!(fld.id.starts_with("fld_"));
        // 改名。
        let r = f.rename(&fld.id, "工作区").unwrap();
        assert_eq!(r.name, "工作区");
        assert!(matches!(f.rename(&fld.id, ""), Err(FolderError::InvalidName)));
        assert!(matches!(f.rename("fld_none", "x"), Err(FolderError::NotFound)));
        // 会话挂 folder → 删文件夹级联清。
        let a = s.create("挂接", "coding", None, true);
        s.patch(
            &a.id,
            &PatchSessionRequest {
                folder_id: Some(Some(fld.id.clone())),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(s.get(&a.id).unwrap().folder_id.as_deref(), Some(fld.id.as_str()));
        assert!(f.delete(&fld.id));
        assert!(!f.delete(&fld.id));
        let cleared = s.clear_folder(&fld.id);
        assert_eq!(cleared, 1);
        assert!(s.get(&a.id).unwrap().folder_id.is_none());
        // 重启恢复(文件夹已删、会话 folderId 已清)。
        let f2 = ChatFolderStore::load(dir.join("chat-folders.json"));
        assert!(f2.list().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
