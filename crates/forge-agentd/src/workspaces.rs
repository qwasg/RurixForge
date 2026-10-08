//! 工作区持久化与 REST:名称 + 磁盘根目录绑定;会话/文件夹归属 workspaceId。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};

use crate::events::{new_id, now_rfc3339};
use crate::sessions::write_atomic;
use crate::AppState;

/// 工作区:侧栏顶层分组 + 文件树/原生工具沙箱根。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug)]
pub enum WorkspaceError {
    NotFound,
    InvalidName,
    InvalidRoot,
    Io(String),
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchWorkspaceRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    root: Option<String>,
}

/// 工作区存贮(Vec 保创建序)。
pub struct WorkspaceStore {
    path: PathBuf,
    inner: Mutex<Vec<Workspace>>,
}

impl WorkspaceStore {
    pub fn load(path: PathBuf) -> Self {
        let workspaces = read_workspaces_file(&path);
        WorkspaceStore {
            path,
            inner: Mutex::new(workspaces),
        }
    }

    fn persist_locked(&self, inner: &[Workspace]) {
        let doc = json!({ "workspaces": inner });
        let text = serde_json::to_string_pretty(&doc).expect("workspaces 序列化失败");
        if let Err(e) = write_atomic(&self.path, &text) {
            eprintln!("workspaces.json 写盘失败({}): {e}", self.path.display());
        }
    }

    pub fn list(&self) -> Vec<Workspace> {
        self.inner.lock().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Option<Workspace> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .find(|w| w.id == id)
            .cloned()
    }

    pub fn root_of(&self, id: &str) -> Option<PathBuf> {
        self.get(id).map(|w| PathBuf::from(w.root))
    }

    /// 留空路径的新项目存放在数据根/workspaces 下；每次分配独立目录。
    /// 不使用项目名称拼接路径，名称可含斜杠且重复名称不会覆盖既有项目。
    pub fn allocate_project_root(&self) -> std::io::Result<PathBuf> {
        let data_root = self
            .path
            .parent()
            .and_then(FsPath::parent)
            .unwrap_or(FsPath::new("."));
        let data_root = if data_root.is_absolute() {
            data_root.to_path_buf()
        } else {
            std::env::current_dir()?.join(data_root)
        };
        Ok(data_root.join("workspaces").join(new_id("project")))
    }

    /// 新建入口显式允许建目录；普通登记和 PATCH 仍只接受已存在目录。
    pub fn create_with_root(&self, name: &str, root: &str) -> Result<Workspace, WorkspaceError> {
        if name.trim().is_empty() {
            return Err(WorkspaceError::InvalidName);
        }
        let root = if root.trim().is_empty() {
            self.allocate_project_root()
                .map_err(|e| WorkspaceError::Io(e.to_string()))?
        } else {
            PathBuf::from(root.trim())
        };
        if !root.is_absolute() {
            return Err(WorkspaceError::InvalidRoot);
        }
        std::fs::create_dir_all(&root).map_err(|e| WorkspaceError::Io(e.to_string()))?;
        self.create(name, &root.to_string_lossy())
    }

    fn validate_root(root: &str) -> Result<String, WorkspaceError> {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            return Err(WorkspaceError::InvalidRoot);
        }
        let p = FsPath::new(trimmed);
        let canon = p.canonicalize().map_err(|_| WorkspaceError::InvalidRoot)?;
        if !canon.is_dir() {
            return Err(WorkspaceError::InvalidRoot);
        }
        Ok(canon.to_string_lossy().into_owned())
    }

    pub fn create(&self, name: &str, root: &str) -> Result<Workspace, WorkspaceError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(WorkspaceError::InvalidName);
        }
        let root = Self::validate_root(root)?;
        let ts = now_rfc3339();
        let ws = Workspace {
            id: new_id("ws"),
            name: name.to_string(),
            root,
            created_at: ts.clone(),
            updated_at: ts,
        };
        let mut inner = self.inner.lock().unwrap();
        inner.push(ws.clone());
        self.persist_locked(&inner);
        Ok(ws)
    }

    pub fn patch(
        &self,
        id: &str,
        req: &PatchWorkspaceRequest,
    ) -> Result<Workspace, WorkspaceError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(ws) = inner.iter_mut().find(|w| w.id == id) else {
            return Err(WorkspaceError::NotFound);
        };
        if let Some(name) = &req.name {
            let name = name.trim();
            if name.is_empty() {
                return Err(WorkspaceError::InvalidName);
            }
            ws.name = name.to_string();
        }
        if let Some(root) = &req.root {
            ws.root = Self::validate_root(root)?;
        }
        ws.updated_at = now_rfc3339();
        let out = ws.clone();
        self.persist_locked(&inner);
        Ok(out)
    }

    pub fn delete(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.len();
        inner.retain(|w| w.id != id);
        let existed = inner.len() != before;
        if existed {
            self.persist_locked(&inner);
        }
        existed
    }
}

fn read_workspaces_file(path: &FsPath) -> Vec<Workspace> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("workspaces.json 解析失败({}): {e},按空处理", path.display());
            return Vec::new();
        }
    };
    v.get("workspaces")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            serde_json::from_value::<Workspace>(item)
                .map_err(|e| eprintln!("workspaces.json 条目解析失败: {e}"))
                .ok()
        })
        .collect()
}

/// 解析会话绑定的工作区根;无绑定或不存在时退回进程默认根。
pub fn resolve_workspace_root(state: &AppState, workspace_id: Option<&str>) -> PathBuf {
    if let Some(id) = workspace_id.filter(|s| !s.is_empty()) {
        if let Some(root) = state.workspaces.root_of(id) {
            return root;
        }
    }
    crate::workspace::workspace_root_path()
}

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

fn ws_error(err: WorkspaceError) -> Response {
    match err {
        WorkspaceError::NotFound => not_found("WORKSPACE_NOT_FOUND", "工作区不存在".into()),
        WorkspaceError::InvalidName => bad_request("INVALID_NAME", "workspace name 不可为空"),
        WorkspaceError::InvalidRoot => bad_request("INVALID_ROOT", "root 须为已存在目录"),
        WorkspaceError::Io(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "code": "IO_ERR", "message": format!("创建工作区目录失败:{message}") } })),
        ).into_response(),
    }
}

/// GET /api/forge/workspaces → {workspaces}。
pub async fn list_workspaces(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "workspaces": state.workspaces.list() }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkspaceRequest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    root: String,
    /// 新建表单可创建目录；root 留空时由服务分配默认目录。
    #[serde(default)]
    create_root: bool,
}

/// POST /api/forge/workspaces {name, root?, createRoot?} → {workspace}。
pub async fn create_workspace(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Response {
    let result = if req.create_root {
        state.workspaces.create_with_root(&req.name, &req.root)
    } else {
        state.workspaces.create(&req.name, &req.root)
    };
    match result {
        Ok(w) => Json(json!({ "workspace": w })).into_response(),
        Err(e) => ws_error(e),
    }
}

/// PATCH /api/forge/workspaces/{id} {name?, root?} → {workspace}。
pub async fn patch_workspace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchWorkspaceRequest>,
) -> Response {
    let Some(existing) = state.workspaces.get(&id) else {
        return ws_error(WorkspaceError::NotFound);
    };
    let root_changes = match req.root.as_deref() {
        Some(root) => match WorkspaceStore::validate_root(root) {
            Ok(root) => root != existing.root,
            Err(error) => return ws_error(error),
        },
        None => false,
    };
    let result = if root_changes {
        match state.collaboration.with_inactive_teams(
            || state.sessions.workspace_session_ids(&id),
            || state.workspaces.patch(&id, &req),
        ) {
            Ok(result) => result,
            Err(_) => return team_workspace_locked(),
        }
    } else {
        state.workspaces.patch(&id, &req)
    };
    match result {
        Ok(w) => Json(json!({ "workspace": w })).into_response(),
        Err(e) => ws_error(e),
    }
}

/// DELETE /api/forge/workspaces/{id} → {ok:true, clearedSessions, clearedFolders}。
pub async fn delete_workspace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let result = state.collaboration.with_inactive_teams(
        || state.sessions.workspace_session_ids(&id),
        || {
            if !state.workspaces.delete(&id) {
                return None;
            }
            Some((
                state.sessions.clear_workspace(&id),
                state.folders.clear_workspace(&id),
            ))
        },
    );
    let (cleared_sessions, cleared_folders) = match result {
        Err(_) => return team_workspace_locked(),
        Ok(Some(counts)) => counts,
        Ok(None) => {
            return not_found("WORKSPACE_NOT_FOUND", format!("工作区不存在: {id}"));
        }
    };
    Json(json!({
        "ok": true,
        "clearedSessions": cleared_sessions,
        "clearedFolders": cleared_folders,
    }))
    .into_response()
}

fn team_workspace_locked() -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({"error": {
            "code": "TEAM_WORKSPACE_LOCKED",
            "message": "此工作区仍有未结束团队，请先停止或完成团队，再修改根目录或删除工作区"
        }})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{ChatFolderStore, PatchSessionRequest, SessionStore};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-workspaces-{tag}-{}-{}",
            std::process::id(),
            new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn create_root_option_creates_directory_workspaces_and_defaults() {
        let (state, dir) = crate::test_app_state("workspace-create-root");
        let explicit = dir.join("new").join("nested");
        let rejected = create_workspace(
            State(state.clone()),
            Json(
                serde_json::from_value(json!({
                    "name": "existing only", "root": explicit,
                }))
                .unwrap(),
            ),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        assert!(
            !explicit.exists(),
            "plain registration still requires an existing directory"
        );

        for requested_root in [explicit.to_string_lossy().into_owned(), String::new()] {
            let response = create_workspace(
                State(state.clone()),
                Json(
                    serde_json::from_value(json!({
                        "name": "目录", "root": requested_root, "createRoot": true,
                    }))
                    .unwrap(),
                ),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let doc: Value = serde_json::from_slice(&body).unwrap();
            let root = PathBuf::from(doc["workspace"]["root"].as_str().unwrap());
            assert!(root.is_dir());
            assert!(!root.join("forge.toml").exists());
            assert!(!root.join("Content").exists());
            if requested_root.is_empty() {
                assert!(root.starts_with(dir.join("workspaces").canonicalize().unwrap()));
            } else {
                assert_eq!(root, explicit.canonicalize().unwrap());
            }
        }
        let reloaded = WorkspaceStore::load(dir.join("agent-sessions/workspaces.json"));
        assert_eq!(reloaded.list().len(), 2);

        let relative = new_id("relative-workspace");
        for (name, root) in [
            (
                "",
                explicit.join("invalid-name").to_string_lossy().into_owned(),
            ),
            ("relative", relative.clone()),
        ] {
            let response = create_workspace(
                State(state.clone()),
                Json(
                    serde_json::from_value(json!({
                        "name": name, "root": root, "createRoot": true,
                    }))
                    .unwrap(),
                ),
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        assert!(!explicit.join("invalid-name").exists());
        assert!(!FsPath::new(&relative).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn workspaces_crud_and_cascade_clear() {
        let dir = temp_dir("crud");
        let ws_store = WorkspaceStore::load(dir.join("workspaces.json"));
        let s = SessionStore::load(dir.join("sessions.json"));
        let f = ChatFolderStore::load(dir.join("chat-folders.json"));

        assert!(matches!(
            ws_store.create("", dir.to_str().unwrap()),
            Err(WorkspaceError::InvalidName)
        ));
        assert!(matches!(
            ws_store.create("测试", "/no_such_dir_xyz"),
            Err(WorkspaceError::InvalidRoot)
        ));

        let ws = ws_store.create("本地项目", dir.to_str().unwrap()).unwrap();
        assert!(ws.id.starts_with("ws_"));
        assert_eq!(ws.name, "本地项目");

        let a = s.create("会话A", "coding", None, true, Some(ws.id.clone()));
        let fld = f.create("文件夹1", Some(ws.id.clone())).unwrap();
        s.patch(
            &a.id,
            &PatchSessionRequest {
                folder_id: Some(Some(fld.id.clone())),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(ws_store.delete(&ws.id));
        assert_eq!(s.clear_workspace(&ws.id), 1);
        assert_eq!(f.clear_workspace(&ws.id), 1);
        assert!(s.get(&a.id).unwrap().workspace_id.is_none());
        assert!(f.list().is_empty());

        let ws2 = WorkspaceStore::load(dir.join("workspaces.json"));
        assert!(ws2.list().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn open_team_locks_workspace_root_and_delete_but_allows_rename() {
        let (state, dir) = crate::test_app_state("team-workspace-lock");
        let original = dir.join("original");
        let replacement = dir.join("replacement");
        std::fs::create_dir_all(&original).unwrap();
        std::fs::create_dir_all(&replacement).unwrap();
        let workspace = state
            .workspaces
            .create("original", original.to_str().unwrap())
            .unwrap();
        let session =
            state
                .sessions
                .create("chat", "coding", None, false, Some(workspace.id.clone()));
        let root = crate::collaboration::root_id(&session.id);
        state
            .collaboration
            .register(crate::collaboration::AgentRegistration {
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
                &crate::collaboration::CreateTeamRequest {
                    name: "team".into(),
                    max_parallel: 4,
                    max_fix_rounds: 3,
                },
            )
            .unwrap();
        for status in ["active", "paused", "blocked", "recoveryRequired"] {
            state
                .collaboration
                .set_team_status(&team.id, status)
                .unwrap();
            let response = patch_workspace(
                State(state.clone()),
                Path(workspace.id.clone()),
                Json(PatchWorkspaceRequest {
                    name: Some("must not partially rename".into()),
                    root: Some(replacement.to_string_lossy().into_owned()),
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::CONFLICT, "{status}");
            assert_eq!(
                state.workspaces.get(&workspace.id).unwrap().name,
                "original"
            );
            assert_eq!(
                state.workspaces.get(&workspace.id).unwrap().root,
                workspace.root
            );
            assert_eq!(
                delete_workspace(State(state.clone()), Path(workspace.id.clone()))
                    .await
                    .status(),
                StatusCode::CONFLICT
            );
            assert_eq!(
                state
                    .sessions
                    .get(&session.id)
                    .unwrap()
                    .workspace_id
                    .as_deref(),
                Some(workspace.id.as_str())
            );
        }
        assert_eq!(
            patch_workspace(
                State(state.clone()),
                Path(workspace.id.clone()),
                Json(PatchWorkspaceRequest {
                    name: Some("renamed".into()),
                    root: Some(original.to_string_lossy().into_owned()),
                })
            )
            .await
            .status(),
            StatusCode::OK,
            "renaming and restating the same root stay allowed"
        );
        assert_eq!(state.workspaces.get(&workspace.id).unwrap().name, "renamed");
        state.collaboration.control_team(&team.id, "stop").unwrap();
        assert_eq!(
            patch_workspace(
                State(state.clone()),
                Path(workspace.id.clone()),
                Json(PatchWorkspaceRequest {
                    root: Some(replacement.to_string_lossy().into_owned()),
                    ..Default::default()
                })
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            delete_workspace(State(state.clone()), Path(workspace.id))
                .await
                .status(),
            StatusCode::OK
        );
        assert!(state
            .sessions
            .get(&session.id)
            .unwrap()
            .workspace_id
            .is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
