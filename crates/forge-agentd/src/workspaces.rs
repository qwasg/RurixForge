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
        self.inner.lock().unwrap().iter().find(|w| w.id == id).cloned()
    }

    pub fn root_of(&self, id: &str) -> Option<PathBuf> {
        self.get(id).map(|w| PathBuf::from(w.root))
    }

    fn validate_root(root: &str) -> Result<String, WorkspaceError> {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            return Err(WorkspaceError::InvalidRoot);
        }
        let p = FsPath::new(trimmed);
        let canon = p
            .canonicalize()
            .map_err(|_| WorkspaceError::InvalidRoot)?;
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

    pub fn patch(&self, id: &str, req: &PatchWorkspaceRequest) -> Result<Workspace, WorkspaceError> {
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
}

/// POST /api/forge/workspaces {name, root} → {workspace}。
pub async fn create_workspace(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Response {
    match state.workspaces.create(&req.name, &req.root) {
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
    match state.workspaces.patch(&id, &req) {
        Ok(w) => Json(json!({ "workspace": w })).into_response(),
        Err(e) => ws_error(e),
    }
}

/// DELETE /api/forge/workspaces/{id} → {ok:true, clearedSessions, clearedFolders}。
pub async fn delete_workspace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    if !state.workspaces.delete(&id) {
        return not_found("WORKSPACE_NOT_FOUND", format!("工作区不存在: {id}"));
    }
    let cleared_sessions = state.sessions.clear_workspace(&id);
    let cleared_folders = state.folders.clear_workspace(&id);
    Json(json!({
        "ok": true,
        "clearedSessions": cleared_sessions,
        "clearedFolders": cleared_folders,
    }))
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
}
