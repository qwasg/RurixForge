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

use crate::events::{new_id, now_rfc3339, DebugEvent, EventDraft};
use crate::ultraplan::UltraPlanState;
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
    /// D-044:UltraPlan 流程状态(阶段机 + 产物目录指针;无流程 → None,不进 wire)。
    /// 随会话一起经 sessions REST 与 design-snapshot 的 activeSession 暴露。
    /// 写入一律走 [`SessionStore::update_ultraplan`];fork 不拷贝(见 fork_session)。
    /// 宽松反序列化:状态块损坏只丢流程状态,不连累整条会话解析失败。
    #[serde(
        default,
        deserialize_with = "crate::ultraplan::deserialize_lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub ultraplan: Option<UltraPlanState>,
    /// D-045:Design 流程状态(设计稿审阅 + 原子级复刻;无流程 → None,不进 wire)。
    /// 写入一律走 [`SessionStore::update_design`];fork 不拷贝;宽松反序列化同 ultraplan。
    #[serde(
        default,
        deserialize_with = "crate::design::deserialize_lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub design: Option<crate::design::DesignState>,
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
            ultraplan: None,
            design: None,
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
    /// D-044:UltraPlan 流程进行中(stage != done)不许换工作区——流程产物路径是工作区相对的,
    /// 换根等于让后续轮次去另一个目录找 spec/demo/计划。
    UltraplanWorkspaceLocked,
}

/// D-044:[`SessionStore::update_ultraplan_idle`] 的拒绝原因。
#[derive(Debug, PartialEq, Eq)]
pub enum IdleUpdateError {
    NotFound,
    /// 会话有运行中的 run(携其 id)。
    Busy(String),
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

    /// sessions.json 路径(同目录的 summaries.json 等会话级旁挂文件据此定位)。
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
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

    /// D-044 同源清扫:UltraPlan 流程的 `phase == running` 同样只在有 run 在跑时才成立,
    /// 进程重启后必然是残留。不清的话流程条永远显示「正在处理」,而前端卡片在 running 相位下
    /// 一律不可交互——用户连重试都点不了。改成 failed + lastError(阶段不动,重发同一动作即重试),
    /// 返回 (sessionId, 改后的状态) 清单,由调用方补发 `ultraplan.stage`。
    /// `running` 保留(与 finish_turn 的失败口径一致:前端据此说清断的是哪一轮);失败码用契约 §2
    /// 的通用码,「进程重启打断」写在 message 里。
    /// 不 touch 会话 updatedAt:这是恢复不是用户活动,不该打乱侧栏排序。
    pub fn sweep_stale_ultraplan_runs(&self) -> Vec<(String, UltraPlanState)> {
        let mut inner = self.inner.lock().unwrap();
        let mut swept: Vec<(String, UltraPlanState)> = Vec::new();
        for s in inner.values_mut() {
            let Some(up) = s.ultraplan.as_mut() else {
                continue;
            };
            if up.phase != crate::ultraplan::PHASE_RUNNING {
                continue;
            }
            up.phase = crate::ultraplan::PHASE_FAILED.to_string();
            up.last_error = Some(crate::ultraplan::FlowError {
                code: crate::ultraplan::ERR_TURN_INVALID.to_string(),
                message: "agentd 进程重启,上一轮中断;重新发送同一操作即可重试".to_string(),
            });
            up.touch();
            swept.push((s.id.clone(), up.clone()));
        }
        if !swept.is_empty() {
            self.persist_locked(&inner);
            eprintln!(
                "[sessions] 清扫 stale UltraPlan running 相位 × {}(进程重启崩溃恢复)",
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

    /// Include hidden Studio sessions when protecting a workspace's live actors.
    pub fn workspace_session_ids(&self, workspace_id: &str) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.workspace_id.as_deref() == Some(workspace_id))
            .map(|session| session.id.clone())
            .collect()
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
        self.inner
            .lock()
            .unwrap()
            .values()
            .find(|s| {
                s.purpose == "studio"
                    && s.studio_node_id.as_deref() == Some(node_id)
                    && s.workspace_id.as_deref() == workspace_id
            })
            .cloned()
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

    /// Publish the active plan without overwriting concurrent session settings.
    pub fn set_ultraplan_active_plan(
        &self,
        id: &str,
        flow_id: &str,
        run_id: &str,
        path: &str,
    ) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let Some(session) = inner.get_mut(id) else {
            return false;
        };
        if session.active_run_id.as_deref() != Some(run_id)
            || !session
                .ultraplan
                .as_ref()
                .is_some_and(|up| up.id == flow_id && up.plan_path.as_deref() == Some(path))
        {
            return false;
        }
        session.active_plan_path = Some(path.into());
        session.touch();
        self.persist_locked(&inner);
        true
    }

    /// D-044:在存贮锁内读-改-写 UltraPlan 流程状态并落盘;返回 (改后的会话, 闭包返回值)。
    /// 会话不存在 → None(闭包不执行)。
    ///
    /// 为什么不用 `get` + `save`:那是整结构覆盖,轮次收尾写阶段、REST 清状态、出口工具推进阶段
    /// 三者交错时后写者会吃掉先写者。闭包里做 compare-and-set(核对 id 与期望阶段再改)即可
    /// 得到原子的阶段推进。闭包没改动状态时不落盘、不动 updatedAt(CAS 落空是常态,
    /// 不该把会话顶到侧栏最前)。
    pub fn update_ultraplan<R>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Option<UltraPlanState>) -> R,
    ) -> Option<(DebugSession, R)> {
        let mut inner = self.inner.lock().unwrap();
        let session = inner.get_mut(id)?;
        let out = Self::apply_ultraplan(session, f);
        let snapshot = session.clone();
        if out.1 {
            self.persist_locked(&inner);
        }
        Some((snapshot, out.0))
    }

    /// 同 [`Self::update_ultraplan`],但要求会话空闲:有运行中的 run → `Busy`,闭包不执行。
    /// 忙判定与改写在同一把锁内,不产生轮次的 REST 操作(重新开始 / 验收 / 回退 Demo)用它,
    /// 不留「查完空闲、改之前被一轮 turn 认领」的窗口。
    pub fn update_ultraplan_idle<R>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Option<UltraPlanState>) -> R,
    ) -> Result<(DebugSession, R), IdleUpdateError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(session) = inner.get_mut(id) else {
            return Err(IdleUpdateError::NotFound);
        };
        if let Some(run) = &session.active_run_id {
            return Err(IdleUpdateError::Busy(run.clone()));
        }
        let out = Self::apply_ultraplan(session, f);
        let snapshot = session.clone();
        if out.1 {
            self.persist_locked(&inner);
        }
        Ok((snapshot, out.0))
    }

    /// D-045:Design 流程状态的读-改-写(存贮锁内;语义同 [`Self::update_ultraplan`])。
    pub fn update_design<R>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Option<crate::design::DesignState>) -> R,
    ) -> Option<(DebugSession, R)> {
        let mut inner = self.inner.lock().unwrap();
        let session = inner.get_mut(id)?;
        let out = Self::apply_design(session, f);
        let snapshot = session.clone();
        if out.1 {
            self.persist_locked(&inner);
        }
        Some((snapshot, out.0))
    }

    /// 同 [`Self::update_design`],但要求会话空闲(重新开始等不产生轮次的 REST 操作用)。
    pub fn update_design_idle<R>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Option<crate::design::DesignState>) -> R,
    ) -> Result<(DebugSession, R), IdleUpdateError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(session) = inner.get_mut(id) else {
            return Err(IdleUpdateError::NotFound);
        };
        if let Some(run) = &session.active_run_id {
            return Err(IdleUpdateError::Busy(run.clone()));
        }
        let out = Self::apply_design(session, f);
        let snapshot = session.clone();
        if out.1 {
            self.persist_locked(&inner);
        }
        Ok((snapshot, out.0))
    }

    fn apply_design<R>(
        session: &mut DebugSession,
        f: impl FnOnce(&mut Option<crate::design::DesignState>) -> R,
    ) -> (R, bool) {
        let before = session.design.clone();
        let out = f(&mut session.design);
        let changed = session.design != before;
        if changed {
            if let Some(d) = session.design.as_mut() {
                d.touch();
            }
            session.touch();
        }
        (out, changed)
    }

    /// D-045:Design 流程的 running 相位同样只在有 run 时成立;重启后清成 failed(可重试)。
    pub fn sweep_stale_design_runs(&self) -> Vec<(String, crate::design::DesignState)> {
        let mut inner = self.inner.lock().unwrap();
        let mut swept = Vec::new();
        for s in inner.values_mut() {
            let Some(d) = s.design.as_mut() else { continue };
            if d.phase != crate::design::PHASE_RUNNING {
                continue;
            }
            d.phase = crate::design::PHASE_FAILED.to_string();
            d.last_error = Some(crate::design::FlowError {
                code: crate::design::ERR_TURN_INVALID.to_string(),
                message: "agentd 进程重启,上一轮中断;重新发送同一操作即可重试".to_string(),
            });
            d.touch();
            swept.push((s.id.clone(), d.clone()));
        }
        if !swept.is_empty() {
            self.persist_locked(&inner);
        }
        swept
    }

    /// revert 收尾:锁内清 activeRunId(`reset_status` 时一并把 status 复位 idle),返回改后的会话。
    /// 只动这两个字段——revert 在读会话与写回之间要重写事件文件(磁盘 IO),
    /// 拿开头那份快照整体 `save` 会把这段时间里别处写入的字段(如 UltraPlan 阶段)盖回去。
    pub fn reset_after_revert(&self, id: &str, reset_status: bool) -> Option<DebugSession> {
        let mut inner = self.inner.lock().unwrap();
        let session = inner.get_mut(id)?;
        session.active_run_id = None;
        if reset_status {
            session.status = default_status();
        }
        session.touch();
        let out = session.clone();
        self.persist_locked(&inner);
        Some(out)
    }

    /// 对会话的流程状态跑闭包;返回 (闭包返回值, 是否有改动)。有改动时顺手刷新两处 updatedAt。
    fn apply_ultraplan<R>(
        session: &mut DebugSession,
        f: impl FnOnce(&mut Option<UltraPlanState>) -> R,
    ) -> (R, bool) {
        let before = session.ultraplan.clone();
        let out = f(&mut session.ultraplan);
        let changed = session.ultraplan != before;
        if changed {
            if let Some(up) = session.ultraplan.as_mut() {
                up.touch();
            }
            session.touch();
        }
        (out, changed)
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
                return Err(PatchError::InvalidModelSpec(format!(
                    "未知 context 档: {c}"
                )));
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
        // D-044:流程进行中拒绝换工作区。与规格档位同理放在任何字段落笔之前(拒绝不留半改状态);
        // 值没变的 PATCH(前端整包回写)照常放行。
        if let Some(workspace) = &req.workspace_id {
            let next = workspace
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let flow_active = session
                .ultraplan
                .as_ref()
                .is_some_and(UltraPlanState::is_active);
            if flow_active && next != session.workspace_id {
                return Err(PatchError::UltraplanWorkspaceLocked);
            }
        }
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

    pub fn create(
        &self,
        name: &str,
        workspace_id: Option<String>,
    ) -> Result<ChatFolder, FolderError> {
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
            eprintln!(
                "chat-folders.json 解析失败({}): {e},按空处理",
                path.display()
            );
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

fn conflict(code: &str, message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

/// D-044:回退截断(保留 seq <= cutoff)是否会切掉本会话进行中流程的 `ultraplan.*` 事件。
///
/// 回退只截事件流,不动会话字段:若放行,阶段还停在比如 plan_review,而产出问卷/Demo 的那几轮
/// 已从对话里消失——卡片没了、状态还在,用户既看不到也推不动。已结束(done)的流程不拦:
/// 它不再有待办的闸,截掉的只是历史。按流程 id 比对,旧流程(已重新开始)的事件也不拦。
pub(crate) fn revert_cuts_active_flow(
    session: &DebugSession,
    events: &[DebugEvent],
    cutoff: i64,
) -> bool {
    let Some(up) = session.ultraplan.as_ref().filter(|u| u.is_active()) else {
        return false;
    };
    events.iter().any(|e| {
        e.seq > cutoff
            && e.event_type.starts_with("ultraplan.")
            && e.payload.get("id").and_then(Value::as_str) == Some(up.id.as_str())
    })
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
    let defaults = match crate::agent_settings::load(&state) {
        Ok(defaults) => defaults,
        Err(message) => return (StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"code":"CONFIG_READ_FAILED","message":message}}))).into_response(),
    };
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
    if let Err(message) = state.permissions.set_mode(&session.id, &defaults.default_permission_mode) {
        state.sessions.delete(&session.id);
        return (StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"code":"PERMISSION_SAVE_FAILED","message":message}}))).into_response();
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
    let existing = state.sessions.get(&id);
    let changes_execution_context = existing.as_ref().is_some_and(|session| {
        req.agent_engine
            .as_deref()
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .is_some_and(|engine| engine != session.agent_engine)
            || req.workspace_id.as_ref().is_some_and(|workspace| {
                workspace
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    != session.workspace_id
            })
    });
    let patched = if changes_execution_context {
        match state
            .collaboration
            .with_inactive_team(&id, || state.sessions.patch(&id, &req))
        {
            Ok(result) => result,
            Err(error) => return error.into_response(),
        }
    } else {
        state.sessions.patch(&id, &req)
    };
    match patched {
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
        Err(PatchError::UltraplanWorkspaceLocked) => conflict(
            "ULTRAPLAN_WORKSPACE_LOCKED",
            "UltraPlan 流程进行中,不能更换工作区(流程产物在当前工作区内);请先完成或重新开始流程",
        ),
    }
}

/// DELETE /api/forge/sessions/{id} → {ok:true}(删元信息 + 事件文件)。
pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    if state.sessions.get(&id).is_none() {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    }
    let participants = state.collaboration.agents(&id);
    if let Err(error) = state.collaboration.close_session(&id) {
        return error.into_response();
    }
    for participant in participants {
        if let Some(run_id) = participant.active_run_id {
            state.runs.cancel(&run_id);
        }
    }
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
    if let Err(message) = state.permissions.set_mode(&forked.id, &state.permissions.mode(&id)) {
        state.sessions.delete(&forked.id);
        return (StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"code":"PERMISSION_SAVE_FAILED","message":message}}))).into_response();
    }
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
    // D-044:UltraPlan 流程**不拷**。流程 id、产物目录与 Demo 令牌都是一条流程一份:
    // 两个会话共用同一份状态,就会各自推进阶段、往同一个目录写 spec/demo,制作阶段还会把
    // 同一批待办物化两遍。分支里回放出的流程卡片因此是只读的历史(没有状态可对上),
    // 要在分支里继续就重新开一条流程。
    forked.ultraplan = None;
    // D-045:Design 流程同理不拷(一条流程一个目录与一套候选批次)。
    forked.design = None;
    // Collaboration actors, mailbox leases and task ownership are keyed by the
    // original session in a separate store. Only historical event text is copied.
    // A fork receives its own root identity and starts without a live team.
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
/// 重写 JSONL + 内存缓冲 + latestSeq;无 messageId 保持历史;发 session.reverted。
/// (参考 truncate_before_event 还会回卷同 run 前导事件——wave.1 无 run,纯 seq 截断,留痕。)
/// D-044:截断若会切掉进行中 UltraPlan 流程的 `ultraplan.*` 事件 → 409 ULTRAPLAN_REVERT_BLOCKED,
/// 事件流与会话都不动(见 [`revert_cuts_active_flow`])。
pub async fn revert_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RevertRequest>,
) -> Response {
    let Some(session) = state.sessions.get(&id) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    if state.collaboration.has_open_team(&id) {
        return conflict(
            "TEAM_REVERT_BLOCKED",
            "团队尚未结束，回退会使任务板与执行历史不一致；请先停止或完成团队",
        );
    }
    let message_id = req.message_id.clone().filter(|m| !m.trim().is_empty());
    let mut truncate_to = None;
    if let Some(message_id) = &message_id {
        let Some(target_seq) = state.events.seq_of_event(&id, message_id) else {
            return not_found("EVENT_NOT_FOUND", format!("事件不存在: {message_id}"));
        };
        let cutoff = if req.mode.as_deref() == Some("before") {
            target_seq - 1
        } else {
            target_seq
        };
        // 查磁盘全文而非环缓冲:流程可能跨很多轮,早期事件早已滑出内存窗口。
        // 只有带着进行中流程的会话才多读这一次盘。
        let flow_active = session
            .ultraplan
            .as_ref()
            .is_some_and(UltraPlanState::is_active);
        if flow_active && revert_cuts_active_flow(&session, &state.events.persisted(&id), cutoff) {
            return conflict(
                "ULTRAPLAN_REVERT_BLOCKED",
                "回退会截掉进行中的 UltraPlan 流程记录(问卷 / Demo / 计划的卡片会消失而流程仍停在原阶段);\
                 请先完成流程,或在流程条上「重新开始」后再回退",
            );
        }
        truncate_to = Some(cutoff);
    }
    if session.active_run_id.is_some()
        || state
            .collaboration
            .agents(&id)
            .iter()
            .any(|p| p.active_run_id.is_some())
    {
        return conflict(
            "SESSION_BUSY",
            "主 agent 或后台子 agent 仍在执行，请先等待完成或停止运行再回退",
        );
    }
    // Use the same CAS as turn startup so no root can start between validation,
    // mailbox reset and event truncation. This reservation never starts a model.
    let reservation_id = new_id("revert");
    if state
        .sessions
        .claim_active_run(&id, &reservation_id)
        .is_err()
    {
        return conflict("SESSION_BUSY", "会话状态已改变，请刷新后重试回退");
    }
    struct RevertReservation<'a> {
        sessions: &'a SessionStore,
        sid: &'a str,
        run_id: &'a str,
    }
    impl Drop for RevertReservation<'_> {
        fn drop(&mut self) {
            self.sessions.release_active_run(self.sid, self.run_id);
        }
    }
    let _reservation = RevertReservation {
        sessions: &state.sessions,
        sid: &id,
        run_id: &reservation_id,
    };
    // A detached job reserves its receipt before waiting for a worker slot.
    // It may not have a participant yet, so check again after the session CAS
    // prevents any further root dispatches from entering this history branch.
    if state.receipts.has_running(&id) {
        return conflict(
            "SESSION_BUSY",
            "后台子 agent 仍在执行或排队，请先等待完成或停止运行再回退",
        );
    }
    if let Some(cutoff) = truncate_to {
        if let Err(error) = state.collaboration.reset_after_revert(&id) {
            return error.into_response();
        }
        state.events.truncate_to_seq(&id, cutoff);
    }
    let Some(session) = state.sessions.reset_after_revert(&id, message_id.is_some()) else {
        return not_found("SESSION_NOT_FOUND", format!("会话不存在: {id}"));
    };
    state.events.emit(
        EventDraft::new(&id, "session.reverted", "session").payload(json!({
            "sessionId": id,
            "messageId": req.message_id,
            "mode": req.mode,
        })),
    );
    if message_id.is_some() {
        crate::collaboration_runtime::emit_snapshot(&state, &id);
    }
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

    fn seed_team(state: &AppState, sid: &str) -> crate::collaboration::TeamState {
        let root = crate::collaboration::root_id(sid);
        state
            .collaboration
            .register(crate::collaboration::AgentRegistration {
                id: root.clone(),
                session_id: sid.into(),
                parent_agent_id: None,
                team_id: None,
                name: "leader".into(),
                role: "root".into(),
                engine: "local".into(),
            })
            .unwrap();
        state
            .collaboration
            .create_team(
                sid,
                &root,
                &crate::collaboration::CreateTeamRequest {
                    name: "team".into(),
                    max_parallel: 4,
                    max_fix_rounds: 3,
                },
            )
            .unwrap()
    }

    #[tokio::test]
    async fn open_team_locks_engine_changes_without_partially_editing_session() {
        let (state, dir) = crate::test_app_state("team-engine-lock");
        let session = state.sessions.create("before", "coding", None, true, None);
        state
            .sessions
            .patch(
                &session.id,
                &PatchSessionRequest {
                    agent_engine: Some("local".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let team = seed_team(&state, &session.id);
        for status in ["active", "paused", "blocked", "recoveryRequired"] {
            state
                .collaboration
                .set_team_status(&team.id, status)
                .unwrap();
            let result = patch_session(
                State(state.clone()),
                Path(session.id.clone()),
                Json(PatchSessionRequest {
                    agent_engine: Some("codex".into()),
                    title: Some("must not change".into()),
                    ..Default::default()
                }),
            )
            .await;
            assert_eq!(result.status(), StatusCode::CONFLICT, "{status}");
            let after = state.sessions.get(&session.id).unwrap();
            assert_eq!(after.agent_engine, "local");
            assert_eq!(after.title, "before");
        }
        state.collaboration.control_team(&team.id, "stop").unwrap();
        let result = patch_session(
            State(state.clone()),
            Path(session.id.clone()),
            Json(PatchSessionRequest {
                agent_engine: Some("codex".into()),
                ..Default::default()
            }),
        )
        .await;
        assert_eq!(result.status(), StatusCode::OK);
        assert_eq!(
            state.sessions.get(&session.id).unwrap().agent_engine,
            "codex"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn fork_does_not_copy_team_actors_and_revert_requires_closed_team() {
        let (state, dir) = crate::test_app_state("team-fork-revert");
        let session = state.sessions.create("source", "coding", None, true, None);
        let team = seed_team(&state, &session.id);
        state
            .collaboration
            .enqueue(
                &session.id,
                None,
                &team.leader_agent_id,
                &crate::collaboration::SendMessageRequest {
                annotations: Vec::new(),
                    text: "pending".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        crate::collaboration::emit_team(&state, &team);
        let fork =
            body_json(fork_session(State(state.clone()), Path(session.id.clone())).await).await;
        let fork_id = fork["session"]["id"].as_str().unwrap();
        assert!(state.collaboration.latest_team(fork_id).is_none());
        assert!(state.collaboration.agents(fork_id).is_empty());
        assert!(state
            .collaboration
            .messages(&crate::collaboration::root_id(fork_id))
            .is_empty());
        let blocked = revert_session(
            State(state.clone()),
            Path(session.id.clone()),
            Json(RevertRequest {
                message_id: None,
                mode: None,
            }),
        )
        .await;
        assert_eq!(blocked.status(), StatusCode::CONFLICT);
        assert_eq!(state.collaboration.messages(&team.leader_agent_id).len(), 1);
        state.collaboration.control_team(&team.id, "stop").unwrap();
        let allowed = revert_session(
            State(state.clone()),
            Path(session.id.clone()),
            Json(RevertRequest {
                message_id: None,
                mode: None,
            }),
        )
        .await;
        assert_eq!(allowed.status(), StatusCode::OK);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn deleting_session_cancels_registered_actors_and_closes_mail() {
        let (state, dir) = crate::test_app_state("team-delete");
        let session = state.sessions.create("source", "coding", None, true, None);
        let team = seed_team(&state, &session.id);
        let (run, _) = state.runs.begin(&session.id, "test");
        state
            .collaboration
            .begin_run(&team.leader_agent_id, &run.id)
            .unwrap();
        state
            .collaboration
            .enqueue(
                &session.id,
                None,
                &team.leader_agent_id,
                &crate::collaboration::SendMessageRequest {
                annotations: Vec::new(),
                    text: "pending".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let result = delete_session(State(state.clone()), Path(session.id.clone())).await;
        assert_eq!(result.status(), StatusCode::OK);
        assert!(state.runs.is_cancelled(&run.id));
        assert_eq!(
            state.collaboration.messages(&team.leader_agent_id)[0].status,
            "failed"
        );
        assert_eq!(
            state.collaboration.team(&team.id).unwrap().status,
            "stopped"
        );
        assert!(state.sessions.get(&session.id).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn revert_rejects_running_root_then_clears_old_mail_and_history_without_closing_identity()
    {
        let (state, dir) = crate::test_app_state("collaboration-revert");
        let session = state.sessions.create("source", "coding", None, true, None);
        let team = seed_team(&state, &session.id);
        state.collaboration.control_team(&team.id, "stop").unwrap();
        let root = team.leader_agent_id;
        state
            .collaboration
            .save_history(&root, vec![json!({"role":"user","content":"old branch"})])
            .unwrap();
        state
            .collaboration
            .enqueue(
                &session.id,
                None,
                &root,
                &crate::collaboration::SendMessageRequest {
                annotations: Vec::new(),
                    text: "old steering".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let target = state.events.emit(
            EventDraft::new(&session.id, "composer.user.message", "user")
                .payload(json!({"text":"kept"})),
        );
        let later = state.events.emit(
            EventDraft::new(&session.id, "agent.message", "agent")
                .payload(json!({"text":"removed"})),
        );
        let request = || {
            Json(RevertRequest {
                message_id: Some(target.id.clone()),
                mode: None,
            })
        };
        state
            .sessions
            .claim_active_run(&session.id, "root-run")
            .unwrap();
        state.collaboration.begin_run(&root, "root-run").unwrap();
        let response =
            revert_session(State(state.clone()), Path(session.id.clone()), request()).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(body_json(response).await["error"]["code"], "SESSION_BUSY");
        assert!(state.events.seq_of_event(&session.id, &later.id).is_some());
        assert_eq!(state.collaboration.messages(&root)[0].status, "queued");
        state.sessions.release_active_run(&session.id, "root-run");
        let response =
            revert_session(State(state.clone()), Path(session.id.clone()), request()).await;
        assert_eq!(
            response.status(),
            StatusCode::CONFLICT,
            "durable actor remains active until its guard releases it"
        );
        state.collaboration.end_run(&root, "root-run").unwrap();
        assert_eq!(
            revert_session(State(state.clone()), Path(session.id.clone()), request())
                .await
                .status(),
            StatusCode::OK
        );
        assert!(state.events.seq_of_event(&session.id, &later.id).is_none());
        assert_eq!(state.collaboration.messages(&root)[0].status, "failed");
        assert!(state.collaboration.history(&root).is_empty());
        assert_eq!(
            state.collaboration.participant(&root).unwrap().status,
            "idle"
        );
        assert!(state
            .sessions
            .get(&session.id)
            .unwrap()
            .active_run_id
            .is_none());
        state
            .sessions
            .claim_active_run(&session.id, "new-root-run")
            .unwrap();
        state
            .collaboration
            .begin_run(&root, "new-root-run")
            .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn revert_waits_for_async_detached_actor_even_when_root_is_idle() {
        use crate::collaboration::{AgentRegistration, SendMessageRequest};
        let (state, dir) = crate::test_app_state("detached-revert");
        let session = state.sessions.create("source", "coding", None, false, None);
        let team = seed_team(&state, &session.id);
        state.collaboration.control_team(&team.id, "stop").unwrap();
        let root = team.leader_agent_id;
        state
            .collaboration
            .save_history(&root, vec![json!({"role":"user","content":"old branch"})])
            .unwrap();
        state
            .collaboration
            .enqueue(
                &session.id,
                None,
                &root,
                &SendMessageRequest {
                annotations: Vec::new(),
                    text: "keep until detached settles".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let target = state.events.emit(
            EventDraft::new(&session.id, "composer.user.message", "user")
                .payload(json!({"text":"kept"})),
        );
        let later = state.events.emit(
            EventDraft::new(&session.id, "agent.message", "agent")
                .payload(json!({"text":"removed on revert"})),
        );
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let background_state = state.clone();
        let background_sid = session.id.clone();
        let background_root = root.clone();
        let background = tokio::spawn(async move {
            let (run, _) = background_state
                .runs
                .begin(&background_sid, "multitask_dispatch");
            background_state
                .collaboration
                .register(AgentRegistration {
                    id: "detached-worker".into(),
                    session_id: background_sid.clone(),
                    parent_agent_id: Some(background_root.clone()),
                    team_id: None,
                    name: "detached worker".into(),
                    role: "subagent".into(),
                    engine: "local".into(),
                })
                .unwrap();
            background_state
                .collaboration
                .begin_run("detached-worker", &run.id)
                .unwrap();
            started_tx.send(()).unwrap();
            finish_rx.await.unwrap();
            // Delivery settles while the actor remains active, matching the
            // host's detached completion boundary.
            background_state
                .collaboration
                .enqueue_receipt(
                    &background_sid,
                    "detached-worker",
                    &background_root,
                    &SendMessageRequest {
                annotations: Vec::new(),
                        text: "old branch detached result".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
            background_state
                .collaboration
                .end_run("detached-worker", &run.id)
                .unwrap();
            background_state
                .collaboration
                .stop_agent("detached-worker")
                .unwrap();
            background_state.runs.finish(&run.id, "completed");
        });
        started_rx.await.unwrap();
        assert!(
            state
                .sessions
                .get(&session.id)
                .unwrap()
                .active_run_id
                .is_none(),
            "the root turn has already ended"
        );
        assert!(state
            .collaboration
            .participant(&root)
            .unwrap()
            .active_run_id
            .is_none());
        assert_eq!(
            state
                .collaboration
                .reset_after_revert(&session.id)
                .unwrap_err()
                .code,
            "SESSION_BUSY",
            "core must recheck under its atomic mutation lock"
        );
        let request = || {
            Json(RevertRequest {
                message_id: Some(target.id.clone()),
                mode: None,
            })
        };
        let rejected =
            revert_session(State(state.clone()), Path(session.id.clone()), request()).await;
        assert_eq!(rejected.status(), StatusCode::CONFLICT);
        assert_eq!(body_json(rejected).await["error"]["code"], "SESSION_BUSY");
        assert!(state.events.seq_of_event(&session.id, &later.id).is_some());
        assert!(!state.collaboration.history(&root).is_empty());
        assert_eq!(state.collaboration.messages(&root)[0].status, "queued");
        finish_tx.send(()).unwrap();
        background.await.unwrap();
        assert_eq!(
            revert_session(State(state.clone()), Path(session.id.clone()), request())
                .await
                .status(),
            StatusCode::OK
        );
        assert!(state.events.seq_of_event(&session.id, &later.id).is_none());
        let messages = state.collaboration.messages(&root);
        assert_eq!(messages.len(), 2);
        assert!(
            messages.iter().all(|m| m.status == "failed"),
            "all old-branch mail stays archived: {messages:?}"
        );
        assert!(state.collaboration.history(&root).is_empty());
        assert_eq!(
            state
                .collaboration
                .participant("detached-worker")
                .unwrap()
                .status,
            "stopped"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn revert_waits_for_queued_detached_receipt_before_participant_registration() {
        let (state, dir) = crate::test_app_state("queued-detached-revert");
        let session = state.sessions.create("source", "coding", None, false, None);
        let target = state.events.emit(
            EventDraft::new(&session.id, "composer.user.message", "user")
                .payload(json!({"text":"keep"})),
        );
        let later = state.events.emit(
            EventDraft::new(&session.id, "agent.message", "agent")
                .payload(json!({"text":"old branch"})),
        );
        // Dispatch has returned and the parent is idle, but the background job
        // is still waiting for a semaphore slot and has no actor registration.
        state.receipts.begin(
            &session.id,
            "queued-background",
            "finished-root",
            None,
            "waiting for slot",
        );
        assert!(state
            .sessions
            .get(&session.id)
            .unwrap()
            .active_run_id
            .is_none());
        assert!(state.collaboration.agents(&session.id).is_empty());
        let request = || {
            Json(RevertRequest {
                message_id: Some(target.id.clone()),
                mode: None,
            })
        };
        let rejected =
            revert_session(State(state.clone()), Path(session.id.clone()), request()).await;
        assert_eq!(rejected.status(), StatusCode::CONFLICT);
        assert_eq!(body_json(rejected).await["error"]["code"], "SESSION_BUSY");
        assert!(state.events.seq_of_event(&session.id, &later.id).is_some());
        assert!(
            state
                .sessions
                .get(&session.id)
                .unwrap()
                .active_run_id
                .is_none(),
            "the failed revert releases its CAS reservation"
        );
        assert!(!state
            .events
            .persisted(&session.id)
            .iter()
            .any(|e| e.event_type == "session.reverted"));
        state
            .receipts
            .finish_with_delivery(
                "queued-background",
                "cancelled",
                "cancelled before starting",
                true,
            )
            .unwrap();
        assert_eq!(
            revert_session(State(state.clone()), Path(session.id.clone()), request())
                .await
                .status(),
            StatusCode::OK
        );
        assert!(state.events.seq_of_event(&session.id, &later.id).is_none());
        assert!(state
            .sessions
            .get(&session.id)
            .unwrap()
            .active_run_id
            .is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn crud_sort_and_reload() {
        let dir = temp_dir("crud");
        let s = store(&dir);
        let a = s.create("甲", "coding", None, true, None);
        std::thread::sleep(std::time::Duration::from_millis(3));
        let b = s.create(
            "乙",
            "coding",
            Some("deepseek-chat".to_string()),
            false,
            None,
        );
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
        assert!(
            wire.get("activePlanPath").is_none(),
            "未设置不该进 wire: {wire}"
        );

        let mut got = got;
        got.active_plan_path = Some(".forge/plans/甲.plan.md".to_string());
        s.save(&got);
        let reread = store(&dir).get("sess_old").expect("落盘后可读回");
        assert_eq!(
            reread.active_plan_path.as_deref(),
            Some(".forge/plans/甲.plan.md")
        );
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
        assert!(matches!(
            f.create("  ", None),
            Err(FolderError::InvalidName)
        ));
        let fld = f.create("工作", None).unwrap();
        assert!(fld.id.starts_with("fld_"));
        // 改名。
        let r = f.rename(&fld.id, "工作区").unwrap();
        assert_eq!(r.name, "工作区");
        assert!(matches!(
            f.rename(&fld.id, ""),
            Err(FolderError::InvalidName)
        ));
        assert!(matches!(
            f.rename("fld_none", "x"),
            Err(FolderError::NotFound)
        ));
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
        assert_eq!(
            s.get(&a.id).unwrap().folder_id.as_deref(),
            Some(fld.id.as_str())
        );
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

    // ---------- D-044:UltraPlan 流程状态 ----------

    async fn body_json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("读响应体失败");
        serde_json::from_slice(&bytes).expect("响应应为 JSON")
    }

    fn flow(title: &str, workspace_id: Option<&str>, ws_root: &FsPath) -> UltraPlanState {
        UltraPlanState::new_flow(title, workspace_id, ws_root)
    }

    /// 旧 sessions.json 兼容、锁内读改写落盘、CAS 落空不动盘、忙时拒绝、fork 不继承。
    #[tokio::test]
    async fn ultraplan_state_defaults_persists_and_not_forked() {
        let dir = temp_dir("ultraplan");
        // 三条旧数据:无字段 / 半截状态(只有 id、slug)/ 状态块损坏。
        let old = r#"{ "sessions": [
            { "id": "sess_old", "title": "旧会话", "createdAt": "2026-01-01T00:00:00Z",
              "updatedAt": "2026-01-01T00:00:00Z" },
            { "id": "sess_half", "title": "半截", "createdAt": "2026-01-01T00:00:01Z",
              "updatedAt": "2026-01-01T00:00:01Z",
              "ultraplan": { "id": "up_1", "slug": "塔防-0001", "stage": "demo_review" } },
            { "id": "sess_bad", "title": "坏块", "createdAt": "2026-01-01T00:00:02Z",
              "updatedAt": "2026-01-01T00:00:02Z",
              "ultraplan": { "id": "up_2", "questionnaireRev": "不是数字" } }
        ] }"#;
        std::fs::write(dir.join("sessions.json"), old).unwrap();
        let s = store(&dir);
        let got = s.get("sess_old").expect("旧会话可读回");
        assert!(got.ultraplan.is_none());
        assert!(
            serde_json::to_value(&got)
                .unwrap()
                .get("ultraplan")
                .is_none(),
            "无流程不该进 wire"
        );
        let half = s
            .get("sess_half")
            .unwrap()
            .ultraplan
            .expect("半截状态补缺省读回");
        assert_eq!(half.stage, "demo_review");
        assert_eq!(half.phase, "waiting");
        assert_eq!((half.questionnaire_rev, half.demo_iteration), (0, 0));
        assert!(half.plan_path.is_none() && half.last_error.is_none());
        let bad = s.get("sess_bad").expect("状态块损坏不得连累整条会话");
        assert_eq!(bad.title, "坏块");
        assert!(bad.ultraplan.is_none());

        // 锁内写入 + 落盘 + 重启读回;wire 为 camelCase。
        let ws_root = dir.join("ws");
        let before = s.get("sess_old").unwrap().updated_at;
        let seeded = flow("做一个塔防", Some("ws_1"), &ws_root);
        let flow_id = seeded.id.clone();
        let (sess, ()) = s
            .update_ultraplan("sess_old", move |slot| *slot = Some(seeded))
            .expect("会话存在");
        let stored = sess.ultraplan.clone().expect("已写入");
        assert_eq!(stored.id, flow_id);
        assert!(sess.updated_at > before, "有改动须 touch 会话");
        let wire = serde_json::to_value(&sess).unwrap();
        assert_eq!(wire["ultraplan"]["questionnaireRev"], 0);
        assert_eq!(wire["ultraplan"]["workspaceId"], "ws_1");
        assert_eq!(wire["ultraplan"]["planPath"], Value::Null);
        assert_eq!(
            store(&dir).get("sess_old").unwrap().ultraplan,
            Some(stored.clone())
        );

        // compare-and-set:阶段对得上才推进,并拿到闭包返回值。
        let advance = |expect: &'static str, next: &'static str| {
            let flow_id = flow_id.clone();
            move |slot: &mut Option<UltraPlanState>| match slot.as_mut() {
                Some(u) if u.id == flow_id && u.stage == expect => {
                    u.stage = next.to_string();
                    u.questionnaire_rev += 1;
                    true
                }
                _ => false,
            }
        };
        let (sess, ok) = s
            .update_ultraplan("sess_old", advance("discovery", "questionnaire"))
            .unwrap();
        assert!(ok);
        let after_cas = sess.ultraplan.clone().unwrap();
        assert_eq!(
            (after_cas.stage.as_str(), after_cas.questionnaire_rev),
            ("questionnaire", 1)
        );
        // 落空的 CAS:状态、updatedAt、磁盘都不动。
        let stamp = sess.updated_at.clone();
        std::thread::sleep(std::time::Duration::from_millis(3));
        let (sess, ok) = s
            .update_ultraplan("sess_old", advance("discovery", "questionnaire"))
            .unwrap();
        assert!(!ok, "阶段已不是 discovery,不得重复推进");
        assert_eq!(sess.ultraplan, Some(after_cas.clone()));
        assert_eq!(
            sess.updated_at, stamp,
            "没改动不该 touch(否则会话被顶到侧栏最前)"
        );
        assert_eq!(
            store(&dir).get("sess_old").unwrap().ultraplan,
            Some(after_cas.clone())
        );
        assert!(s.update_ultraplan("sess_none", |_| ()).is_none());

        // 要求空闲的变体:有运行中的 run → Busy,闭包不执行。
        s.claim_active_run("sess_old", "run_1").unwrap();
        let busy = s.update_ultraplan_idle("sess_old", |slot| slot.take());
        assert_eq!(busy.err(), Some(IdleUpdateError::Busy("run_1".to_string())));
        assert_eq!(
            s.get("sess_old").unwrap().ultraplan,
            Some(after_cas.clone())
        );
        s.release_active_run("sess_old", "run_1");
        assert_eq!(
            s.update_ultraplan_idle("sess_none", |_| ()).err(),
            Some(IdleUpdateError::NotFound)
        );

        // fork:计划指针继承(对照),流程状态不继承;源会话不受影响。
        let (state, state_dir) = crate::test_app_state("ultraplan-fork");
        let src = state.sessions.create("源会话", "coding", None, true, None);
        let mut with_plan = state.sessions.get(&src.id).unwrap();
        with_plan.active_plan_path = Some(".forge/plans/甲.plan.md".to_string());
        state.sessions.save(&with_plan);
        let seeded = flow("做一个塔防", None, &ws_root);
        state
            .sessions
            .update_ultraplan(&src.id, move |slot| *slot = Some(seeded))
            .unwrap();
        let resp = fork_session(State(state.clone()), Path(src.id.clone())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        let forked_id = v["session"]["id"].as_str().unwrap().to_string();
        assert_eq!(v["session"]["activePlanPath"], ".forge/plans/甲.plan.md");
        assert!(
            v["session"].get("ultraplan").is_none(),
            "fork 不得继承流程: {v}"
        );
        assert!(state.sessions.get(&forked_id).unwrap().ultraplan.is_none());
        assert!(state.sessions.get(&src.id).unwrap().ultraplan.is_some());
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&state_dir).ok();
    }

    /// 崩溃恢复:重启后停在 running 相位的流程改 failed(阶段不动),等待相位与无流程的会话不碰。
    #[test]
    fn stale_running_ultraplan_phase_swept_on_restart() {
        let dir = temp_dir("ultraplan-sweep");
        let s = store(&dir);
        let ws_root = dir.join("ws");
        let running = s.create("跑到一半", "coding", None, true, None);
        let waiting = s.create("在等用户", "coding", None, true, None);
        let plain = s.create("普通会话", "coding", None, true, None);
        let mut up = flow("塔防", None, &ws_root);
        up.stage = crate::ultraplan::STAGE_DEMO_REVIEW.to_string();
        up.phase = crate::ultraplan::PHASE_RUNNING.to_string();
        up.running = Some(crate::ultraplan::RUNNING_SPEC_DEMO.to_string());
        s.update_ultraplan(&running.id, move |slot| *slot = Some(up))
            .unwrap();
        let idle = flow("跑酷", None, &ws_root);
        let (waiting_before, ()) = s
            .update_ultraplan(&waiting.id, move |slot| *slot = Some(idle))
            .unwrap();
        let session_stamp = s.get(&running.id).unwrap().updated_at;

        // 「重启」:同一份 sessions.json 重新 load 后清扫。
        let s2 = store(&dir);
        let swept = s2.sweep_stale_ultraplan_runs();
        assert_eq!(swept.len(), 1, "只清 running 相位的那一条");
        assert_eq!(swept[0].0, running.id);
        let fixed = &swept[0].1;
        assert_eq!(fixed.stage, "demo_review", "阶段不动,重发同一动作即重试");
        assert_eq!(fixed.phase, "failed");
        assert_eq!(
            fixed.running.as_deref(),
            Some(crate::ultraplan::RUNNING_SPEC_DEMO),
            "失败时保留断掉的那一轮的种类"
        );
        let err = fixed.last_error.as_ref().expect("须留下原因");
        assert_eq!(
            err.code, "ULTRAPLAN_TURN_INVALID",
            "只用契约 §2 失败码表里的码"
        );
        assert!(crate::ultraplan::TURN_FAILURE_CODES.contains(&err.code.as_str()));
        assert!(
            err.message.contains("重启") && err.message.contains("重试"),
            "{}",
            err.message
        );
        let after = s2.get(&running.id).unwrap();
        assert_eq!(after.ultraplan.as_ref(), Some(fixed));
        assert_eq!(
            after.updated_at, session_stamp,
            "恢复不是用户活动,不 touch 会话"
        );
        assert_eq!(
            s2.get(&waiting.id).unwrap().ultraplan,
            waiting_before.ultraplan
        );
        assert!(s2.get(&plain.id).unwrap().ultraplan.is_none());
        // 已落盘;再扫一次是空操作。
        assert_eq!(
            store(&dir)
                .get(&running.id)
                .unwrap()
                .ultraplan
                .unwrap()
                .phase,
            "failed"
        );
        assert!(s2.sweep_stale_ultraplan_runs().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 流程进行中 PATCH 换工作区 → 拒绝且不留半改;值不变 / 流程结束 / 无流程都放行。
    #[tokio::test]
    async fn workspace_patch_blocked_mid_flow() {
        let (state, dir) = crate::test_app_state("ultraplan-wslock");
        let s = &state.sessions;
        let a = s.create("流程会话", "coding", None, true, Some("ws_1".to_string()));
        let seeded = flow("塔防", Some("ws_1"), &dir.join("ws"));
        s.update_ultraplan(&a.id, move |slot| *slot = Some(seeded))
            .unwrap();
        let switch = |to: Option<&str>| PatchSessionRequest {
            title: Some("顺手改个标题".to_string()),
            workspace_id: Some(to.map(str::to_string)),
            ..Default::default()
        };
        for to in [Some("ws_2"), None, Some("  ")] {
            assert!(
                matches!(
                    s.patch(&a.id, &switch(to)),
                    Err(PatchError::UltraplanWorkspaceLocked)
                ),
                "换到 {to:?} 应被拒"
            );
        }
        let kept = s.get(&a.id).unwrap();
        assert_eq!(kept.workspace_id.as_deref(), Some("ws_1"));
        assert_eq!(kept.title, "流程会话", "拒绝的 PATCH 不得留下半改状态");
        assert!(!kept.title_manually_set);
        // 值没变的整包回写照常放行(其它字段照改)。
        let same = s.patch(&a.id, &switch(Some(" ws_1 "))).unwrap();
        assert_eq!(same.title, "顺手改个标题");
        assert_eq!(same.workspace_id.as_deref(), Some("ws_1"));
        // 不带 workspaceId 的 PATCH 与流程无关。
        let pinned = PatchSessionRequest {
            pinned: Some(true),
            ..Default::default()
        };
        assert!(s.patch(&a.id, &pinned).unwrap().pinned);

        // REST 面:409 + 错误码,且不发 session.updated。
        let resp = patch_session(
            State(state.clone()),
            Path(a.id.clone()),
            Json(switch(Some("ws_2"))),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = body_json(resp).await;
        assert_eq!(v["error"]["code"], "ULTRAPLAN_WORKSPACE_LOCKED");
        assert!(v["error"]["message"].as_str().unwrap().contains("工作区"));
        assert!(state.events.persisted(&a.id).is_empty());

        // 流程结束(done)后可以换;没有流程的会话一直可以换。
        s.update_ultraplan(&a.id, |slot| {
            if let Some(u) = slot.as_mut() {
                u.stage = crate::ultraplan::STAGE_DONE.to_string();
            }
        })
        .unwrap();
        let moved = s.patch(&a.id, &switch(Some("ws_2"))).unwrap();
        assert_eq!(moved.workspace_id.as_deref(), Some("ws_2"));
        let b = s.create("普通会话", "coding", None, true, Some("ws_1".to_string()));
        assert!(s
            .patch(&b.id, &switch(None))
            .unwrap()
            .workspace_id
            .is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回退若会截掉进行中流程的 ultraplan.* 事件 → 409,事件流与会话原样;
    /// 只截流程之后的普通消息、旧流程的事件、已结束的流程都不拦。
    #[tokio::test]
    async fn revert_blocked_when_cutting_flow_events() {
        let (state, dir) = crate::test_app_state("ultraplan-revert");
        let session = state
            .sessions
            .create("流程会话", "coding", None, true, None);
        let sid = session.id.clone();
        let seeded = flow("塔防", None, &dir.join("ws"));
        let up_id = seeded.id.clone();
        state
            .sessions
            .update_ultraplan(&sid, move |slot| *slot = Some(seeded))
            .unwrap();
        let emit = |etype: &str, payload: Value| {
            state
                .events
                .emit(EventDraft::new(&sid, etype, "agent").payload(payload))
        };
        // seq: 1 普通消息 → 2 旧流程残留事件 → 3 本流程 started → 4 本流程 stage → 5 流程后的普通消息 → 6 收尾。
        let first_msg = emit("composer.user.message", json!({ "text": "流程之前" }));
        emit("ultraplan.cleared", json!({ "id": "up_old" }));
        let started = emit("ultraplan.started", json!({ "id": up_id, "runId": "r1" }));
        let staged = emit(
            "ultraplan.stage",
            json!({ "id": up_id, "stage": "questionnaire" }),
        );
        let later_msg = emit("composer.user.message", json!({ "text": "流程之后的闲聊" }));
        emit("agent.completed", json!({ "runId": "r2" }));
        let revert = |message_id: &str, mode: Option<&str>| {
            revert_session(
                State(state.clone()),
                Path(sid.clone()),
                Json(RevertRequest {
                    message_id: Some(message_id.to_string()),
                    mode: mode.map(str::to_string),
                }),
            )
        };
        let seqs = || -> Vec<i64> { state.events.persisted(&sid).iter().map(|e| e.seq).collect() };

        // 纯判据:cutoff 之后有没有本流程的事件。
        let log = state.events.persisted(&sid);
        let live = state.sessions.get(&sid).unwrap();
        assert!(revert_cuts_active_flow(&live, &log, 0));
        assert!(
            revert_cuts_active_flow(&live, &log, started.seq),
            "会截掉 stage 事件"
        );
        assert!(!revert_cuts_active_flow(&live, &log, staged.seq));
        assert!(
            !revert_cuts_active_flow(&session, &log, 0),
            "没有流程的会话不拦"
        );

        // 编辑流程之前的消息重发(before)→ 会截掉整条流程 → 409,什么都不动。
        state.sessions.claim_active_run(&sid, "run_live").unwrap();
        for (target, mode) in [
            (&first_msg, Some("before")),
            (&first_msg, None),
            (&started, None),
            (&staged, Some("before")),
        ] {
            let resp = revert(&target.id, mode).await;
            assert_eq!(
                resp.status(),
                StatusCode::CONFLICT,
                "seq={} {mode:?}",
                target.seq
            );
            assert_eq!(
                body_json(resp).await["error"]["code"],
                "ULTRAPLAN_REVERT_BLOCKED"
            );
        }
        assert_eq!(
            seqs(),
            vec![1, 2, 3, 4, 5, 6],
            "被拒的回退不得截断事件流,也不发 reverted"
        );
        let untouched = state.sessions.get(&sid).unwrap();
        assert_eq!(
            untouched.active_run_id.as_deref(),
            Some("run_live"),
            "被拒时会话原样"
        );
        assert_eq!(
            untouched.ultraplan.as_ref().map(|u| u.id.as_str()),
            Some(up_id.as_str())
        );

        // Even a safe history cutoff cannot interrupt a running root.
        let busy = revert(&later_msg.id, Some("before")).await;
        assert_eq!(body_json(busy).await["error"]["code"], "SESSION_BUSY");
        state.sessions.release_active_run(&sid, "run_live");
        // 只截流程之后的普通消息 → 放行:截到 seq 4,再追加 session.reverted;流程状态原样保留。
        let resp = revert(&later_msg.id, Some("before")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["session"]["ultraplan"]["id"], up_id.as_str());
        assert_eq!(
            v["session"]["activeRunId"],
            Value::Null,
            "回退完成后释放临时占用"
        );
        let after = state.events.persisted(&sid);
        assert_eq!(after.len(), 5);
        assert_eq!(after[3].event_type, "ultraplan.stage");
        assert_eq!(after[4].event_type, "session.reverted");

        // 流程结束后,截掉它的事件不再拦(只是历史)。
        state
            .sessions
            .update_ultraplan(&sid, |slot| {
                if let Some(u) = slot.as_mut() {
                    u.stage = crate::ultraplan::STAGE_DONE.to_string();
                }
            })
            .unwrap();
        let resp = revert(&first_msg.id, None).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let after = state.events.persisted(&sid);
        assert_eq!(
            after
                .iter()
                .map(|e| e.event_type.as_str())
                .collect::<Vec<_>>(),
            ["composer.user.message", "session.reverted"]
        );
        // 事件不存在仍是 404(既有语义不变)。
        let resp = revert("evt_none", None).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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
