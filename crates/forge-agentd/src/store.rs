//! F11(D-025)资产商店 REST 面:源 CRUD / 聚合搜索 / 包详情 / 安装长任务 / 卸载 /
//! 已装清单 / 更新检查 / 个人资产库 / 发布。
//!
//! 与 `store-mcp` 的分工:MCP 是 agent 的工具面(P-2 / I-2),本模块是 IDE 的 REST 面,
//! 两者内嵌**同一个** `forge-store` 库,因此 UI 与 agent 的行为逐字一致(11 §2.5 落地机制)。
//!
//! 三条纪律钉在本层:
//! - **长任务不阻塞请求**:安装/卸载提交后立即返 `taskId`,真正的活在 `spawn_blocking`
//!   里跑(`ureq` 是阻塞式的,不能占 tokio 工作线程)。host 代理已为这三个端点开
//!   `isLongLivedPath` 豁免,但请求本身仍是秒回——豁免是给 publish 的同步腿留的。
//! - **destructive 经 Proposal**(I-6):卸载先查 `has_approved_covering("store.uninstall", …)`,
//!   未批准则自动建单并返 409 `GOV_PROPOSAL_REQUIRED` 携 `proposalId`,与 `mcp_call` 里
//!   `forced_asset_delete` 的两阶段形态一致(乐观调用 → 被拒建单 → 批准 → 原样重发)。
//! - **密钥红线**(R-5):私有源令牌只进 `gend::keystore` 的 `store:<sourceId>` 槽位,
//!   响应体一律只回 `hasToken` 布尔,错误消息不带令牌值。
//!
//! 任务表是**进程内**的(照 `proposals`/`runs` 同惯例),agentd 重启即空;
//! 已完成的安装事实落在项目的 `.forge/store/installed.json`,不依赖任务表存活。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use assetd::project::ForgeProject;
use forge_store::install::{
    check_updates, install_package, uninstall_package, InstallOptions, InstalledDb,
};
use forge_store::library::Library;
use forge_store::manifest::{PackageKind, PackageManifest};
use forge_store::publish::{draft_from_project, publish_package, PublishInput};
use forge_store::registry::{load_sources, save_sources, search_all, SourcesConfig};
use forge_store::source::{open_source, SearchQuery, SourceConfig};
use forge_store::StoreError;

use crate::AppState;

/// Proposal kind:卸载(destructive,I-6)。
const UNINSTALL_PROPOSAL_KIND: &str = "store.uninstall";

// ---------- 路径锚 ----------

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)")
        .to_path_buf()
}

/// 源配置与个人库的数据根。与 gend 同源(`FORGE_GEN_DATA_DIR` 可覆盖做测试隔离)。
fn data_dir() -> PathBuf {
    gend::config::data_dir()
}

fn skills_root() -> PathBuf {
    workspace_root().join("skills")
}

/// 源清单。
///
/// 不直接用 `load_sources`:它在无配置文件时靠 `data_dir.parent()` 推 workspace 根来锚
/// 内置官方源,而本进程的 data 根可被 `FORGE_GEN_DATA_DIR` 指到临时目录(测试隔离),
/// 那样官方源会指向一个不存在的 `<tmp>/registry`。这里在「确实没有配置文件」时用真实
/// 仓根锚定;一旦用户写过配置就完全以文件为准,不覆盖用户的选择。
fn sources() -> SourcesConfig {
    let dir = data_dir();
    if !forge_store::registry::sources_config_path(&dir).exists() {
        return SourcesConfig {
            sources: vec![forge_store::registry::default_official_source(
                &workspace_root(),
            )],
        };
    }
    load_sources(&dir)
}

fn project() -> ForgeProject {
    let root = crate::mcp::asset_project_root();
    ForgeProject::load(&root).unwrap_or_else(|e| {
        eprintln!("[store] forge.toml 加载失败: {e};用缺省配置");
        ForgeProject::with_defaults(root)
    })
}

/// 私有源令牌槽位:keystore 内 `store:<sourceId>`(与 LLM/生成渠道同一保险箱)。
fn token_key(source_id: &str) -> String {
    format!("store:{source_id}")
}

fn token_of(source_id: &str) -> Option<String> {
    gend::keystore::Keystore::load().key_for(&token_key(source_id))
}

/// 已配置源 id 清单(mcp.rs spawn 时按此逐个取 keystore 令牌注入子进程环境)。
pub(crate) fn configured_source_ids() -> Vec<String> {
    sources().sources.into_iter().map(|s| s.id).collect()
}

// ---------- 错误映射 ----------

/// `StoreError` → HTTP。语义化状态码:找不到 404、冲突 409、参数/清单 400、
/// 源不可达 502(上游问题不是本机 500)、付费 402。
fn store_status(code: &str) -> StatusCode {
    match code {
        "STORE_SOURCE_NOT_FOUND"
        | "STORE_PACKAGE_NOT_FOUND"
        | "STORE_VERSION_NOT_FOUND"
        | "STORE_NOT_INSTALLED"
        | "STORE_TASK_NOT_FOUND" => StatusCode::NOT_FOUND,
        "STORE_ALREADY_INSTALLED" => StatusCode::CONFLICT,
        "STORE_MANIFEST_INVALID" | "STORE_CHECKSUM_MISMATCH" | "STORE_DEPENDENCY_UNRESOLVED" => {
            StatusCode::BAD_REQUEST
        }
        "STORE_PAYMENT_REQUIRED" => StatusCode::PAYMENT_REQUIRED,
        "STORE_SOURCE_UNREACHABLE" | "STORE_PUBLISH_REJECTED" => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn store_error_response(e: &StoreError) -> Response {
    (
        store_status(e.code),
        Json(json!({ "error": { "code": e.code, "message": e.message } })),
    )
        .into_response()
}

fn bad_request(code: &str, msg: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": { "code": code, "message": msg.into() } })),
    )
        .into_response()
}

// ---------- 长任务表(进程内) ----------

#[derive(Debug, Clone)]
struct TaskState {
    id: String,
    kind: &'static str,
    status: &'static str,
    phase: String,
    done: u32,
    total: u32,
    error: Option<(String, String)>,
    result: Option<Value>,
}

impl TaskState {
    fn to_json(&self) -> Value {
        let mut v = json!({
            "taskId": self.id,
            "kind": self.kind,
            "status": self.status,
            "phase": self.phase,
            "done": self.done,
            "total": self.total,
        });
        if let Some((code, msg)) = &self.error {
            v["error"] = json!({ "code": code, "message": msg });
        }
        if let Some(r) = &self.result {
            v["result"] = r.clone();
        }
        v
    }
}

#[derive(Default)]
struct TaskTable {
    inner: Mutex<HashMap<String, TaskState>>,
    seq: AtomicU64,
}

impl TaskTable {
    fn new_task(&self, kind: &'static str) -> String {
        let n = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("stask_{n}");
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).insert(
            id.clone(),
            TaskState {
                id: id.clone(),
                kind,
                status: "running",
                phase: "queued".into(),
                done: 0,
                total: 0,
                error: None,
                result: None,
            },
        );
        id
    }

    fn progress(&self, id: &str, phase: &str, done: u32, total: u32) {
        if let Some(t) = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(id)
        {
            t.phase = phase.to_string();
            t.done = done;
            t.total = total;
        }
    }

    fn finish(&self, id: &str, outcome: Result<Value, StoreError>) {
        if let Some(t) = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(id)
        {
            match outcome {
                Ok(v) => {
                    t.status = "completed";
                    t.phase = "done".into();
                    t.result = Some(v);
                }
                Err(e) => {
                    t.status = "failed";
                    t.error = Some((e.code.to_string(), e.message));
                }
            }
        }
    }

    fn get(&self, id: &str) -> Option<TaskState> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }
}

fn tasks() -> &'static TaskTable {
    static T: OnceLock<TaskTable> = OnceLock::new();
    T.get_or_init(TaskTable::default)
}

// ---------- 源 CRUD ----------

fn source_view(s: &SourceConfig) -> Value {
    json!({
        "id": s.id,
        "name": s.name,
        "baseUrl": s.base_url,
        "enabled": s.enabled,
        // R-5:只报有无,不报值。
        "hasToken": token_of(&s.id).is_some(),
    })
}

/// GET /api/forge/store/sources
pub(crate) async fn sources_list() -> Json<Value> {
    let cfg = sources();
    let items: Vec<Value> = cfg.sources.iter().map(source_view).collect();
    Json(json!({ "sources": items }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceCreateRequest {
    id: String,
    name: String,
    base_url: String,
    #[serde(default)]
    token: Option<String>,
}

/// POST /api/forge/store/sources
pub(crate) async fn sources_create(Json(req): Json<SourceCreateRequest>) -> Response {
    let id = req.id.trim().to_string();
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return bad_request(
            "FORGE_INVALID_ARGS",
            "源 id 须为小写英文/数字/中划线/点(如 acme-studio)",
        );
    }
    if req.base_url.trim().is_empty() {
        return bad_request("FORGE_INVALID_ARGS", "baseUrl 不可空");
    }
    let dir = data_dir();
    let mut cfg = sources();
    if cfg.find(&id).is_some() {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": { "code": "FORGE_INVALID_ARGS", "message": format!("源已存在: {id}") } })),
        )
            .into_response();
    }
    // 令牌落 keystore,不进 store-sources.json(R-5)。
    if let Some(t) = req
        .token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if let Err(e) = gend::keystore::set_key(&token_key(&id), t) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
            )
                .into_response();
        }
    }
    let entry = SourceConfig {
        id: id.clone(),
        name: req.name.trim().to_string(),
        base_url: req.base_url.trim().to_string(),
        enabled: true,
        token: None,
    };
    cfg.upsert(entry);
    match save_sources(&dir, &cfg) {
        Ok(()) => {
            let saved = load_sources(&dir);
            let s = saved.find(&id).expect("刚写入的源必在");
            Json(json!({ "source": source_view(s) })).into_response()
        }
        Err(e) => store_error_response(&e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourcePatchRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    token: Option<String>,
}

/// PATCH /api/forge/store/sources/{id}
pub(crate) async fn sources_patch(
    Path(id): Path<String>,
    Json(req): Json<SourcePatchRequest>,
) -> Response {
    let dir = data_dir();
    let mut cfg = sources();
    let Some(existing) = cfg.find(&id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "STORE_SOURCE_NOT_FOUND", "message": format!("未配置源: {id}") } })),
        )
            .into_response();
    };
    if let Some(t) = req.token.as_deref().map(str::trim) {
        // 空串 = 清除令牌(写空串等价于「不再带 Authorization」;keystore 无删除面,写空即失效)。
        if let Err(e) = gend::keystore::set_key(&token_key(&id), t) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
            )
                .into_response();
        }
    }
    let entry = SourceConfig {
        id: id.clone(),
        name: req
            .name
            .map(|n| n.trim().to_string())
            .unwrap_or(existing.name),
        base_url: existing.base_url,
        enabled: req.enabled.unwrap_or(existing.enabled),
        token: None,
    };
    cfg.upsert(entry);
    match save_sources(&dir, &cfg) {
        Ok(()) => {
            let saved = load_sources(&dir);
            let s = saved.find(&id).expect("刚写入的源必在");
            Json(json!({ "source": source_view(s) })).into_response()
        }
        Err(e) => store_error_response(&e),
    }
}

/// DELETE /api/forge/store/sources/{id}
pub(crate) async fn sources_delete(Path(id): Path<String>) -> Response {
    let dir = data_dir();
    let mut cfg = sources();
    if !cfg.remove(&id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "STORE_SOURCE_NOT_FOUND", "message": format!("未配置源: {id}") } })),
        )
            .into_response();
    }
    match save_sources(&dir, &cfg) {
        Ok(()) => Json(json!({ "removed": true, "id": id })).into_response(),
        Err(e) => store_error_response(&e),
    }
}

// ---------- 搜索与详情 ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchParams {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    source_id: Option<String>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    page_size: Option<u32>,
}

/// GET /api/forge/store/search
pub(crate) async fn search(Query(p): Query<SearchParams>) -> Response {
    let mut q = SearchQuery::new(p.q.as_deref().unwrap_or(""));
    q.kind = match p.kind.as_deref() {
        Some("asset-pack") => Some(PackageKind::AssetPack),
        Some("skill") => Some(PackageKind::Skill),
        _ => None,
    };
    if let Some(v) = p.page {
        q.page = v;
    }
    if let Some(v) = p.page_size {
        q.page_size = v;
    }
    let mut cfg = sources();
    if let Some(only) = p.source_id.as_deref().filter(|s| !s.is_empty()) {
        cfg.sources.retain(|s| s.id == only);
        if cfg.sources.is_empty() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "STORE_SOURCE_NOT_FOUND", "message": format!("未配置源: {only}") } })),
            )
                .into_response();
        }
    }
    let names: HashMap<String, String> = cfg
        .sources
        .iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect();
    // ureq 阻塞:搬到阻塞线程池,别占 tokio 工作线程。
    let agg = tokio::task::spawn_blocking(move || search_all(&cfg, &q, &token_of))
        .await
        .expect("search_all 阻塞任务 join 失败");
    let (page, page_size) = (p.page.unwrap_or(1).max(1), p.page_size.unwrap_or(20));
    let items: Vec<Value> = agg
        .items
        .iter()
        .map(|(sid, pkg)| {
            json!({
                "sourceId": sid,
                "sourceName": names.get(sid).cloned().unwrap_or_default(),
                "package": serde_json::to_value(pkg).unwrap_or(Value::Null),
            })
        })
        .collect();
    // 源不可达不吞:与结果并列返回,由 UI 如实展示「N 个源不可达」。
    let errors: Vec<Value> = agg
        .errors
        .iter()
        .map(|(sid, e)| json!({ "sourceId": sid, "code": e.code, "message": e.message }))
        .collect();
    Json(json!({
        "total": items.len(),
        "page": page,
        "pageSize": page_size,
        "partial": agg.is_partial(),
        "items": items,
        "errors": errors,
    }))
    .into_response()
}

/// GET /api/forge/store/packages/{sourceId}/{pkgId}
pub(crate) async fn package_detail(Path((source_id, pkg_id)): Path<(String, String)>) -> Response {
    let cfg = sources();
    let Some(scfg) = cfg.find(&source_id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "STORE_SOURCE_NOT_FOUND", "message": format!("未配置源: {source_id}") } })),
        )
            .into_response();
    };
    let with_token = SourceConfig {
        token: token_of(&source_id),
        ..scfg
    };
    let r = tokio::task::spawn_blocking(move || -> Result<Value, StoreError> {
        let src = open_source(&with_token)?;
        let d = src.detail(&pkg_id)?;
        Ok(json!({
            "summary": serde_json::to_value(&d.summary).unwrap_or(Value::Null),
            "versions": d.versions,
        }))
    })
    .await
    .expect("detail 阻塞任务 join 失败");
    match r {
        Ok(mut v) => {
            v["sourceId"] = json!(source_id);
            Json(v).into_response()
        }
        Err(e) => store_error_response(&e),
    }
}

/// GET /api/forge/store/packages/{sourceId}/{pkgId}/{version}
pub(crate) async fn package_manifest(
    Path((source_id, pkg_id, version)): Path<(String, String, String)>,
) -> Response {
    let cfg = sources();
    let Some(scfg) = cfg.find(&source_id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "STORE_SOURCE_NOT_FOUND", "message": format!("未配置源: {source_id}") } })),
        )
            .into_response();
    };
    let with_token = SourceConfig {
        token: token_of(&source_id),
        ..scfg
    };
    let r = tokio::task::spawn_blocking(move || -> Result<PackageManifest, StoreError> {
        let src = open_source(&with_token)?;
        src.manifest(&pkg_id, &version)
    })
    .await
    .expect("manifest 阻塞任务 join 失败");
    match r {
        Ok(m) => Json(json!({
            "sourceId": source_id,
            "manifest": serde_json::to_value(&m).unwrap_or(Value::Null),
        }))
        .into_response(),
        Err(e) => store_error_response(&e),
    }
}

// ---------- 安装 / 卸载 / 任务 ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstallRequest {
    source_id: String,
    pkg_id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    dest_folder: Option<String>,
    #[serde(default)]
    force: bool,
}

/// POST /api/forge/store/install:提交长任务,秒回 taskId。
pub(crate) async fn install(Json(req): Json<InstallRequest>) -> Response {
    if req.source_id.trim().is_empty() || req.pkg_id.trim().is_empty() {
        return bad_request("FORGE_INVALID_ARGS", "sourceId / pkgId 不可空");
    }
    let task_id = tasks().new_task("install");
    let tid = task_id.clone();
    // 下载 + 校验 + assetd 构建链全是阻塞 IO,进阻塞线程池。
    tokio::task::spawn_blocking(move || {
        let outcome = run_install(&req, &tid);
        tasks().finish(&tid, outcome);
    });
    Json(json!({ "taskId": task_id, "status": "running" })).into_response()
}

fn run_install(req: &InstallRequest, task_id: &str) -> Result<Value, StoreError> {
    let cfg = sources();
    let scfg = cfg.find(&req.source_id).cloned().ok_or_else(|| {
        StoreError::new(
            forge_store::STORE_SOURCE_NOT_FOUND,
            format!("未配置源: {}", req.source_id),
        )
    })?;
    let source_name = scfg.name.clone();
    let with_token = SourceConfig {
        token: token_of(&req.source_id),
        ..scfg
    };
    let src = open_source(&with_token)?;
    let ver = match req.version.as_deref().filter(|v| !v.is_empty()) {
        Some(v) => v.to_string(),
        None => {
            let d = src.detail(&req.pkg_id)?;
            d.versions
                .last()
                .cloned()
                .unwrap_or(d.summary.latest_version)
        }
    };
    let manifest = src.manifest(&req.pkg_id, &ver)?;
    let proj = project();
    let skills = skills_root();
    let tid = task_id.to_string();
    let progress = move |phase: &str, done: u32, total: u32| {
        tasks().progress(&tid, phase, done, total);
    };
    let opts = InstallOptions {
        dest_folder: req.dest_folder.as_deref().filter(|s| !s.is_empty()),
        skills_root: &skills,
        force: req.force,
        progress: Some(&progress),
        source_name: Some(&source_name),
    };
    let rec = install_package(&proj, src.as_ref(), &req.source_id, &manifest, &opts)?;
    serde_json::to_value(&rec).map_err(|e| StoreError::new("SERIALIZE", e.to_string()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UninstallRequest {
    source_id: String,
    pkg_id: String,
    #[serde(default)]
    force: bool,
}

/// POST /api/forge/store/uninstall:destructive,须 approved Proposal(I-6)。
///
/// 两阶段形态与 `mcp_call` 的 `forced_asset_delete` 一致:未批准 → 自动建单 + 409;
/// 批准后原样重发即执行。full-auto 不豁免。
pub(crate) async fn uninstall(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UninstallRequest>,
) -> Response {
    if req.source_id.trim().is_empty() || req.pkg_id.trim().is_empty() {
        return bad_request("FORGE_INVALID_ARGS", "sourceId / pkgId 不可空");
    }
    let key = format!("{}/{}", req.source_id, req.pkg_id);
    if !state
        .proposals
        .has_approved_covering(UNINSTALL_PROPOSAL_KIND, &[key.clone()])
    {
        // dry-run 影响面:该包落地的资产清单与技能名(读已装记录,不动盘)。
        let (assets, skills) = match InstalledDb::load(&project()) {
            Ok(db) => match db.find(&req.source_id, &req.pkg_id) {
                Some(r) => (r.asset_paths.clone(), r.skill_names.clone()),
                None => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({ "error": { "code": "STORE_NOT_INSTALLED", "message": format!("{key} 未安装,无法卸载") } })),
                    )
                        .into_response();
                }
            },
            Err(e) => return store_error_response(&e),
        };
        let id = state.proposals.create(
            UNINSTALL_PROPOSAL_KIND,
            format!(
                "卸载商店包 {key}:移除 {} 个资产、{} 个技能",
                assets.len(),
                skills.len()
            ),
            // has_approved_covering 读 impact.assets,故覆盖键放在 assets 里;
            // 真实影响面另列 removeAssets/removeSkills 供 UI 展示。
            json!({
                "assets": [key],
                "removeAssets": assets,
                "removeSkills": skills,
            }),
            json!({ "sessionId": "http", "tool": "store.uninstall" }),
        );
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": {
                    "code": "GOV_PROPOSAL_REQUIRED",
                    "message": "卸载为 destructive,须先批准 Proposal(I-6)",
                    "proposalId": id,
                }
            })),
        )
            .into_response();
    }
    let task_id = tasks().new_task("uninstall");
    let tid = task_id.clone();
    tokio::task::spawn_blocking(move || {
        let outcome = run_uninstall(&req, &tid);
        tasks().finish(&tid, outcome);
    });
    Json(json!({ "taskId": task_id, "status": "running" })).into_response()
}

fn run_uninstall(req: &UninstallRequest, task_id: &str) -> Result<Value, StoreError> {
    tasks().progress(task_id, "resolve", 0, 1);
    let proj = project();
    let mut db = InstalledDb::load(&proj)?;
    tasks().progress(task_id, "remove", 0, 1);
    let out = uninstall_package(
        &proj,
        &mut db,
        &req.source_id,
        &req.pkg_id,
        &skills_root(),
        req.force,
    )?;
    tasks().progress(task_id, "record", 1, 1);
    let blocked: Vec<Value> = out
        .blocked_by_refs
        .iter()
        .map(|(p, refs)| json!({ "assetPath": p, "referencedBy": refs }))
        .collect();
    Ok(json!({
        "removedAssets": out.removed_assets,
        "removedSkills": out.removed_skills,
        "blockedByRefs": blocked,
    }))
}

/// GET /api/forge/store/tasks/{taskId}
pub(crate) async fn task_status(Path(id): Path<String>) -> Response {
    match tasks().get(&id) {
        Some(t) => Json(t.to_json()).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "STORE_TASK_NOT_FOUND", "message": format!("无此任务: {id}(任务表在进程内,agentd 重启即空)") } })),
        )
            .into_response(),
    }
}

// ---------- 已装清单与更新 ----------

/// GET /api/forge/store/installed
pub(crate) async fn installed_list() -> Response {
    match InstalledDb::load(&project()) {
        Ok(db) => Json(json!({
            "installed": serde_json::to_value(db.list()).unwrap_or(Value::Null),
        }))
        .into_response(),
        Err(e) => store_error_response(&e),
    }
}

/// GET /api/forge/store/updates
pub(crate) async fn updates() -> Response {
    let db = match InstalledDb::load(&project()) {
        Ok(d) => d,
        Err(e) => return store_error_response(&e),
    };
    let cfg: SourcesConfig = sources();
    let list = tokio::task::spawn_blocking(move || check_updates(&db, &cfg, &token_of))
        .await
        .expect("check_updates 阻塞任务 join 失败");
    Json(json!({ "updates": serde_json::to_value(&list).unwrap_or(Value::Null) })).into_response()
}

// ---------- 个人资产库 ----------

/// GET /api/forge/store/library
pub(crate) async fn library_list() -> Response {
    match Library::open(&data_dir()) {
        Ok(lib) => Json(json!({
            "items": serde_json::to_value(lib.list()).unwrap_or(Value::Null),
        }))
        .into_response(),
        Err(e) => store_error_response(&e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryAddRequest {
    asset_path: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// POST /api/forge/store/library:把项目内资产收藏进个人库。
pub(crate) async fn library_add(Json(req): Json<LibraryAddRequest>) -> Response {
    let proj = project();
    let rel = match forge_store::manifest::safe_rel_path(&req.asset_path) {
        Ok(r) => r,
        Err(e) => return store_error_response(&e),
    };
    let abs = proj.content_root().join(&rel);
    if !abs.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "STORE_PACKAGE_NOT_FOUND", "message": format!("项目内无此资产: {rel}") } })),
        )
            .into_response();
    }
    let name = std::path::Path::new(&rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("asset")
        .to_string();
    let mut lib = match Library::open(&data_dir()) {
        Ok(l) => l,
        Err(e) => return store_error_response(&e),
    };
    match lib
        .add_file(&name, &abs, "project", &req.tags)
        .and_then(|item| {
            lib.save()?;
            Ok(item)
        }) {
        Ok(item) => Json(json!({ "item": serde_json::to_value(&item).unwrap_or(Value::Null) }))
            .into_response(),
        Err(e) => store_error_response(&e),
    }
}

/// DELETE /api/forge/store/library/{id}
pub(crate) async fn library_remove(Path(id): Path<String>) -> Response {
    let mut lib = match Library::open(&data_dir()) {
        Ok(l) => l,
        Err(e) => return store_error_response(&e),
    };
    match lib.remove(&id) {
        Ok(()) => Json(json!({ "removed": true, "id": id })).into_response(),
        Err(e) => store_error_response(&e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryInstallRequest {
    #[serde(default)]
    dest_folder: Option<String>,
}

/// POST /api/forge/store/library/{id}:install 的路由入口。
///
/// matchit 不允许「参数 + 静态后缀」同段,故对外契约的冒号动作整段被 `{id}` 收下,
/// 在此剥后缀分派(与 `skills/{name}:validate` 同处置)。不带动作后缀 = 400,
/// 因为「对一个库条目 POST」本身没有默认语义,含糊放行只会让调用方以为做了什么。
pub(crate) async fn library_post(
    Path(seg): Path<String>,
    body: Option<Json<LibraryInstallRequest>>,
) -> Response {
    let Some(id) = seg.strip_suffix(":install") else {
        return match seg.rsplit_once(':') {
            Some((_, action)) => bad_request(
                "STORE_ACTION_UNKNOWN",
                format!("未知动作 `:{action}`;个人库条目仅支持 `:install`"),
            ),
            None => bad_request(
                "STORE_ACTION_REQUIRED",
                "POST 个人库条目须带动作后缀(如 /library/{id}:install)",
            ),
        };
    };
    let req = body
        .map(|Json(b)| b)
        .unwrap_or(LibraryInstallRequest { dest_folder: None });
    library_install(id.to_string(), req).await
}

/// 从个人库装进当前项目(走 asset_import 同链)。
async fn library_install(id: String, req: LibraryInstallRequest) -> Response {
    let dest = req.dest_folder.filter(|s| !s.is_empty());
    let r = tokio::task::spawn_blocking(move || -> Result<Value, StoreError> {
        let proj = project();
        let lib = Library::open(&data_dir())?;
        let item = lib.get(&id).ok_or_else(|| {
            StoreError::new(
                forge_store::STORE_NOT_INSTALLED,
                format!("资产库无此条目: {id}"),
            )
        })?;
        let blob = lib.blob_path(&item.sha256);
        if !blob.is_file() {
            return Err(StoreError::new(
                forge_store::STORE_PACKAGE_NOT_FOUND,
                format!("资产库 blob 缺失(索引与内容不一致): {}", item.sha256),
            ));
        }
        // 与商店安装同路:staging 命名副本 → import_assets,不自己拷进 Content/。
        let staging = proj.root.join(".forge/tmp/store/library");
        std::fs::create_dir_all(&staging)?;
        let file_name = if item.ext.is_empty() {
            item.name.clone()
        } else {
            format!("{}.{}", item.name, item.ext)
        };
        let staged = staging.join(&file_name);
        std::fs::copy(&blob, &staged)?;
        let dest_folder = dest
            .unwrap_or_else(|| forge_store::install::default_dest_folder(&item.ext).to_string());
        let staged_rel = format!(".forge/tmp/store/library/{file_name}");
        let outcome = assetd::import::import_assets(&proj, &[staged_rel], &dest_folder, None)?;
        std::fs::remove_file(&staged).ok();
        if let Some(f) = outcome.failed.first() {
            return Err(StoreError::new(
                forge_store::STORE_MANIFEST_INVALID,
                format!("入管线失败({}): {}", f.source, f.error),
            ));
        }
        let one =
            outcome.imported.into_iter().next().ok_or_else(|| {
                StoreError::new(forge_store::STORE_MANIFEST_INVALID, "入管线无结果")
            })?;
        Ok(json!({ "assetPath": one.asset_path, "guid": one.guid }))
    })
    .await
    .expect("library_install 阻塞任务 join 失败");
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => store_error_response(&e),
    }
}

// ---------- 发布 ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublishRequest {
    source_id: String,
    manifest: PackageManifest,
    #[serde(default)]
    asset_paths: Vec<String>,
}

/// POST /api/forge/store/publish:从项目资产打包并发布到指定源。
pub(crate) async fn publish(Json(req): Json<PublishRequest>) -> Response {
    if req.asset_paths.is_empty() {
        return bad_request("FORGE_INVALID_ARGS", "assetPaths 不可空");
    }
    let cfg = sources();
    let Some(scfg) = cfg.find(&req.source_id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "STORE_SOURCE_NOT_FOUND", "message": format!("未配置源: {}", req.source_id) } })),
        )
            .into_response();
    };
    let with_token = SourceConfig {
        token: token_of(&req.source_id),
        ..scfg
    };
    let r = tokio::task::spawn_blocking(move || -> Result<PackageManifest, StoreError> {
        let proj = project();
        let input: PublishInput = draft_from_project(&proj, &req.asset_paths, req.manifest)?;
        let src = open_source(&with_token)?;
        publish_package(src.as_ref(), &input)
    })
    .await
    .expect("publish 阻塞任务 join 失败");
    match r {
        Ok(m) => Json(json!({ "manifest": serde_json::to_value(&m).unwrap_or(Value::Null) }))
            .into_response(),
        Err(e) => store_error_response(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_status_maps_semantically() {
        assert_eq!(
            store_status("STORE_PACKAGE_NOT_FOUND"),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            store_status("STORE_ALREADY_INSTALLED"),
            StatusCode::CONFLICT
        );
        assert_eq!(
            store_status("STORE_CHECKSUM_MISMATCH"),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            store_status("STORE_PAYMENT_REQUIRED"),
            StatusCode::PAYMENT_REQUIRED
        );
        // 上游不可达是 502 而非 500——问题不在本机。
        assert_eq!(
            store_status("STORE_SOURCE_UNREACHABLE"),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(store_status("IO"), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn token_key_is_namespaced() {
        // 与 LLM/生成渠道共用 keystore,故必须带命名空间前缀避免撞槽。
        assert_eq!(token_key("official"), "store:official");
        assert_ne!(token_key("deepseek"), "deepseek");
    }

    #[test]
    fn task_table_lifecycle_and_json_shape() {
        let t = TaskTable::default();
        let id = t.new_task("install");
        let s = t.get(&id).unwrap();
        assert_eq!(s.status, "running");
        assert_eq!(s.to_json()["taskId"], id);
        t.progress(&id, "download", 3, 7);
        let s = t.get(&id).unwrap();
        assert_eq!((s.phase.as_str(), s.done, s.total), ("download", 3, 7));
        t.finish(&id, Ok(json!({ "assetPaths": ["Textures/a.png"] })));
        let s = t.get(&id).unwrap();
        assert_eq!(s.status, "completed");
        assert_eq!(s.to_json()["result"]["assetPaths"][0], "Textures/a.png");

        let bad = t.new_task("install");
        t.finish(
            &bad,
            Err(StoreError::new(
                forge_store::STORE_CHECKSUM_MISMATCH,
                "sha256 不符",
            )),
        );
        let s = t.get(&bad).unwrap();
        assert_eq!(s.status, "failed");
        assert_eq!(s.to_json()["error"]["code"], "STORE_CHECKSUM_MISMATCH");
        assert!(t.get("stask_nope").is_none());
    }

    #[test]
    fn source_view_never_exposes_token_value() {
        // R-5:视图只回 hasToken 布尔。此处 token 字段本身也被 serde skip,双保险。
        let s = SourceConfig {
            id: "acme".into(),
            name: "Acme".into(),
            base_url: "https://acme.example/registry".into(),
            enabled: true,
            token: Some("super-secret".into()),
        };
        let v = source_view(&s);
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("super-secret"), "令牌值泄漏: {text}");
        assert_eq!(v["id"], "acme");
        assert_eq!(v["enabled"], true);
    }
}
