//! Durable Blender authoring handoff and fixed exporter. Desktop authoring belongs
//! to an independent Codex task; this service only leases jobs and publishes assets.
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

const LEASE_MS: u64 = 300_000;
const EXPORT_SECONDS: u64 = 300;
type BridgeResult<T> = Result<T, String>;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    id: String,
    source_id: String,
    workspace_id: String,
    name: String,
    prompt: String,
    kind: String,
    node_id: Option<String>,
    #[serde(default)]
    idle_clip: Option<String>,
    #[serde(default)]
    walk_clip: Option<String>,
    state: String,
    stage: String,
    message: String,
    source_path: String,
    source_bound: bool,
    auto_sync: bool,
    revision: u64,
    executor_id: Option<String>,
    lease_expires_at: Option<u64>,
    #[serde(default, skip_serializing)]
    lease_token: Option<String>,
    created_at: u64,
    updated_at: u64,
    published: Option<Value>,
    error: Option<String>,
    reload: Option<Value>,
    dependencies: Vec<String>,
    fingerprint: Option<String>,
    observed_fingerprint: Option<String>,
    dirty_since: Option<u64>,
    #[serde(default)]
    setup: Value,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    jobs: BTreeMap<String, Job>,
    executable_path: Option<String>,
}

struct Service {
    root: PathBuf,
    path: PathBuf,
    inner: Mutex<Document>,
    // One engine process owns the project's durable queue at a time. A process
    // crash releases this OS lock automatically; no stale lock-file deletion.
    _owner: std::fs::File,
    // Export/import commits are serialized for this project. Different projects
    // remain independent, as do the existing engine MCP connection pools.
    publishing: tokio::sync::Mutex<()>,
}

fn now() -> u64 {
    gend::timeutil::unix_millis() as u64
}
fn registry() -> &'static Mutex<HashMap<PathBuf, Arc<Service>>> {
    static SERVICES: OnceLock<Mutex<HashMap<PathBuf, Arc<Service>>>> = OnceLock::new();
    SERVICES.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Service {
    fn load(root: PathBuf) -> BridgeResult<Self> {
        let path = root.join(".forge/blender/state.json");
        confine_output(&root, &path)?;
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let owner = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_file_name("service.lock"))
            .map_err(|e| e.to_string())?;
        fs2::FileExt::try_lock_exclusive(&owner).map_err(|_| {
            "PROJECT_BUSY: another engine process owns this project's Blender queue".to_string()
        })?;
        let mut doc: Document = if path.is_file() {
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(
                |e| {
                    format!(
                        "Blender state is corrupt; preserve and repair {}: {e}",
                        path.display()
                    )
                },
            )?
        } else {
            Document::default()
        };
        let mut recovered = false;
        for job in doc.jobs.values_mut() {
            if matches!(job.state.as_str(), "exporting" | "validating" | "importing") {
                job.state = "failed".into();
                job.error = Some(
                    "Engine restarted during publication; retry from the saved Blender source"
                        .into(),
                );
                job.message = job.error.clone().unwrap();
                recovered = true;
            } else if matches!(job.state.as_str(), "claimed" | "authoring") {
                job.state = "awaiting_codex".into();
                job.message = "Engine restarted; claim this job again in Codex".into();
                recovered = true;
            }
            job.lease_token = None;
            job.lease_expires_at = None;
        }
        let service = Self {
            root,
            path,
            inner: Mutex::new(doc),
            _owner: owner,
            publishing: tokio::sync::Mutex::new(()),
        };
        if recovered {
            service.persist(&service.inner.lock().unwrap())?;
        }
        Ok(service)
    }
    fn persist(&self, doc: &Document) -> BridgeResult<()> {
        crate::sessions::write_atomic(
            &self.path,
            &serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
    fn update<T>(&self, change: impl FnOnce(&mut Document) -> BridgeResult<T>) -> BridgeResult<T> {
        let mut guard = self.inner.lock().unwrap();
        let mut next = guard.clone();
        let value = change(&mut next)?;
        self.persist(&next)?;
        *guard = next;
        Ok(value)
    }
    fn job(&self, id: &str) -> BridgeResult<Job> {
        self.inner
            .lock()
            .unwrap()
            .jobs
            .get(id)
            .cloned()
            .ok_or_else(|| "JOB_NOT_FOUND: unknown job in this workspace".into())
    }
    fn job_update(
        &self,
        id: &str,
        change: impl FnOnce(&mut Job) -> BridgeResult<()>,
    ) -> BridgeResult<Job> {
        self.update(|doc| {
            let job = doc
                .jobs
                .get_mut(id)
                .ok_or("JOB_NOT_FOUND: unknown job in this workspace")?;
            change(job)?;
            job.updated_at = now();
            Ok(job.clone())
        })
    }
    fn view(&self, job: &Job) -> Value {
        let handoff = format!("Use $blender-production. Claim RurixForge Blender job {} in workspace {} through rurix-blender MCP. Use this Codex desktop task's native computer-use plugin to create the requested Blender asset, save to {}, bind and publish it, then verify its real engine template preview. Read the job for the full brief. Do not use the engine's open-computer-use substitute.", job.id, job.workspace_id, display_path(&self.root.join(&job.source_path)));
        let mut value = serde_json::to_value(job).unwrap();
        let object = value.as_object_mut().unwrap();
        for key in [
            "dependencies",
            "fingerprint",
            "observedFingerprint",
            "dirtySince",
        ] {
            object.remove(key);
        }
        object.insert(
            "sourceAbsolutePath".into(),
            json!(display_path(&self.root.join(&job.source_path))),
        );
        object.insert("sourceFingerprint".into(), json!(job.fingerprint));
        object.insert(
            "codexUrl".into(),
            json!(format!(
                "codex://threads/new?path={}&prompt={}",
                encode(&display_path(&self.root)),
                encode(&handoff)
            )),
        );
        object.insert("handoffPrompt".into(), json!(handoff));
        object.insert(
            "error".into(),
            job.error
                .as_ref()
                .map(|message| json!({"code":error_code(message),"message":message}))
                .unwrap_or(Value::Null),
        );
        value
    }
    fn executable(&self) -> Option<PathBuf> {
        let configured = self.inner.lock().unwrap().executable_path.clone();
        let choices = [
            configured,
            std::env::var("FORGE_BLENDER_EXECUTABLE").ok(),
            Some("E:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe".into()),
            Some("C:\\Program Files\\Blender Foundation\\Blender 5.2\\blender.exe".into()),
            Some("C:\\Program Files\\Blender Foundation\\Blender 4.5\\blender.exe".into()),
        ];
        choices
            .into_iter()
            .flatten()
            .map(PathBuf::from)
            .find(|p| valid_executable(p))
    }
}

fn display_path(path: &FsPath) -> String {
    path.to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .to_string()
}
fn confine_output(root: &FsPath, path: &FsPath) -> BridgeResult<()> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut ancestor = path;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or("PATH_INVALID: output has no existing project ancestor")?;
    }
    let ancestor = ancestor.canonicalize().map_err(|e| e.to_string())?;
    if !ancestor.starts_with(&root) {
        return Err("PATH_INVALID: project output resolves outside this project".into());
    }
    Ok(())
}
fn valid_executable(path: &FsPath) -> bool {
    path.is_file()
        && path.file_name().is_some_and(|n| {
            matches!(
                n.to_string_lossy().to_ascii_lowercase().as_str(),
                "blender.exe" | "blender"
            )
        })
}
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn fail(message: String) -> Response {
    let code = error_code(&message);
    let status = match code {
        "JOB_NOT_FOUND" | "WORKSPACE_NOT_FOUND" => StatusCode::NOT_FOUND,
        "LEASE_CONFLICT" | "LEASE_EXPIRED" | "INVALID_STATE" | "SETUP_CONFLICT" => {
            StatusCode::CONFLICT
        }
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(json!({"error":{"code":code,"message":message}})),
    )
        .into_response()
}
fn error_code(message: &str) -> &str {
    message
        .split(':')
        .next()
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        .unwrap_or("BLENDER_ERROR")
}
fn workspace(state: &AppState, id: &str) -> BridgeResult<PathBuf> {
    if id.is_empty() {
        return Err("WORKSPACE_REQUIRED: workspaceId is required".into());
    }
    if id != "default" && state.workspaces.get(id).is_none() {
        return Err("WORKSPACE_NOT_FOUND: unknown workspaceId".into());
    }
    Ok(crate::scope::project_of(state, (id != "default").then_some(id)).project_root)
}
fn service(state: &AppState, id: &str) -> BridgeResult<Arc<Service>> {
    let root = workspace(state, id)?;
    let mut services = registry().lock().unwrap();
    if let Some(service) = services.get(&root) {
        return Ok(service.clone());
    }
    let service = Arc::new(Service::load(root.clone())?);
    services.insert(root, service.clone());
    let watcher = service.clone();
    tokio::spawn(async move {
        watch(watcher).await;
    });
    Ok(service)
}

pub(crate) fn start(state: &Arc<AppState>) {
    // Recover durable bindings even if nobody opens the Blender panel after a restart.
    let mut ids = vec!["default".to_string()];
    ids.extend(state.workspaces.list().into_iter().map(|w| w.id));
    for id in ids {
        if let Ok(root) = workspace(state, &id) {
            if root.join(".forge/blender/state.json").is_file() {
                if let Err(error) = service(state, &id) {
                    eprintln!("Blender recovery: {error}");
                }
            }
        }
    }
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/forge/blender/status", get(status))
        .route("/api/forge/blender/config", post(config))
        .route("/api/forge/blender/setup", post(setup))
        .route("/api/forge/blender/jobs", get(list).post(create))
        .route("/api/forge/blender/jobs/{id}", get(get_job))
        .route("/api/forge/blender/jobs/{id}/{action}", post(action))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopeQuery {
    workspace_id: String,
}

async fn status(State(state): State<Arc<AppState>>, Query(query): Query<ScopeQuery>) -> Response {
    let service = match service(&state, &query.workspace_id) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    Json(status_value(&service)).into_response()
}
fn status_value(service: &Service) -> Value {
    let executable = service.executable();
    let jobs: Vec<Job> = service
        .inner
        .lock()
        .unwrap()
        .jobs
        .values()
        .cloned()
        .collect();
    let verified = jobs
        .iter()
        .any(|j| j.executor_id.is_some() && j.lease_expires_at.is_some_and(|t| t > now()));
    json!({"blender":{"found":executable.is_some(),"executablePath":executable.as_deref().map(display_path)},"computerUse":{"status":if verified {"verified"} else {"unknown"},"evidence":"executor capability reported on an active lease"},"setup":setup_status(&service.root),"jobs":jobs.len()})
}
async fn list(State(state): State<Arc<AppState>>, Query(query): Query<ScopeQuery>) -> Response {
    let service = match service(&state, &query.workspace_id) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let mut jobs: Vec<Job> = service
        .inner
        .lock()
        .unwrap()
        .jobs
        .values()
        .cloned()
        .collect();
    jobs.sort_by_key(|j| std::cmp::Reverse(j.created_at));
    Json(json!({"jobs":jobs.iter().map(|j| service.view(j)).collect::<Vec<_>>()})).into_response()
}
async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<ScopeQuery>,
) -> Response {
    match service(&state, &query.workspace_id).and_then(|s| s.job(&id).map(|j| s.view(&j))) {
        Ok(v) => Json(v).into_response(),
        Err(e) => fail(e),
    }
}

fn requested_scope<'a>(body: &'a Value) -> BridgeResult<&'a str> {
    body.get("workspaceId")
        .and_then(Value::as_str)
        .ok_or_else(|| "WORKSPACE_REQUIRED: workspaceId is required".into())
}
fn field<'a>(body: &'a Value, name: &str) -> BridgeResult<&'a str> {
    body.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("BAD_PARAMS: {name} is required"))
}
async fn create(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let result = (|| {
        let workspace_id = requested_scope(&body)?;
        let service = service(&state, workspace_id)?;
        let name = field(&body, "name")?;
        let prompt = field(&body, "prompt")?;
        let kind = field(&body, "kind")?;
        if !["prop", "map", "character"].contains(&kind) || name.len() > 128 || prompt.len() > 32000
        {
            return Err("BAD_PARAMS: invalid kind, name or prompt length".into());
        }
        let id = crate::events::new_id("blend");
        let file_name: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let source_path = format!("Sources/Blender/{id}/{file_name}.blend");
        confine_output(&service.root, &service.root.join(&source_path))?;
        std::fs::create_dir_all(service.root.join(&source_path).parent().unwrap())
            .map_err(|e| e.to_string())?;
        let setup = {
            let _guard = service.inner.lock().unwrap();
            prepare_setup(&service.root, workspace_id)?
        };
        let job = Job {
            id: id.clone(),
            source_id: id.clone(),
            workspace_id: workspace_id.into(),
            name: name.into(),
            prompt: prompt.into(),
            kind: kind.into(),
            node_id: body
                .get("nodeId")
                .and_then(Value::as_str)
                .map(str::to_string),
            idle_clip: body
                .get("idleClip")
                .and_then(Value::as_str)
                .map(str::to_string),
            walk_clip: body
                .get("walkClip")
                .and_then(Value::as_str)
                .map(str::to_string),
            state: "awaiting_codex".into(),
            stage: "handoff".into(),
            message: "Open in Codex and send the prepared task to begin Blender authoring".into(),
            source_path,
            source_bound: false,
            auto_sync: true,
            revision: 0,
            executor_id: None,
            lease_expires_at: None,
            lease_token: None,
            created_at: now(),
            updated_at: now(),
            published: None,
            error: None,
            reload: None,
            dependencies: vec![],
            fingerprint: None,
            observed_fingerprint: None,
            dirty_since: None,
            setup,
        };
        service.update(|d| {
            d.jobs.insert(id, job.clone());
            Ok(())
        })?;
        Ok(service.view(&job))
    })();
    match result {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => fail(e),
    }
}

async fn config(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let result = (|| {
        let service = service(&state, requested_scope(&body)?)?;
        let executable = PathBuf::from(field(&body, "executablePath")?);
        if !executable.is_absolute() || !valid_executable(&executable) {
            return Err("BAD_PARAMS: executablePath must be an existing Blender executable".into());
        }
        let executable = executable.canonicalize().map_err(|e| e.to_string())?;
        service.update(|d| {
            d.executable_path = Some(display_path(&executable));
            Ok(())
        })?;
        Ok(status_value(&service))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => fail(e),
    }
}
async fn setup(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let result = (|| {
        let id = requested_scope(&body)?;
        let s = service(&state, id)?;
        let _guard = s.inner.lock().unwrap();
        prepare_setup(&s.root, id)
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => fail(e),
    }
}

fn ensure_lease(job: &mut Job, body: &Value) -> BridgeResult<()> {
    if job.state == "cancelled" {
        return Err("INVALID_STATE: job was cancelled".into());
    }
    if job.lease_expires_at.map_or(true, |t| t <= now()) {
        return Err("LEASE_EXPIRED: claim the job before modifying it".into());
    }
    if job.lease_token.as_deref() != Some(field(body, "leaseToken")?) {
        return Err("LEASE_CONFLICT: lease token does not own this job".into());
    }
    job.lease_expires_at = Some(now() + LEASE_MS);
    Ok(())
}

async fn action(
    State(state): State<Arc<AppState>>,
    Path((id, action)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let service = match requested_scope(&body).and_then(|ws| service(&state, ws)) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    if action == "preview" {
        let job = match service.job(&id) {
            Ok(j) => j,
            Err(e) => return fail(e),
        };
        let Some(published) = job.published else {
            return fail("INVALID_STATE: publish a valid template before previewing".into());
        };
        let mut args = json!({"prefabRef":published["prefabPath"]});
        for name in ["width", "height", "clip", "time", "yaw"] {
            if let Some(v) = body.get(name) {
                args[name] = v.clone();
            }
        }
        return match engine(&service.root, "template_preview", args).await {
            Ok(v) => Json(v).into_response(),
            Err(e) => fail(e),
        };
    }
    let mut launch = false;
    let mut claim_token = None;
    let result = service.job_update(&id, |job| {
        match action.as_str() {
            "claim" => {
                if body.pointer("/capabilities/computerUse").and_then(Value::as_bool) != Some(true) { return Err("COMPUTER_USE_REQUIRED: native Codex computer-use must be available before claiming".into()); }
                if matches!(job.state.as_str(), "cancelled" | "exporting" | "validating" | "importing") { return Err("INVALID_STATE: cannot claim this job now".into()); }
                if job.lease_expires_at.is_some_and(|t| t > now()) { return Err("LEASE_CONFLICT: job is already claimed; heartbeat with its lease token".into()); }
                let token = assetd::new_guid();
                job.lease_token = Some(token.clone()); job.lease_expires_at = Some(now() + LEASE_MS);
                job.executor_id = Some(field(&body, "executorId")?.into());
                job.state = "claimed".into(); job.stage = "authoring".into(); job.error = None;
                job.message = "Claimed by a Codex desktop task with native computer-use".into(); claim_token = Some(token);
            }
            "progress" | "heartbeat" => {
                ensure_lease(job, &body)?;
                if matches!(job.state.as_str(), "claimed" | "authoring") { job.state = "authoring".into(); }
                if let Some(text) = body.get("stage").and_then(Value::as_str) { job.stage = text.chars().take(100).collect(); }
                if let Some(text) = body.get("message").and_then(Value::as_str) { job.message = text.chars().take(2000).collect(); }
            }
            "bind" => {
                ensure_lease(job, &body)?;
                if matches!(job.state.as_str(), "exporting" | "validating" | "importing") { return Err("INVALID_STATE: wait for publication before rebinding".into()); }
                let raw = FsPath::new(field(&body, "sourcePath")?);
                let path = if raw.is_absolute() { raw.to_path_buf() } else { service.root.join(raw) };
                let path = path.canonicalize().map_err(|e| format!("SOURCE_MISSING: save the Blender project first: {e}"))?;
                let root = service.root.canonicalize().map_err(|e| e.to_string())?;
                if !path.starts_with(&root) || !path.is_file() || path.extension().map_or(true, |e| !e.eq_ignore_ascii_case("blend")) { return Err("SOURCE_INVALID: .blend source must be inside this project".into()); }
                job.source_path = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                job.source_bound = true; job.auto_sync = body.get("autoSync").and_then(Value::as_bool).unwrap_or(true);
                job.dependencies = vec![display_path(&path)]; job.observed_fingerprint = None;
                job.message = "Saved source bound; publish to create the engine template".into();
            }
            "publish" => {
                ensure_lease(job, &body)?;
                queue(job)?; launch = true;
            }
            "cancel" => {
                if job.state == "importing" { return Err("INVALID_STATE: atomic publication is committing; cancel after it completes".into()); }
                // Project UI owns the job too; a token is required only when one is supplied.
                if body.get("leaseToken").is_some() { ensure_lease(job, &body)?; }
                job.state = "cancelled".into(); job.stage = "cancelled".into(); job.auto_sync = false;
                job.lease_token = None; job.lease_expires_at = None;
                job.message = "Cancelled; previous published assets remain available".into();
            }
            "retry" => {
                if matches!(job.state.as_str(), "exporting" | "validating" | "importing") { return Err("INVALID_STATE: a publication is already running".into()); }
                job.error = None;
                if job.source_bound { job.auto_sync = true; queue(job)?; launch = true; }
                else { job.state = "awaiting_codex".into(); job.lease_token = None; job.lease_expires_at = None; job.message = "Ready for a new Codex claim".into(); }
            }
            _ => return Err("BAD_PARAMS: unknown Blender job action".into()),
        }
        Ok(())
    });
    match result {
        Ok(job) => {
            let mut value = service.view(&job);
            if let Some(token) = claim_token {
                value["leaseToken"] = json!(token);
            }
            if launch {
                let s = service.clone();
                let id = id.clone();
                tokio::spawn(async move {
                    publish(s, id).await;
                });
            }
            (
                if launch {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::OK
                },
                Json(value),
            )
                .into_response()
        }
        Err(e) => fail(e),
    }
}

fn queue(job: &mut Job) -> BridgeResult<()> {
    if !job.source_bound {
        return Err("SOURCE_REQUIRED: bind the saved Blender source before publishing".into());
    }
    if matches!(job.state.as_str(), "exporting" | "validating" | "importing") {
        return Err("INVALID_STATE: a publication is already running".into());
    }
    job.state = "exporting".into();
    job.stage = "exporting".into();
    job.error = None;
    job.message = "Queued fixed Blender GLB export".into();
    job.dirty_since = None;
    Ok(())
}

fn setup_status(root: &FsPath) -> Value {
    let configured = std::fs::read_to_string(root.join(".codex/config.toml"))
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
        .is_some_and(|doc| {
            doc.get("mcp_servers")
                .and_then(|value| value.as_table())
                .is_some_and(|servers| servers.contains_key("rurix-blender"))
        });
    json!({"mcpConfigured":configured,"skillPath":display_path(&root.join(".agents/skills/blender-production/SKILL.md")),"addonPath":display_path(&root.join(".forge/blender/addons/rurix_forge.py")),"addonEnabled":"unknown"})
}
fn prepare_setup(root: &FsPath, workspace_id: &str) -> BridgeResult<Value> {
    let repository = crate::workspace_root();
    let executable_name = if cfg!(windows) {
        "blender-bridge-mcp.exe"
    } else {
        "blender-bridge-mcp"
    };
    let mcp = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join(executable_name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| repository.join("target/debug").join(executable_name));
    let origin = format!(
        "http://{}",
        std::env::var("FORGE_AGENTD_ADDR").unwrap_or_else(|_| "127.0.0.1:8103".into())
    );
    let section = format!(
        "[mcp_servers.rurix-blender]\ncommand = {}\nargs = {}\n",
        json!(display_path(&mcp)),
        json!(["--origin", origin.as_str(), "--workspace-id", workspace_id])
    );
    let config_path = root.join(".codex/config.toml");
    confine_output(root, &config_path)?;
    let old = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .map_err(|e| format!("SETUP_CONFLICT: cannot read existing project MCP config: {e}"))?
    } else {
        String::new()
    };
    let parsed: toml::Value = toml::from_str(&old).map_err(|e| format!("SETUP_CONFLICT: existing project MCP config could not be parsed, preserved unchanged: {e}"))?;
    let existing = parsed
        .get("mcp_servers")
        .and_then(|value| value.as_table())
        .and_then(|servers| servers.get("rurix-blender"));
    if let Some(existing) = existing {
        let existing = existing
            .as_table()
            .ok_or("SETUP_CONFLICT: rurix-blender MCP entry must be a table")?;
        let args = existing
            .get("args")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
            });
        if existing.get("command").and_then(|value| value.as_str())
            != Some(display_path(&mcp).as_str())
            || args.as_deref()
                != Some(["--origin", origin.as_str(), "--workspace-id", workspace_id].as_slice())
            || existing.get("enabled").and_then(|value| value.as_bool()) == Some(false)
        {
            return Err("SETUP_CONFLICT: project .codex/config.toml already defines a different or disabled rurix-blender MCP server; preserved unchanged".into());
        }
    } else {
        crate::sessions::write_atomic(
            &config_path,
            &format!("{old}\n# RurixForge Blender bridge\n{section}"),
        )
        .map_err(|e| e.to_string())?;
    }
    let skill = root.join(".agents/skills/blender-production/SKILL.md");
    confine_output(root, &skill)?;
    install_managed(
        &repository.join("skills/blender-production/SKILL.md"),
        &skill,
    )?;
    let addon = root.join(".forge/blender/addons/rurix_forge.py");
    confine_output(root, &addon)?;
    install_managed(
        &repository.join("tools/blender/rurix_forge/__init__.py"),
        &addon,
    )?;
    let mut status = setup_status(root);
    status["mcpExecutableFound"] = json!(mcp.is_file());
    status["restartCodexRequired"] = json!(true);
    Ok(status)
}
fn install_managed(source: &FsPath, dest: &FsPath) -> BridgeResult<()> {
    let bytes =
        std::fs::read(source).map_err(|e| format!("SETUP_MISSING: {}: {e}", source.display()))?;
    if dest.is_file() {
        if std::fs::read(dest).map_err(|e| e.to_string())? != bytes {
            return Err(format!(
                "SETUP_CONFLICT: preserve existing customized file {}",
                dest.display()
            ));
        }
    } else {
        std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(dest, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn fingerprint(paths: &[String]) -> BridgeResult<String> {
    let mut hasher = forge_util::hashutil::Sha256Stream::new();
    let mut buffer = [0u8; 65536];
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    for path in &paths {
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        let mut file =
            std::fs::File::open(path).map_err(|e| format!("DEPENDENCY_MISSING: {path}: {e}"))?;
        loop {
            let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        hasher.update(&[0]);
    }
    Ok(hasher.finish())
}

async fn watch(service: Arc<Service>) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        interval.tick().await;
        let jobs: Vec<Job> = service
            .inner
            .lock()
            .unwrap()
            .jobs
            .values()
            .cloned()
            .collect();
        for job in jobs {
            if matches!(job.state.as_str(), "claimed" | "authoring")
                && job.lease_expires_at.is_some_and(|t| t <= now())
            {
                let _ = service.job_update(&job.id, |j| {
                    if j.lease_expires_at.is_some_and(|t| t <= now())
                        && matches!(j.state.as_str(), "claimed" | "authoring")
                    {
                        j.state = "awaiting_codex".into();
                        j.lease_token = None;
                        j.lease_expires_at = None;
                        j.message = "Codex lease expired; claim again to resume".into();
                    }
                    Ok(())
                });
            }
            if !job.auto_sync
                || !job.source_bound
                || job.published.is_none()
                || !matches!(job.state.as_str(), "ready" | "failed")
            {
                continue;
            }
            let paths = job.dependencies.clone();
            let current = match tokio::task::spawn_blocking(move || fingerprint(&paths)).await {
                Ok(Ok(hash)) => hash,
                Ok(Err(error)) => {
                    if job.error.as_deref() != Some(&error) {
                        let _ = service.job_update(&job.id, |j| {
                            if matches!(j.state.as_str(), "ready" | "failed") && j.auto_sync {
                                j.error = Some(error.clone());
                                j.state = "failed".into();
                                j.stage = "watching".into();
                                j.message = error;
                            }
                            Ok(())
                        });
                    }
                    continue;
                }
                Err(_) => continue,
            };
            if job.fingerprint.as_deref() == Some(&current) {
                if job
                    .error
                    .as_deref()
                    .is_some_and(|e| e.starts_with("DEPENDENCY_MISSING:"))
                {
                    let _ = service.job_update(&job.id, |j| {
                        if let Some(error) = j
                            .reload
                            .as_ref()
                            .and_then(|value| value.get("error"))
                            .and_then(Value::as_str)
                        {
                            j.error = Some(format!("ENGINE_RELOAD_FAILED: {error}"));
                            j.state = "failed".into();
                            j.stage = "reload".into();
                            j.message =
                                "Source dependency restored; retry the pending engine reload"
                                    .into();
                        } else {
                            j.error = None;
                            j.state = "ready".into();
                            j.stage = "ready".into();
                            j.message =
                                "Source dependency restored; published template is current".into();
                        }
                        Ok(())
                    });
                }
                continue;
            }
            let mut launch = false;
            let result = service.job_update(&job.id, |j| {
                if !matches!(j.state.as_str(), "ready" | "failed") || !j.auto_sync {
                    return Ok(());
                }
                if j.observed_fingerprint.as_deref() != Some(&current) {
                    j.observed_fingerprint = Some(current);
                    j.dirty_since = Some(now());
                } else if j
                    .dirty_since
                    .is_some_and(|t| now().saturating_sub(t) >= 2000)
                {
                    queue(j)?;
                    launch = true;
                }
                Ok(())
            });
            if result.is_ok() && launch {
                let s = service.clone();
                tokio::spawn(async move {
                    publish(s, job.id).await;
                });
            }
        }
    }
}

async fn engine(root: &FsPath, name: &str, args: Value) -> BridgeResult<Value> {
    let result = crate::mcp::call_tool_in(root, &format!("mcp__engine-scene__{name}"), Some(args))
        .await
        .map_err(|e| e.to_string())?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(format!("ENGINE_ERROR: {result}"));
    }
    if let Some(value) = result.get("structuredContent") {
        return Ok(value.clone());
    }
    if let Some(text) = result.pointer("/content/0/text").and_then(Value::as_str) {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            return Ok(value);
        }
    }
    Ok(result)
}

async fn publish(service: Arc<Service>, id: String) {
    let _lock = service.publishing.lock().await;
    let result = publish_inner(&service, &id).await;
    if let Err(error) = result {
        let _ = service.job_update(&id, |job| {
            if job.state != "cancelled" {
                job.state = "failed".into();
                job.stage = "failed".into();
                job.message = error.clone();
                job.error = Some(error);
                job.dirty_since = None;
            }
            Ok(())
        });
    }
}
async fn publish_inner(service: &Service, id: &str) -> BridgeResult<()> {
    let job = service.job(id)?;
    if job.state == "cancelled" {
        return Ok(());
    }
    let executable = service
        .executable()
        .ok_or("BLENDER_NOT_FOUND: configure an installed Blender executable")?;
    let source = service
        .root
        .join(&job.source_path)
        .canonicalize()
        .map_err(|e| format!("SOURCE_MISSING: {e}"))?;
    if !source.starts_with(service.root.canonicalize().map_err(|e| e.to_string())?) {
        return Err("SOURCE_INVALID: bound source now resolves outside the project".into());
    }
    let input_paths = if job.dependencies.is_empty() {
        vec![display_path(&source)]
    } else {
        job.dependencies.clone()
    };
    let before = fingerprint(&input_paths)?;
    let stage = service
        .root
        .join(".forge/blender/staging")
        .join(id)
        .join(crate::events::new_id("export"));
    confine_output(&service.root, &stage)?;
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let request = stage.join("request.json");
    let project = assetd::project::ForgeProject::load(&service.root).map_err(|e| e.to_string())?;
    let model_path = format!(
        "{}/model.rxmodel",
        assetd::model::package_path(&job.source_id)
    );
    // load_model runs journal recovery before looking up the file. Checking for
    // existence first would miss a committed package temporarily in its backup.
    let committed_revision = match assetd::model::load_model(&project, &model_path) {
        Ok(model) => model.revision,
        Err(error) if error.code == "IO" && !project.content_root().join(&model_path).exists() => 0,
        Err(error) => return Err(error.to_string()),
    };
    let revision = job
        .revision
        .max(committed_revision)
        .checked_add(1)
        .ok_or("REVISION_EXHAUSTED: model revision counter cannot advance")?;
    std::fs::write(&request, serde_json::to_vec(&json!({"sourceBlend":display_path(&source),"outputDirectory":display_path(&stage),"sourceId":job.source_id,"name":job.name,"kind":job.kind,"revision":revision,"idleClip":job.idle_clip,"walkClip":job.walk_clip})).unwrap()).map_err(|e| e.to_string())?;
    let script = crate::workspace_root().join("tools/blender/export_bundle.py");
    if !script.is_file() {
        return Err("EXPORT_SCRIPT_MISSING: fixed Blender export script is not installed".into());
    }
    let log = std::fs::File::create(stage.join("blender.log")).map_err(|e| e.to_string())?;
    let stderr = log.try_clone().map_err(|e| e.to_string())?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args([
            "--background",
            "--factory-startup",
            "--disable-autoexec",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(script)
        .arg("--")
        .arg("--request")
        .arg(&request)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|e| format!("BLENDER_START_FAILED: {e}"))?;
    let started = std::time::Instant::now();
    loop {
        if service.job(id)?.state == "cancelled" {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Ok(());
        }
        if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
            if !exit.success() {
                let log = std::fs::read_to_string(stage.join("blender.log")).unwrap_or_default();
                let tail: String = log
                    .chars()
                    .rev()
                    .take(6000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                return Err(format!("BLENDER_EXPORT_FAILED: {tail}"));
            }
            break;
        }
        if started.elapsed() > Duration::from_secs(EXPORT_SECONDS) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err("BLENDER_TIMEOUT: fixed export exceeded 300 seconds".into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    service.job_update(id, |j| {
        if j.state == "cancelled" {
            return Err("INVALID_STATE: cancelled before validation".into());
        }
        j.state = "validating".into();
        j.stage = "validating".into();
        j.message = "Validating GLB and source revision".into();
        Ok(())
    })?;
    if fingerprint(&input_paths)? != before {
        return Err("SOURCE_CHANGED: source changed during export; save and retry".into());
    }
    let manifest: assetd::model::ModelManifest = serde_json::from_slice(
        &std::fs::read(stage.join("manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if manifest.source_id != job.source_id
        || manifest.revision != revision
        || manifest.kind != job.kind
    {
        return Err("MANIFEST_INVALID: export identity or revision mismatch".into());
    }
    let export: Value = serde_json::from_slice(
        &std::fs::read(stage.join("export-result.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let dependencies: Vec<String> = export["dependencies"]
        .as_array()
        .ok_or("MANIFEST_INVALID: no dependency list")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let fingerprint = fingerprint(&dependencies)?;
    if export["sourceFingerprint"].as_str() != Some(&fingerprint) {
        return Err(
            "SOURCE_CHANGED: an image or linked source changed during export; save and retry"
                .into(),
        );
    }
    service.job_update(id, |j| {
        if j.state == "cancelled" {
            return Err("INVALID_STATE: cancelled before import".into());
        }
        j.state = "importing".into();
        j.stage = "importing".into();
        j.message = "Building engine model and template".into();
        Ok(())
    })?;
    // Cancellation is rejected during this commit phase; it cannot acknowledge
    // success and then have a late importer overwrite assets.
    let root = service.root.clone();
    let glb = stage.join("model.glb");
    let (imported, clips, content_hash) = tokio::task::spawn_blocking(move || {
        let project = assetd::project::ForgeProject::load(&root).map_err(|e| e.to_string())?;
        let imported = assetd::model::import_model_bundle(&project, &glb, &manifest)
            .map_err(|e| e.to_string())?;
        let bundle =
            assetd::model::load_model(&project, &imported.asset_path).map_err(|e| e.to_string())?;
        let clips = bundle
            .animations
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>();
        Ok::<_, String>((imported, clips, bundle.source_hash))
    })
    .await
    .map_err(|e| e.to_string())??;
    let raw = serde_json::to_value(&imported).map_err(|e| e.to_string())?;
    let published = json!({"modelGuid":raw["guid"],"modelPath":raw["assetPath"],"prefabGuid":raw["prefabGuid"],"prefabPath":raw["prefabPath"],"revision":raw["revision"],"clips":clips,"contentHash":content_hash});
    let reload = engine(
        &service.root,
        "asset_reload",
        json!({"guids":raw["assetGuids"],"revision":raw["revision"]}),
    )
    .await;
    service.job_update(id, |j| {
        // A cancellation during import does not hide an already committed asset.
        j.published = Some(published);
        j.revision = imported.revision;
        j.dependencies = dependencies;
        j.fingerprint = Some(fingerprint.clone());
        j.observed_fingerprint = Some(fingerprint);
        j.dirty_since = None;
        if j.state != "cancelled" {
            j.state = "ready".into();
            j.stage = "ready".into();
            j.message =
                "Template published; saved source and texture changes will synchronize".into();
        }
        match reload {
            Ok(value) => {
                j.reload = Some(value);
                j.error = None;
            }
            Err(error) => {
                j.reload = Some(json!({"error":error}));
                j.state = "failed".into();
                j.stage = "reload".into();
                j.message = "Template published; engine reload failed, retry to synchronize".into();
                j.error = Some(format!("ENGINE_RELOAD_FAILED: {error}"));
            }
        }
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let root = std::env::temp_dir().join(crate::events::new_id("blender-test"));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    #[test]
    fn deeplink_encodes_paths_and_prompt_instead_of_executing_text() {
        assert_eq!(
            encode("D:\\My Project&prompt=x"),
            "D%3A%5CMy%20Project%26prompt%3Dx"
        );
        assert_eq!(encode("角色"), "%E8%A7%92%E8%89%B2");
    }
    #[test]
    fn fingerprint_detects_same_length_texture_edits() {
        let root = temp();
        let path = root.join("texture.png");
        std::fs::write(&path, b"first").unwrap();
        let paths = vec![display_path(&path)];
        let first = fingerprint(&paths).unwrap();
        std::fs::write(&path, b"other").unwrap();
        assert_ne!(first, fingerprint(&paths).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_durable_state_is_not_silently_replaced() {
        let root = temp();
        std::fs::create_dir_all(root.join(".forge/blender")).unwrap();
        std::fs::write(root.join(".forge/blender/state.json"), b"broken").unwrap();
        assert!(Service::load(root.clone()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    fn leased_job() -> Job {
        Job {
            id: "test-job".into(),
            source_id: "test-job".into(),
            workspace_id: "default".into(),
            state: "authoring".into(),
            lease_token: Some("secret-token".into()),
            lease_expires_at: Some(now() + LEASE_MS),
            ..Job::default()
        }
    }
    #[test]
    fn wrong_or_expired_lease_cannot_mutate_a_job() {
        let mut job = leased_job();
        assert!(
            ensure_lease(&mut job, &json!({"leaseToken":"another-task"}))
                .unwrap_err()
                .starts_with("LEASE_CONFLICT")
        );
        job.lease_expires_at = Some(now() - 1);
        assert!(
            ensure_lease(&mut job, &json!({"leaseToken":"secret-token"}))
                .unwrap_err()
                .starts_with("LEASE_EXPIRED")
        );
        job.lease_expires_at = Some(now() + 1000);
        ensure_lease(&mut job, &json!({"leaseToken":"secret-token"})).unwrap();
        assert!(job.lease_expires_at.unwrap() > now() + 250000);
    }
    #[test]
    fn restart_reclaims_authoring_and_marks_partial_publication_failed() {
        let root = temp();
        let service = Service::load(root.clone()).unwrap();
        service
            .update(|doc| {
                let job = leased_job();
                doc.jobs.insert(job.id.clone(), job);
                let mut publishing = leased_job();
                publishing.id = "publishing".into();
                publishing.state = "importing".into();
                publishing.published = Some(json!({"revision":1}));
                doc.jobs.insert(publishing.id.clone(), publishing);
                Ok(())
            })
            .unwrap();
        drop(service);
        let recovered = Service::load(root.clone()).unwrap();
        assert_eq!(recovered.job("test-job").unwrap().state, "awaiting_codex");
        assert!(recovered.job("test-job").unwrap().lease_token.is_none());
        let job = recovered.job("publishing").unwrap();
        assert_eq!(job.state, "failed");
        assert_eq!(job.published.unwrap()["revision"], 1);
        drop(recovered);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn job_identity_is_confined_to_each_project_and_lease_is_not_exposed() {
        let first = temp();
        let second = temp();
        let service = Service::load(first.clone()).unwrap();
        let other = Service::load(second.clone()).unwrap();
        let job = leased_job();
        service
            .update(|doc| {
                doc.jobs.insert(job.id.clone(), job.clone());
                Ok(())
            })
            .unwrap();
        assert!(other.job(&job.id).is_err());
        assert!(service.view(&job).get("leaseToken").is_none());
        drop(service);
        drop(other);
        std::fs::remove_dir_all(first).unwrap();
        std::fs::remove_dir_all(second).unwrap();
    }
    #[test]
    fn publication_requires_saved_binding_and_cannot_queue_twice() {
        let mut job = leased_job();
        assert!(queue(&mut job).is_err());
        job.source_bound = true;
        queue(&mut job).unwrap();
        assert_eq!(job.state, "exporting");
        assert!(queue(&mut job).is_err());
    }
    #[test]
    fn customized_setup_files_are_preserved() {
        let root = temp();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        let text = "[mcp_servers.rurix-blender]\ncommand = 'custom-tool'\n";
        std::fs::write(root.join(".codex/config.toml"), text).unwrap();
        assert!(prepare_setup(&root, "default")
            .unwrap_err()
            .starts_with("SETUP_CONFLICT"));
        assert_eq!(
            std::fs::read_to_string(root.join(".codex/config.toml")).unwrap(),
            text
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn setup_is_idempotent_and_preserves_other_mcp_servers() {
        let root = temp();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        let original = "# User configuration\n[mcp_servers.example]\ncommand = 'custom-tool'\nargs = ['keep-me']\n";
        std::fs::write(root.join(".codex/config.toml"), original).unwrap();
        prepare_setup(&root, "workspace-test").unwrap();
        let first = std::fs::read_to_string(root.join(".codex/config.toml")).unwrap();
        assert!(first.starts_with(original));
        prepare_setup(&root, "workspace-test").unwrap();
        assert_eq!(
            first,
            std::fs::read_to_string(root.join(".codex/config.toml")).unwrap()
        );
        assert!(root
            .join(".agents/skills/blender-production/SKILL.md")
            .is_file());
        assert!(root.join(".forge/blender/addons/rurix_forge.py").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn duplicate_engine_process_cannot_lease_the_same_project() {
        let root = temp();
        let first = Service::load(root.clone()).unwrap();
        assert!(Service::load(root.clone()).is_err());
        drop(first);
        assert!(Service::load(root.clone()).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
