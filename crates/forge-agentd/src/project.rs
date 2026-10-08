//! 项目脚手架(F-GAME-3 游戏选型):POST /api/forge/project/init —— 制作前选定
//! 2D/3D 模式,写 forge.toml([project] mode)+ Content 目录骨架 + 按模式的起始场景:
//! 2D = 正交相机([0,0,10] 朝 -Z,orthoSize 5,XY 平面约定);3D = 透视相机 + 平行光 + 地面。

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use assetd::project::{ForgeProject, GameMode, RenderConfig};
use forge_scene::{Component, Entity, Scene, Transform};

use crate::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInitRequest {
    /// 项目根(留空则分配默认目录;不存在则创建;已含 forge.toml 则拒绝覆盖)。
    #[serde(default)]
    root: String,
    /// 项目名(缺省 = 目录名)。
    #[serde(default)]
    name: String,
    /// 游戏维度模式:"2d" | "3d"(必填——选型即本端点的存在意义)。
    mode: String,
    /// 实现后端：2D 缺省 Godot；3D 缺省 rurix，也可选择 Godot。
    #[serde(default)]
    backend: Option<String>,
}

fn err_response(status: StatusCode, code: &str, message: String) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

/// 起始场景(按模式):返回 (场景, 说明)。
fn starter_scene(name: &str, mode: GameMode) -> Scene {
    match mode {
        GameMode::TwoD => {
            let mut s = Scene::with_mode(name, "2d");
            let id = s.alloc_id();
            s.entities.push(Entity {
                entity_guid: None,
                id,
                name: "MainCamera".into(),
                // 2D 约定:XY 平面,相机在 +Z 朝 -Z(恒等旋转),正交。
                transform: Transform {
                    translation: [0.0, 0.0, 10.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                components: vec![
                    Component::new(
                        "Camera",
                        json!({
                            "projection": "orthographic",
                            "orthoSize": 5.0,
                            "fov": 60.0,
                            "near": 0.1,
                            "far": 100.0
                        }),
                    ),
                    Component::new("Category", json!({ "category": "map" })),
                ],
            });
            s
        }
        GameMode::ThreeD => {
            let mut s = Scene::new(name);
            let cam_id = s.alloc_id();
            s.entities.push(Entity {
                entity_guid: None,
                id: cam_id,
                name: "MainCamera".into(),
                transform: Transform {
                    translation: [0.0, 1.5, 6.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                components: vec![
                    Component::new(
                        "Camera",
                        json!({
                            "projection": "perspective",
                            "orthoSize": 5.0,
                            "fov": 60.0,
                            "near": 0.1,
                            "far": 500.0
                        }),
                    ),
                    Component::new("Category", json!({ "category": "map" })),
                ],
            });
            let light_id = s.alloc_id();
            s.entities.push(Entity {
                entity_guid: None,
                id: light_id,
                name: "Sun".into(),
                transform: Transform {
                    translation: [3.0, 6.0, 2.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                components: vec![Component::new(
                    "Light",
                    json!({
                        "kind": "directional",
                        "color": [1.0, 0.96, 0.88],
                        "intensity": 2.0,
                        "castShadow": false
                    }),
                )],
            });
            let ground_id = s.alloc_id();
            s.entities.push(Entity {
                entity_guid: None,
                id: ground_id,
                name: "Ground".into(),
                transform: Transform {
                    translation: [0.0, -0.5, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [20.0, 1.0, 20.0],
                },
                components: vec![
                    Component::new("MeshRenderer", json!({ "mesh": "cube", "material": "" })),
                    Component::new("RigidBody", json!({ "kind": "static", "mass": 1.0 })),
                    Component::new("Category", json!({ "category": "map" })),
                ],
            });
            s
        }
    }
}

/// 项目初始化核心(纯函数式,便于测试):建目录树 + forge.toml + 起始场景。
/// 已含 forge.toml → Err(PROJECT_ALREADY_INITIALIZED);mode 非法 → Err(INVALID_MODE)。
pub fn init_project(root: &Path, name: &str, mode: &str) -> Result<Value, (String, String)> {
    init_project_with_backend(root, name, mode, None)
}

pub fn init_project_with_backend(
    root: &Path,
    name: &str,
    mode: &str,
    backend: Option<&str>,
) -> Result<Value, (String, String)> {
    let mode = GameMode::parse(mode).ok_or_else(|| {
        (
            "INVALID_MODE".to_string(),
            format!("mode 须为 \"2d\"|\"3d\",实际 {mode:?}"),
        )
    })?;
    if root.join("forge.toml").is_file() {
        return Err((
            "PROJECT_ALREADY_INITIALIZED".to_string(),
            format!("{} 已含 forge.toml(如需改模式请编辑该文件)", root.display()),
        ));
    }
    let render = RenderConfig::for_mode(mode, backend)
        .map_err(|e| ("INVALID_BACKEND".to_string(), e.message))?;
    std::fs::create_dir_all(root)
        .map_err(|e| ("IO_ERR".to_string(), format!("创建项目根失败: {e}")))?;
    let root = root.canonicalize().map_err(|e| {
        (
            "IO_ERR".to_string(),
            format!("项目根 canonicalize 失败: {e}"),
        )
    })?;
    let name = if name.trim().is_empty() {
        root.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_string())
    } else {
        name.trim().to_string()
    };

    let mut proj = ForgeProject::with_defaults(root.clone());
    proj.name = name.clone();
    proj.mode = mode;
    proj.render = render;
    proj.save_manifest()
        .map_err(|e| ("IO_ERR".to_string(), format!("forge.toml 写入失败: {e}")))?;
    proj.ensure_dirs().map_err(|e| {
        (
            "IO_ERR".to_string(),
            format!("Content 目录骨架创建失败: {e}"),
        )
    })?;
    // Graphs 目录(demo 惯例;ensure_dirs 不含)。
    std::fs::create_dir_all(proj.content_root().join("Graphs"))
        .map_err(|e| ("IO_ERR".to_string(), format!("Graphs 目录创建失败: {e}")))?;

    let scene = starter_scene(&name, mode);
    let scene_rel = proj.entry_scene.clone();
    let scene_path = root.join(&scene_rel);
    if let Some(dir) = scene_path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| ("IO_ERR".to_string(), format!("场景目录创建失败: {e}")))?;
    }
    scene
        .save(&scene_path)
        .map_err(|e| ("IO_ERR".to_string(), format!("起始场景写入失败: {e}")))?;

    Ok(json!({
        "root": root.to_string_lossy(),
        "name": name,
        "mode": mode.as_str(),
        "backend": render.as_strs().0,
        "entryScene": scene_rel,
    }))
}

/// POST /api/forge/project/init {root?, name?, mode, backend?} → {project:{root,name,mode,backend,entryScene}}。
pub async fn project_init(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ProjectInitRequest>,
) -> Response {
    let root = if req.root.trim().is_empty() {
        match state.workspaces.allocate_project_root() {
            Ok(root) => root,
            Err(e) => {
                return err_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "IO_ERR",
                    format!("分配项目目录失败:{e}"),
                )
            }
        }
    } else {
        PathBuf::from(req.root.trim())
    };
    if !root.is_absolute() {
        return err_response(
            StatusCode::BAD_REQUEST,
            "INVALID_ROOT",
            "root 须为绝对目录路径".into(),
        );
    }
    match init_project_with_backend(&root, &req.name, &req.mode, req.backend.as_deref()) {
        Ok(project) => Json(json!({ "project": project })).into_response(),
        Err((code, message)) => {
            let status = match code.as_str() {
                "INVALID_MODE" | "INVALID_ROOT" | "INVALID_BACKEND" => StatusCode::BAD_REQUEST,
                "PROJECT_ALREADY_INITIALIZED" => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            err_response(status, &code, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-project-{tag}-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        dir
    }

    #[tokio::test]
    async fn init_without_root_allocates_distinct_2d_projects() {
        let (state, dir) = crate::test_app_state("project-default-root");
        let mut roots = Vec::new();
        for root in [None, Some("   ")] {
            let mut req = json!({ "name": "../新游戏", "mode": "2d" });
            if let Some(root) = root {
                req["root"] = json!(root);
            }
            let response = project_init(
                State(state.clone()),
                Json(serde_json::from_value(req).unwrap()),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let doc: Value = serde_json::from_slice(&body).unwrap();
            let root = PathBuf::from(doc["project"]["root"].as_str().unwrap());
            assert!(root.starts_with(dir.join("workspaces").canonicalize().unwrap()));
            let proj = ForgeProject::load(&root).unwrap();
            assert_eq!(proj.mode, GameMode::TwoD);
            assert_eq!(proj.render.as_strs().0, "godot");
            assert!(root.join("Content/Scenes/Main.rxscene").is_file());
            roots.push(root);
        }
        assert_ne!(
            roots[0], roots[1],
            "same project name must not overwrite a prior project"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn init_rejects_a_relative_root_before_writing() {
        let (state, dir) = crate::test_app_state("project-relative-root");
        let relative = crate::events::new_id("relative-project");
        let response = project_init(
            State(state),
            Json(
                serde_json::from_value(json!({
                    "root": relative, "name": "x", "mode": "2d"
                }))
                .unwrap(),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!Path::new(&relative).exists());
        assert!(
            !dir.exists(),
            "rejected input must not create a data directory"
        );
    }

    #[test]
    fn init_2d_project_scaffolds_manifest_and_ortho_scene() {
        let dir = temp_dir("2d");
        let r = init_project(&dir, "我的2D游戏", "2d").expect("2d 初始化须成功");
        assert_eq!(r["mode"], json!("2d"));
        assert_eq!(r["name"], json!("我的2D游戏"));
        // forge.toml 回读:mode=2d。
        let proj = ForgeProject::load(&dir).unwrap();
        assert_eq!(proj.mode, GameMode::TwoD);
        assert_eq!(r["backend"], json!("godot"));
        assert_eq!(proj.render.as_strs().0, "godot");
        // 起始场景:mode=2d + 正交相机实体。
        let scene = Scene::load(dir.join("Content/Scenes/Main.rxscene")).unwrap();
        assert!(scene.is_2d());
        let cam = scene
            .entities
            .iter()
            .find(|e| e.component("Camera").is_some())
            .expect("须有相机实体");
        let props = &cam.component("Camera").unwrap().props;
        assert_eq!(props["projection"], json!("orthographic"));
        assert_eq!(cam.transform.translation, [0.0, 0.0, 10.0]);
        // 组件 props 过注册表校验。
        for e in &scene.entities {
            for c in &e.components {
                forge_scene::validate_component(c).unwrap();
            }
        }
        // 重复初始化 → 拒绝覆盖。
        let (code, _) = init_project(&dir, "x", "3d").unwrap_err();
        assert_eq!(code, "PROJECT_ALREADY_INITIALIZED");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn init_3d_project_scaffolds_perspective_scene() {
        let dir = temp_dir("3d");
        let r = init_project(&dir, "", "3d").expect("3d 初始化须成功");
        // 缺省名 = 目录名。
        assert!(r["name"].as_str().unwrap().len() > 0);
        assert_eq!(r["backend"], json!("rurix"));
        let scene = Scene::load(dir.join("Content/Scenes/Main.rxscene")).unwrap();
        assert!(!scene.is_2d());
        assert_eq!(scene.entities.len(), 3, "3D 起始场景 = 相机+光+地面");
        let cam = scene
            .entities
            .iter()
            .find_map(|e| e.component("Camera"))
            .expect("须有相机");
        assert_eq!(cam.props["projection"], json!("perspective"));
        // 非法 mode 拒绝。
        let dir2 = temp_dir("bad");
        let (code, _) = init_project(&dir2, "x", "5d").unwrap_err();
        assert_eq!(code, "INVALID_MODE");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&dir2).ok();
    }

    #[test]
    fn init_explicit_backend_persists_and_invalid_backend_does_not_create_root() {
        for (mode, backend) in [("3d", "godot"), ("3d", "rurix"), ("2d", "rurix")] {
            let dir = temp_dir(&format!("{mode}-{backend}"));
            let r = init_project_with_backend(&dir, "choice", mode, Some(backend)).unwrap();
            assert_eq!(r["backend"], json!(backend));
            assert_eq!(
                ForgeProject::load(&dir).unwrap().render.as_strs().0,
                backend
            );
            std::fs::remove_dir_all(dir).ok();
        }
        let dir = temp_dir("invalid-backend");
        let (code, _) = init_project_with_backend(&dir, "bad", "3d", Some("unity")).unwrap_err();
        assert_eq!(code, "INVALID_BACKEND");
        assert!(!dir.exists(), "校验失败不得创建项目目录");
    }
}
