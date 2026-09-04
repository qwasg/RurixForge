//! 资源作用域:一次 turn 能看见/能改动的项目集合。
//!
//! 此前 MCP 一律锚死 `<workspace>/projects/demo`(mcp.rs asset_project_root),
//! 会话选的 workspace 只影响原生文件工具——切了工作区,asset_list/context_search
//! 仍在读上一个项目。ScopeContext 把「当前项目 + 显式勾选的只读项目 + 全局库」
//! 收成一个服务端解析的结构:请求侧只能递 workspaceId,根路径一律由本模块
//! 按注册表解析并 canonicalize,模型与客户端都递不进任意磁盘路径。

use std::path::{Path, PathBuf};

use crate::workspaces::resolve_workspace_root;
use crate::AppState;

/// 作用域内的单个项目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeProject {
    /// 工作区 id;None = 进程默认根(未绑定工作区的会话)。
    pub workspace_id: Option<String>,
    /// 展示名(工作区名;默认根用 "默认工作区")。
    pub name: String,
    /// 工作区根(原生文件工具沙箱根)。
    pub workspace_root: PathBuf,
    /// 资产项目根(含 Content/ 的那一层;见 project_root_of)。
    pub project_root: PathBuf,
    /// 游戏维度模式(forge.toml [project] mode;无清单/解析失败 → ThreeD,F-GAME-3)。
    pub game_mode: assetd::project::GameMode,
}

impl ScopeProject {
    /// 模型侧稳定标识:工作区 id 或 "default"。
    pub fn id(&self) -> &str {
        self.workspace_id.as_deref().unwrap_or("default")
    }
}

/// 一次 turn 的资源作用域。
#[derive(Debug, Clone)]
pub struct ScopeContext {
    /// 当前项目:唯一可写目标。
    pub current: ScopeProject,
    /// 用户显式勾选的其他项目:只读检索,永不作为写目标。
    pub readonly: Vec<ScopeProject>,
    /// 全局素材库 / 商店是否纳入检索。
    pub include_library: bool,
}

impl ScopeContext {
    /// 只含当前项目(普通聊天会话的缺省作用域)。
    pub fn single(current: ScopeProject) -> Self {
        ScopeContext {
            current,
            readonly: Vec::new(),
            include_library: true,
        }
    }

    /// 当前 + 只读项目(检索遍历序;当前项目恒在首位)。
    pub fn searchable(&self) -> Vec<&ScopeProject> {
        std::iter::once(&self.current).chain(self.readonly.iter()).collect()
    }

    /// 按模型给的 projectId 定位项目(未知 id → None)。
    pub fn find(&self, project_id: &str) -> Option<&ScopeProject> {
        self.searchable().into_iter().find(|p| p.id() == project_id)
    }
}

/// 资产项目根:工作区根自身像项目(有 forge.toml 或 Content/)就用它,
/// 否则退回仓内 `projects/demo`。
///
/// 退回分支保住既有单项目装机的行为:默认工作区根是仓根,仓根没有 Content/,
/// 此前所有 MCP 都锚在 projects/demo,直接改成仓根会让 asset_list 一次性丢失全部资产。
pub fn project_root_of(workspace_root: &Path) -> PathBuf {
    if workspace_root.join("forge.toml").is_file() || workspace_root.join("Content").is_dir() {
        return canonical(workspace_root);
    }
    let demo = workspace_root.join("projects").join("demo");
    if demo.is_dir() {
        return canonical(&demo);
    }
    crate::mcp::default_project_root()
}

/// canonicalize 失败(路径不存在/权限)时退回原路径:解析不该在这一层报错,
/// 真正的越界判定在各工具的 confine 面。
fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// 项目游戏模式:读 project_root/forge.toml [project] mode;缺失/解析失败 → ThreeD。
pub fn game_mode_of(project_root: &Path) -> assetd::project::GameMode {
    assetd::project::ForgeProject::load(project_root)
        .map(|p| p.mode)
        .unwrap_or(assetd::project::GameMode::ThreeD)
}

/// 解析单个工作区 id → ScopeProject。
pub fn project_of(state: &AppState, workspace_id: Option<&str>) -> ScopeProject {
    let id = workspace_id.map(str::trim).filter(|s| !s.is_empty());
    let workspace_root = canonical(&resolve_workspace_root(state, id));
    let name = id
        .and_then(|i| state.workspaces.get(i))
        .map(|w| w.name)
        .unwrap_or_else(|| "默认工作区".to_string());
    let project_root = project_root_of(&workspace_root);
    let game_mode = game_mode_of(&project_root);
    ScopeProject {
        workspace_id: id.map(str::to_string),
        name,
        project_root,
        workspace_root,
        game_mode,
    }
}

/// 解析一次 turn 的作用域。
///
/// `readonly_ids` 来自请求(用户在界面上勾的其他项目):未注册的 id 直接丢弃,
/// 与当前项目重复的也丢弃——只读集合永远不会悄悄把当前项目变成两份。
pub fn resolve(
    state: &AppState,
    current_workspace_id: Option<&str>,
    readonly_ids: &[String],
    include_library: bool,
) -> ScopeContext {
    let current = project_of(state, current_workspace_id);
    let mut readonly: Vec<ScopeProject> = Vec::new();
    for id in readonly_ids {
        let id = id.trim();
        if id.is_empty() || Some(id) == current.workspace_id.as_deref() {
            continue;
        }
        // 未注册工作区不解析:否则 resolve_workspace_root 会静默退回默认根,
        // 用户以为勾了 B 项目,实际检索的是仓根。
        if state.workspaces.get(id).is_none() {
            continue;
        }
        let p = project_of(state, Some(id));
        if readonly.iter().any(|x| x.project_root == p.project_root)
            || p.project_root == current.project_root
        {
            continue;
        }
        readonly.push(p);
    }
    ScopeContext {
        current,
        readonly,
        include_library,
    }
}

/// 作用域摘要(事件留痕/工具反馈用;只给 id 与名字,不给磁盘路径)。
/// F-GAME-3:附带当前项目游戏模式(2d/3d),客户端徽标与提示词注入共用此事实源。
pub fn summary_json(scope: &ScopeContext) -> serde_json::Value {
    serde_json::json!({
        "currentProjectId": scope.current.id(),
        "currentProjectName": scope.current.name,
        "currentProjectMode": scope.current.game_mode.as_str(),
        "readonlyProjectIds": scope.readonly.iter().map(|p| p.id()).collect::<Vec<_>>(),
        "includeLibrary": scope.include_library,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_root_prefers_workspace_when_it_looks_like_a_project() {
        let dir = std::env::temp_dir().join(format!(
            "forge-scope-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(dir.join("Content")).unwrap();
        assert_eq!(project_root_of(&dir), dir.canonicalize().unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_root_falls_back_to_projects_demo() {
        let dir = std::env::temp_dir().join(format!(
            "forge-scope-demo-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(dir.join("projects").join("demo")).unwrap();
        assert_eq!(
            project_root_of(&dir),
            dir.join("projects").join("demo").canonicalize().unwrap()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// F-TEAM-3 守门:default_project_root 必须与 scope canonical 同形态——
    /// 连接池按路径字符串分池,形态漂移会为同一目录开两套 MCP + 两个 engine-host。
    #[test]
    fn default_root_is_canonical_same_as_scope_form() {
        let d = crate::mcp::default_project_root();
        assert_eq!(d, canonical(&d), "default_project_root 须为 canonical 形态");
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::*;
    use crate::events::EventBus;
    use crate::sessions::{ChatFolderStore, SessionStore};
    use crate::workspaces::WorkspaceStore;
    use crate::AppState;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn resolve_drops_unregistered_and_current_dup() {
        let dir = std::env::temp_dir().join(format!(
            "forge-scope-iso-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(dir.join("a").join("Content")).unwrap();
        std::fs::create_dir_all(dir.join("b").join("Content")).unwrap();
        std::fs::create_dir_all(dir.join("c").join("Content")).unwrap();
        let ws = WorkspaceStore::load(dir.join("workspaces.json"));
        let a = ws.create("A", dir.join("a").to_str().unwrap()).unwrap();
        let b = ws.create("B", dir.join("b").to_str().unwrap()).unwrap();
        let _c = ws.create("C", dir.join("c").to_str().unwrap()).unwrap();
        let state = AppState {
            started: Instant::now(),
            proposals: crate::proposals::ProposalStore::default(),
            swarm: crate::swarm::SwarmCoordinator::default(),
            events: Arc::new(EventBus::new(dir.join("ev"), 16)),
            sessions: Arc::new(SessionStore::load(dir.join("sessions.json"))),
            folders: Arc::new(ChatFolderStore::load(dir.join("folders.json"))),
            workspaces: Arc::new(ws),
            runs: Arc::new(crate::agent::RunRegistry::default()),
            todos: Arc::new(crate::agent::TodoStore::load(dir.join("todos.json"))),
            receipts: Arc::new(crate::receipts::ReceiptStore::load(dir.join("receipts.json"))),
            wakes: Arc::new(crate::agent::WakeRegistry::default()),
            permissions: Arc::new(crate::permission::PermissionService::load(dir.join("perm.json"))),
        };
        let scope = resolve(
            &state,
            Some(&a.id),
            &[b.id.clone(), "ws_ghost".into(), a.id.clone()],
            true,
        );
        assert_eq!(scope.current.workspace_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(scope.readonly.len(), 1);
        assert_eq!(scope.readonly[0].workspace_id.as_deref(), Some(b.id.as_str()));
        assert!(scope.find(&a.id).is_some());
        assert!(scope.find(&b.id).is_some());
        assert!(scope.find("ws_ghost").is_none());
        assert!(scope.find(&_c.id).is_none(), "未勾选的 C 不得进入检索作用域");
        std::fs::remove_dir_all(&dir).ok();
    }
}
