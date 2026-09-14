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
fn default_purpose() -> String {
    "chat".to_string()
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
    /// 模型规格三档之一:思考总开关(见 modelspec.rs;不支持的模型解析时如实忽略)。
    #[serde(default)]
    pub thinking_enabled: bool,
    /// 模型规格三档之一:reasoning_effort 档 id(None = 用该模型 defaultEffort)。
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// 模型规格三档之一:上下文窗口档 id(None = 用该模型 defaultContext)。
    #[serde(default)]
    pub context_option_id: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// chat = 侧栏可见;studio = 素材创作隐藏会话。
    #[serde(default = "default_purpose")]
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studio_node_id: Option<String>,
    /// D-035:当前计划文件(工作区相对路径 `.forge/plans/<名>.plan.md`)。
    /// plan 模式 create_plan 落盘时写入;前端据此在快照回放后重开 Plan 页签,
    /// 后续 plan 轮据此原地迭代同一份计划。旧 sessions.json 无此字段 → None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_plan_path: Option<String>,
    /// 执行引擎:`local` = 本仓自研工具循环;`codex` = 交给 codex app-server。
    /// 会话级而非全局:同一个项目里「这条会话用 Codex 跑,那条用本地模型跑」是常态。
    /// 旧 sessions.json 无此字段 → 缺省 `local`(既有会话行为不变)。
    #[serde(default = "default_agent_engine")]
    pub agent_engine: String,
    /// Codex 线程 id(首轮 `thread/start` 后写回,后续轮 `thread/resume` 续同一线程)。
    /// fork 出的会话不拷贝它:两条会话共用一个 Codex 线程会互相污染上下文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_thread_id: Option<String>,
}

fn default_agent_engine() -> String {
    crate::codex::config::ENGINE_LOCAL.to_string()
}

impl DebugSession {
    fn new(
        title: &str,
        agent_kind: &str,
        model_id: Option<String>,
        web_search: bool,
        workspace_id: Option<String>,
    ) -> Self {
        let ts = now_rfc3339();
        DebugSession {
            id: new_id("sess"),
            title: title.to_string(),
            status: default_status(),
            agent_kind: agent_kind.to_string(),
            selected_model_id: model_id,
            // 缺省起于该模型默认档(resolve 现算)。全屏主页无会话先改规格再发送时,
            // create_session 可把 thinking/effort/context 一并写入(见 CreateSessionRequest);
            // fork 侧仍显式拷贝源会话的选择。
            thinking_enabled: false,
            reasoning_effort: None,
            context_option_id: None,
            web_search_enabled: web_search,
            active_run_id: None,
            created_at: ts.clone(),
            updated_at: ts,
            pinned: false,
            title_manually_set: false,
            folder_id: None,
            workspace_id,
            purpose: default_purpose(),
            studio_node_id: None,
            active_plan_path: None,
            // 新会话默认引擎取设置页里配的那个(设置页改了默认,下一条新会话就该跟上)。
            agent_engine: crate::codex::config::load().default_engine,
            codex_thread_id: None,
        }
    }

    /// 本会话是否跑在 Codex 引擎上。
    pub(crate) fn is_codex(&self) -> bool {
        self.agent_engine == crate::codex::config::ENGINE_CODEX
    }

    pub(crate) fn is_studio(&self) -> bool {
        self.purpose == "studio"
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

#[derive(Debug)]
pub enum PatchError {
    NotFound,
    InvalidTitle,
    /// 模型规格档位不在 modelspec 已知集合内(当前模型是否支持该档交给 resolve 回落,此处只拦生造值)。
    InvalidModelSpec(String),
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
    /// 模型规格三档(effort/context 同 folderId 的 Option<Option<_>> 形态;thinking 是纯布尔)。
    ///
    /// 三态在 REST 层的实况留痕(与 folderId/selectedModelId/workspaceId 同源,非本波引入):
    /// serde 对 Option<Option<T>> 把 JSON null 折成外层 None,即「null」与「缺省」在 wire 上
    /// 不可分,故经 HTTP 只有两态可达——缺省不变 / 字符串设置。要清回「跟随模型默认档」,
    /// 传空串:下面的 filter(!is_empty) 会把它归为 None(client 未用到该腿,档位一律显式设值)。
    /// Some(None) 仅 Rust 内部调用方可构造,单测覆盖该腿。
    #[serde(default)]
    pub(crate) thinking_enabled: Option<bool>,
    #[serde(default)]
    pub(crate) reasoning_effort: Option<Option<String>>,
    #[serde(default)]
    pub(crate) context_option_id: Option<Option<String>>,
    #[serde(default)]
    pub(crate) workspace_id: Option<Option<String>>,
    /// 执行引擎切换(`local` | `codex`);未知值 → 400,不静默落库。
    #[serde(default)]
    pub(crate) agent_engine: Option<String>,
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

    /// 崩溃恢复清扫:run 只存内存(agent.rs RunRegistry),持久层的 active_run_id 在
    /// 进程重启后必然是残留(实测:强杀 agentd 留下幽灵 run,前端回放 run.created
    /// 无终止事件 → activeRunId 永久卡住、「中止运行」常驻、发送被软禁)。
    /// 清空并返回 (sessionId, runId) 清单,由调用方补发 run.failed 终止事件。
    pub fn clear_stale_active_runs(&self) -> Vec<(String, String)> {
        let mut inner = self.inner.lock().unwrap();
        let mut swept: Vec<(String, String)> = Vec::new();
        for s in inner.values_mut() {
            if let Some(rid) = s.active_run_id.take() {
                swept.push((s.id.clone(), rid));
            }
        }
        if !swept.is_empty() {
            self.persist_locked(&inner);
            eprintln!(
                "[sessions] 清扫 stale activeRunId × {}(进程重启崩溃恢复)",
                swept.len()
            );
        }
        swept
    }

    /// D-038:原子认领 activeRunId(CAS)。会话空闲 → 置 run_id 并落盘,Ok;
    /// 已有运行中的 run → Err(其 id),调用方不得起第二条 turn。
    /// 此前 execute_turn 是「读-改-写」三步无锁覆盖——用户轮之间靠前端 canSend 挡着尚可,
    /// 服务端自起的回执唤醒轮与用户轮之间没有任何前端门,必须在存贮层做原子性。
    pub fn claim_active_run(&self, id: &str, run_id: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let Some(s) = inner.get_mut(id) else {
            return Err("SESSION_NOT_FOUND".to_string());
        };
        if let Some(existing) = &s.active_run_id {
            return Err(existing.clone());
        }
        s.active_run_id = Some(run_id.to_string());
        s.touch();
        self.persist_locked(&inner);
        Ok(())
    }

    /// D-038:释放 activeRunId —— 只清自己认领的那一个(别的 turn 已接管则不动)。
    pub fn release_active_run(&self, id: &str, run_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        let Some(s) = inner.get_mut(id) else {
            return;
        };
        if s.active_run_id.as_deref() == Some(run_id) {
            s.active_run_id = None;
            s.touch();
            self.persist_locked(&inner);
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

    /// updatedAt 倒序(同刻 tie-break:id 倒序,确定性)。studio 隐藏会话不进侧栏清单。
    pub fn list(&self) -> Vec<DebugSession> {
        let inner = self.inner.lock().unwrap();
        let mut v: Vec<DebugSession> = inner
            .values()
            .filter(|s| s.purpose != "studio")
            .cloned()
            .collect();
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
        v
    }

    /// 素材创作隐藏会话:同一工作区 + 同一节点复用。
    pub fn find_studio(&self, workspace_id: Option<&str>, node_id: &str) -> Option<DebugSession> {
        self.inner.lock().unwrap().values().find(|s| {
            s.purpose == "studio"
                && s.studio_node_id.as_deref() == Some(node_id)
                && s.workspace_id.as_deref() == workspace_id
        }).cloned()
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
        workspace_id: Option<String>,
    ) -> DebugSession {
        let session = DebugSession::new(title, agent_kind, model_id, web_search, workspace_id);
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
        // 规格档位先验:非法值在拿到 &mut session 之前拒掉,避免半改状态。
        if let Some(Some(e)) = &req.reasoning_effort {
            let e = e.trim();
            if !e.is_empty() && !crate::modelspec::is_known_effort(e) {
                return Err(PatchError::InvalidModelSpec(format!("未知 effort 档: {e}")));
            }
        }
        if let Some(Some(c)) = &req.context_option_id {
            let c = c.trim();
            if !c.is_empty() && !crate::modelspec::is_known_context(c) {
                return Err(PatchError::InvalidModelSpec(format!("未知 context 档: {c}")));
            }
        }
        if let Some(e) = &req.agent_engine {
            let e = e.trim();
            if !e.is_empty() && !crate::codex::config::is_known_engine(e) {
                return Err(PatchError::InvalidModelSpec(format!(
                    "未知引擎: {e}(支持 local|codex)"
                )));
            }
        }
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
        if let Some(thinking) = req.thinking_enabled {
            session.thinking_enabled = thinking;
        }
        if let Some(effort) = &req.reasoning_effort {
            session.reasoning_effort = effort
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if let Some(ctx) = &req.context_option_id {
            session.context_option_id = ctx
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if let Some(workspace) = &req.workspace_id {
            session.workspace_id = workspace
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if let Some(engine) = req.agent_engine.as_deref().map(str::trim) {
            if !engine.is_empty() && engine != session.agent_engine {
                session.agent_engine = engine.to_string();
                // 切回本地再切回来时不该复用旧线程:那条线程的上下文里没有本地引擎
                // 期间发生的任何事,续上去只会让 Codex 基于过期认知继续干活。
                session.codex_thread_id = None;
            }
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

    /// 工作区删除级联:清引用该 workspaceId 的会话;返回清理数。
    pub fn clear_workspace(&self, workspace_id: &str) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let mut n = 0;
        for s in inner.values_mut() {
            if s.workspace_id.as_deref() == Some(workspace_id) {
                s.workspace_id = None;
                s.touch();
                n += 1;
            }
        }
        if n > 0 {
            self.persist_locked(&inner);
        }
        n
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

    pub fn create(&self, name: &str, workspace_id: Option<String>) -> Result<ChatFolder, FolderError> {
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
            workspace_id,
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

    /// 工作区删除级联:清引用该 workspaceId 的文件夹;返回清理数。
    pub fn clear_workspace(&self, workspace_id: &str) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.len();
        inner.retain(|f| f.workspace_id.as_deref() != Some(workspace_id));
        let cleared = before - inner.len();
        if cleared > 0 {
            self.persist_locked(&inner);
        }
        cleared
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
pub(crate) fn write_atomic(path: &FsPath, text: &str) -> std::io::Result<()> {
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
    /// 全屏主页无会话直发:把 Composer 里先勾的规格带进新会话;缺省 = 模型默认档。
    #[serde(default)]
    thinking_enabled: Option<bool>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    context_option_id: Option<String>,
    #[serde(default)]
    web_search_enabled: Option<bool>,
    #[serde(default)]
    workspace_id: Option<String>,
    /// 执行引擎(`local` | `codex`)。主页无会话时先切引擎再发送,创建时一并带上,
    /// 免得「先建成 local 再 PATCH 成 codex」中间那一瞬用错引擎发出首轮。
    #[serde(default)]
    agent_engine: Option<String>,
}

/// POST /api/forge/sessions → {session};发持久事件 session.created。
pub async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Response {
    if let Err(msg) = validate_create_spec(&req) {
        return bad_request("INVALID_MODEL_SPEC", &msg);
    }
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
    let mut session = state.sessions.create(
        &title,
        &kind,
        req.selected_model_id,
        req.web_search_enabled.unwrap_or(true),
        req.workspace_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    );
    if req.thinking_enabled.is_some()
        || req.reasoning_effort.is_some()
        || req.context_option_id.is_some()
        || req.agent_engine.is_some()
    {
        match state.sessions.patch(
            &session.id,
            &PatchSessionRequest {
                thinking_enabled: req.thinking_enabled,
                reasoning_effort: req.reasoning_effort.map(Some),
                context_option_id: req.context_option_id.map(Some),
                agent_engine: req.agent_engine,
                ..Default::default()
            },
        ) {
            Ok(s) => session = s,
            Err(PatchError::InvalidModelSpec(msg)) => {
                return bad_request("INVALID_MODEL_SPEC", &msg)
            }
            Err(_) => {}
        }
    }
    state.events.emit(
        EventDraft::new(&session.id, "session.created", "session")
            .payload(json!({ "sessionId": session.id, "title": session.title })),
    );
    Json(json!({ "session": session })).into_response()
}

fn validate_create_spec(req: &CreateSessionRequest) -> Result<(), String> {
    if let Some(e) = req
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !crate::modelspec::is_known_effort(e) {
            return Err(format!("未知 effort 档: {e}"));
        }
    }
    if let Some(c) = req
        .context_option_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !crate::modelspec::is_known_context(c) {
            return Err(format!("未知 context 档: {c}"));
        }
    }
    if let Some(e) = req
        .agent_engine
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !crate::codex::config::is_known_engine(e) {
            return Err(format!("未知引擎: {e}(支持 local|codex)"));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsureStudioSessionRequest {
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    selected_model_id: Option<String>,
}

/// POST /api/forge/studio/sessions {workspaceId?, nodeId} → {session}
/// 同一工作区+节点复用隐藏会话;默认 agentKind=studio,权限 auto。
pub async fn ensure_studio_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EnsureStudioSessionRequest>,
) -> Response {
    let node_id = req.node_id.trim().to_string();
    if node_id.is_empty() {
        return bad_request("INVALID_INPUT", "nodeId 不可空");
    }
    let ws = req
        .workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(existing) = state.sessions.find_studio(ws, &node_id) {
        return Json(json!({ "session": existing })).into_response();
    }
    let title = format!("studio:{node_id}");
    let mut session = state.sessions.create(
        &title,
        "studio",
        req.selected_model_id,
        false,
        ws.map(str::to_string),
    );
    session.purpose = "studio".into();
    session.studio_node_id = Some(node_id);
    session.touch();
    state.sessions.save(&session);
    let _ = state.permissions.set_mode(&session.id, "auto");
    Json(json!({ "session": session })).into_response()
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
        Err(PatchError::InvalidModelSpec(msg)) => bad_request("INVALID_MODEL_SPEC", &msg),
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
        src.workspace_id.clone(),
    );
    forked.folder_id = src.folder_id.clone();
    forked.thinking_enabled = src.thinking_enabled;
    forked.reasoning_effort = src.reasoning_effort.clone();
    forked.context_option_id = src.context_option_id.clone();
    // D-035:计划文件是工作区文件、两个会话共用同一份;分支会话继承指针,
    // Plan 页签在分支里照常可见可 Build(后续 create_plan 也会原地迭代同一文件)。
    forked.active_plan_path = src.active_plan_path.clone();
    // 引擎跟着分支走(在 Codex 会话上分叉,分支自然还是 Codex),但 Codex 线程**不拷**:
    // 一条 Codex 线程被两个会话同时续,两边的消息会互相串进对方的上下文。
    // 分支的首轮会新开一条线程,起点是分叉时的对话历史。
    forked.agent_engine = src.agent_engine.clone();
    forked.codex_thread_id = None;
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
#[serde(rename_all = "camelCase")]
pub struct FolderNameRequest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    workspace_id: Option<String>,
}

/// POST /api/forge/chat-folders {name} → {folder}(空名 400 INVALID_NAME)。
pub async fn create_chat_folder(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FolderNameRequest>,
) -> Response {
    match state.folders.create(
        &req.name,
        req.workspace_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    ) {
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
        let a = s.create("甲", "coding", None, true, None);
        std::thread::sleep(std::time::Duration::from_millis(3));
        let b = s.create("乙", "coding", Some("deepseek-chat".to_string()), false, None);
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
        let a = s.create("原题", "coding", None, true, None);
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
        let a = s.create("x", "coding", None, true, None);
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

    /// D-035:旧 sessions.json(无 activePlanPath)反序列化兼容;未设置不进 wire;
    /// 落库后可读回(Plan 页签刷新后据此重开)。
    #[test]
    fn active_plan_path_defaults_and_persists() {
        let dir = temp_dir("planptr");
        let old = r#"{ "sessions": [{
            "id": "sess_old", "title": "旧会话", "status": "idle", "agentKind": "coding",
            "webSearchEnabled": true, "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z", "pinned": false, "titleManuallySet": false
        }] }"#;
        std::fs::write(dir.join("sessions.json"), old).unwrap();
        let s = store(&dir);
        let got = s.get("sess_old").expect("旧会话可读回");
        assert!(got.active_plan_path.is_none());
        let wire = serde_json::to_value(&got).unwrap();
        assert!(wire.get("activePlanPath").is_none(), "未设置不该进 wire: {wire}");

        let mut got = got;
        got.active_plan_path = Some(".forge/plans/甲.plan.md".to_string());
        s.save(&got);
        let reread = store(&dir).get("sess_old").expect("落盘后可读回");
        assert_eq!(reread.active_plan_path.as_deref(), Some(".forge/plans/甲.plan.md"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn patch_selected_model_id_three_states() {
        // F7 wave.4:selectedModelId 三态(设置/缺省不变/null 清除),同 folderId 口径。
        let dir = temp_dir("model3");
        let s = store(&dir);
        let a = s.create("x", "coding", None, true, None);
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

    /// 规格波:三档落库 + 重启恢复 + 生造档位 400(当前模型是否支持交给 resolve 回落,此处只拦未知值)。
    #[test]
    fn patch_model_spec_persists_and_rejects_unknown_tier() {
        let dir = temp_dir("spec3");
        let s = store(&dir);
        let a = s.create("x", "coding", None, true, None);
        assert!(!a.thinking_enabled);
        assert!(a.reasoning_effort.is_none());
        assert!(a.context_option_id.is_none());

        let p1 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    thinking_enabled: Some(true),
                    reasoning_effort: Some(Some("xhigh".to_string())),
                    context_option_id: Some(Some("1m".to_string())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p1.thinking_enabled);
        assert_eq!(p1.reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(p1.context_option_id.as_deref(), Some("1m"));

        // 缺省不变 / null 清除(effort、context 同 folderId 三态;thinking 是纯布尔)。
        let p2 = s.patch(&a.id, &PatchSessionRequest::default()).unwrap();
        assert_eq!(p2.reasoning_effort.as_deref(), Some("xhigh"), "缺省不变");
        let p3 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    reasoning_effort: Some(None),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p3.reasoning_effort.is_none(), "Some(None) 清除");

        // 经 HTTP 只能拿空串表达「清回默认档」(wire 上 null 与缺省不可分,见字段注);
        // 空串既要绕过档位先验,又要落到 None。
        let p4 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    context_option_id: Some(Some("  ".to_string())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p4.context_option_id.is_none(), "空串清回默认档");
        let p5 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    context_option_id: Some(Some("1m".to_string())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p5.context_option_id.as_deref(), Some("1m"));

        // 生造档位一律拒绝,且拒绝时不得半改(context 仍是上一轮的 1m)。
        for bad in [
            PatchSessionRequest {
                reasoning_effort: Some(Some("ludicrous".to_string())),
                context_option_id: Some(Some("64k".to_string())),
                ..Default::default()
            },
            PatchSessionRequest {
                context_option_id: Some(Some("9m".to_string())),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                s.patch(&a.id, &bad),
                Err(PatchError::InvalidModelSpec(_))
            ));
        }
        assert_eq!(
            s.get(&a.id).unwrap().context_option_id.as_deref(),
            Some("1m"),
            "拒绝的 PATCH 不得留下半改状态"
        );

        // 重启恢复:三档随 sessions.json 落盘。
        let s2 = store(&dir);
        let re = s2.get(&a.id).unwrap();
        assert!(re.thinking_enabled);
        assert_eq!(re.context_option_id.as_deref(), Some("1m"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn patch_workspace_id_three_states() {
        let dir = temp_dir("ws3");
        let s = store(&dir);
        let a = s.create("x", "coding", None, true, None);
        let p1 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    workspace_id: Some(Some("ws_1".to_string())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p1.workspace_id.as_deref(), Some("ws_1"));
        let p2 = s.patch(&a.id, &PatchSessionRequest::default()).unwrap();
        assert_eq!(p2.workspace_id.as_deref(), Some("ws_1"));
        let p3 = s
            .patch(
                &a.id,
                &PatchSessionRequest {
                    workspace_id: Some(None),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p3.workspace_id.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn folders_crud_and_cascade_clear() {
        let dir = temp_dir("folders");
        let s = store(&dir);
        let f = ChatFolderStore::load(dir.join("chat-folders.json"));
        // 空名 400 语义。
        assert!(matches!(f.create("  ", None), Err(FolderError::InvalidName)));
        let fld = f.create("工作", None).unwrap();
        assert!(fld.id.starts_with("fld_"));
        // 改名。
        let r = f.rename(&fld.id, "工作区").unwrap();
        assert_eq!(r.name, "工作区");
        assert!(matches!(f.rename(&fld.id, ""), Err(FolderError::InvalidName)));
        assert!(matches!(f.rename("fld_none", "x"), Err(FolderError::NotFound)));
        // 会话挂 folder → 删文件夹级联清。
        let a = s.create("挂接", "coding", None, true, None);
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

    #[test]
    fn studio_sessions_hidden_from_list_and_reusable() {
        let dir = temp_dir("studio");
        let s = store(&dir);
        let mut a = s.create("chat", "coding", None, true, None);
        a.purpose = "studio".into();
        a.studio_node_id = Some("n1".into());
        a.workspace_id = Some("ws_a".into());
        s.save(&a);
        let b = s.create("可见", "coding", None, true, None);
        let list = s.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, b.id);
        assert_eq!(s.find_studio(Some("ws_a"), "n1").unwrap().id, a.id);
        assert!(s.find_studio(Some("ws_b"), "n1").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
