//! D-045:Design 模式——第八个 composer 模式:设计意图 → 生图出设计稿候选 → 用户审阅
//! (挑选 / 修改 / 重出 / 采用)→ 引擎内原子级复刻 → 服务端截图对比验收。
//!
//! 形态照 UltraPlan(D-044)「会话上的阶段机 + 每道闸一轮 + 文件即事实源」,但只有三道闸:
//! `concept`(出图)→ `design_review`(等用户)→ `replication`(复刻)→ `done`。
//! 为什么不做成一轮里的阻塞审批卡:审稿可能隔夜,审批卡随轮结束失效、重启即丢。
//!
//! 分层:
//! - 状态 [`DesignState`](挂 `DebugSession.design`,宽松反序列化;写一律走 `SessionStore::update_design`);
//! - 纯路由 [`route`]:(阶段, 模式, 动作) → 本轮做什么;
//! - 轮次接入:[`resolve_request`] → [`revalidate`] → [`begin_turn`] → 原生工具 [`dispatch`] →
//!   [`check_completed`] → [`finish_turn`];
//! - 工具实现:出图在本文件,复刻的素材 / 建场景 / 验收在 `design/` 子模块;
//! - REST:`GET …/design`、`GET …/design/file`、`POST …/design/{select|restart}`。
//!
//! 纪律:生成、入库、建场景、验收全在服务端执行并落盘,结果不取模型自报(I-5);
//! 改图后端不支持就如实失败,不拿文生图冒充改图。

pub mod assets;
pub mod build;
pub mod compare;
pub mod layout;
pub mod verify;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path as UrlPath, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::events::{new_id, now_rfc3339, EventDraft};
use crate::llm::ToolFeedback;
use crate::ultraplan::{err_response, read_json, write_atomic, write_json_atomic};
use crate::AppState;

// ---------------- 常量 ----------------

pub const MODE: &str = "design";
pub const ROOT_DIR: &str = ".forge/design";

pub const STAGE_CONCEPT: &str = "concept";
pub const STAGE_REVIEW: &str = "design_review";
pub const STAGE_REPLICATION: &str = "replication";
pub const STAGE_DONE: &str = "done";
#[cfg_attr(not(test), allow(dead_code))]
pub const STAGES: [&str; 4] = [STAGE_CONCEPT, STAGE_REVIEW, STAGE_REPLICATION, STAGE_DONE];

pub const PHASE_WAITING: &str = "waiting";
pub const PHASE_RUNNING: &str = "running";
pub const PHASE_FAILED: &str = "failed";

pub const ERR_TURN_INVALID: &str = "DESIGN_TURN_INVALID";
pub const ERR_STAGE_MISMATCH: &str = "DESIGN_STAGE_MISMATCH";

pub const TOOL_VIEW: &str = "design_view";
pub const TOOL_GENERATE: &str = "design_generate";
pub const TOOL_SUBMIT: &str = "design_submit";
pub const TOOL_LAYOUT: &str = "design_layout";
pub const TOOL_ASSETS: &str = "design_assets";
pub const TOOL_BUILD: &str = "design_build";
pub const TOOL_VERIFY: &str = "design_verify";
pub const TOOL_COMPLETE: &str = "design_complete";
pub const TOOLS: [&str; 8] = [
    TOOL_VIEW,
    TOOL_GENERATE,
    TOOL_SUBMIT,
    TOOL_LAYOUT,
    TOOL_ASSETS,
    TOOL_BUILD,
    TOOL_VERIFY,
    TOOL_COMPLETE,
];

/// 每轮出图调用上限(首出 + 自检后重出一次)。
pub const MAX_GENERATE_CALLS: usize = 2;
/// 每轮暂存候选上限。
pub const MAX_STAGED: usize = 8;
/// 每轮提交候选上限。
pub const MAX_CANDIDATES: usize = 4;
/// 每轮复刻验收截帧上限(exact 截帧可能触发会话重建,不能无限刷)。
pub const MAX_VERIFY_CALLS: usize = 6;
/// 给视觉模型的图长边上限。
pub const MODEL_IMAGE_MAX: u32 = 1024;

pub fn is_tool(name: &str) -> bool {
    TOOLS.contains(&name)
}

/// 写类工具(走会话权限门):出图、入库、建场景。其余只写流程目录,放在权限门之前。
pub fn is_write_tool(name: &str) -> bool {
    matches!(name, TOOL_GENERATE | TOOL_ASSETS | TOOL_BUILD)
}

// ---------------- 状态 ----------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowError {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

/// 用户采用的定稿。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approved {
    pub rev: u32,
    pub candidate: u32,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    /// 入库后的定稿资产(Content/Designs/<slug>/mockup.png)。
    #[serde(default)]
    pub asset_path: Option<String>,
    #[serde(default)]
    pub guid: Option<String>,
}

/// 最近一次验收摘要(完整报告在 verify/<n>/report.json)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifySummary {
    pub n: u32,
    pub passed: bool,
    pub global_ssim: f64,
    pub failed_elements: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignState {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub slug: String,
    /// `.forge/design/<slug>`(工作区根相对)。
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default = "default_stage")]
    pub stage: String,
    #[serde(default = "default_phase")]
    pub phase: String,
    #[serde(default)]
    pub running: Option<String>,
    #[serde(default)]
    pub last_error: Option<FlowError>,
    /// 已提交给用户的候选批次(首批提交前为 0)。
    #[serde(default)]
    pub design_rev: u32,
    /// 当前批次提交的候选序号。
    #[serde(default)]
    pub candidates: Vec<u32>,
    /// 用户当前选中的候选(REST select 持久化;缺省首个)。
    #[serde(default)]
    pub selected: Option<u32>,
    /// scene | ui。
    #[serde(default)]
    pub design_type: Option<String>,
    /// square | landscape | portrait。
    #[serde(default)]
    pub aspect: Option<String>,
    #[serde(default)]
    pub approved: Option<Approved>,
    /// 复刻轮次(批准时 1,每次修复 +1)。
    #[serde(default)]
    pub replication_round: u32,
    #[serde(default)]
    pub layout_ready: bool,
    #[serde(default)]
    pub assets_ready: bool,
    /// 复刻场景(项目根相对,如 Content/Scenes/Design/<slug>.rxscene)。
    #[serde(default)]
    pub scene_path: Option<String>,
    #[serde(default)]
    pub verify_count: u32,
    #[serde(default)]
    pub last_verify: Option<VerifySummary>,
    /// 收尾结论:验收是否通过 / 是否带未通过项收尾。
    #[serde(default)]
    pub passed: Option<bool>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_stage() -> String {
    STAGE_CONCEPT.to_string()
}
fn default_phase() -> String {
    PHASE_WAITING.to_string()
}

impl DesignState {
    pub fn new_flow(title: &str, workspace_id: Option<&str>, ws_root: &Path) -> Self {
        let id = new_id("dz");
        let title = crate::ultraplan::derive_title(title);
        let slug = allocate_slug(ws_root, &title, &id);
        let ts = now_rfc3339();
        DesignState {
            dir: flow_dir_rel(&slug),
            slug,
            title,
            id,
            workspace_id: workspace_id.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
            stage: default_stage(),
            phase: default_phase(),
            running: None,
            last_error: None,
            design_rev: 0,
            candidates: Vec::new(),
            selected: None,
            design_type: None,
            aspect: None,
            approved: None,
            replication_round: 0,
            layout_ready: false,
            assets_ready: false,
            scene_path: None,
            verify_count: 0,
            last_verify: None,
            passed: None,
            created_at: ts.clone(),
            updated_at: ts,
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != STAGE_DONE
    }

    pub fn touch(&mut self) {
        self.updated_at = now_rfc3339();
    }

    /// 流程目录绝对路径;`dir` 形态不对或途经符号链接 → None(按产物不存在处理)。
    pub fn dir_abs(&self, ws_root: &Path) -> Option<PathBuf> {
        if !is_flow_dir(&self.dir) {
            return None;
        }
        let mut path = ws_root.to_path_buf();
        for component in self.dir.split(['/', '\\']) {
            path.push(component);
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return None;
            }
        }
        Some(path)
    }

    /// 当前用户选中的候选(落在本批内);否则本批首个。
    pub fn current_candidate(&self) -> Option<u32> {
        self.selected
            .filter(|c| self.candidates.contains(c))
            .or_else(|| self.candidates.first().copied())
    }
}

pub fn flow_dir_rel(slug: &str) -> String {
    format!("{ROOT_DIR}/{slug}")
}

pub fn is_flow_dir(rel: &str) -> bool {
    let Some(slug) = rel.strip_prefix(&format!("{ROOT_DIR}/")) else {
        return false;
    };
    !slug.is_empty() && !slug.contains(['/', '\\', ':']) && slug != "." && slug != ".."
}

fn allocate_slug(ws_root: &Path, title: &str, id: &str) -> String {
    let base = crate::ultraplan::slug_base(title);
    let tail: String = id.chars().rev().filter(|c| c.is_ascii_alphanumeric()).take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let mut n = 0usize;
    loop {
        let slug = if n == 0 { format!("{base}-{tail}") } else { format!("{base}-{tail}-{n}") };
        if !crate::plan_doc::abs_path(ws_root, &flow_dir_rel(&slug)).exists() {
            return slug;
        }
        n += 1;
    }
}

/// `DebugSession.design` 宽松反序列化:状态块坏了只当没有流程,不连累整条会话。
pub fn deserialize_lenient<'de, D>(de: D) -> Result<Option<DesignState>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<Value>::deserialize(de)?;
    Ok(raw.and_then(|v| match serde_json::from_value::<DesignState>(v) {
        Ok(s) if !s.id.is_empty() => Some(s),
        Ok(_) => None,
        Err(e) => {
            eprintln!("[design] 会话流程状态解析失败,按无流程处理: {e}");
            None
        }
    }))
}

pub fn stage_payload(d: &DesignState, run_id: Option<&str>) -> Value {
    let mut v = json!({
        "id": d.id,
        "stage": d.stage,
        "phase": d.phase,
        "running": d.running,
        "designRev": d.design_rev,
        "replicationRound": d.replication_round,
    });
    if let Some(r) = run_id {
        v["runId"] = json!(r);
    }
    if let Some(e) = &d.last_error {
        v["lastError"] = json!(e);
    }
    v
}

// ---------------- 路由 ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    ApproveDesign,
    ReviseDesign,
    RegenerateDesign,
    ResumeReplication,
    FixReplication,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::ApproveDesign,
        Action::ReviseDesign,
        Action::RegenerateDesign,
        Action::ResumeReplication,
        Action::FixReplication,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        Action::ALL.into_iter().find(|a| a.as_str() == s.trim())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Action::ApproveDesign => "approve_design",
            Action::ReviseDesign => "revise_design",
            Action::RegenerateDesign => "regenerate_design",
            Action::ResumeReplication => "resume_replication",
            Action::FixReplication => "fix_replication",
        }
    }

    /// 该动作接受的阶段。修复既可在复刻失败停住时发,也可在完成后对结果提意见。
    pub fn accepts_stage(self, stage: &str) -> bool {
        match self {
            Action::ApproveDesign | Action::ReviseDesign | Action::RegenerateDesign => stage == STAGE_REVIEW,
            Action::ResumeReplication => stage == STAGE_REPLICATION,
            Action::FixReplication => stage == STAGE_REPLICATION || stage == STAGE_DONE,
        }
    }

    pub fn expected_rev(self, d: &DesignState) -> u32 {
        match self {
            Action::ApproveDesign | Action::ReviseDesign | Action::RegenerateDesign => d.design_rev,
            Action::ResumeReplication | Action::FixReplication => d.replication_round,
        }
    }

    pub fn requires_text(self) -> bool {
        matches!(self, Action::ReviseDesign | Action::FixReplication)
    }

    pub fn display_text(self) -> &'static str {
        match self {
            Action::ApproveDesign => "采用这一版设计稿,开始复刻",
            Action::ReviseDesign => "请按意见修改设计稿",
            Action::RegenerateDesign => "重新生成一批设计稿",
            Action::ResumeReplication => "继续复刻",
            Action::FixReplication => "请按意见修复复刻结果",
        }
    }

    fn turn_kind(self) -> TurnKind {
        match self {
            Action::ApproveDesign => TurnKind::Replicate(ReplPhase::Start),
            Action::ReviseDesign => TurnKind::Revise,
            Action::RegenerateDesign => TurnKind::Regenerate,
            Action::ResumeReplication => TurnKind::Replicate(ReplPhase::Resume),
            Action::FixReplication => TurnKind::Replicate(ReplPhase::Fix),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplPhase {
    Start,
    Resume,
    Fix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    /// 构思出图。`fresh` = 开新流程。
    Concept { fresh: bool },
    /// 在选中稿上改图。
    Revise,
    /// 按原意图(可带补充)重出一批。
    Regenerate,
    Replicate(ReplPhase),
}

impl TurnKind {
    pub fn running(self) -> &'static str {
        match self {
            TurnKind::Concept { .. } => "concept",
            TurnKind::Revise => "revise",
            TurnKind::Regenerate => "regenerate",
            TurnKind::Replicate(_) => "replication",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TurnKind::Concept { .. } => "设计构思与出图",
            TurnKind::Revise => "修改设计稿",
            TurnKind::Regenerate => "重新出图",
            TurnKind::Replicate(_) => "原子级复刻",
        }
    }

    pub fn is_concept_like(self) -> bool {
        !matches!(self, TurnKind::Replicate(_))
    }

    /// 只有首轮构思带多轮对话历史(意图与补充就在对话里);其余以流程产物为准。
    pub fn wants_history(self) -> bool {
        matches!(self, TurnKind::Concept { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteError {
    pub stage: Option<String>,
    pub reason: String,
}

impl RouteError {
    fn at(stage: Option<&str>, reason: impl Into<String>) -> Self {
        RouteError { stage: stage.map(str::to_string), reason: reason.into() }
    }

    pub fn details(&self) -> Value {
        json!({ "stage": self.stage, "allowed": allowed_actions(self.stage.as_deref()) })
    }

    pub fn message(&self) -> String {
        let at = match self.stage.as_deref() {
            Some(s) => format!("当前阶段 {s}"),
            None => "本会话没有进行中的 Design 流程".to_string(),
        };
        format!("{}({at});可用操作: {}", self.reason, allowed_actions(self.stage.as_deref()).join(" / "))
    }

    pub fn into_response(self) -> Response {
        err_response(StatusCode::CONFLICT, ERR_STAGE_MISMATCH, self.message(), Some(self.details()))
    }
}

pub const ALLOWED_FREE_TEXT: &str = "free_text";

pub fn allowed_actions(stage: Option<&str>) -> Vec<&'static str> {
    match stage {
        None | Some(STAGE_CONCEPT) => vec![ALLOWED_FREE_TEXT],
        Some(STAGE_REVIEW) => vec![
            Action::ApproveDesign.as_str(),
            Action::ReviseDesign.as_str(),
            Action::RegenerateDesign.as_str(),
            ALLOWED_FREE_TEXT,
        ],
        Some(STAGE_REPLICATION) => vec![Action::ResumeReplication.as_str(), Action::FixReplication.as_str()],
        Some(STAGE_DONE) => vec![Action::FixReplication.as_str(), ALLOWED_FREE_TEXT],
        Some(_) => Vec::new(),
    }
}

/// 纯路由:(当前阶段, 请求模式, 请求动作) → 本轮种类。
/// - 带动作:模式须为 design、阶段须接受该动作;带动作永不开新流程。
/// - design 模式自由文本:无流程 / done → 新流程;concept → 补充说明重试;design_review → 对选中稿修改;
///   replication → 不接受(只有续跑 / 修复两个动作)。
/// - 其它模式且无动作 → 普通轮次。
pub fn route(stage: Option<&str>, mode: &str, action: Option<Action>) -> Result<Option<TurnKind>, RouteError> {
    match action {
        Some(a) => {
            if mode != MODE {
                return Err(RouteError::at(stage, format!("操作 {} 需在 design 模式下发送", a.as_str())));
            }
            match stage {
                Some(s) if a.accepts_stage(s) => Ok(Some(a.turn_kind())),
                _ => Err(RouteError::at(stage, format!("不接受操作 {}", a.as_str()))),
            }
        }
        None if mode != MODE => Ok(None),
        None => match stage {
            None | Some(STAGE_DONE) => Ok(Some(TurnKind::Concept { fresh: true })),
            Some(STAGE_CONCEPT) => Ok(Some(TurnKind::Concept { fresh: false })),
            Some(STAGE_REVIEW) => Ok(Some(TurnKind::Revise)),
            Some(_) => Err(RouteError::at(stage, "复刻进行中不接受自由文本")),
        },
    }
}

/// ask:execute 里的 `design` 对象。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignReq {
    pub id: String,
    #[serde(default)]
    pub rev: Option<u32>,
    pub action: String,
    #[serde(default)]
    pub candidate: Option<u32>,
}

/// 已校验的轮次意图(副作用在 begin_turn)。
#[derive(Debug, Clone)]
pub struct DesignTurn {
    pub kind: TurnKind,
    pub flow: Option<DesignState>,
    pub action: Option<Action>,
    /// 修改 / 采用所针对的候选。
    pub candidate: Option<u32>,
}

impl DesignTurn {
    pub fn display_text<'a>(&self, user_input: &'a str) -> std::borrow::Cow<'a, str> {
        match self.action {
            Some(a) if user_input.trim().is_empty() => std::borrow::Cow::Borrowed(a.display_text()),
            _ => std::borrow::Cow::Borrowed(user_input),
        }
    }

    pub fn user_message_tag(&self) -> Option<Value> {
        let a = self.action?;
        let flow = self.flow.as_ref()?;
        Some(json!({
            "id": flow.id,
            "action": a.as_str(),
            "rev": a.expected_rev(flow),
            "candidate": self.candidate,
        }))
    }
}

/// 起 run 之前的全部校验。Ok(None) = 普通轮次。
pub fn resolve_request(
    session: &crate::sessions::DebugSession,
    mode: &str,
    req: Option<&DesignReq>,
    user_text: &str,
) -> Result<Option<DesignTurn>, Response> {
    let flow = session.design.clone();
    let stage = flow.as_ref().map(|d| d.stage.as_str());
    let action = match req {
        Some(r) => Some(Action::parse(&r.action).ok_or_else(|| {
            err_response(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                format!("未知 design.action: {}", r.action),
                None,
            )
        })?),
        None => None,
    };
    let kind = match route(stage, mode, action) {
        Ok(Some(k)) => k,
        Ok(None) => return Ok(None),
        Err(e) => return Err(e.into_response()),
    };
    if let (Some(a), Some(r)) = (action, req) {
        let d = flow.as_ref().expect("带动作的路由必有流程");
        if r.id != d.id {
            return Err(RouteError::at(stage, "请求指向的流程已不是当前流程,请刷新").into_response());
        }
        if r.rev != Some(a.expected_rev(d)) {
            return Err(RouteError::at(stage, "版本已过期,请刷新").into_response());
        }
        if a.requires_text() && user_text.trim().is_empty() {
            return Err(err_response(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                format!("{} 需要写明意见", a.as_str()),
                None,
            ));
        }
    }
    if let Some(d) = flow.as_ref().filter(|d| d.is_active()) {
        if d.workspace_id.as_deref() != session.workspace_id.as_deref() {
            return Err(RouteError::at(stage, "流程进行中不能换工作区,切回原工作区或重新开始").into_response());
        }
        if d.phase == PHASE_RUNNING {
            return Err(RouteError::at(stage, "流程正在处理上一轮").into_response());
        }
    }
    let candidate = match kind {
        TurnKind::Revise | TurnKind::Replicate(ReplPhase::Start) => {
            let d = flow.as_ref().expect("审阅阶段必有流程");
            let c = req.and_then(|r| r.candidate).or_else(|| d.current_candidate());
            match c {
                Some(c) if d.candidates.contains(&c) => Some(c),
                _ => {
                    return Err(RouteError::at(stage, "所选候选不在当前这一批设计稿里").into_response());
                }
            }
        }
        _ => None,
    };
    Ok(Some(DesignTurn {
        kind,
        flow: if matches!(kind, TurnKind::Concept { fresh: true }) { None } else { flow },
        action,
        candidate,
    }))
}

/// 认领 run 之后按最新状态再核一次(期间流程可能已被推进或重开)。
pub fn revalidate(latest: Option<&DesignState>, mode: &str, dt: &DesignTurn) -> Result<(), RouteError> {
    let stage = latest.map(|d| d.stage.as_str());
    let routed = route(stage, mode, dt.action)?;
    let same_flow = match (&dt.flow, latest) {
        (None, _) => true,
        (Some(a), Some(b)) => a.id == b.id && a.design_rev == b.design_rev && a.replication_round == b.replication_round,
        (Some(_), None) => false,
    };
    if routed != Some(dt.kind) || !same_flow {
        return Err(RouteError::at(stage, "流程状态在本轮开始前发生了变化"));
    }
    Ok(())
}

// ---------------- 运行时 ----------------

/// 本轮暂存的一张候选(提交前)。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    pub index: u32,
    pub path: String,
    pub prompt: String,
    pub op: String,
    pub backend: String,
}

#[derive(Debug)]
pub struct DesignRuntime {
    pub kind: TurnKind,
    pub flow_id: String,
    pub slug: String,
    pub dir_rel: String,
    pub dir_abs: PathBuf,
    pub ws_root: PathBuf,
    pub project_root: PathBuf,
    /// 本轮批次号(构思类轮次 = 提交后的 designRev)。
    pub next_rev: u32,
    /// 修改轮的基准稿 (rev, candidate)。
    pub base: Option<(u32, u32)>,
    pub replication_round: u32,
    pub prompt_suffix: String,
    pub preamble: String,
    pub generate_calls: AtomicUsize,
    pub verify_calls: AtomicUsize,
    pub staged: Mutex<Vec<Staged>>,
}

impl DesignRuntime {
    pub fn rel(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.ws_root).unwrap_or(abs).to_string_lossy().replace('\\', "/")
    }

    pub fn round_dir(&self, rev: u32) -> PathBuf {
        self.dir_abs.join("rounds").join(format!("r{rev}"))
    }

    pub fn candidate_path(&self, rev: u32, c: u32) -> PathBuf {
        self.round_dir(rev).join(format!("c{c}.png"))
    }

    pub fn approved_path(&self) -> PathBuf {
        self.dir_abs.join("approved.png")
    }
}

fn emit(state: &AppState, sid: &str, event_type: &str, payload: Value) {
    state.events.emit(EventDraft::new(sid, event_type, "design").payload(payload));
}

fn emit_session_updated(state: &AppState, sid: &str) {
    state.events.emit(
        EventDraft::new(sid, "session.updated", "session").payload(json!({ "sessionId": sid })),
    );
}

fn failure(e: impl std::fmt::Display) -> String {
    format!("{ERR_TURN_INVALID}: {e}")
}

fn append_section(path: &Path, title: &str, text: &str) -> std::io::Result<()> {
    let mut doc = std::fs::read_to_string(path).unwrap_or_default();
    if !doc.is_empty() && !doc.ends_with('\n') {
        doc.push('\n');
    }
    doc.push_str(&format!("\n## {title}({})\n\n{}\n", now_rfc3339(), text.trim()));
    write_atomic(path, &doc)
}

/// 开场:**唯一**做副作用的地方(认领 run、再核对通过之后)。
pub fn begin_turn(
    state: &AppState,
    sid: &str,
    run_id: &str,
    scope: &crate::scope::ScopeProject,
    user_text: &str,
    dt: &DesignTurn,
) -> Result<DesignRuntime, String> {
    let ws_root = scope.workspace_root.clone();
    let mut flow = match (&dt.flow, dt.kind) {
        (_, TurnKind::Concept { fresh: true }) => DesignState::new_flow(user_text, scope.workspace_id.as_deref(), &ws_root),
        (Some(f), _) => f.clone(),
        (None, _) => return Err(failure("缺少流程")),
    };
    let dir_abs = flow.dir_abs(&ws_root).ok_or_else(|| failure("流程目录非法"))?;
    std::fs::create_dir_all(&dir_abs).map_err(|e| failure(format!("建流程目录失败: {e}")))?;
    let brief = dir_abs.join("brief.md");
    let mut base = None;
    let mut decision: Option<Value> = None;
    match dt.kind {
        TurnKind::Concept { fresh: true } => {
            write_atomic(&brief, &format!("# 设计意图\n\n{}\n", user_text.trim()))
                .map_err(|e| failure(format!("写 brief.md 失败: {e}")))?;
        }
        TurnKind::Concept { fresh: false } => {
            append_section(&brief, "补充说明", user_text).map_err(failure)?;
        }
        TurnKind::Revise => {
            let c = dt.candidate.ok_or_else(|| failure("缺少基准候选"))?;
            base = Some((flow.design_rev, c));
            append_section(&brief, &format!("修改意见(基于第 {} 批 #{c})", flow.design_rev), user_text).map_err(failure)?;
            decision = Some(json!({"action": "revise_design", "rev": flow.design_rev, "candidate": c, "feedback": user_text}));
        }
        TurnKind::Regenerate => {
            if !user_text.trim().is_empty() && dt.action.is_some_and(|_| user_text != Action::RegenerateDesign.display_text()) {
                append_section(&brief, "重新出图要求", user_text).map_err(failure)?;
            }
            decision = Some(json!({"action": "regenerate_design", "rev": flow.design_rev}));
        }
        TurnKind::Replicate(ReplPhase::Start) => {
            let c = dt.candidate.ok_or_else(|| failure("缺少采用的候选"))?;
            let src = dir_abs.join("rounds").join(format!("r{}", flow.design_rev)).join(format!("c{c}.png"));
            let bytes = std::fs::read(&src).map_err(|e| failure(format!("读取候选失败 {}: {e}", src.display())))?;
            let img = compare::Img::decode(&bytes).map_err(failure)?;
            std::fs::write(dir_abs.join("approved.png"), &bytes).map_err(failure)?;
            let sha = forge_util::hashutil::sha256_hex(&bytes);
            let accepted = assets::accept_png(
                &scope.project_root,
                &bytes,
                &format!("Designs/{}", flow.slug),
                "mockup",
                json!({
                    "op": "mockup",
                    "flowId": flow.id,
                    "rev": flow.design_rev,
                    "candidate": c,
                    "sourceRefs": [format!("{}/rounds/r{}/c{c}.png", flow.dir, flow.design_rev)],
                    "prompt": read_json(&src.with_file_name("prompt.json")).and_then(|v| v[c.to_string()]["prompt"].as_str().map(str::to_string)),
                }),
            );
            let (asset_path, guid) = match accepted {
                Ok((p, g)) => (Some(p), Some(g)),
                Err(e) => {
                    emit(state, sid, "design.notice", json!({"runId": run_id, "id": flow.id, "code": "MOCKUP_ACCEPT_FAILED", "message": e}));
                    (None, None)
                }
            };
            let approved = Approved {
                rev: flow.design_rev,
                candidate: c,
                sha256: sha,
                width: img.w,
                height: img.h,
                asset_path,
                guid,
            };
            write_json_atomic(&dir_abs.join("approved.json"), &json!(approved)).map_err(failure)?;
            flow.approved = Some(approved);
            flow.stage = STAGE_REPLICATION.into();
            flow.replication_round = 1;
            flow.layout_ready = false;
            flow.assets_ready = false;
            flow.verify_count = 0;
            flow.last_verify = None;
            flow.passed = None;
            decision = Some(json!({"action": "approve_design", "rev": flow.design_rev, "candidate": c}));
        }
        TurnKind::Replicate(ReplPhase::Fix) => {
            append_section(&dir_abs.join("feedback.md"), &format!("第 {} 轮修复意见", flow.replication_round + 1), user_text)
                .map_err(failure)?;
            flow.replication_round += 1;
            flow.stage = STAGE_REPLICATION.into();
            flow.passed = None;
        }
        TurnKind::Replicate(ReplPhase::Resume) => {}
    }
    if matches!(dt.kind, TurnKind::Replicate(_)) && flow.approved.is_none() {
        return Err(failure("没有已采用的设计稿"));
    }
    if matches!(dt.kind, TurnKind::Concept { .. }) {
        flow.stage = STAGE_CONCEPT.into();
    }
    flow.phase = PHASE_RUNNING.into();
    flow.running = Some(dt.kind.running().into());
    flow.last_error = None;
    flow.touch();
    let fresh = matches!(dt.kind, TurnKind::Concept { fresh: true });
    let expected_id = dt.flow.as_ref().map(|f| f.id.clone());
    let written = state
        .sessions
        .update_design(sid, |slot| {
            let ok = match (&expected_id, slot.as_ref()) {
                (None, _) => true,
                (Some(id), Some(cur)) => &cur.id == id,
                (Some(_), None) => false,
            };
            if ok {
                *slot = Some(flow.clone());
            }
            ok
        })
        .is_some_and(|(_, ok)| ok);
    if !written {
        return Err(failure("流程状态在本轮开始时发生了变化,请刷新后重试"));
    }
    if fresh {
        emit(state, sid, "design.started", json!({"runId": run_id, "id": flow.id, "title": flow.title, "slug": flow.slug, "dir": flow.dir}));
    }
    if let Some(mut d) = decision {
        d["runId"] = json!(run_id);
        d["id"] = json!(flow.id);
        emit(state, sid, "design.decision", d);
    }
    emit(state, sid, "design.stage", stage_payload(&flow, Some(run_id)));
    emit_session_updated(state, sid);

    let next_rev = flow.design_rev + 1;
    let preamble = preamble(&flow, &dir_abs, dt.kind, base);
    Ok(DesignRuntime {
        kind: dt.kind,
        flow_id: flow.id.clone(),
        slug: flow.slug.clone(),
        dir_rel: flow.dir.clone(),
        dir_abs,
        ws_root,
        project_root: scope.project_root.clone(),
        next_rev,
        base,
        replication_round: flow.replication_round,
        prompt_suffix: prompt_suffix(dt.kind).to_string(),
        preamble,
        generate_calls: AtomicUsize::new(0),
        verify_calls: AtomicUsize::new(0),
        staged: Mutex::new(Vec::new()),
    })
}

/// 流程上下文段(本轮注入给模型的事实底稿)。
fn preamble(flow: &DesignState, dir_abs: &Path, kind: TurnKind, base: Option<(u32, u32)>) -> String {
    let brief = std::fs::read_to_string(dir_abs.join("brief.md")).unwrap_or_default();
    let brief: String = brief.chars().take(6000).collect();
    let mut s = format!(
        "【Design 流程】{}\n- 流程 id:{};目录:{}\n- 阶段:{};本轮:{}\n",
        flow.title,
        flow.id,
        flow.dir,
        flow.stage,
        kind.label()
    );
    if let Some(t) = &flow.design_type {
        s.push_str(&format!("- 设计类型:{t};画幅:{}\n", flow.aspect.as_deref().unwrap_or("未定")));
    }
    if let Some((rev, c)) = base {
        s.push_str(&format!(
            "- 修改基准:第 {rev} 批候选 #{c}({}/rounds/r{rev}/c{c}.png);先 design_view 看它,再按修改意见改图\n",
            flow.dir
        ));
    }
    if let Some(a) = &flow.approved {
        s.push_str(&format!(
            "- 定稿:第 {} 批 #{}({}x{},{}/approved.png{})\n- 复刻轮次:{}\n",
            a.rev,
            a.candidate,
            a.width,
            a.height,
            flow.dir,
            a.asset_path.as_deref().map(|p| format!(";资产 Content/{p}")).unwrap_or_default(),
            flow.replication_round
        ));
        s.push_str(&format!(
            "- 进度:元素清单{};素材{};场景 {};验收 {} 次{}\n",
            if flow.layout_ready { "已定" } else { "未定" },
            if flow.assets_ready { "已入库" } else { "未生产" },
            flow.scene_path.as_deref().unwrap_or("未建"),
            flow.verify_count,
            flow.last_verify
                .as_ref()
                .map(|v| format!("(最近一次 #{} {} ssim={:.3})", v.n, if v.passed { "通过" } else { "未通过" }, v.global_ssim))
                .unwrap_or_default()
        ));
        if let Ok(fb) = std::fs::read_to_string(dir_abs.join("feedback.md")) {
            let fb: String = fb.chars().rev().take(3000).collect::<Vec<_>>().into_iter().rev().collect();
            s.push_str(&format!("\n【用户对复刻结果的修复意见】\n{fb}\n"));
        }
    }
    s.push_str(&format!("\n【设计意图与历次补充(brief.md)】\n{brief}\n"));
    s
}

// ---------------- 提示词 ----------------

// 留痕:构思轮——出图纪律决定了后面能不能拆得开、切得干净,所以写死在这里而不是靠模型自觉。
pub const CONCEPT_PROMPT_SUFFIX: &str = "\n当前为 Design 模式 · 设计构思:你是游戏美术总监兼 UI 设计师,按用户的设计意图产出设计稿,交用户审阅。\
\n工作步骤:\
\n1. 读意图与项目事实(必要时用只读工具看场景/资产,保持与项目已有风格一致),判定设计类型——ui(界面/菜单/HUD)或 scene(2D 场景画面),并定画幅:landscape(1536x1024,横屏界面与场景缺省)、portrait(1024x1536)或 square(1024)。\
\n2. 写英文生图提示词调用 design_generate(n=2~4)。出图纪律:ui 必须是正对屏幕的平面界面截图,不要设备外框、手、桌面、透视与景深;元素边缘清晰、彼此留足间距、不重叠;界面里的文字逐字写进提示词(用户语言原文,尽量短),写明字体风格;不要水印、签名与多余文字。scene 必须是 2D 正交游戏画面,前景物体与背景分层清楚、物体之间不粘连。\
\n3. 看返回的候选图自检:文字是否可读且与要求一致、布局是否符合意图、元素是否可以拆分。不合格就修正提示词再调用一次 design_generate(本轮最多两次)。\
\n4. 调用 design_submit 提交 2~4 张最好的候选(说明各自取舍),然后用一两句话结束本轮——用户会在审阅卡上挑选、提修改或采用。\
\n不要在本轮修改场景、导入资产或写文件;不要假装出过图——工具报错(如生图后端未配置)就原样告诉用户并停下。\n";

pub const REVISE_PROMPT_SUFFIX: &str = "\n当前为 Design 模式 · 修改设计稿:用户对选中的设计稿提了修改意见(见用户消息与流程上下文)。\
\n1. 先 design_view{what:\"candidate\"} 看基准稿。\
\n2. 调用 design_generate,带 editOf(基准稿的 rev 与 candidate),提示词写成英文改图指令:「Keep the overall composition, layout and art style unchanged; only change: …」逐条落实修改意见,未提到的部分保持不变。n=2~3。\
\n3. 自检改动是否落实、其余部分是否保持;不满意可再改一次。然后 design_submit 提交,一两句话说明改了什么并结束本轮。\n";

pub const REGENERATE_PROMPT_SUFFIX: &str = "\n当前为 Design 模式 · 重新出图:用户对这一批都不满意,要求重出(可能附了新要求)。\
\n结合设计意图与新要求换一种构图/风格方向重写提示词,design_generate 出 2~4 张,自检后 design_submit 提交,一两句话结束本轮。出图纪律与首轮相同。\n";

// 留痕:复刻轮——「原子级」的含义与工序顺序写死,验收只认服务端截图对比。
pub const REPLICATE_PROMPT_SUFFIX: &str = "\n当前为 Design 模式 · 原子级复刻:用户已采用定稿,你要在引擎里把定稿画面逐元素复刻成一个 2D 场景。\
\n「原子级」= 定稿上每个可分辨的视觉元素(每个按钮、图标、面板、标签文字、装饰、角色)都是场景里独立的实体,位置、尺寸、层级与定稿像素对齐;文字必须是可编辑的 Text 组件,不能烤进图片。\
\n工序(按顺序,工具都由服务端执行并落盘):\
\n1. design_view{what:\"approved\"} 仔细看定稿。\
\n2. 选字体:mcp__asset-pipeline__font_list(界面是中文就 cjkOnly:true)挑一款最接近定稿字形的,未入库就 mcp__asset-pipeline__font_import,拿到字体 GUID。\
\n3. design_layout 提交元素清单:canvas = 定稿尺寸;恰有一个 kind=background、bbox 铺满画布、source=cleanplate 的背景;其余元素逐个列出,bbox 用定稿像素 [x,y,w,h] 紧贴元素外缘(含阴影与描边),z 按叠放从下到上递增;source:crop(能从定稿干净切下的按钮/图标/面板,缺省)、regen(被遮挡、与背景粘连或切不干净的元素,附 regenPrompt 英文单体描述)、text(全部文字,kind=text,text.content 逐字照抄,size 为字号像素,color 0..1);按钮上的文字与按钮底板分成两个元素。看返回的叠框图核对有无漏框、错框,有就改了重交。\
\n4. design_assets 生产素材(干净底图、切图抠底、单体重绘并入库);看拼版图,抠坏或残缺的元素改成 regen 或调整 bbox 后重新 design_layout + design_assets(可只传 elementIds)。\
\n5. design_build 编译成新场景(相机、实体、Text 全部由服务端按清单生成并保存)。\
\n6. design_verify 截图对比;看差异热力图与逐元素得分,未通过的元素用 engine-scene 工具(transform_set / component_set,Text 改字号颜色)或回到第 3~4 步修正,修完 scene_save 到同一路径再验收。验收最多 6 次。\
\n7. design_complete 收尾:验收通过直接提交;到上限仍未通过,写明哪些元素没对上、原因,并带 acceptFailures:true 如实收尾。\
\n不调用 gen-image 工具自行出图(素材统一经 design_assets,溯源一致);不改与本场景无关的场景与资产;引擎处于 play 态时先如实报告,不要强行退出。\n";

pub fn prompt_suffix(kind: TurnKind) -> &'static str {
    match kind {
        TurnKind::Concept { .. } => CONCEPT_PROMPT_SUFFIX,
        TurnKind::Revise => REVISE_PROMPT_SUFFIX,
        TurnKind::Regenerate => REGENERATE_PROMPT_SUFFIX,
        TurnKind::Replicate(_) => REPLICATE_PROMPT_SUFFIX,
    }
}

// ---------------- 工具面 ----------------

fn spec(name: &str, desc: &str, params: Value) -> Value {
    json!({ "type": "function", "function": { "name": name, "description": desc, "parameters": params } })
}

fn view_spec() -> Value {
    spec(
        TOOL_VIEW,
        "查看流程里的图(交给视觉模型):approved=定稿;candidate=某批某张候选;element=已生产的元素素材;verify=某次验收的截帧/定稿/差异图。",
        json!({
            "type": "object",
            "properties": {
                "what": { "type": "string", "enum": ["approved", "candidate", "element", "verify"] },
                "rev": { "type": "integer", "description": "candidate 的批次(缺省当前批 / 本轮暂存批)" },
                "candidate": { "type": "integer" },
                "elementId": { "type": "string" },
                "n": { "type": "integer", "description": "verify 的序号(缺省最近一次)" }
            },
            "required": ["what"]
        }),
    )
}

pub fn tool_specs(kind: TurnKind) -> Vec<Value> {
    let mut out = vec![view_spec()];
    if kind.is_concept_like() {
        out.push(spec(
            TOOL_GENERATE,
            "生成设计稿候选(服务端调用生图模型;带 editOf 则在该稿上改图)。返回候选序号与图片。本轮最多调用两次。",
            json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "英文生图 / 改图指令" },
                    "negativePrompt": { "type": "string" },
                    "aspect": { "type": "string", "enum": ["landscape", "portrait", "square"] },
                    "n": { "type": "integer", "minimum": 1, "maximum": 4 },
                    "quality": { "type": "string", "enum": ["low", "medium", "high", "auto"] },
                    "editOf": {
                        "type": "object",
                        "properties": { "rev": { "type": "integer" }, "candidate": { "type": "integer" } },
                        "required": ["rev", "candidate"]
                    }
                },
                "required": ["prompt"]
            }),
        ));
        out.push(spec(
            TOOL_SUBMIT,
            "把本轮生成的候选提交给用户审阅(1~4 张;用 design_generate 返回的序号)。调用后用一两句话结束本轮。",
            json!({
                "type": "object",
                "properties": {
                    "candidates": { "type": "array", "items": { "type": "integer" }, "minItems": 1, "maxItems": 4 },
                    "summary": { "type": "string", "description": "给用户看的说明:各候选的取舍 / 本次改了什么" },
                    "designType": { "type": "string", "enum": ["ui", "scene"] }
                },
                "required": ["candidates", "summary", "designType"]
            }),
        ));
    } else {
        out.push(spec(
            TOOL_LAYOUT,
            "提交元素清单(定稿拆成原子元素)。服务端校验后落 layout.json,并返回叠框预览图。",
            json!({
                "type": "object",
                "properties": {
                    "layout": {
                        "type": "object",
                        "properties": {
                            "canvas": { "type": "object", "properties": { "width": { "type": "integer" }, "height": { "type": "integer" } }, "required": ["width", "height"] },
                            "font": { "type": "string", "description": "文字缺省字体 GUID" },
                            "elements": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string", "description": "[A-Za-z0-9_-],即场景实体名" },
                                        "kind": { "type": "string", "enum": layout::KINDS },
                                        "bbox": { "type": "array", "items": { "type": "integer" }, "description": "[x,y,w,h] 定稿像素" },
                                        "z": { "type": "integer" },
                                        "source": { "type": "string", "enum": layout::SOURCES },
                                        "regenPrompt": { "type": "string" },
                                        "note": { "type": "string" },
                                        "text": {
                                            "type": "object",
                                            "properties": {
                                                "content": { "type": "string" },
                                                "size": { "type": "number" },
                                                "color": { "type": "array", "items": { "type": "number" } },
                                                "align": { "type": "string", "enum": ["left", "center", "right"] },
                                                "verticalAlign": { "type": "string", "enum": ["top", "middle", "bottom"] },
                                                "outlineColor": { "type": "array", "items": { "type": "number" } },
                                                "outlineWidth": { "type": "number" },
                                                "shadowColor": { "type": "array", "items": { "type": "number" } },
                                                "shadowOffset": { "type": "array", "items": { "type": "number" } },
                                                "letterSpacing": { "type": "number" },
                                                "lineHeight": { "type": "number" },
                                                "font": { "type": "string" }
                                            },
                                            "required": ["content", "size", "color"]
                                        }
                                    },
                                    "required": ["id", "kind", "bbox", "z", "source"]
                                }
                            }
                        },
                        "required": ["canvas", "elements"]
                    }
                },
                "required": ["layout"]
            }),
        ));
        out.push(spec(
            TOOL_ASSETS,
            "按元素清单生产素材并入库(干净底图、切图抠底、单体重绘)。可只传 elementIds 重做部分元素。返回每个元素的 GUID 与抠图质量,附拼版图。",
            json!({
                "type": "object",
                "properties": { "elementIds": { "type": "array", "items": { "type": "string" } } }
            }),
        ));
        out.push(spec(
            TOOL_BUILD,
            "把元素清单编译成新的 2D 场景(正交相机与像素对齐、Sprite / Text 实体、层级)并保存到 Content/Scenes/Design/<slug>.rxscene。会切换引擎当前场景(原场景先另存到流程目录)。",
            json!({ "type": "object", "properties": {} }),
        ));
        out.push(spec(
            TOOL_VERIFY,
            "服务端以场景相机、定稿像素尺寸截帧,与定稿逐元素对比,返回报告与截帧 / 定稿 / 差异热力图。本轮最多 6 次。",
            json!({ "type": "object", "properties": {} }),
        ));
        out.push(spec(
            TOOL_COMPLETE,
            "收尾复刻。验收通过直接调用;未通过须 acceptFailures:true 并写明原因。",
            json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "string" },
                    "acceptFailures": { "type": "boolean" },
                    "reason": { "type": "string" }
                },
                "required": ["summary"]
            }),
        ));
    }
    out
}

// ---------------- 工具执行 ----------------

fn ok_text(text: impl Into<String>) -> (bool, ToolFeedback) {
    (true, ToolFeedback { text: text.into(), images: Vec::new() })
}

fn err_text(text: impl Into<String>) -> (bool, ToolFeedback) {
    (false, ToolFeedback { text: text.into(), images: Vec::new() })
}

pub fn data_url(img: &compare::Img) -> Option<String> {
    let png = img.fit_long_side(MODEL_IMAGE_MAX).encode_png().ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

fn load_img(path: &Path) -> Result<compare::Img, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    compare::Img::decode(&bytes)
}

/// 当前流程快照(工具执行时按最新状态校验:流程被重开 / 推进后旧轮次的工具一律拒)。
fn current_flow(state: &AppState, sid: &str, rt: &DesignRuntime) -> Result<DesignState, String> {
    let flow = state
        .sessions
        .get(sid)
        .and_then(|s| s.design)
        .ok_or("DESIGN_FLOW_GONE: 流程已不存在")?;
    if flow.id != rt.flow_id || flow.phase != PHASE_RUNNING {
        return Err("DESIGN_FLOW_GONE: 流程已变化,本轮工具失效".into());
    }
    Ok(flow)
}

/// 本轮的设计工具分派(权限门由调用方先过)。
pub async fn dispatch(
    state: &Arc<AppState>,
    sid: &str,
    rid: &str,
    rt: Option<&DesignRuntime>,
    name: &str,
    args: &Value,
) -> (bool, ToolFeedback) {
    let Some(rt) = rt else {
        return err_text(format!("TOOL_FORBIDDEN: {name} 只在 Design 流程的轮次里可用"));
    };
    let concept_tool = matches!(name, TOOL_GENERATE | TOOL_SUBMIT);
    let repl_tool = matches!(name, TOOL_LAYOUT | TOOL_ASSETS | TOOL_BUILD | TOOL_VERIFY | TOOL_COMPLETE);
    if (concept_tool && !rt.kind.is_concept_like()) || (repl_tool && rt.kind.is_concept_like()) {
        return err_text(format!("TOOL_FORBIDDEN: 本轮是「{}」,不提供 {name}", rt.kind.label()));
    }
    let flow = match current_flow(state, sid, rt) {
        Ok(f) => f,
        Err(e) => return err_text(e),
    };
    match name {
        TOOL_VIEW => view(rt, &flow, args),
        TOOL_GENERATE => generate(state, sid, rid, rt, &flow, args).await,
        TOOL_SUBMIT => submit(state, sid, rid, rt, args),
        TOOL_LAYOUT => layout_tool(state, sid, rid, rt, &flow, args),
        TOOL_ASSETS => assets::tool(state, sid, rid, rt, &flow, args).await,
        TOOL_BUILD => build::tool(state, sid, rid, rt, &flow).await,
        TOOL_VERIFY => verify::tool(state, sid, rid, rt, &flow).await,
        TOOL_COMPLETE => complete(state, sid, rid, rt, &flow, args),
        _ => err_text(format!("未知设计工具 {name}")),
    }
}

fn view(rt: &DesignRuntime, flow: &DesignState, args: &Value) -> (bool, ToolFeedback) {
    let what = args["what"].as_str().unwrap_or("");
    let paths: Vec<PathBuf> = match what {
        "approved" => vec![rt.approved_path()],
        "candidate" => {
            let staged_rev = rt.next_rev;
            let rev = args["rev"].as_u64().map(|v| v as u32).or(rt.base.map(|b| b.0)).unwrap_or(if flow.design_rev > 0 { flow.design_rev } else { staged_rev });
            let c = args["candidate"].as_u64().map(|v| v as u32).or(rt.base.map(|b| b.1)).or(flow.current_candidate()).unwrap_or(0);
            vec![rt.candidate_path(rev, c)]
        }
        "element" => {
            let Some(id) = args["elementId"].as_str().filter(|s| layout_id_ok(s)) else {
                return err_text("element 需要合法的 elementId");
            };
            vec![rt.dir_abs.join("elements").join(format!("{id}.png"))]
        }
        "verify" => {
            let n = args["n"].as_u64().map(|v| v as u32).unwrap_or(flow.verify_count);
            let d = rt.dir_abs.join("verify").join(n.to_string());
            vec![d.join("frame.png"), d.join("mockup.png"), d.join("diff.png")]
        }
        _ => return err_text("what 须为 approved|candidate|element|verify"),
    };
    let mut images = Vec::new();
    let mut lines = Vec::new();
    for p in &paths {
        match load_img(p) {
            Ok(img) => {
                lines.push(format!("{}({}x{})", rt.rel(p), img.w, img.h));
                if let Some(u) = data_url(&img) {
                    images.push(u);
                }
            }
            Err(e) => return err_text(format!("DESIGN_FILE_MISSING: {e}")),
        }
    }
    (true, ToolFeedback { text: format!("已附图:{}", lines.join(";")), images })
}

fn layout_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 48 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn aspect_of(args: &Value, flow: &DesignState) -> Result<gend::backends::Aspect, String> {
    let raw = args["aspect"].as_str().or(flow.aspect.as_deref()).unwrap_or("landscape");
    gend::backends::Aspect::parse(raw).ok_or_else(|| format!("aspect 须为 landscape|portrait|square,实: {raw}"))
}

async fn generate(
    state: &Arc<AppState>,
    sid: &str,
    rid: &str,
    rt: &DesignRuntime,
    flow: &DesignState,
    args: &Value,
) -> (bool, ToolFeedback) {
    let prompt = args["prompt"].as_str().unwrap_or("").trim().to_string();
    if prompt.is_empty() {
        return err_text("prompt 不可空");
    }
    if rt.generate_calls.fetch_add(1, Ordering::SeqCst) >= MAX_GENERATE_CALLS {
        return err_text(format!("DESIGN_GENERATE_LIMIT: 本轮最多出图 {MAX_GENERATE_CALLS} 次,请从已有候选中 design_submit"));
    }
    let n = args["n"].as_u64().unwrap_or(2).clamp(1, 4) as u32;
    let staged_len = rt.staged.lock().unwrap().len();
    if staged_len + n as usize > MAX_STAGED {
        return err_text(format!("本轮暂存候选已有 {staged_len} 张,上限 {MAX_STAGED}"));
    }
    let edit_of = match (&args["editOf"], rt.base, rt.kind) {
        (v, _, _) if v.is_object() => Some((
            v["rev"].as_u64().unwrap_or(0) as u32,
            v["candidate"].as_u64().unwrap_or(0) as u32,
        )),
        (_, Some(b), TurnKind::Revise) => Some(b),
        _ => None,
    };
    let aspect = match edit_of {
        // 改图沿用基准稿画幅(不让模型改图时顺手换比例)。
        Some(_) => aspect_of(&json!({}), flow),
        None => aspect_of(args, flow),
    };
    let aspect = match aspect {
        Ok(a) => a,
        Err(e) => return err_text(e),
    };
    let base_bytes = match edit_of {
        Some((rev, c)) => match std::fs::read(rt.candidate_path(rev, c)) {
            Ok(b) => Some(b),
            Err(e) => return err_text(format!("改图基准 r{rev}/c{c} 读取失败: {e}")),
        },
        None => None,
    };
    let negative = args["negativePrompt"].as_str().map(str::to_string);
    let quality = args["quality"].as_str().map(str::to_string);
    let seed = gend::hash_parts(&[prompt.as_bytes(), rid.as_bytes(), &(staged_len as u64).to_le_bytes()]);
    let prompt_c = prompt.clone();
    let generated = if state.sessions.get(sid).is_some_and(|s| s.is_codex()) {
        crate::codex::imagegen::generate(state, sid, rid, &rt.project_root, crate::codex::imagegen::Request {
            prompt: prompt_c,
            negative_prompt: negative,
            images: base_bytes.into_iter().collect(),
            mask: None,
            aspect,
            n,
            quality,
            background: None,
        }).await
            .map(|candidates| (crate::codex::imagegen::BACKEND_ID.to_string(), candidates))
            .map_err(|e| e.0)
    } else {
        tokio::task::spawn_blocking(move || -> gend::Result<(String, Vec<gend::backends::GenCandidate>)> {
        let cfg = gend::config::GenConfig::load();
        let keys = gend::keystore::Keystore::load();
        match base_bytes {
            Some(b) => {
                let backend = gend::backends::resolve_backend(None, "img2img", &cfg, &keys)?;
                let req = gend::backends::EditRequest {
                    prompt: prompt_c,
                    images: vec![b],
                    mask: None,
                    aspect,
                    seed,
                    n,
                    quality,
                    background: None,
                };
                Ok((backend.id().to_string(), backend.edit(&req, &cfg, &keys)?))
            }
            None => {
                let backend = gend::backends::resolve_backend(None, "text2img", &cfg, &keys)?;
                let mut req = gend::backends::GenRequest::square(prompt_c, negative, 1024, seed, n);
                req.aspect = aspect;
                req.quality = quality;
                Ok((backend.id().to_string(), backend.generate(&req, &cfg, &keys)?))
            }
        }
        }).await
            .map_err(|e| format!("生图任务异常: {e}"))
            .and_then(|r| r.map_err(|e| format!("{}: {}", e.code, e.message)))
    };
    let (backend, cands) = match generated {
        Ok(v) => v,
        Err(e) => return err_text(e),
    };
    let dir = rt.round_dir(rt.next_rev);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return err_text(format!("建批次目录失败: {e}"));
    }
    let (w_want, h_want) = aspect.dims(1024);
    let mut lines = Vec::new();
    let mut images = Vec::new();
    let mut new_staged = Vec::new();
    {
        let staged = rt.staged.lock().unwrap();
        let mut index = staged.iter().map(|s| s.index + 1).max().unwrap_or(0);
        drop(staged);
        for c in cands {
            let img = match compare::Img::decode(&c.png_bytes) {
                Ok(i) => i,
                Err(e) => return err_text(format!("候选解码失败: {e}")),
            };
            // 改图后端可能回原生尺寸;统一到本流程画幅,后续审阅 / 复刻都按同一画布。
            let img = if (img.w, img.h) != (w_want, h_want) && edit_of.is_some() { img.resize(w_want, h_want) } else { img };
            let png = match img.encode_png() {
                Ok(p) => p,
                Err(e) => return err_text(e),
            };
            let path = dir.join(format!("c{index}.png"));
            if let Err(e) = std::fs::write(&path, &png) {
                return err_text(format!("写候选失败: {e}"));
            }
            lines.push(format!("#{index} {}x{} → {}", img.w, img.h, rt.rel(&path)));
            if images.len() < 4 {
                if let Some(u) = data_url(&img.fit_long_side(768)) {
                    images.push(u);
                }
            }
            new_staged.push(Staged {
                index,
                path: rt.rel(&path),
                prompt: prompt.clone(),
                op: if edit_of.is_some() { "edit".into() } else { "generate".into() },
                backend: backend.clone(),
            });
            index += 1;
        }
    }
    {
        let mut staged = rt.staged.lock().unwrap();
        staged.extend(new_staged.iter().cloned());
        let manifest: serde_json::Map<String, Value> = staged
            .iter()
            .map(|s| {
                (
                    s.index.to_string(),
                    json!({"prompt": s.prompt, "op": s.op, "backend": s.backend, "editOf": edit_of.map(|(r, c)| json!({"rev": r, "candidate": c}))}),
                )
            })
            .collect();
        let _ = write_json_atomic(&dir.join("prompt.json"), &Value::Object(manifest));
    }
    emit(
        state,
        sid,
        "design.candidates.generated",
        json!({"runId": rid, "id": rt.flow_id, "rev": rt.next_rev, "candidates": new_staged}),
    );
    (
        true,
        ToolFeedback {
            text: format!(
                "已生成 {} 张候选(后端 {backend};{}):\n{}\n逐张自检后用 design_submit 提交序号。",
                lines.len(),
                if edit_of.is_some() { "改图" } else { "文生图" },
                lines.join("\n")
            ),
            images,
        },
    )
}

fn submit(state: &Arc<AppState>, sid: &str, rid: &str, rt: &DesignRuntime, args: &Value) -> (bool, ToolFeedback) {
    let staged = rt.staged.lock().unwrap().clone();
    if staged.is_empty() {
        return err_text("本轮还没有生成任何候选(先 design_generate)");
    }
    let mut chosen: Vec<u32> = Vec::new();
    for v in args["candidates"].as_array().cloned().unwrap_or_default() {
        let Some(c) = v.as_u64().map(|c| c as u32) else {
            return err_text("candidates 须为整数序号");
        };
        if !staged.iter().any(|s| s.index == c) {
            return err_text(format!("候选 #{c} 不是本轮生成的"));
        }
        if !chosen.contains(&c) {
            chosen.push(c);
        }
    }
    if chosen.is_empty() || chosen.len() > MAX_CANDIDATES {
        return err_text(format!("须提交 1..={MAX_CANDIDATES} 张候选"));
    }
    let summary = args["summary"].as_str().unwrap_or("").trim().to_string();
    let design_type = match args["designType"].as_str() {
        Some(t @ ("ui" | "scene")) => t.to_string(),
        _ => return err_text("designType 须为 ui|scene"),
    };
    // 画幅以实际图片为准。
    let first = rt.candidate_path(rt.next_rev, chosen[0]);
    let (w, h) = match load_img(&first) {
        Ok(i) => (i.w, i.h),
        Err(e) => return err_text(e),
    };
    let aspect = if w > h { "landscape" } else if h > w { "portrait" } else { "square" };
    let submission = json!({
        "rev": rt.next_rev,
        "candidates": chosen,
        "summary": summary,
        "designType": design_type,
        "aspect": aspect,
        "width": w,
        "height": h,
        "staged": staged,
        "runId": rid,
        "at": now_rfc3339(),
    });
    if let Err(e) = write_json_atomic(&rt.round_dir(rt.next_rev).join("submission.json"), &submission) {
        return err_text(format!("写 submission.json 失败: {e}"));
    }
    let next = rt.next_rev;
    let fid = rt.flow_id.clone();
    let updated = state.sessions.update_design(sid, |slot| match slot.as_mut() {
        Some(d) if d.id == fid && d.phase == PHASE_RUNNING && d.design_rev + 1 == next => {
            d.design_rev = next;
            d.stage = STAGE_REVIEW.into();
            d.candidates = chosen.clone();
            d.selected = chosen.first().copied();
            d.design_type = Some(design_type.clone());
            d.aspect = Some(aspect.into());
            true
        }
        _ => false,
    });
    if !updated.as_ref().is_some_and(|(_, ok)| *ok) {
        return err_text("DESIGN_FLOW_GONE: 流程状态已变化,提交未生效(同一轮只能提交一次)");
    }
    let cands: Vec<Value> = chosen
        .iter()
        .map(|c| {
            let s = staged.iter().find(|s| s.index == *c);
            json!({"index": c, "path": format!("{}/rounds/r{next}/c{c}.png", rt.dir_rel), "prompt": s.map(|s| s.prompt.clone()), "op": s.map(|s| s.op.clone())})
        })
        .collect();
    emit(
        state,
        sid,
        "design.review.ready",
        json!({
            "runId": rid, "id": rt.flow_id, "rev": next, "candidates": cands, "summary": summary,
            "designType": design_type, "aspect": aspect, "width": w, "height": h,
            "base": rt.base.map(|(r, c)| json!({"rev": r, "candidate": c})),
        }),
    );
    emit_session_updated(state, sid);
    ok_text(format!(
        "已提交第 {next} 批 {} 张候选给用户审阅。用一两句话总结后结束本轮,不要再调用工具。",
        chosen.len()
    ))
}

fn font_guids(project_root: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let mut stack = vec![project_root.join("Content")];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some("meta") {
                if let Ok(m) = assetd::meta::MetaDoc::load(&p) {
                    if m.atype == "font" {
                        out.insert(m.guid);
                    }
                }
            }
        }
    }
    out
}

fn layout_tool(
    state: &Arc<AppState>,
    sid: &str,
    rid: &str,
    rt: &DesignRuntime,
    flow: &DesignState,
    args: &Value,
) -> (bool, ToolFeedback) {
    let Some(a) = &flow.approved else {
        return err_text("没有定稿");
    };
    let fonts = font_guids(&rt.project_root);
    let parsed = match layout::validate(&args["layout"], (a.width, a.height), &|g| fonts.contains(g)) {
        Ok(l) => l,
        Err(e) => return err_text(format!("DESIGN_LAYOUT_INVALID: {e}")),
    };
    if let Err(e) = write_json_atomic(&rt.dir_abs.join("layout.json"), &json!(parsed)) {
        return err_text(format!("写 layout.json 失败: {e}"));
    }
    let mockup = match load_img(&rt.approved_path()) {
        Ok(i) => i,
        Err(e) => return err_text(e),
    };
    let ov = layout::overlay(&mockup, &parsed);
    let ov_path = rt.dir_abs.join("overlay.png");
    if let Ok(png) = ov.encode_png() {
        let _ = std::fs::write(&ov_path, png);
    }
    let fid = rt.flow_id.clone();
    let _ = state.sessions.update_design(sid, |slot| match slot.as_mut() {
        Some(d) if d.id == fid => {
            d.layout_ready = true;
            d.assets_ready = false;
            true
        }
        _ => false,
    });
    let counts = |k: &str| parsed.elements.iter().filter(|e| e.source == k).count();
    emit(
        state,
        sid,
        "design.layout.ready",
        json!({
            "runId": rid, "id": rt.flow_id, "round": rt.replication_round,
            "canvas": parsed.canvas, "count": parsed.elements.len(),
            "overlay": rt.rel(&ov_path),
            "elements": parsed.elements.iter().map(|e| json!({"id": e.id, "kind": e.kind, "source": e.source, "bbox": e.bbox, "z": e.z, "text": e.text.as_ref().map(|t| t.content.clone())})).collect::<Vec<_>>(),
        }),
    );
    (
        true,
        ToolFeedback {
            text: format!(
                "元素清单已落盘({} 个:crop {}、regen {}、text {}、背景 1)。核对附图里的叠框(蓝=文字、橙=按钮、绿=其它)有无漏框错框;无误就 design_assets。",
                parsed.elements.len(),
                counts("crop"),
                counts("regen"),
                counts("text")
            ),
            images: data_url(&ov).into_iter().collect(),
        },
    )
}

fn complete(
    state: &Arc<AppState>,
    sid: &str,
    rid: &str,
    rt: &DesignRuntime,
    flow: &DesignState,
    args: &Value,
) -> (bool, ToolFeedback) {
    let Some(last) = flow.last_verify.clone() else {
        return err_text("还没有验收记录(先 design_build + design_verify)");
    };
    let accept = args["acceptFailures"].as_bool().unwrap_or(false);
    let reason = args["reason"].as_str().unwrap_or("").trim().to_string();
    if !last.passed && (!accept || reason.is_empty()) {
        return err_text(format!(
            "最近一次验收(#{})未通过。继续修正后重新 design_verify;确实无法达成时带 acceptFailures:true 与 reason 如实收尾。",
            last.n
        ));
    }
    let summary = args["summary"].as_str().unwrap_or("").trim().to_string();
    let result = json!({
        "runId": rid, "id": rt.flow_id, "round": rt.replication_round,
        "passed": last.passed, "verify": last, "summary": summary,
        "acceptedFailures": if last.passed { Value::Null } else { json!(reason) },
        "scenePath": flow.scene_path, "at": now_rfc3339(),
    });
    let _ = write_json_atomic(&rt.dir_abs.join("result.json"), &result);
    let fid = rt.flow_id.clone();
    let passed = last.passed;
    let ok = state
        .sessions
        .update_design(sid, |slot| match slot.as_mut() {
            Some(d) if d.id == fid && d.stage == STAGE_REPLICATION => {
                d.stage = STAGE_DONE.into();
                d.passed = Some(passed);
                true
            }
            _ => false,
        })
        .is_some_and(|(_, ok)| ok);
    if !ok {
        return err_text("DESIGN_FLOW_GONE: 流程状态已变化");
    }
    emit(state, sid, "design.done", result);
    emit_session_updated(state, sid);
    ok_text("复刻已收尾。用两三句话向用户总结结果(场景路径、验收结论、未对上的元素),然后结束本轮。")
}

/// 回写验收摘要(verify 子模块调用)。
pub(crate) fn record_verify(state: &AppState, sid: &str, rt: &DesignRuntime, summary: VerifySummary, scene_path: Option<String>) {
    let fid = rt.flow_id.clone();
    let _ = state.sessions.update_design(sid, |slot| match slot.as_mut() {
        Some(d) if d.id == fid => {
            d.verify_count = summary.n;
            d.last_verify = Some(summary.clone());
            if scene_path.is_some() {
                d.scene_path = scene_path.clone();
            }
            true
        }
        _ => false,
    });
}

/// 循环正常结束后的核对:构思类轮次必须已提交候选;复刻轮必须已收尾,否则本轮如实失败(可续跑)。
pub fn check_completed(state: &AppState, sid: &str, rt: &DesignRuntime) -> Result<(), String> {
    let flow = state.sessions.get(sid).and_then(|s| s.design);
    let Some(flow) = flow.filter(|f| f.id == rt.flow_id) else {
        return Err(failure("流程已不存在"));
    };
    if rt.kind.is_concept_like() {
        if flow.stage == STAGE_REVIEW && flow.design_rev == rt.next_rev {
            return Ok(());
        }
        return Err("DESIGN_NO_SUBMISSION: 本轮没有提交设计稿候选(可能生图失败),请重试".into());
    }
    if flow.stage == STAGE_DONE {
        return Ok(());
    }
    Err("DESIGN_REPLICATION_INCOMPLETE: 复刻未收尾,可点「继续复刻」接着做".into())
}

/// 收尾:相位回 waiting / failed(阶段只经工具推进;失败原地可重试)。
pub fn finish_turn(state: &AppState, sid: &str, run_id: &str, flow_id: &str, status: &str, error: Option<&str>) {
    let updated = state.sessions.update_design(sid, |slot| match slot.as_mut() {
        Some(d) if d.id == flow_id && d.phase == PHASE_RUNNING => {
            match status {
                "completed" | "cancelled" => {
                    d.phase = PHASE_WAITING.into();
                    d.running = None;
                    d.last_error = None;
                }
                _ => {
                    d.phase = PHASE_FAILED.into();
                    let msg = error.unwrap_or("本轮失败");
                    let (code, message) = match msg.split_once(": ") {
                        Some((c, m)) if c.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_') => (c.to_string(), m.to_string()),
                        _ => (ERR_TURN_INVALID.to_string(), msg.to_string()),
                    };
                    d.last_error = Some(FlowError { code, message: message.chars().take(300).collect() });
                }
            }
            true
        }
        _ => false,
    });
    if let Some((s, true)) = updated {
        if let Some(d) = s.design.as_ref() {
            emit(state, sid, "design.stage", stage_payload(d, Some(run_id)));
        }
        emit_session_updated(state, sid);
    }
}

// ---------------- REST ----------------

fn session_not_found(id: &str) -> Response {
    err_response(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND", format!("会话不存在: {id}"), None)
}

fn flow_ws_root(state: &AppState, d: &DesignState) -> PathBuf {
    crate::scope::workspace_root_for(state, d.workspace_id.as_deref())
}

/// GET /api/forge/sessions/{id}/design → {design, review?, layout?, verify?, result?}
pub async fn get_design(State(state): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    let Some(session) = state.sessions.get(&id) else {
        return session_not_found(&id);
    };
    let Some(d) = session.design else {
        return Json(json!({ "design": null })).into_response();
    };
    let ws = flow_ws_root(&state, &d);
    let dir = d.dir_abs(&ws);
    let file = |rel: &str| dir.as_ref().and_then(|p| read_json(&p.join(rel)));
    let review = (d.design_rev > 0).then(|| file(&format!("rounds/r{}/submission.json", d.design_rev))).flatten();
    let verify = (d.verify_count > 0).then(|| file(&format!("verify/{}/report.json", d.verify_count))).flatten();
    Json(json!({
        "design": d,
        "review": review,
        "layout": file("layout.json"),
        "assets": file("assets.json"),
        "verify": verify,
        "result": file("result.json"),
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct FileQuery {
    path: String,
}

/// GET /api/forge/sessions/{id}/design/file?path=<流程目录相对路径> → PNG / JSON 原字节。
/// 只出当前流程目录内的 .png / .json(防 `..`、盘符与符号链接)。
pub async fn get_design_file(
    State(state): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<FileQuery>,
) -> Response {
    let Some(session) = state.sessions.get(&id) else {
        return session_not_found(&id);
    };
    let Some(d) = session.design else {
        return err_response(StatusCode::NOT_FOUND, "DESIGN_NOT_FOUND", "本会话没有 Design 流程", None);
    };
    match resolve_flow_file(&flow_ws_root(&state, &d), &d, &q.path) {
        Ok((path, mime)) => match std::fs::read(&path) {
            Ok(bytes) => ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-cache")], bytes).into_response(),
            Err(_) => err_response(StatusCode::NOT_FOUND, "DESIGN_FILE_MISSING", format!("文件不存在: {}", q.path), None),
        },
        Err(e) => err_response(StatusCode::BAD_REQUEST, "INVALID_INPUT", e, None),
    }
}

/// 流程目录相对路径 → 绝对路径 + MIME。
pub fn resolve_flow_file(ws_root: &Path, d: &DesignState, rel: &str) -> Result<(PathBuf, &'static str), String> {
    let rel = rel.replace('\\', "/");
    let rel = rel.strip_prefix(&format!("{}/", d.dir)).unwrap_or(&rel).to_string();
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains(':')
        || rel.split('/').any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(format!("非法路径: {rel}"));
    }
    let mime = if rel.ends_with(".png") {
        "image/png"
    } else if rel.ends_with(".json") {
        "application/json; charset=utf-8"
    } else {
        return Err("只提供 .png / .json".into());
    };
    let base = d.dir_abs(ws_root).ok_or("流程目录非法")?;
    let mut p = base.clone();
    for c in rel.split('/') {
        p.push(c);
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("路径含符号链接".into());
        }
    }
    Ok((p, mime))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionBody {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    rev: Option<u32>,
    #[serde(default)]
    candidate: Option<u32>,
}

/// POST /api/forge/sessions/{id}/design/{select|restart}(不产生模型轮次)。
pub async fn post_design_action(
    State(state): State<Arc<AppState>>,
    UrlPath((id, action)): UrlPath<(String, String)>,
    body: Option<Json<ActionBody>>,
) -> Response {
    let body = body.map(|b| b.0).unwrap_or_default();
    match action.as_str() {
        "select" => {
            let res = state.sessions.update_design(&id, |slot| match slot.as_mut() {
                None => Err(RouteError::at(None, "没有流程")),
                Some(d) => {
                    if d.stage != STAGE_REVIEW || body.id.as_deref() != Some(d.id.as_str()) || body.rev != Some(d.design_rev) {
                        return Err(RouteError::at(Some(&d.stage), "候选批次已过期,请刷新"));
                    }
                    match body.candidate {
                        Some(c) if d.candidates.contains(&c) => {
                            d.selected = Some(c);
                            Ok(c)
                        }
                        _ => Err(RouteError::at(Some(&d.stage), "候选不在当前批次")),
                    }
                }
            });
            match res {
                None => session_not_found(&id),
                Some((_, Err(e))) => e.into_response(),
                Some((_, Ok(c))) => {
                    emit_session_updated(&state, &id);
                    Json(json!({ "ok": true, "selected": c })).into_response()
                }
            }
        }
        "restart" => match state.sessions.update_design_idle(&id, |slot| slot.take()) {
            Err(crate::sessions::IdleUpdateError::NotFound) => session_not_found(&id),
            Err(crate::sessions::IdleUpdateError::Busy(run)) => err_response(
                StatusCode::CONFLICT,
                "SESSION_BUSY",
                format!("会话有运行中的 run({run}),请等它结束或先停止"),
                None,
            ),
            Ok(_) => {
                emit_session_updated(&state, &id);
                Json(json!({ "ok": true })).into_response()
            }
        },
        other => err_response(StatusCode::NOT_FOUND, "NOT_FOUND", format!("未知操作: {other}"), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_table_covers_every_stage_action_pair() {
        let stages: Vec<Option<&str>> = std::iter::once(None).chain(STAGES.iter().map(|s| Some(*s))).collect();
        for stage in &stages {
            for mode in ["build", "plan", MODE, "ultraplan"] {
                for action in std::iter::once(None).chain(Action::ALL.iter().map(|a| Some(*a))) {
                    let r = route(*stage, mode, action);
                    match (action, mode == MODE) {
                        (Some(_), false) => assert!(r.is_err()),
                        (None, false) => assert_eq!(r, Ok(None)),
                        (Some(a), true) => assert_eq!(r.is_ok(), stage.is_some_and(|s| a.accepts_stage(s)), "{stage:?} {a:?}"),
                        (None, true) => {
                            let want = match *stage {
                                None | Some(STAGE_DONE) => Some(TurnKind::Concept { fresh: true }),
                                Some(STAGE_CONCEPT) => Some(TurnKind::Concept { fresh: false }),
                                Some(STAGE_REVIEW) => Some(TurnKind::Revise),
                                _ => None,
                            };
                            match want {
                                Some(k) => assert_eq!(r, Ok(Some(k))),
                                None => assert!(r.is_err()),
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn allowed_actions_match_route() {
        for stage in STAGES {
            for a in Action::ALL {
                assert_eq!(
                    allowed_actions(Some(stage)).contains(&a.as_str()),
                    a.accepts_stage(stage),
                    "{stage} {a:?}"
                );
            }
        }
    }

    #[test]
    fn lenient_state_and_defaults() {
        let v = json!({"id": "dz_1", "slug": "x", "dir": ".forge/design/x", "designRev": 2, "candidates": [0, 3]});
        let s: DesignState = serde_json::from_value(v).unwrap();
        assert_eq!(s.stage, STAGE_CONCEPT);
        assert_eq!(s.current_candidate(), Some(0));
        let mut s2 = s.clone();
        s2.selected = Some(3);
        assert_eq!(s2.current_candidate(), Some(3));
        s2.selected = Some(9);
        assert_eq!(s2.current_candidate(), Some(0));
        let bad: Option<DesignState> =
            deserialize_lenient(serde_json::json!({"id": "dz", "designRev": "x"})).unwrap();
        assert!(bad.is_none());
    }

    #[test]
    fn flow_dir_and_file_resolution_are_confined() {
        assert!(is_flow_dir(".forge/design/abc-1"));
        assert!(!is_flow_dir(".forge/design/../x"));
        assert!(!is_flow_dir(".forge/plans/x"));
        let ws = std::env::temp_dir().join(format!("design-ws-{}", std::process::id()));
        let mut d = DesignState::new_flow("主菜单", None, &ws);
        d.dir = ".forge/design/menu-1".into();
        assert!(resolve_flow_file(&ws, &d, "approved.png").is_ok());
        assert!(resolve_flow_file(&ws, &d, ".forge/design/menu-1/verify/1/diff.png").is_ok());
        assert!(resolve_flow_file(&ws, &d, "../../secret.png").is_err());
        assert!(resolve_flow_file(&ws, &d, "brief.md").is_err());
        assert!(resolve_flow_file(&ws, &d, "C:/x.png").is_err());
    }

    #[test]
    fn tool_sets_follow_turn_kind() {
        let names = |k: TurnKind| -> Vec<String> {
            tool_specs(k).iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect()
        };
        let concept = names(TurnKind::Concept { fresh: true });
        assert!(concept.contains(&TOOL_GENERATE.to_string()) && concept.contains(&TOOL_SUBMIT.to_string()));
        assert!(!concept.contains(&TOOL_BUILD.to_string()));
        let rep = names(TurnKind::Replicate(ReplPhase::Start));
        for t in [TOOL_VIEW, TOOL_LAYOUT, TOOL_ASSETS, TOOL_BUILD, TOOL_VERIFY, TOOL_COMPLETE] {
            assert!(rep.contains(&t.to_string()), "{t}");
        }
        assert!(!rep.contains(&TOOL_GENERATE.to_string()));
        assert!(is_write_tool(TOOL_GENERATE) && !is_write_tool(TOOL_VIEW));
    }
}
