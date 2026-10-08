//! Homepage suggestions use the same workspace, profile and permission facts as agent turns.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{profile::AgentProfile, AppState};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationQuery {
    session_id: Option<String>,
    workspace_id: Option<String>,
    agent_engine: Option<String>,
}

fn card(id: &str, label: &str, desc: &str, draft: String, mode: &str, image: &str) -> Value {
    json!({"id":id,"label":label,"desc":desc,"draft":draft,"mode":mode,"image":image})
}

fn suggestions(
    is_2d: bool,
    project_name: &str,
    profile: &AgentProfile,
    engine: &str,
    read_only: bool,
    plan: Option<&str>,
) -> Vec<Value> {
    let dimension = if is_2d { "2D" } else { "3D" };
    let can_edit = profile.wants_edit_tools() && !read_only;
    let name = project_name.to_lowercase();
    let is_defense = is_2d
        && ["植物大战僵尸", "plants vs zombies"]
            .iter()
            .any(|hint| name.contains(hint));
    let planning_mode = if profile.allowed_modes().contains(&"plan") {
        "plan"
    } else {
        "ask"
    };
    let mut cards = vec![
        if can_edit && is_defense {
            card("greybox", "搭建草坪防线", "五条进攻路线、植物与敌人出生点",
                "先检查当前工作区的塔防玩法、场景与资产，复用现有素材搭建五条草坪进攻路线、植物放置格和敌人出生点，保持项目现有风格，并验证单位显示、碰撞和路线配置。".into(), "build", "defense")
        } else if can_edit {
            card("greybox", if is_2d { "搭一个 2D 横版关卡" } else { "搭一个 3D 灰盒关卡" },
                if is_2d { "地面瓦片、三段平台与一个出生点" } else { "地面、三级平台与一个出生点" },
                format!("先检查当前工作区的场景与资产，再搭一个 {dimension} {}：{}、三段平台和一个出生点。复用已有素材，并验证相机与碰撞。",
                    if is_2d { "横版关卡" } else { "灰盒关卡" }, if is_2d { "地面瓦片" } else { "地面" }), "build", "level")
        } else {
            card("greybox", "了解当前项目", "读一读场景、资产与项目结构", "只读检查当前工作区的项目结构、场景和资产，概括已经具备的能力，并推荐下一步。请勿修改文件。".into(), "ask", "level")
        },
        if let Some(path) = plan {
            card("plan", "梳理下一步计划", "结合已有计划，确定下个里程碑", format!("先阅读当前工作区的计划 {path} 和项目现状，梳理下一个可验证的里程碑，并更新实施计划；暂不修改场景与源码。"), planning_mode, "plan")
        } else if is_defense {
            card("plan", "规划一局塔防 Demo", "安排阳光、种植与出怪节奏", "结合当前工作区已有的玩法、场景与资产，规划一局可玩塔防 Demo：阳光资源、植物种植、敌人波次和胜负条件。列出制作里程碑与验收标准；先规划，暂不修改场景与源码。".into(), planning_mode, "plan")
        } else {
            card("plan", "从想法到可玩 Demo", "拆解玩法、素材与制作里程碑", format!("结合当前工作区的项目结构和已有资产，给我一份从零做出可玩 {dimension} Demo 的分步计划。列出核心玩法、素材需求和每一步的验收标准；先规划，暂不修改场景与源码。"), planning_mode, "plan")
        },
        card("debug", if is_defense { "检查防线与画面" } else if is_2d { "检查精灵与画面" } else { "检查场景与画面" }, "从相机、资源到渲染逐项定位", format!("只读检查当前工作区的{}，从相机、可见性、资源引用和渲染配置定位潜在问题，给出证据和修复建议；不要在没有诊断结果时改动项目。", if is_defense { "植物、敌人与草坪显示" } else if is_2d { "精灵显示" } else { "场景显示" }),
            if can_edit { "debug" } else { "ask" }, "debug"),
    ];
    cards.retain(|c| {
        let mode = c["mode"].as_str().unwrap_or("");
        profile.allowed_modes().contains(&mode)
            && (engine != "codex" || crate::codex::turn::mode_supported(mode))
    });
    cards
}

pub async fn get_recommendations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RecommendationQuery>,
) -> Response {
    let session = match query.session_id.as_deref() {
        Some(id) => match state.sessions.get(id) {
            Some(session) => Some(session),
            None => return error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND", "会话不存在"),
        },
        None => None,
    };
    // A bound session's scope and engine always win over draft UI selections, just as in a turn.
    let workspace_id = session
        .as_ref()
        .map(|s| s.workspace_id.as_deref())
        .unwrap_or(query.workspace_id.as_deref());
    if workspace_id.is_some_and(|id| state.workspaces.get(id).is_none()) {
        return error(StatusCode::NOT_FOUND, "WORKSPACE_NOT_FOUND", "工作区不存在");
    }
    let engine = session
        .as_ref()
        .map(|s| s.agent_engine.clone())
        .or(query.agent_engine)
        .unwrap_or_else(|| crate::codex::config::load().default_engine);
    if !crate::codex::config::is_known_engine(&engine) {
        return error(StatusCode::BAD_REQUEST, "INVALID_ENGINE", "未知 Agent 引擎");
    }
    let permission = match &session {
        Some(s) => state.permissions.mode(&s.id),
        None => match crate::agent_settings::load(&state) {
            Ok(config) => config.default_permission_mode,
            Err(message) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "CONFIG_READ_FAILED",
                    &message,
                )
            }
        },
    };
    let scope = crate::scope::project_of(&state, workspace_id);
    let project_name = assetd::project::ForgeProject::load(&scope.project_root)
        .map(|project| project.name)
        .unwrap_or_else(|_| scope.name.clone());
    let profile = AgentProfile::from_kind_str(
        session
            .as_ref()
            .map(|s| s.agent_kind.as_str())
            .unwrap_or("coding"),
    );
    let cards = suggestions(
        scope.game_mode == assetd::project::GameMode::TwoD,
        &project_name,
        &profile,
        &engine,
        permission == "plan",
        session.as_ref().and_then(|s| s.active_plan_path.as_deref()),
    );
    Json(json!({
        "source":"agent-capabilities",
        "context":{"workspaceId":workspace_id,"projectName":project_name,"gameMode":scope.game_mode.as_str(),
            "agentEngine":engine,"agentKind":profile.kind.as_str(),"permissionMode":permission},
        "recommendations":cards
    })).into_response()
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":message}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::get, Router};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[test]
    fn modes_match_execution_and_readonly_profiles_never_suggest_scene_edits() {
        for kind in ["coding", "general", "document", "studio"] {
            let profile = AgentProfile::from_kind_str(kind);
            for engine in ["local", "codex"] {
                for readonly in [true, false] {
                    let cards = suggestions(true, "测试项目", &profile, engine, readonly, None);
                    assert_eq!(cards.len(), 3);
                    for card in &cards {
                        let mode = card["mode"].as_str().unwrap();
                        assert!(profile.allowed_modes().contains(&mode));
                        assert!(engine != "codex" || crate::codex::turn::mode_supported(mode));
                    }
                    if readonly || !profile.wants_edit_tools() {
                        assert_eq!(cards[0]["mode"], "ask");
                        assert_eq!(cards[2]["mode"], "ask");
                    }
                }
            }
        }
    }

    #[test]
    fn defense_projects_recommend_their_actual_gameplay_and_existing_plans_win() {
        let profile = AgentProfile::from_kind_str("coding");
        let cards = suggestions(true, "植物大战僵尸", &profile, "local", false, None);
        assert_eq!(cards[0]["image"], "defense");
        assert!(cards[0]["draft"].as_str().unwrap().contains("草坪"));
        assert!(cards[1]["draft"].as_str().unwrap().contains("敌人波次"));
        let cards = suggestions(
            true,
            "植物大战僵尸",
            &profile,
            "local",
            false,
            Some(".forge/plans/current.plan.md"),
        );
        assert!(cards[1]["draft"]
            .as_str()
            .unwrap()
            .contains(".forge/plans/current.plan.md"));
        assert_eq!(
            suggestions(false, "植物大战僵尸", &profile, "local", false, None)[0]["image"],
            "level"
        );
    }

    #[tokio::test]
    async fn project_manifest_and_bound_session_are_authoritative() {
        let (state, dir) = crate::test_app_state("recommendations");
        let root = dir.join("project-2d");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("forge.toml"),
            "[project]\nname = \"真实 2D 项目\"\nmode = \"2d\"\n",
        )
        .unwrap();
        let workspace = state
            .workspaces
            .create("测试工作区", &root.to_string_lossy())
            .unwrap();
        let mut session =
            state
                .sessions
                .create("test", "coding", None, false, Some(workspace.id.clone()));
        session.agent_engine = "local".into();
        state.sessions.save(&session);
        state.permissions.set_mode(&session.id, "plan").unwrap();
        let app = Router::new()
            .route("/recommendations", get(get_recommendations))
            .with_state(state.clone());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/recommendations?sessionId={}&workspaceId=ignored&agentEngine=codex",
                        session.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let face: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(face["context"]["gameMode"], "2d");
        assert_eq!(face["context"]["projectName"], "真实 2D 项目");
        assert_eq!(face["context"]["agentEngine"], "local");
        assert_eq!(face["context"]["permissionMode"], "plan");
        assert_eq!(face["recommendations"][0]["mode"], "ask");
        assert!(face["recommendations"][1]["draft"]
            .as_str()
            .unwrap()
            .contains("2D"));
        for uri in [
            "/recommendations?workspaceId=missing",
            "/recommendations?sessionId=missing",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
