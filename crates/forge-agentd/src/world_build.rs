//! crates/forge-agentd/src/world_build.rs
//!
//! State machine, session persistence, routing, and lifecycle management
//! for the `world_build` composer mode.

pub mod survey;
pub mod preview;
pub mod verify;

pub use preview::{generate_preview_package, BuildPreviewBuilding, BuildPreviewSceneState};
pub use verify::{run_coherence_verification, verify_coherence_for_flow, CoherenceAuditResult, CoherenceViolation};

use std::path::{Path, PathBuf};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::AppState;
use crate::events::{new_id, now_rfc3339, EventDraft};

pub const MODE: &str = "world_build";
pub const ROOT_DIR: &str = ".forge/world_build";

// 5-Stage Lifecycle Machine
pub const STAGE_SURVEY: &str = "survey";
pub const STAGE_PREVIEW_REVIEW: &str = "preview_review";
pub const STAGE_CONSTRUCTION: &str = "construction";
pub const STAGE_COHERENCE_VERIFY: &str = "coherence_verify";
pub const STAGE_DONE: &str = "done";

pub const STAGES: [&str; 5] = [
    STAGE_SURVEY,
    STAGE_PREVIEW_REVIEW,
    STAGE_CONSTRUCTION,
    STAGE_COHERENCE_VERIFY,
    STAGE_DONE,
];

// Phase States
pub const PHASE_WAITING: &str = "waiting";
pub const PHASE_RUNNING: &str = "running";
pub const PHASE_FAILED: &str = "failed";

// Standard Error Codes
pub const ERR_STAGE_MISMATCH: &str = "WORLD_BUILD_STAGE_MISMATCH";
pub const ERR_INVALID_ACTION: &str = "WORLD_BUILD_INVALID_ACTION";
pub const ERR_BUILDING_NOT_FOUND: &str = "WORLD_BUILD_BUILDING_NOT_FOUND";
pub const ERR_NOT_FOUND: &str = "WORLD_BUILD_NOT_FOUND";
pub const ERR_TURN_INVALID: &str = "WORLD_BUILD_TURN_INVALID";
pub const ERR_SURVEY_FAILED: &str = "WORLD_BUILD_SURVEY_FAILED";
pub const ERR_PREVIEW_FAILED: &str = "WORLD_BUILD_PREVIEW_FAILED";

// Standard File Names in .forge/world_build/<slug>/
pub const FACTS_FILE: &str = "facts.json";
pub const PREVIEW_DIR: &str = "preview";
pub const PREVIEW_SCENE_FILE: &str = "preview/scene.json";
pub const PREVIEW_INDEX_FILE: &str = "preview/index.html";
pub const PREVIEW_MANIFEST_FILE: &str = "preview/manifest.json";
pub const TEMPLATE_FILE: &str = "template.json";
pub const APPROVED_PREVIEW_FILE: &str = "approved_preview.json";
pub const TEAM_PLAN_FILE: &str = "construction/team-plan.json";
pub const VERIFY_REPORT_FILE: &str = "verify/report.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowError {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovedPreviewSummary {
    pub rev: u32,
    pub building_count: u32,
    pub style: String,
    pub approved_at: String,
}

/// The core WorldBuild flow state persisted on DebugSession.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldBuildState {
    #[serde(default)]
    pub id: String,                         // e.g. "wb_01J..."
    #[serde(default)]
    pub slug: String,                       // Sanitized folder slug e.g. "cyber-harbor-a1b2"
    #[serde(default)]
    pub dir: String,                        // Relative path: ".forge/world_build/<slug>"
    #[serde(default)]
    pub title: String,                      // Human-readable title
    #[serde(default)]
    pub token: String,                      // 32-hex loopback hosting token for demo_host
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default = "default_stage")]
    pub stage: String,                      // "survey" | "preview_review" | "construction" | "coherence_verify" | "done"
    #[serde(default = "default_phase")]
    pub phase: String,                      // "waiting" | "running" | "failed"
    #[serde(default)]
    pub running: Option<String>,            // Currently executing sub-task name
    #[serde(default)]
    pub last_error: Option<FlowError>,
    #[serde(default)]
    pub preview_rev: u32,                   // Incremented on every rebuild/tune/style action
    #[serde(default)]
    pub selected_building: Option<String>,  // Currently selected building ID in 3D preview
    #[serde(default)]
    pub style_preset: Option<String>,       // Active style preset name
    #[serde(default)]
    pub approved_preview: Option<ApprovedPreviewSummary>,
    #[serde(default)]
    pub construction_round: u32,            // Incremented on fix rounds
    #[serde(default)]
    pub coherence_round: u32,               // Incremented on coherence check runs
    #[serde(default)]
    pub scene_path: Option<String>,         // Target .rxscene path
    #[serde(default)]
    pub passed: Option<bool>,               // Final verification outcome
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_stage() -> String {
    STAGE_SURVEY.to_string()
}

fn default_phase() -> String {
    PHASE_WAITING.to_string()
}

impl WorldBuildState {
    pub fn new_flow(title: &str, workspace_id: Option<&str>, ws_root: &Path) -> Self {
        let id = new_id("wb");
        let title_clean = crate::ultraplan::derive_title(title);
        let slug = allocate_slug(ws_root, &title_clean, &id);
        let ts = now_rfc3339();
        let token = allocate_token();
        WorldBuildState {
            id,
            dir: flow_dir_rel(&slug),
            slug,
            title: title_clean,
            token,
            workspace_id: workspace_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            stage: default_stage(),
            phase: default_phase(),
            running: None,
            last_error: None,
            preview_rev: 0,
            selected_building: None,
            style_preset: None,
            approved_preview: None,
            construction_round: 0,
            coherence_round: 0,
            scene_path: None,
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

    pub fn dir_abs(&self, ws_root: &Path) -> Option<PathBuf> {
        if !is_flow_dir(&self.dir) {
            return None;
        }
        let mut path = ws_root.to_path_buf();
        for comp in self.dir.split(['/', '\\']) {
            path.push(comp);
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return None;
            }
        }
        Some(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldBuildAction {
    Survey,
    RebuildBuilding,
    TuneBuilding,
    AdjustStyle,
    ApprovePreview,
    ResumeConstruction,
    FixCoherence,
    ApproveOverride,
}

impl WorldBuildAction {
    pub const ALL: [WorldBuildAction; 8] = [
        WorldBuildAction::Survey,
        WorldBuildAction::RebuildBuilding,
        WorldBuildAction::TuneBuilding,
        WorldBuildAction::AdjustStyle,
        WorldBuildAction::ApprovePreview,
        WorldBuildAction::ResumeConstruction,
        WorldBuildAction::FixCoherence,
        WorldBuildAction::ApproveOverride,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        WorldBuildAction::ALL
            .into_iter()
            .find(|a| a.as_str() == trimmed)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            WorldBuildAction::Survey => "survey",
            WorldBuildAction::RebuildBuilding => "rebuild_building",
            WorldBuildAction::TuneBuilding => "tune_building",
            WorldBuildAction::AdjustStyle => "adjust_style",
            WorldBuildAction::ApprovePreview => "approve_preview",
            WorldBuildAction::ResumeConstruction => "resume_construction",
            WorldBuildAction::FixCoherence => "fix_coherence",
            WorldBuildAction::ApproveOverride => "approve_override",
        }
    }

    /// Returns whether this action is accepted in the given stage.
    pub fn accepts_stage(self, stage: &str) -> bool {
        match self {
            WorldBuildAction::Survey => stage == STAGE_SURVEY,
            WorldBuildAction::RebuildBuilding
            | WorldBuildAction::TuneBuilding
            | WorldBuildAction::AdjustStyle
            | WorldBuildAction::ApprovePreview => stage == STAGE_PREVIEW_REVIEW,
            WorldBuildAction::ResumeConstruction => stage == STAGE_CONSTRUCTION,
            WorldBuildAction::FixCoherence | WorldBuildAction::ApproveOverride => {
                stage == STAGE_COHERENCE_VERIFY
            }
        }
    }

    /// Revision check: for preview review, rev must match preview_rev.
    pub fn expected_rev(self, state: &WorldBuildState) -> Option<u32> {
        match self {
            WorldBuildAction::Survey => None,
            WorldBuildAction::RebuildBuilding
            | WorldBuildAction::TuneBuilding
            | WorldBuildAction::AdjustStyle
            | WorldBuildAction::ApprovePreview => Some(state.preview_rev),
            WorldBuildAction::ResumeConstruction => Some(state.construction_round),
            WorldBuildAction::FixCoherence | WorldBuildAction::ApproveOverride => {
                Some(state.coherence_round)
            }
        }
    }

    pub fn requires_building_id(self) -> bool {
        matches!(
            self,
            WorldBuildAction::RebuildBuilding | WorldBuildAction::TuneBuilding
        )
    }

    pub fn requires_text(self) -> bool {
        matches!(
            self,
            WorldBuildAction::RebuildBuilding
                | WorldBuildAction::TuneBuilding
                | WorldBuildAction::FixCoherence
        )
    }

    pub fn display_text(self) -> &'static str {
        match self {
            WorldBuildAction::Survey => "调研项目与地图现状",
            WorldBuildAction::RebuildBuilding => "重构选中的建筑",
            WorldBuildAction::TuneBuilding => "微调选中的建筑",
            WorldBuildAction::AdjustStyle => "调整场景风格",
            WorldBuildAction::ApprovePreview => "确认预览设计，开始引擎场景制作",
            WorldBuildAction::ResumeConstruction => "继续多智能体场景制作",
            WorldBuildAction::FixCoherence => "修复场景一致性与碰撞问题",
            WorldBuildAction::ApproveOverride => "确认并收尾场景制作",
        }
    }

    pub fn turn_kind(self) -> TurnKind {
        match self {
            WorldBuildAction::Survey => TurnKind::Survey { fresh: false },
            WorldBuildAction::RebuildBuilding => TurnKind::RebuildBuilding,
            WorldBuildAction::TuneBuilding => TurnKind::TuneBuilding,
            WorldBuildAction::AdjustStyle => TurnKind::AdjustStyle,
            WorldBuildAction::ApprovePreview => TurnKind::Construction,
            WorldBuildAction::ResumeConstruction => TurnKind::Construction,
            WorldBuildAction::FixCoherence => TurnKind::FixCoherence,
            WorldBuildAction::ApproveOverride => TurnKind::CoherenceVerify,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnKind {
    Survey { fresh: bool },
    RebuildBuilding,
    TuneBuilding,
    AdjustStyle,
    Construction,
    CoherenceVerify,
    FixCoherence,
}

impl TurnKind {
    pub fn running(&self) -> &'static str {
        match self {
            TurnKind::Survey { .. } => "survey",
            TurnKind::RebuildBuilding => "rebuild_building",
            TurnKind::TuneBuilding => "tune_building",
            TurnKind::AdjustStyle => "adjust_style",
            TurnKind::Construction => "construction",
            TurnKind::CoherenceVerify => "coherence_verify",
            TurnKind::FixCoherence => "fix_coherence",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            TurnKind::Survey { .. } => "项目与地图现状调查",
            TurnKind::RebuildBuilding => "单体建筑重构",
            TurnKind::TuneBuilding => "单体建筑微调",
            TurnKind::AdjustStyle => "场景风格调整",
            TurnKind::Construction => "多智能体场景施工",
            TurnKind::CoherenceVerify => "全局一致性验收",
            TurnKind::FixCoherence => "场景一致性修复",
        }
    }

    pub fn wants_history(&self) -> bool {
        matches!(self, TurnKind::Survey { fresh: false })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldBuildReq {
    pub id: String,
    #[serde(default)]
    pub rev: Option<u32>,
    pub action: Option<String>,
    #[serde(default)]
    pub building_id: Option<String>,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub style_preset: Option<String>,
    #[serde(default)]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct WorldBuildTurn {
    pub kind: TurnKind,
    pub flow: Option<WorldBuildState>,
    pub action: Option<WorldBuildAction>,
    pub building_id: Option<String>,
    pub instruction: Option<String>,
    pub style_preset: Option<String>,
    pub rev: Option<u32>,
    pub parameters: Option<Value>,
}

impl WorldBuildTurn {
    pub fn display_text<'a>(&'a self, fallback: &'a str) -> &'a str {
        if let Some(ins) = self.instruction.as_deref().filter(|s| !s.trim().is_empty()) {
            return ins;
        }
        if let Some(act) = self.action {
            return act.display_text();
        }
        fallback
    }

    pub fn user_message_tag(&self) -> Option<Value> {
        self.action.map(|a| {
            json!({
                "kind": "world_build",
                "action": a.as_str(),
                "buildingId": self.building_id,
                "stylePreset": self.style_preset,
            })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldBuildError {
    StageMismatch {
        current_stage: Option<String>,
        action: String,
        allowed: Vec<&'static str>,
    },
    InvalidAction(String),
    BuildingNotFound {
        building_id: String,
        available: Vec<String>,
    },
    StaleRevision {
        expected: u32,
        actual: Option<u32>,
    },
    SessionBusy(String),
    WorkspaceLocked,
    MissingText(String),
    NotFound(String),
}

impl WorldBuildError {
    pub fn code(&self) -> &'static str {
        match self {
            WorldBuildError::StageMismatch { .. } => ERR_STAGE_MISMATCH,
            WorldBuildError::InvalidAction(_) => ERR_INVALID_ACTION,
            WorldBuildError::BuildingNotFound { .. } => ERR_BUILDING_NOT_FOUND,
            WorldBuildError::StaleRevision { .. } => ERR_STAGE_MISMATCH,
            WorldBuildError::SessionBusy(_) => "SESSION_BUSY",
            WorldBuildError::WorkspaceLocked => ERR_STAGE_MISMATCH,
            WorldBuildError::MissingText(_) => "INVALID_INPUT",
            WorldBuildError::NotFound(_) => ERR_NOT_FOUND,
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            WorldBuildError::StageMismatch { .. }
            | WorldBuildError::StaleRevision { .. }
            | WorldBuildError::SessionBusy(_)
            | WorldBuildError::WorkspaceLocked => StatusCode::CONFLICT,
            WorldBuildError::BuildingNotFound { .. } | WorldBuildError::NotFound(_) => {
                StatusCode::NOT_FOUND
            }
            WorldBuildError::InvalidAction(_) | WorldBuildError::MissingText(_) => {
                StatusCode::BAD_REQUEST
            }
        }
    }

    pub fn message(&self) -> String {
        match self {
            WorldBuildError::StageMismatch {
                current_stage,
                action,
                allowed,
            } => {
                let stage_desc = current_stage.as_deref().unwrap_or("无流程");
                format!(
                    "当前阶段 '{stage_desc}' 不接受操作 '{action}'；当前允许的操作: {}",
                    allowed.join(", ")
                )
            }
            WorldBuildError::InvalidAction(a) => format!("未知操作: '{a}'"),
            WorldBuildError::BuildingNotFound {
                building_id,
                available,
            } => {
                format!(
                    "在当前 3D 预览场景中未找到建筑 '{building_id}'；可用建筑列表: [{}]",
                    available.join(", ")
                )
            }
            WorldBuildError::StaleRevision { expected, actual } => {
                format!(
                    "版本号已过期 (期望版本: {expected}, 提交版本: {:?})，请刷新预览后重试",
                    actual
                )
            }
            WorldBuildError::SessionBusy(run_id) => {
                format!("会话正在执行上一轮操作 ({run_id})，请稍候")
            }
            WorldBuildError::WorkspaceLocked => {
                "流程进行中不可切换工作区，请切回原工作区或重新开始流程".to_string()
            }
            WorldBuildError::MissingText(action) => {
                format!("操作 '{action}' 需要写明具体意见或提示词")
            }
            WorldBuildError::NotFound(id) => format!("未找到指定的 WorldBuild 流程: {id}"),
        }
    }

    pub fn details(&self) -> Option<Value> {
        match self {
            WorldBuildError::StageMismatch {
                current_stage,
                action,
                allowed,
            } => Some(json!({
                "stage": current_stage,
                "action": action,
                "allowedActions": allowed,
            })),
            WorldBuildError::BuildingNotFound {
                building_id,
                available,
            } => Some(json!({
                "buildingId": building_id,
                "availableBuildings": available,
            })),
            WorldBuildError::StaleRevision { expected, actual } => Some(json!({
                "expectedRev": expected,
                "actualRev": actual,
            })),
            _ => None,
        }
    }

    pub fn into_response(self) -> Response {
        let status = self.status_code();
        let body = json!({
            "error": {
                "code": self.code(),
                "message": self.message(),
                "details": self.details()
            }
        });
        (status, Json(body)).into_response()
    }
}

pub const ALLOWED_FREE_TEXT: &str = "free_text";

pub fn allowed_actions(stage: Option<&str>) -> Vec<&'static str> {
    match stage {
        None | Some(STAGE_SURVEY) => vec![ALLOWED_FREE_TEXT, WorldBuildAction::Survey.as_str()],
        Some(STAGE_PREVIEW_REVIEW) => vec![
            WorldBuildAction::RebuildBuilding.as_str(),
            WorldBuildAction::TuneBuilding.as_str(),
            WorldBuildAction::AdjustStyle.as_str(),
            WorldBuildAction::ApprovePreview.as_str(),
            ALLOWED_FREE_TEXT,
        ],
        Some(STAGE_CONSTRUCTION) => vec![WorldBuildAction::ResumeConstruction.as_str()],
        Some(STAGE_COHERENCE_VERIFY) => vec![
            WorldBuildAction::FixCoherence.as_str(),
            WorldBuildAction::ApproveOverride.as_str(),
        ],
        Some(STAGE_DONE) => vec![ALLOWED_FREE_TEXT],
        Some(_) => Vec::new(),
    }
}

pub fn validate_action(
    current_stage: Option<&str>,
    action: &WorldBuildAction,
) -> Result<(), WorldBuildError> {
    match current_stage {
        Some(s) if action.accepts_stage(s) => Ok(()),
        _ => Err(WorldBuildError::StageMismatch {
            current_stage: current_stage.map(str::to_string),
            action: action.as_str().to_string(),
            allowed: allowed_actions(current_stage),
        }),
    }
}

pub fn route(
    stage: Option<&str>,
    mode: &str,
    action: Option<WorldBuildAction>,
) -> Result<Option<TurnKind>, WorldBuildError> {
    match action {
        Some(a) => {
            if mode != MODE {
                return Err(WorldBuildError::InvalidAction(format!(
                    "操作 '{}' 必须在 mode='{}' 模式下提交",
                    a.as_str(),
                    MODE
                )));
            }
            validate_action(stage, &a)?;
            Ok(Some(a.turn_kind()))
        }
        None if mode != MODE => Ok(None),
        None => match stage {
            None | Some(STAGE_DONE) => Ok(Some(TurnKind::Survey { fresh: true })),
            Some(STAGE_SURVEY) => Ok(Some(TurnKind::Survey { fresh: false })),
            Some(STAGE_PREVIEW_REVIEW) => Ok(Some(TurnKind::AdjustStyle)),
            Some(STAGE_CONSTRUCTION) => Err(WorldBuildError::StageMismatch {
                current_stage: stage.map(str::to_string),
                action: "free_text".to_string(),
                allowed: allowed_actions(stage),
            }),
            Some(STAGE_COHERENCE_VERIFY) => Err(WorldBuildError::StageMismatch {
                current_stage: stage.map(str::to_string),
                action: "free_text".to_string(),
                allowed: allowed_actions(stage),
            }),
            Some(_) => Err(WorldBuildError::StageMismatch {
                current_stage: stage.map(str::to_string),
                action: "unknown".to_string(),
                allowed: allowed_actions(stage),
            }),
        },
    }
}

pub fn resolve_request(
    session: &crate::sessions::DebugSession,
    mode: &str,
    req: Option<&WorldBuildReq>,
    user_text: &str,
) -> Result<Option<WorldBuildTurn>, Response> {
    let flow = session.world_build.clone();
    let stage = flow.as_ref().map(|w| w.stage.as_str());

    let action = match req.and_then(|r| r.action.as_deref()) {
        Some(a_str) => Some(
            WorldBuildAction::parse(a_str)
                .ok_or_else(|| WorldBuildError::InvalidAction(a_str.to_string()).into_response())?,
        ),
        None => None,
    };

    let kind = match route(stage, mode, action) {
        Ok(Some(k)) => k,
        Ok(None) => return Ok(None),
        Err(e) => return Err(e.into_response()),
    };

    // When an action is provided, validate ID, rev, text, and building existence
    if let (Some(a), Some(r)) = (action, req) {
        let w = flow
            .as_ref()
            .ok_or_else(|| WorldBuildError::NotFound(r.id.clone()).into_response())?;

        if r.id != w.id {
            return Err(WorldBuildError::NotFound(format!(
                "请求流程 ID '{}' 与会话当前流程 ID '{}' 不匹配",
                r.id, w.id
            ))
            .into_response());
        }

        if let Some(expected) = a.expected_rev(w) {
            if r.rev != Some(expected) {
                return Err(WorldBuildError::StaleRevision {
                    expected,
                    actual: r.rev,
                }
                .into_response());
            }
        }

        let effective_text = r.instruction.as_deref().unwrap_or(user_text).trim();
        if a.requires_text() && effective_text.is_empty() {
            return Err(WorldBuildError::MissingText(a.as_str().to_string()).into_response());
        }

        if a.requires_building_id() {
            let bldg = r.building_id.as_deref().unwrap_or_default().trim();
            if bldg.is_empty() {
                return Err(WorldBuildError::InvalidAction(format!(
                    "操作 '{}' 必须包含 buildingId",
                    a.as_str()
                ))
                .into_response());
            }
            // Check building existence in scene.json if workspace root is available
            if let Some(ws_root) = session.workspace_id.as_deref() {
                let ws_path = Path::new(ws_root);
                if let Some(dir_abs) = w.dir_abs(ws_path) {
                    let scene_file = dir_abs.join(PREVIEW_SCENE_FILE);
                    if let Some(val) = read_json(&scene_file) {
                        let available: Vec<String> = val["buildings"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|b| b["id"].as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !available.is_empty() && !available.iter().any(|b| b == bldg) {
                            return Err(WorldBuildError::BuildingNotFound {
                                building_id: bldg.to_string(),
                                available,
                            }
                            .into_response());
                        }
                    }
                }
            }
        }
    }

    // Check workspace lock and active running state
    if let Some(w) = flow.as_ref().filter(|w| w.is_active()) {
        if w.workspace_id.as_deref() != session.workspace_id.as_deref() {
            return Err(WorldBuildError::WorkspaceLocked.into_response());
        }
        if w.phase == PHASE_RUNNING {
            return Err(WorldBuildError::SessionBusy(
                w.running.clone().unwrap_or_else(|| "task".to_string()),
            )
            .into_response());
        }
    }

    Ok(Some(WorldBuildTurn {
        kind: kind.clone(),
        flow: if matches!(kind, TurnKind::Survey { fresh: true }) {
            None
        } else {
            flow
        },
        action,
        building_id: req.and_then(|r| r.building_id.clone()),
        instruction: req.and_then(|r| r.instruction.clone()),
        style_preset: req.and_then(|r| r.style_preset.clone()),
        rev: req.and_then(|r| r.rev),
        parameters: req.and_then(|r| r.parameters.clone()),
    }))
}

/// Revalidates latest session state after claim_active_run lock
pub fn revalidate(
    latest: Option<&WorldBuildState>,
    mode: &str,
    turn: &WorldBuildTurn,
) -> Result<(), WorldBuildError> {
    let stage = latest.map(|w| w.stage.as_str());
    let routed = route(stage, mode, turn.action)?;
    let same_flow = match (&turn.flow, latest) {
        (None, _) => true,
        (Some(a), Some(b)) => a.id == b.id && a.preview_rev == b.preview_rev && a.stage == b.stage,
        (Some(_), None) => false,
    };
    if routed != Some(turn.kind.clone()) || !same_flow {
        return Err(WorldBuildError::StageMismatch {
            current_stage: stage.map(str::to_string),
            action: turn
                .action
                .map(|a| a.as_str().to_string())
                .unwrap_or_else(|| "turn".to_string()),
            allowed: allowed_actions(stage),
        });
    }
    Ok(())
}

pub fn flow_dir_rel(slug: &str) -> String {
    format!("{ROOT_DIR}/{slug}")
}

pub fn is_flow_dir(rel: &str) -> bool {
    let norm = rel.replace('\\', "/");
    let Some(slug) = norm.strip_prefix(&format!("{ROOT_DIR}/")) else {
        return false;
    };
    !slug.is_empty()
        && !slug.contains("..")
        && !slug.contains(['/', '\\', ':'])
        && slug != "."
}

pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp_path = PathBuf::from(tmp);
    std::fs::write(&tmp_path, text)?;
    std::fs::rename(&tmp_path, path)
}

pub fn write_json_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, &text)
}

pub fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

pub fn allocate_slug(ws_root: &Path, title: &str, id: &str) -> String {
    let base = crate::ultraplan::slug_base(title);
    let tail: String = id
        .chars()
        .rev()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let mut n = 0usize;
    loop {
        let slug = if n == 0 {
            format!("{base}-{tail}")
        } else {
            format!("{base}-{tail}-{n}")
        };
        let candidate_rel = flow_dir_rel(&slug);
        let cand_abs = ws_root.join(&candidate_rel);
        if !cand_abs.exists() {
            return slug;
        }
        n += 1;
    }
}

fn allocate_token() -> String {
    use std::fmt::Write;
    let mut buf = String::with_capacity(32);
    let mut rng = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for _ in 0..16 {
        let byte = (rng & 0xff) as u8;
        let _ = write!(buf, "{:02x}", byte);
        rng = rng.rotate_left(9) ^ 0x9e3779b97f4a7c15;
    }
    buf
}

/// Lenient deserializer for DebugSession.world_build
pub fn deserialize_lenient<'de, D>(de: D) -> Result<Option<WorldBuildState>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<Value>::deserialize(de)?;
    Ok(raw.and_then(|v| match serde_json::from_value::<WorldBuildState>(v) {
        Ok(s) if !s.id.is_empty() => Some(s),
        Ok(_) => None,
        Err(e) => {
            eprintln!("[world_build] 会话流程状态解析失败, 按无流程处理: {e}");
            None
        }
    }))
}

pub fn stage_payload(wb: &WorldBuildState, run_id: Option<&str>) -> Value {
    let mut v = json!({
        "id": wb.id,
        "slug": wb.slug,
        "token": wb.token,
        "stage": wb.stage,
        "phase": wb.phase,
        "running": wb.running,
        "previewRev": wb.preview_rev,
        "selectedBuilding": wb.selected_building,
        "stylePreset": wb.style_preset,
        "constructionRound": wb.construction_round,
        "coherenceRound": wb.coherence_round,
    });
    if let Some(r) = run_id {
        v["runId"] = json!(r);
    }
    if let Some(e) = &wb.last_error {
        v["lastError"] = json!(e);
    }
    if let Some(ap) = &wb.approved_preview {
        v["approvedPreview"] = json!(ap);
    }
    v
}

/// Loads the manifest from preview directory, falling back to scene.json if manifest.json is absent.
pub fn read_manifest(preview_dir: &Path) -> Result<Value, WorldBuildError> {
    let p = preview_dir.join("manifest.json");
    if p.is_file() {
        if let Some(val) = read_json(&p) {
            return Ok(val);
        }
    }
    let p_scene = preview_dir.join("scene.json");
    if p_scene.is_file() {
        if let Some(val) = read_json(&p_scene) {
            return Ok(val);
        }
    }
    Err(WorldBuildError::NotFound(format!(
        "预览清单不存在于目录: {}",
        preview_dir.display()
    )))
}

/// Atomically persists the manifest to both preview/manifest.json and preview/scene.json.
pub fn write_manifest_atomic(preview_dir: &Path, manifest: &Value) -> Result<(), WorldBuildError> {
    let p_manifest = preview_dir.join("manifest.json");
    let p_scene = preview_dir.join("scene.json");
    write_json_atomic(&p_manifest, manifest)
        .map_err(|e| WorldBuildError::NotFound(format!("写入 manifest.json 失败: {e}")))?;
    write_json_atomic(&p_scene, manifest)
        .map_err(|e| WorldBuildError::NotFound(format!("写入 scene.json 失败: {e}")))?;
    Ok(())
}

pub fn rebuild_building_manifest(
    manifest: &mut Value,
    building_id: &str,
    instruction: &str,
) -> Result<u32, WorldBuildError> {
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return Err(WorldBuildError::MissingText("rebuild_building".into()));
    }
    let building_id = building_id.trim();
    if building_id.is_empty() {
        return Err(WorldBuildError::InvalidAction("必须提供 buildingId".into()));
    }

    let cur_rev = manifest.get("rev").and_then(Value::as_u64).unwrap_or(0) as u32;
    let new_rev = cur_rev + 1;

    let buildings = manifest
        .get_mut("buildings")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| WorldBuildError::NotFound("manifest.buildings 必须是数组".into()))?;

    let bldg_idx = buildings
        .iter()
        .position(|b| b.get("id").and_then(Value::as_str) == Some(building_id))
        .ok_or_else(|| {
            let available: Vec<String> = buildings
                .iter()
                .filter_map(|b| b.get("id").and_then(Value::as_str).map(str::to_string))
                .collect();
            WorldBuildError::BuildingNotFound {
                building_id: building_id.to_string(),
                available,
            }
        })?;

    let b = &mut buildings[bldg_idx];
    b["prompt"] = json!(instruction);
    b["status"] = json!("ready");
    b["rebuilt"] = json!(true);
    b["lastInstruction"] = json!(instruction);

    manifest["rev"] = json!(new_rev);
    manifest["updatedAt"] = json!(now_rfc3339());

    Ok(new_rev)
}

pub fn tune_building_manifest(
    manifest: &mut Value,
    building_id: &str,
    instruction: &str,
    parameters: Option<Value>,
) -> Result<u32, WorldBuildError> {
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return Err(WorldBuildError::MissingText("tune_building".into()));
    }
    let building_id = building_id.trim();
    if building_id.is_empty() {
        return Err(WorldBuildError::InvalidAction("必须提供 buildingId".into()));
    }

    let cur_rev = manifest.get("rev").and_then(Value::as_u64).unwrap_or(0) as u32;
    let new_rev = cur_rev + 1;

    let buildings = manifest
        .get_mut("buildings")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| WorldBuildError::NotFound("manifest.buildings 必须是数组".into()))?;

    let bldg_idx = buildings
        .iter()
        .position(|b| b.get("id").and_then(Value::as_str) == Some(building_id))
        .ok_or_else(|| {
            let available: Vec<String> = buildings
                .iter()
                .filter_map(|b| b.get("id").and_then(Value::as_str).map(str::to_string))
                .collect();
            WorldBuildError::BuildingNotFound {
                building_id: building_id.to_string(),
                available,
            }
        })?;

    let b = &mut buildings[bldg_idx];
    b["prompt"] = json!(instruction);
    if let Some(p) = parameters {
        if let Some(tunings) = b.get_mut("tunings").and_then(Value::as_object_mut) {
            if let Some(p_obj) = p.as_object() {
                for (k, v) in p_obj {
                    tunings.insert(k.clone(), v.clone());
                }
            }
        } else {
            b["tunings"] = p.clone();
        }
        b["parameters"] = p;
    }
    b["status"] = json!("ready");

    manifest["rev"] = json!(new_rev);
    manifest["updatedAt"] = json!(now_rfc3339());

    Ok(new_rev)
}

pub fn adjust_style_manifest(
    manifest: &mut Value,
    style_preset: &str,
) -> Result<u32, WorldBuildError> {
    let style_preset = style_preset.trim();
    if style_preset.is_empty() {
        return Err(WorldBuildError::InvalidAction("必须提供 stylePreset".into()));
    }

    let cur_rev = manifest.get("rev").and_then(Value::as_u64).unwrap_or(0) as u32;
    let new_rev = cur_rev + 1;
    manifest["style"] = json!(style_preset);

    if style_preset.contains("medieval") {
        manifest["environment"] = json!({
            "skybox": "day_medieval_clear",
            "fog": false,
            "ambientIntensity": 0.75
        });
        if let Some(bldgs) = manifest.get_mut("buildings").and_then(Value::as_array_mut) {
            for b in bldgs {
                b["materials"] = json!(["mat_aged_timber", "mat_rough_stone"]);
            }
        }
    } else if style_preset.contains("sci_fi") || style_preset.contains("scifi") {
        manifest["environment"] = json!({
            "skybox": "overcast_clean",
            "fog": false,
            "ambientIntensity": 0.85
        });
        if let Some(bldgs) = manifest.get_mut("buildings").and_then(Value::as_array_mut) {
            for b in bldgs {
                b["materials"] = json!(["mat_brushed_aluminum", "mat_tinted_glass"]);
            }
        }
    } else {
        manifest["environment"] = json!({
            "skybox": "night_cyberpunk",
            "fog": true,
            "ambientIntensity": 0.35
        });
        if let Some(bldgs) = manifest.get_mut("buildings").and_then(Value::as_array_mut) {
            for b in bldgs {
                b["materials"] = json!(["mat_cyber_neon", "mat_wet_metal"]);
            }
        }
    }

    manifest["rev"] = json!(new_rev);
    manifest["updatedAt"] = json!(now_rfc3339());

    Ok(new_rev)
}

pub fn rebuild_building(
    session_store: &crate::sessions::SessionStore,
    sid: &str,
    req_rev: Option<u32>,
    building_id: &str,
    instruction: &str,
    ws_root: &Path,
) -> Result<Value, WorldBuildError> {
    let (_, res) = session_store
        .update_world_build(sid, |slot| {
            let Some(wb) = slot.as_mut() else {
                return Err(WorldBuildError::NotFound(format!("会话 {sid} 无流程")));
            };
            if wb.stage != STAGE_PREVIEW_REVIEW {
                return Err(WorldBuildError::StageMismatch {
                    current_stage: Some(wb.stage.clone()),
                    action: "rebuild_building".into(),
                    allowed: allowed_actions(Some(&wb.stage)),
                });
            }
            if let Some(expected) = req_rev {
                if expected != wb.preview_rev {
                    return Err(WorldBuildError::StaleRevision {
                        expected: wb.preview_rev,
                        actual: Some(expected),
                    });
                }
            }
            wb.selected_building = Some(building_id.to_string());
            Ok((wb.slug.clone(), wb.dir.clone(), wb.token.clone()))
        })
        .ok_or_else(|| WorldBuildError::NotFound(format!("会话不存在: {sid}")))?;
    let (slug, dir_rel, token) = res?;

    let preview_dir = ws_root.join(&dir_rel).join(PREVIEW_DIR);
    let mut manifest = read_manifest(&preview_dir)?;
    let new_rev = rebuild_building_manifest(&mut manifest, building_id, instruction)?;
    if !token.is_empty() {
        manifest["token"] = json!(&token);
    }
    write_manifest_atomic(&preview_dir, &manifest)?;

    let _ = preview::generate_preview_package(ws_root, &slug, Some(&manifest));
    crate::demo_host::global().register(&slug, &preview_dir);
    if !token.is_empty() && token != slug {
        crate::demo_host::global().register(&token, &preview_dir);
    }

    session_store.update_world_build(sid, |slot| {
        if let Some(wb) = slot.as_mut() {
            wb.preview_rev = new_rev;
            wb.selected_building = Some(building_id.to_string());
            wb.touch();
        }
    });

    Ok(manifest)
}

pub fn tune_building(
    session_store: &crate::sessions::SessionStore,
    sid: &str,
    req_rev: Option<u32>,
    building_id: &str,
    instruction: &str,
    parameters: Option<Value>,
    ws_root: &Path,
) -> Result<Value, WorldBuildError> {
    let (_, res) = session_store
        .update_world_build(sid, |slot| {
            let Some(wb) = slot.as_mut() else {
                return Err(WorldBuildError::NotFound(format!("会话 {sid} 无流程")));
            };
            if wb.stage != STAGE_PREVIEW_REVIEW {
                return Err(WorldBuildError::StageMismatch {
                    current_stage: Some(wb.stage.clone()),
                    action: "tune_building".into(),
                    allowed: allowed_actions(Some(&wb.stage)),
                });
            }
            if let Some(expected) = req_rev {
                if expected != wb.preview_rev {
                    return Err(WorldBuildError::StaleRevision {
                        expected: wb.preview_rev,
                        actual: Some(expected),
                    });
                }
            }
            wb.selected_building = Some(building_id.to_string());
            Ok((wb.slug.clone(), wb.dir.clone(), wb.token.clone()))
        })
        .ok_or_else(|| WorldBuildError::NotFound(format!("会话不存在: {sid}")))?;
    let (slug, dir_rel, token) = res?;

    let preview_dir = ws_root.join(&dir_rel).join(PREVIEW_DIR);
    let mut manifest = read_manifest(&preview_dir)?;
    let new_rev = tune_building_manifest(&mut manifest, building_id, instruction, parameters)?;
    if !token.is_empty() {
        manifest["token"] = json!(&token);
    }
    write_manifest_atomic(&preview_dir, &manifest)?;

    let _ = preview::generate_preview_package(ws_root, &slug, Some(&manifest));
    crate::demo_host::global().register(&slug, &preview_dir);
    if !token.is_empty() && token != slug {
        crate::demo_host::global().register(&token, &preview_dir);
    }

    session_store.update_world_build(sid, |slot| {
        if let Some(wb) = slot.as_mut() {
            wb.preview_rev = new_rev;
            wb.selected_building = Some(building_id.to_string());
            wb.touch();
        }
    });

    Ok(manifest)
}

pub fn adjust_style(
    session_store: &crate::sessions::SessionStore,
    sid: &str,
    req_rev: Option<u32>,
    style_preset: &str,
    ws_root: &Path,
) -> Result<Value, WorldBuildError> {
    let (_, res) = session_store
        .update_world_build(sid, |slot| {
            let Some(wb) = slot.as_mut() else {
                return Err(WorldBuildError::NotFound(format!("会话 {sid} 无流程")));
            };
            if wb.stage != STAGE_PREVIEW_REVIEW {
                return Err(WorldBuildError::StageMismatch {
                    current_stage: Some(wb.stage.clone()),
                    action: "adjust_style".into(),
                    allowed: allowed_actions(Some(&wb.stage)),
                });
            }
            if let Some(expected) = req_rev {
                if expected != wb.preview_rev {
                    return Err(WorldBuildError::StaleRevision {
                        expected: wb.preview_rev,
                        actual: Some(expected),
                    });
                }
            }
            wb.style_preset = Some(style_preset.to_string());
            Ok((wb.slug.clone(), wb.dir.clone(), wb.token.clone()))
        })
        .ok_or_else(|| WorldBuildError::NotFound(format!("会话不存在: {sid}")))?;
    let (slug, dir_rel, token) = res?;

    let preview_dir = ws_root.join(&dir_rel).join(PREVIEW_DIR);
    let mut manifest = read_manifest(&preview_dir)?;
    let new_rev = adjust_style_manifest(&mut manifest, style_preset)?;
    if !token.is_empty() {
        manifest["token"] = json!(&token);
    }
    write_manifest_atomic(&preview_dir, &manifest)?;

    let _ = preview::generate_preview_package(ws_root, &slug, Some(&manifest));
    crate::demo_host::global().register(&slug, &preview_dir);
    if !token.is_empty() && token != slug {
        crate::demo_host::global().register(&token, &preview_dir);
    }

    session_store.update_world_build(sid, |slot| {
        if let Some(wb) = slot.as_mut() {
            wb.preview_rev = new_rev;
            wb.style_preset = Some(style_preset.to_string());
            wb.touch();
        }
    });

    Ok(manifest)
}

pub fn approve_preview(
    session_store: &crate::sessions::SessionStore,
    sid: &str,
    req_rev: Option<u32>,
    ws_root: &Path,
) -> Result<ApprovedPreviewSummary, WorldBuildError> {
    let (_, res) = session_store
        .update_world_build(sid, |slot| {
            let Some(wb) = slot.as_mut() else {
                return Err(WorldBuildError::NotFound(format!("会话 {sid} 无流程")));
            };
            if wb.stage != STAGE_PREVIEW_REVIEW {
                return Err(WorldBuildError::StageMismatch {
                    current_stage: Some(wb.stage.clone()),
                    action: "approve_preview".into(),
                    allowed: allowed_actions(Some(&wb.stage)),
                });
            }
            if let Some(expected) = req_rev {
                if expected != wb.preview_rev {
                    return Err(WorldBuildError::StaleRevision {
                        expected: wb.preview_rev,
                        actual: Some(expected),
                    });
                }
            }
            Ok((wb.slug.clone(), wb.preview_rev, wb.dir.clone(), wb.token.clone()))
        })
        .ok_or_else(|| WorldBuildError::NotFound(format!("会话不存在: {sid}")))?;
    let (slug, cur_rev, dir_rel, token) = res?;

    let preview_dir = ws_root.join(&dir_rel).join(PREVIEW_DIR);
    let manifest = read_manifest(&preview_dir)?;
    let bldg_count = manifest
        .get("buildings")
        .and_then(Value::as_array)
        .map_or(0, |a| a.len());

    if bldg_count == 0 {
        return Err(WorldBuildError::InvalidAction("场景中没有建筑，无法确认进入施工".into()));
    }

    let flow_dir = ws_root.join(&dir_rel);
    let approved_path = flow_dir.join(APPROVED_PREVIEW_FILE);
    let template_path = flow_dir.join(TEMPLATE_FILE);
    let style = manifest
        .get("style")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();

    let approved_data = json!({
        "slug": slug,
        "rev": cur_rev,
        "style": style,
        "buildingCount": bldg_count,
        "buildings": manifest.get("buildings"),
        "environment": manifest.get("environment"),
        "cameraViews": manifest.get("cameraViews"),
        "approvedAt": now_rfc3339(),
    });

    write_json_atomic(&approved_path, &approved_data)
        .map_err(|e| WorldBuildError::NotFound(format!("写入 approved_preview.json 失败: {e}")))?;
    write_json_atomic(&template_path, &manifest)
        .map_err(|e| WorldBuildError::NotFound(format!("写入 template.json 失败: {e}")))?;

    // Materialize DAG tasks and atomically persist construction/team-plan.json
    let bldgs_slice = manifest
        .get("buildings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let dag_tasks = crate::collaboration::build_world_build_dag_tasks(&bldgs_slice, &style)
        .map_err(|e| WorldBuildError::InvalidAction(format!("生成施工 DAG 失败: {}", e.message)))?;

    let team_plan_path = flow_dir.join(TEAM_PLAN_FILE);
    let team_plan_data = json!({
        "slug": slug,
        "team": format!("world_build_{slug}"),
        "maxParallel": 4,
        "maxFixRounds": 3,
        "stage": STAGE_CONSTRUCTION,
        "tasks": &dag_tasks,
        "materializedAt": now_rfc3339(),
    });
    write_json_atomic(&team_plan_path, &team_plan_data)
        .map_err(|e| WorldBuildError::NotFound(format!("写入 team-plan.json 失败: {e}")))?;

    crate::demo_host::global().register(&slug, &preview_dir);
    if !token.is_empty() && token != slug {
        crate::demo_host::global().register(&token, &preview_dir);
    }

    let summary = ApprovedPreviewSummary {
        rev: cur_rev,
        building_count: bldg_count as u32,
        style,
        approved_at: now_rfc3339(),
    };

    session_store.update_world_build(sid, |slot| {
        if let Some(wb) = slot.as_mut() {
            wb.stage = STAGE_CONSTRUCTION.to_string();
            wb.phase = PHASE_WAITING.to_string();
            wb.running = None;
            wb.approved_preview = Some(summary.clone());
            wb.construction_round = 1;
            wb.touch();
        }
    });

    Ok(summary)
}

pub fn fix_coherence(
    session_store: &crate::sessions::SessionStore,
    sid: &str,
    req_rev: Option<u32>,
    _instruction: &str,
    parameters: Option<Value>,
    ws_root: &Path,
) -> Result<Value, WorldBuildError> {
    let (_, res) = session_store
        .update_world_build(sid, |slot| {
            let Some(wb) = slot.as_mut() else {
                return Err(WorldBuildError::NotFound(format!("会话 {sid} 无流程")));
            };
            if wb.stage != STAGE_COHERENCE_VERIFY {
                return Err(WorldBuildError::StageMismatch {
                    current_stage: Some(wb.stage.clone()),
                    action: "fix_coherence".into(),
                    allowed: allowed_actions(Some(&wb.stage)),
                });
            }
            if let Some(expected) = req_rev {
                if expected != wb.coherence_round {
                    return Err(WorldBuildError::StaleRevision {
                        expected: wb.coherence_round,
                        actual: Some(expected),
                    });
                }
            }
            Ok((wb.slug.clone(), wb.dir.clone(), wb.coherence_round))
        })
        .ok_or_else(|| WorldBuildError::NotFound(format!("会话不存在: {sid}")))?;
    let (slug, dir_rel, _cur_round) = res?;

    let flow_dir = ws_root.join(&dir_rel);
    let scene_path = flow_dir.join("scene.json");
    let prev_scene_path = flow_dir.join(PREVIEW_SCENE_FILE);

    // Read existing entities from scene.json, preview/scene.json, or template.json
    let scene_data = if scene_path.is_file() {
        read_json(&scene_path).unwrap_or(json!({}))
    } else if prev_scene_path.is_file() {
        read_json(&prev_scene_path).unwrap_or(json!({}))
    } else {
        read_json(&flow_dir.join(TEMPLATE_FILE)).unwrap_or(json!({}))
    };

    let mut entities: Vec<Value> = if let Some(arr) = scene_data.as_array() {
        arr.clone()
    } else if let Some(arr) = scene_data.get("entities").and_then(Value::as_array) {
        arr.clone()
    } else if let Some(arr) = scene_data.get("buildings").and_then(Value::as_array) {
        arr.clone()
    } else {
        Vec::new()
    };

    // Apply remediation parameters
    let mut applied_param = false;
    if let Some(p) = &parameters {
        if let Some(transforms) = p.get("transforms").and_then(Value::as_array) {
            for t in transforms {
                let bid = t.get("buildingId").or_else(|| t.get("id")).and_then(Value::as_str);
                if let Some(target_id) = bid {
                    for ent in &mut entities {
                        let matches_bldg = ent.get("buildingId").and_then(Value::as_str) == Some(target_id)
                            || ent.get("id").and_then(Value::as_str) == Some(target_id);
                        if matches_bldg {
                            if let Some(pos) = t.get("position") {
                                if let Some(tf) = ent.get_mut("transform").and_then(Value::as_object_mut) {
                                    tf.insert("position".into(), pos.clone());
                                } else {
                                    ent["transform"] = json!({ "position": pos });
                                }
                            }
                            if let Some(rot) = t.get("rotation") {
                                if let Some(tf) = ent.get_mut("transform").and_then(Value::as_object_mut) {
                                    tf.insert("rotation".into(), rot.clone());
                                }
                            }
                            applied_param = true;
                        }
                    }
                }
            }
        }
        if let Some(links) = p.get("parentLinks").and_then(Value::as_array) {
            for link in links {
                let prop_id = link.get("propId").or_else(|| link.get("id")).and_then(Value::as_str);
                let parent_id = link.get("parentEntityId").and_then(Value::as_str);
                if let (Some(pr_id), Some(pa_id)) = (prop_id, parent_id) {
                    for ent in &mut entities {
                        if ent.get("id").and_then(Value::as_str) == Some(pr_id) {
                            ent["parentEntityId"] = json!(pa_id);
                            applied_param = true;
                        }
                    }
                }
            }
        }
    }

    if !applied_param {
        // Automatic separation of colliding building pairs
        for i in 0..entities.len() {
            for j in (i + 1)..entities.len() {
                let b1 = &entities[i];
                let b2 = &entities[j];
                let (min1, max1) = verify::parse_bounds(b1);
                let (min2, max2) = verify::parse_bounds(b2);
                let pos1 = verify::parse_position(b1);
                let pos2 = verify::parse_position(b2);
                let (collides, ox, _oz) = verify::check_aabb_xz_collision(min1, max1, pos1, min2, max2, pos2, verify::COLLISION_EPSILON);
                if collides {
                    let new_x = pos2[0] + ox + 0.1;
                    if let Some(tf) = entities[j].get_mut("transform").and_then(Value::as_object_mut) {
                        tf.insert("position".into(), json!([new_x, pos2[1], pos2[2]]));
                    } else {
                        entities[j]["transform"] = json!({ "position": [new_x, pos2[1], pos2[2]] });
                    }
                }
            }
        }
        // Ensure props have parentEntityId
        let building_ent_ids: Vec<String> = entities.iter()
            .filter(|e| e.get("isBuilding").and_then(Value::as_bool).unwrap_or(false))
            .filter_map(|e| e.get("id").and_then(Value::as_str).map(str::to_string))
            .collect();
        if let Some(first_bldg) = building_ent_ids.first() {
            for ent in &mut entities {
                if ent.get("isInteriorProp").and_then(Value::as_bool).unwrap_or(false) {
                    let has_valid_parent = ent.get("parentEntityId")
                        .and_then(Value::as_str)
                        .is_some_and(|p| building_ent_ids.contains(&p.to_string()));
                    if !has_valid_parent {
                        ent["parentEntityId"] = json!(first_bldg);
                    }
                }
            }
        }
    }

    // Persist updated scene
    let updated_scene = if scene_data.is_array() {
        json!(entities)
    } else {
        let mut obj = scene_data.as_object().cloned().unwrap_or_default();
        if obj.contains_key("entities") {
            obj.insert("entities".into(), json!(entities));
        } else {
            obj.insert("buildings".into(), json!(entities));
        }
        Value::Object(obj)
    };
    let _ = write_json_atomic(&scene_path, &updated_scene);

    // Re-run verification
    let template_val = read_json(&flow_dir.join(TEMPLATE_FILE));
    let audit = verify::run_coherence_verification(&ws_root, &slug, &entities, template_val.as_ref(), None)?;

    let is_pass = audit.status == "PASS";

    session_store.update_world_build(sid, |slot| {
        if let Some(wb) = slot.as_mut() {
            wb.coherence_round += 1;
            if is_pass {
                wb.stage = STAGE_DONE.to_string();
                wb.passed = Some(true);
                wb.phase = PHASE_WAITING.to_string();
                wb.running = None;
            } else {
                wb.stage = STAGE_COHERENCE_VERIFY.to_string();
                wb.phase = PHASE_WAITING.to_string();
                wb.running = None;
            }
            wb.touch();
        }
    });

    Ok(serde_json::to_value(&audit).unwrap_or_default())
}

pub fn approve_override(
    session_store: &crate::sessions::SessionStore,
    sid: &str,
    req_rev: Option<u32>,
    instruction: Option<&str>,
    ws_root: &Path,
) -> Result<Value, WorldBuildError> {
    let (_, res) = session_store
        .update_world_build(sid, |slot| {
            let Some(wb) = slot.as_mut() else {
                return Err(WorldBuildError::NotFound(format!("会话 {sid} 无流程")));
            };
            if wb.stage != STAGE_COHERENCE_VERIFY {
                return Err(WorldBuildError::StageMismatch {
                    current_stage: Some(wb.stage.clone()),
                    action: "approve_override".into(),
                    allowed: allowed_actions(Some(&wb.stage)),
                });
            }
            if let Some(expected) = req_rev {
                if expected != wb.coherence_round {
                    return Err(WorldBuildError::StaleRevision {
                        expected: wb.coherence_round,
                        actual: Some(expected),
                    });
                }
            }
            Ok((wb.slug.clone(), wb.dir.clone(), wb.coherence_round))
        })
        .ok_or_else(|| WorldBuildError::NotFound(format!("会话不存在: {sid}")))?;
    let (_slug, dir_rel, _cur_round) = res?;

    let flow_dir = ws_root.join(&dir_rel);
    let audit_path = flow_dir.join("audit.json");
    let report_path = flow_dir.join(VERIFY_REPORT_FILE);

    let mut audit_val = read_json(&audit_path).unwrap_or_else(|| {
        json!({
            "status": "PASS",
            "stage": "done",
            "evaluatedBuildings": 0,
            "evaluatedProps": 0,
            "violations": [],
            "cameraFrames": [],
        })
    });

    audit_val["status"] = json!("PASS");
    audit_val["stage"] = json!("done");
    audit_val["overrideApproved"] = json!(true);
    if let Some(ins) = instruction {
        audit_val["overrideReason"] = json!(ins);
    }
    audit_val["overriddenAt"] = json!(now_rfc3339());
    audit_val["evaluatedAt"] = json!(now_rfc3339());

    let _ = write_json_atomic(&audit_path, &audit_val);
    let _ = write_json_atomic(&report_path, &audit_val);

    session_store.update_world_build(sid, |slot| {
        if let Some(wb) = slot.as_mut() {
            wb.stage = STAGE_DONE.to_string();
            wb.passed = Some(true);
            wb.phase = PHASE_WAITING.to_string();
            wb.running = None;
            wb.coherence_round += 1;
            wb.touch();
        }
    });

    Ok(audit_val)
}



fn emit(state: &AppState, sid: &str, event_type: &str, payload: Value) {
    state.events.emit(EventDraft::new(sid, event_type, "world_build").payload(payload));
}

fn emit_session_updated(state: &AppState, sid: &str) {
    state.events.emit(
        EventDraft::new(sid, "session.updated", "session").payload(json!({ "sessionId": sid })),
    );
}

#[derive(Debug, Clone)]
pub struct WorldBuildRuntime {
    pub flow_id: String,
    pub kind: TurnKind,
    pub action_result: Option<Value>,
}

pub(crate) fn begin_turn(
    state: &AppState,
    sid: &str,
    run_id: &str,
    scope: &crate::scope::ScopeProject,
    user_text: &str,
    wt: &WorldBuildTurn,
) -> Result<WorldBuildRuntime, String> {
    let ws_root = &scope.workspace_root;

    // 1. When an action is provided (rebuild_building, tune_building, adjust_style, approve_preview)
    if let Some(action) = wt.action {
        let action_result = match action {
            WorldBuildAction::RebuildBuilding => {
                let building_id = wt.building_id.as_deref().unwrap_or("");
                let instruction = wt.instruction.as_deref().unwrap_or(user_text);
                let val = rebuild_building(&state.sessions, sid, wt.rev, building_id, instruction, ws_root)
                    .map_err(|e| format!("{}: {}", e.code(), e.message()))?;
                Some(val)
            }
            WorldBuildAction::TuneBuilding => {
                let building_id = wt.building_id.as_deref().unwrap_or("");
                let instruction = wt.instruction.as_deref().unwrap_or(user_text);
                let val = tune_building(&state.sessions, sid, wt.rev, building_id, instruction, wt.parameters.clone(), ws_root)
                    .map_err(|e| format!("{}: {}", e.code(), e.message()))?;
                Some(val)
            }
            WorldBuildAction::AdjustStyle => {
                let style_preset = wt.style_preset.as_deref().unwrap_or("cyberpunk");
                let val = adjust_style(&state.sessions, sid, wt.rev, style_preset, ws_root)
                    .map_err(|e| format!("{}: {}", e.code(), e.message()))?;
                Some(val)
            }
            WorldBuildAction::ApprovePreview => {
                let summary = approve_preview(&state.sessions, sid, wt.rev, ws_root)
                    .map_err(|e| format!("{}: {}", e.code(), e.message()))?;
                let leader_id = crate::collaboration::root_id(sid);
                if let Some(s) = state.sessions.get(sid) {
                    if let Some(wb) = &s.world_build {
                        let prev_dir = ws_root.join(&wb.dir).join(PREVIEW_DIR);
                        if let Ok(manifest) = read_manifest(&prev_dir) {
                            if let Some(bldgs) = manifest.get("buildings").and_then(Value::as_array) {
                                let style = manifest.get("style").and_then(Value::as_str).unwrap_or("default");
                                let _ = state.collaboration.materialize_world_build_dag(
                                    sid,
                                    &leader_id,
                                    &wb.slug,
                                    bldgs,
                                    style,
                                );
                            }
                        }
                    }
                }
                Some(serde_json::to_value(&summary).unwrap_or_default())
            }
            WorldBuildAction::FixCoherence => {
                let instruction = wt.instruction.as_deref().unwrap_or(user_text);
                let val = fix_coherence(
                    &state.sessions,
                    sid,
                    wt.rev,
                    instruction,
                    wt.parameters.clone(),
                    ws_root,
                )
                .map_err(|e| format!("{}: {}", e.code(), e.message()))?;
                Some(val)
            }
            WorldBuildAction::ApproveOverride => {
                let instruction = wt.instruction.as_deref();
                let val = approve_override(&state.sessions, sid, wt.rev, instruction, ws_root)
                    .map_err(|e| format!("{}: {}", e.code(), e.message()))?;
                Some(val)
            }
            _ => None,
        };

        let flow = state
            .sessions
            .get(sid)
            .and_then(|s| s.world_build)
            .ok_or_else(|| format!("{}: 会话中未找到流程", ERR_NOT_FOUND))?;

        emit(state, sid, "world_build.stage", stage_payload(&flow, Some(run_id)));
        emit_session_updated(state, sid);

        return Ok(WorldBuildRuntime {
            flow_id: flow.id,
            kind: wt.kind.clone(),
            action_result,
        });
    }

    // 2. Fresh survey turn
    if matches!(wt.kind, TurnKind::Survey { fresh: true }) {
        let mut flow = WorldBuildState::new_flow(user_text, scope.workspace_id.as_deref(), ws_root);
        let dir = flow
            .dir_abs(ws_root)
            .ok_or_else(|| format!("{}: 分配流程目录失败", ERR_TURN_INVALID))?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("{}: 建流程目录失败: {e}", ERR_TURN_INVALID))?;

        // Extract and persist facts
        let facts = survey::survey_workspace(ws_root, &scope.project_root, user_text).ok();
        if let Some(f) = &facts {
            let _ = survey::persist_facts(ws_root, &flow.slug, f);
        }

        // Generate initial preview package
        let mut manifest = preview::synthesize_initial_manifest(&flow.slug, None);
        manifest["token"] = json!(&flow.token);
        let preview_dir = preview::generate_preview_package(ws_root, &flow.slug, Some(&manifest))
            .map_err(|e| format!("{}: 生成预览包失败: {e}", ERR_PREVIEW_FAILED))?;
        crate::demo_host::global().register(&flow.slug, &preview_dir);
        if !flow.token.is_empty() && flow.token != flow.slug {
            crate::demo_host::global().register(&flow.token, &preview_dir);
        }

        flow.stage = STAGE_PREVIEW_REVIEW.to_string();
        flow.phase = PHASE_RUNNING.to_string();
        flow.running = Some("survey".into());
        flow.touch();

        state.sessions.update_world_build(sid, |slot| {
            *slot = Some(flow.clone());
        });

        emit(
            state,
            sid,
            "world_build.started",
            json!({
                "runId": run_id,
                "id": flow.id,
                "title": flow.title,
                "slug": flow.slug,
                "dir": flow.dir,
                "token": flow.token,
            }),
        );
        emit(state, sid, "world_build.stage", stage_payload(&flow, Some(run_id)));
        emit_session_updated(state, sid);

        return Ok(WorldBuildRuntime {
            flow_id: flow.id,
            kind: wt.kind.clone(),
            action_result: None,
        });
    }

    // 3. Followup turn
    let flow = wt
        .flow
        .clone()
        .or_else(|| state.sessions.get(sid).and_then(|s| s.world_build))
        .ok_or_else(|| format!("{}: 缺少流程", ERR_NOT_FOUND))?;

    state.sessions.update_world_build(sid, |slot| {
        if let Some(w) = slot.as_mut() {
            w.phase = PHASE_RUNNING.to_string();
            w.running = Some(wt.kind.running().to_string());
            w.touch();
        }
    });

    emit(state, sid, "world_build.stage", stage_payload(&flow, Some(run_id)));
    emit_session_updated(state, sid);

    Ok(WorldBuildRuntime {
        flow_id: flow.id,
        kind: wt.kind.clone(),
        action_result: None,
    })
}

pub(crate) fn finish_turn(
    state: &AppState,
    sid: &str,
    run_id: &str,
    flow_id: &str,
    status: &str,
    error: Option<&str>,
) {
    let updated = state.sessions.update_world_build(sid, |slot| match slot.as_mut() {
        Some(w) if w.id == flow_id => {
            match status {
                "completed" | "cancelled" => {
                    w.phase = PHASE_WAITING.into();
                    w.running = None;
                    w.last_error = None;
                }
                _ => {
                    w.phase = PHASE_FAILED.into();
                    let msg = error.unwrap_or("本轮失败");
                    let (code, message) = match msg.split_once(": ") {
                        Some((c, m))
                            if c.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_') =>
                        {
                            (c.to_string(), m.to_string())
                        }
                        _ => (ERR_TURN_INVALID.to_string(), msg.to_string()),
                    };
                    w.last_error = Some(FlowError {
                        code,
                        message: message.chars().take(300).collect(),
                    });
                }
            }
            w.touch();
            true
        }
        _ => false,
    });
    if let Some((s, true)) = updated {
        if let Some(w) = s.world_build.as_ref() {
            emit(state, sid, "world_build.stage", stage_payload(w, Some(run_id)));
        }
        emit_session_updated(state, sid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_world_build_route_exhaustive() {
        let all_stages = [
            None,
            Some(STAGE_SURVEY),
            Some(STAGE_PREVIEW_REVIEW),
            Some(STAGE_CONSTRUCTION),
            Some(STAGE_COHERENCE_VERIFY),
            Some(STAGE_DONE),
        ];

        for stage in all_stages {
            for action in WorldBuildAction::ALL {
                let res_wb = route(stage, MODE, Some(action));
                let expected_accept = stage.map(|s| action.accepts_stage(s)).unwrap_or(false);
                if expected_accept {
                    assert!(
                        res_wb.is_ok(),
                        "Expected Ok for stage={stage:?}, action={:?}",
                        action.as_str()
                    );
                } else {
                    assert!(
                        matches!(res_wb, Err(WorldBuildError::StageMismatch { .. })),
                        "Expected StageMismatch for stage={stage:?}, action={:?}",
                        action.as_str()
                    );
                }

                // Wrong mode must always fail with InvalidAction
                let res_other = route(stage, "build", Some(action));
                assert!(
                    matches!(res_other, Err(WorldBuildError::InvalidAction(_))),
                    "Expected InvalidAction for wrong mode 'build' with action={:?}",
                    action.as_str()
                );
            }

            // Free text (action = None)
            let res_free = route(stage, MODE, None);
            match stage {
                None | Some(STAGE_SURVEY) | Some(STAGE_PREVIEW_REVIEW) | Some(STAGE_DONE) => {
                    assert!(
                        res_free.is_ok(),
                        "Expected free text allowed for stage={stage:?}"
                    );
                }
                Some(STAGE_CONSTRUCTION) | Some(STAGE_COHERENCE_VERIFY) => {
                    assert!(
                        matches!(res_free, Err(WorldBuildError::StageMismatch { .. })),
                        "Expected free text rejected in construction/verify stage={stage:?}"
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_world_build_action_parsing() {
        for a in WorldBuildAction::ALL {
            assert_eq!(WorldBuildAction::parse(a.as_str()), Some(a));
        }
        assert_eq!(WorldBuildAction::parse("not_an_action"), None);
    }

    #[test]
    fn test_world_build_stage_transition_lifecycle() {
        let tmp = std::env::temp_dir().join(format!("wb-lifecycle-{}", std::process::id()));
        let mut state = WorldBuildState::new_flow("SciFi Harbor", Some("ws_test"), &tmp);
        assert_eq!(state.stage, STAGE_SURVEY);
        assert_eq!(state.phase, PHASE_WAITING);
        assert_eq!(state.preview_rev, 0);

        // Transition: survey -> preview_review
        state.stage = STAGE_PREVIEW_REVIEW.to_string();
        state.touch();
        assert!(WorldBuildAction::RebuildBuilding.accepts_stage(&state.stage));
        assert!(WorldBuildAction::TuneBuilding.accepts_stage(&state.stage));
        assert!(WorldBuildAction::AdjustStyle.accepts_stage(&state.stage));
        assert!(WorldBuildAction::ApprovePreview.accepts_stage(&state.stage));
        assert!(!WorldBuildAction::ResumeConstruction.accepts_stage(&state.stage));

        // Simulate preview action: increment rev
        state.preview_rev += 1;
        state.selected_building = Some("bldg_01".to_string());
        state.touch();
        assert_eq!(state.preview_rev, 1);

        // Transition: preview_review -> construction
        state.stage = STAGE_CONSTRUCTION.to_string();
        state.approved_preview = Some(ApprovedPreviewSummary {
            rev: state.preview_rev,
            building_count: 3,
            style: "cyberpunk".to_string(),
            approved_at: now_rfc3339(),
        });
        state.touch();
        assert!(WorldBuildAction::ResumeConstruction.accepts_stage(&state.stage));
        assert!(!WorldBuildAction::ApprovePreview.accepts_stage(&state.stage));

        // Transition: construction -> coherence_verify
        state.stage = STAGE_COHERENCE_VERIFY.to_string();
        state.coherence_round += 1;
        state.touch();
        assert!(WorldBuildAction::FixCoherence.accepts_stage(&state.stage));
        assert!(WorldBuildAction::ApproveOverride.accepts_stage(&state.stage));

        // Transition: coherence_verify -> done
        state.stage = STAGE_DONE.to_string();
        state.passed = Some(true);
        state.touch();
        assert!(!state.is_active());
    }

    #[test]
    fn test_world_build_revalidate_concurrent_protection() {
        let tmp = std::env::temp_dir().join(format!("wb-revalidate-{}", std::process::id()));
        let mut base_state = WorldBuildState::new_flow("Test", None, &tmp);
        base_state.stage = STAGE_PREVIEW_REVIEW.to_string();
        base_state.preview_rev = 1;

        let turn = WorldBuildTurn {
            kind: TurnKind::RebuildBuilding,
            flow: Some(base_state.clone()),
            action: Some(WorldBuildAction::RebuildBuilding),
            building_id: Some("b1".to_string()),
            instruction: Some("add neon".to_string()),
            style_preset: None,
            rev: None,
            parameters: None,
        };

        // Same state passes
        assert!(revalidate(Some(&base_state), MODE, &turn).is_ok());

        // Concurrent rev bump fails
        let mut bumped = base_state.clone();
        bumped.preview_rev = 2;
        assert!(revalidate(Some(&bumped), MODE, &turn).is_err());

        // Concurrent stage transition fails
        let mut advanced = base_state.clone();
        advanced.stage = STAGE_CONSTRUCTION.to_string();
        assert!(revalidate(Some(&advanced), MODE, &turn).is_err());
    }

    #[test]
    fn test_atomic_persistence_and_path_security() {
        let tmp = std::env::temp_dir().join(format!("wb-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Valid path check
        assert!(is_flow_dir(".forge/world_build/test-slug"));
        assert!(!is_flow_dir(".forge/world_build/../escape"));
        assert!(!is_flow_dir(".forge/world_build/nested/dir"));
        assert!(!is_flow_dir(".forge/other/test-slug"));
        assert!(!is_flow_dir(".forge/world_build/"));

        // Atomic write
        let target = tmp.join("test.json");
        write_json_atomic(&target, &json!({"key": "val"})).unwrap();
        assert!(target.is_file());
        assert_eq!(read_json(&target).unwrap()["key"], "val");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_preview_generation_and_contract_artifacts() {
        let tmp = std::env::temp_dir().join(format!("wb-preview-gen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let slug = "cyber-test-preview";
        let preview_dir = generate_preview_package(&tmp, slug, None).unwrap();

        assert!(preview_dir.join("index.html").is_file());
        assert!(preview_dir.join("preview.js").is_file());
        assert!(preview_dir.join("scene.json").is_file());
        assert!(preview_dir.join("manifest.json").is_file());
        assert!(preview_dir.join("probe.json").is_file());

        // Check index.html contains zero external CDN dependencies
        let html = std::fs::read_to_string(preview_dir.join("index.html")).unwrap();
        assert!(!html.contains("unpkg.com"));
        assert!(!html.contains("cdnjs.cloudflare.com"));
        assert!(!html.contains("fonts.googleapis.com"));
        assert!(html.contains("window.__buildPreview"));
        assert!(html.contains("window.__demo"));

        // Check scene.json schema
        let scene: Value = read_json(&preview_dir.join("scene.json")).unwrap();
        assert_eq!(scene["slug"], slug);
        assert_eq!(scene["rev"], 1);
        let bldgs = scene["buildings"].as_array().unwrap();
        assert!(!bldgs.is_empty());
        assert_eq!(bldgs[0]["id"], "bldg_terminal_01");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_rebuild_building_preserves_other_buildings() {
        let tmp = std::env::temp_dir().join(format!("wb-rebuild-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let slug = "rebuild-flow";
        let preview_dir = generate_preview_package(&tmp, slug, None).unwrap();

        let mut manifest = read_manifest(&preview_dir).unwrap();
        let b2_before = manifest["buildings"][1].clone();

        let new_rev = rebuild_building_manifest(&mut manifest, "bldg_terminal_01", "顶层加装全息雷达").unwrap();
        assert_eq!(new_rev, 2);
        assert_eq!(manifest["rev"], 2);

        // Targeted building updated
        assert_eq!(manifest["buildings"][0]["prompt"], "顶层加装全息雷达");
        assert_eq!(manifest["buildings"][0]["rebuilt"], true);

        // Other building strictly preserved
        assert_eq!(manifest["buildings"][1], b2_before);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_tune_building_updates_parameters() {
        let tmp = std::env::temp_dir().join(format!("wb-tune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let slug = "tune-flow";
        let preview_dir = generate_preview_package(&tmp, slug, None).unwrap();

        let mut manifest = read_manifest(&preview_dir).unwrap();
        let b1_before = manifest["buildings"][0].clone();

        let params = json!({ "signColor": "neon_magenta", "doorMaterial": "carbon_fiber" });
        let new_rev = tune_building_manifest(
            &mut manifest,
            "bldg_warehouse_02",
            "调整外墙招牌与大门材质",
            Some(params.clone()),
        ).unwrap();

        assert_eq!(new_rev, 2);
        assert_eq!(manifest["buildings"][1]["parameters"], params);
        assert_eq!(manifest["buildings"][1]["prompt"], "调整外墙招牌与大门材质");

        // Other building strictly preserved
        assert_eq!(manifest["buildings"][0], b1_before);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_adjust_style_updates_lighting_and_materials() {
        let tmp = std::env::temp_dir().join(format!("wb-style-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let slug = "style-flow";
        let preview_dir = generate_preview_package(&tmp, slug, None).unwrap();

        let mut manifest = read_manifest(&preview_dir).unwrap();
        let pos_before = manifest["buildings"][0]["transform"]["position"].clone();

        let new_rev = adjust_style_manifest(&mut manifest, "medieval_fantasy").unwrap();
        assert_eq!(new_rev, 2);
        assert_eq!(manifest["style"], "medieval_fantasy");
        assert_eq!(manifest["environment"]["skybox"], "day_medieval_clear");

        // Geometry transforms preserved
        assert_eq!(manifest["buildings"][0]["transform"]["position"], pos_before);
        // Materials updated to medieval
        assert!(manifest["buildings"][0]["materials"].as_array().unwrap().contains(&json!("mat_aged_timber")));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_error_handling_building_not_found() {
        let mut manifest = json!({
            "rev": 1,
            "buildings": [
                { "id": "b1", "name": "B1" },
                { "id": "b2", "name": "B2" }
            ]
        });

        let err = rebuild_building_manifest(&mut manifest, "ghost_building", "new prompt").unwrap_err();
        match err {
            WorldBuildError::BuildingNotFound { building_id, available } => {
                assert_eq!(building_id, "ghost_building");
                assert_eq!(available, vec!["b1", "b2"]);
            }
            other => panic!("Expected BuildingNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_approve_preview_creates_snapshot_and_advances_stage() {
        let tmp = std::env::temp_dir().join(format!("wb-approve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let session_store = crate::sessions::SessionStore::load(tmp.join("sessions.json"));
        let sess = session_store.create("Approve Test", "agent", None, false, Some("ws_test".to_string()));
        let sid = &sess.id;

        let mut wb = WorldBuildState::new_flow("Approve Test", Some("ws_test"), &tmp);
        wb.stage = STAGE_PREVIEW_REVIEW.to_string();
        wb.preview_rev = 3;
        let slug = wb.slug.clone();

        session_store.update_world_build(sid, |slot| *slot = Some(wb)).unwrap();

        let preview_dir = generate_preview_package(&tmp, &slug, None).unwrap();
        let mut manifest = read_manifest(&preview_dir).unwrap();
        manifest["rev"] = json!(3);
        write_manifest_atomic(&preview_dir, &manifest).unwrap();

        // Stale rev rejected
        let stale_err = approve_preview(&session_store, sid, Some(2), &tmp).unwrap_err();
        assert!(matches!(stale_err, WorldBuildError::StaleRevision { .. }));

        // Correct approval
        let summary = approve_preview(&session_store, sid, Some(3), &tmp).unwrap();
        assert_eq!(summary.rev, 3);
        assert_eq!(summary.building_count, 3);

        let after = session_store.get(sid).unwrap().world_build.unwrap();
        assert_eq!(after.stage, STAGE_CONSTRUCTION);
        assert_eq!(after.construction_round, 1);
        assert_eq!(after.phase, PHASE_WAITING);

        // Artifacts created
        let flow_dir = tmp.join(".forge/world_build").join(&slug);
        assert!(flow_dir.join("approved_preview.json").is_file());
        assert!(flow_dir.join("template.json").is_file());
        assert!(flow_dir.join("construction/team-plan.json").is_file());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_fix_coherence_and_approve_override_actions() {
        let tmp = std::env::temp_dir().join(format!("wb-coherence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let session_store = crate::sessions::SessionStore::load(tmp.join("sessions.json"));
        let sess = session_store.create("Coherence Test", "agent", None, false, Some("ws_test".to_string()));
        let sid = &sess.id;

        let mut wb = WorldBuildState::new_flow("Coherence Test", Some("ws_test"), &tmp);
        wb.stage = STAGE_COHERENCE_VERIFY.to_string();
        wb.coherence_round = 1;
        let slug = wb.slug.clone();

        session_store.update_world_build(sid, |slot| *slot = Some(wb)).unwrap();

        let flow_dir = tmp.join(".forge/world_build").join(&slug);
        std::fs::create_dir_all(&flow_dir).unwrap();

        // 1. Initial scene with colliding buildings
        let scene_colliding = json!([
            {"id": "e1", "isBuilding": true, "buildingId": "b1", "bounds": {"min": [-2,-1,-2], "max": [2,1,2]}, "transform": {"position": [0,0,0]}},
            {"id": "e2", "isBuilding": true, "buildingId": "b2", "bounds": {"min": [-2,-1,-2], "max": [2,1,2]}, "transform": {"position": [2,0,0]}}
        ]);
        let template_data = json!({
            "buildings": [
                {"id": "b1", "bounds": {"min": [-2,-1,-2], "max": [2,1,2]}},
                {"id": "b2", "bounds": {"min": [-2,-1,-2], "max": [2,1,2]}}
            ]
        });
        write_json_atomic(&flow_dir.join("scene.json"), &scene_colliding).unwrap();
        write_json_atomic(&flow_dir.join(TEMPLATE_FILE), &template_data).unwrap();

        // Stale rev rejected
        let stale = fix_coherence(&session_store, sid, Some(0), "fix it", None, &tmp).unwrap_err();
        assert!(matches!(stale, WorldBuildError::StaleRevision { .. }));

        // Fix coherence with parameters moving b2 to [6, 0, 0]
        let params = json!({
            "transforms": [
                {"buildingId": "b2", "position": [6.0, 0.0, 0.0]}
            ]
        });
        let audit_res = fix_coherence(&session_store, sid, Some(1), "separate buildings", Some(params), &tmp).unwrap();
        assert_eq!(audit_res.get("status").and_then(Value::as_str), Some("PASS"));

        let after = session_store.get(sid).unwrap().world_build.unwrap();
        assert_eq!(after.stage, STAGE_DONE);
        assert_eq!(after.coherence_round, 2);
        assert_eq!(after.passed, Some(true));

        // Test approve_override on another session
        let mut wb2 = WorldBuildState::new_flow("Override Test", Some("ws_test"), &tmp);
        wb2.stage = STAGE_COHERENCE_VERIFY.to_string();
        wb2.coherence_round = 1;
        let slug2 = wb2.slug.clone();
        let sid2 = &session_store.create("Override Test", "agent", None, false, Some("ws_test".to_string())).id;
        session_store.update_world_build(sid2, |slot| *slot = Some(wb2)).unwrap();
        let flow_dir2 = tmp.join(".forge/world_build").join(&slug2);
        std::fs::create_dir_all(&flow_dir2).unwrap();

        let override_res = approve_override(&session_store, sid2, Some(1), Some("intentional overlap"), &tmp).unwrap();
        assert_eq!(override_res.get("status").and_then(Value::as_str), Some("PASS"));
        assert_eq!(override_res.get("overrideApproved").and_then(Value::as_bool), Some(true));

        let after2 = session_store.get(sid2).unwrap().world_build.unwrap();
        assert_eq!(after2.stage, STAGE_DONE);
        assert_eq!(after2.passed, Some(true));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}


