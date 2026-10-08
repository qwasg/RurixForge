//! 资料同步引擎(设置命名空间 / 记忆 / 个人技能)与 `/api/forge/account/settings*`、
//! `/api/forge/account/sync` 路由(15_CLOUD_SERVICE.md §3.4、§8.2)。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{CloudError, SyncKind};
use crate::memory::{MemoryEntry, MemoryStore};
use crate::AppState;

const SETTINGS_MAX_BYTES: usize = 64 * 1024;
const MEMORY_PUSH_BATCH: usize = 200;
const SKILL_MAX_BYTES: usize = 2 * 1024 * 1024;
const SYNC_INTERVAL: Duration = Duration::from_secs(60);

/// 个人技能的同步元数据(上次与云端一致时的内容指纹与时间)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSyncMeta {
    pub hash: String,
    pub updated_at: String,
}

/// `data/cloud-sync-state.json`:游标、最近同步结果与个人技能同步账本。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncState {
    pub memory_cursor: i64,
    pub skills_cursor: i64,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    /// 游标所属账号(serverUrl + 用户 id);换账号 → 游标清零重拉。
    pub account: Option<String>,
    /// 已与云端一致的个人技能。
    pub skills: BTreeMap<String, SkillSyncMeta>,
    /// 本地改动时间标记(技能名 → updatedAt),推送时作 LWW 时间戳。
    pub skill_marks: BTreeMap<String, String>,
    /// 本地删除标记(技能名 → 删除时间),推送墓碑用。
    pub skill_tombstones: BTreeMap<String, String>,
}

/// 设置命名空间缓存条目(§8.2 GET /settings 的 items 值 + 待推送标记)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedSetting {
    pub value: Value,
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub pending: bool,
}

/// `data/cloud-settings-cache.json`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsCache {
    pub items: BTreeMap<String, CachedSetting>,
}

/// 同步侧持久状态(AppState.sync)。dir=None 为纯内存(单测)。
pub struct SyncStore {
    dir: Option<PathBuf>,
    state: Mutex<SyncState>,
    settings: Mutex<SettingsCache>,
    nudged: AtomicBool,
    notify: tokio::sync::Notify,
    /// 全量同步串行化(后台循环与 POST /sync 不并发跑)。
    running: tokio::sync::Mutex<()>,
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &std::path::Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

impl SyncStore {
    pub fn load(dir: PathBuf) -> Self {
        let state = read_json(&dir.join("cloud-sync-state.json"));
        let settings = read_json(&dir.join("cloud-settings-cache.json"));
        Self::with(Some(dir), state, settings)
    }

    pub fn ephemeral() -> Self {
        Self::with(None, SyncState::default(), SettingsCache::default())
    }

    fn with(dir: Option<PathBuf>, state: SyncState, settings: SettingsCache) -> Self {
        SyncStore {
            dir,
            state: Mutex::new(state),
            settings: Mutex::new(settings),
            nudged: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
            running: tokio::sync::Mutex::new(()),
        }
    }

    fn write(&self, file: &str, value: &impl Serialize) {
        let Some(dir) = &self.dir else { return };
        match serde_json::to_string_pretty(value) {
            Ok(text) => {
                if let Err(e) = crate::memory::write_atomic(&dir.join(file), &text) {
                    eprintln!("[sync] {file} 写入失败: {e}");
                }
            }
            Err(e) => eprintln!("[sync] {file} 序列化失败: {e}"),
        }
    }

    /// 本地有改动待推送:唤醒后台循环尽快同步。
    pub fn nudge(&self) {
        self.nudged.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }

    pub fn state(&self) -> SyncState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn update_state<R>(&self, f: impl FnOnce(&mut SyncState) -> R) -> R {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let out = f(&mut st);
        self.write("cloud-sync-state.json", &*st);
        out
    }

    pub fn settings(&self) -> SettingsCache {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn update_settings<R>(&self, f: impl FnOnce(&mut SettingsCache) -> R) -> R {
        let mut cache = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        let out = f(&mut cache);
        self.write("cloud-settings-cache.json", &*cache);
        out
    }

    /// 个人技能本地改动(新建/覆盖):记改动时间,下轮推送。
    pub fn mark_skill_dirty(&self, name: &str) {
        let now = crate::events::now_rfc3339();
        self.update_state(|st| {
            st.skill_tombstones.remove(name);
            st.skill_marks.insert(name.to_string(), now);
        });
        self.nudge();
    }

    /// 个人技能本地删除:记墓碑时间,下轮推送 DELETE。
    pub fn mark_skill_deleted(&self, name: &str) {
        let now = crate::events::now_rfc3339();
        self.update_state(|st| {
            st.skill_marks.remove(name);
            st.skill_tombstones.insert(name.to_string(), now);
        });
        self.nudge();
    }
}

/// 同步状态摘要(memory REST 与 account status 共用)。
pub(crate) fn sync_status(state: &AppState) -> Value {
    use crate::cloud::SyncKind;
    let st = state.sync.state();
    json!({
        "settings": state.cloud.sync_enabled(SyncKind::Settings),
        "memory": state.cloud.sync_enabled(SyncKind::Memory),
        "skills": state.cloud.sync_enabled(SyncKind::Skills),
        "lastSyncAt": st.last_sync_at,
        "lastError": st.last_error,
    })
}

fn cloud_err(e: CloudError) -> Response {
    let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({ "error": { "code": e.code, "message": e.message } })),
    )
        .into_response()
}

fn valid_namespace(ns: &str) -> bool {
    let b = ns.as_bytes();
    if b.is_empty() || b.len() > 32 {
        return false;
    }
    if !b[0].is_ascii_lowercase() {
        return false;
    }
    b[1..]
        .iter()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

fn account_key(cloud: &crate::cloud::CloudService) -> Option<String> {
    let url = cloud.server_url();
    let uid = cloud.user_summary()?.get("id")?.as_i64()?;
    Some(format!("{url}#{uid}"))
}

fn wire_setting(entry: &CachedSetting) -> Value {
    json!({
        "value": entry.value,
        "version": entry.version,
        "updatedAt": entry.updated_at,
    })
}

/// 云端 settings 条目并入本地缓存:待推送项不被云端旧版本覆盖。
pub(crate) fn merge_settings_pull(cache: &mut SettingsCache, cloud_items: &Value) {
    let Some(map) = cloud_items.as_object() else {
        return;
    };
    for (ns, raw) in map {
        let Some(obj) = raw.as_object() else {
            continue;
        };
        let version = obj.get("version").and_then(Value::as_i64).unwrap_or(0);
        let updated_at = obj
            .get("updatedAt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let value = obj.get("value").cloned().unwrap_or(Value::Null);
        match cache.items.get(ns) {
            Some(local) if local.pending => {
                if version > local.version {
                    cache.items.insert(
                        ns.clone(),
                        CachedSetting {
                            value,
                            version,
                            updated_at,
                            pending: false,
                        },
                    );
                }
            }
            _ => {
                cache.items.insert(
                    ns.clone(),
                    CachedSetting {
                        value,
                        version,
                        updated_at,
                        pending: false,
                    },
                );
            }
        }
    }
}

async fn get_settings(State(state): State<Arc<AppState>>) -> Response {
    let cache = state.sync.settings();
    if state.cloud.is_logged_in() {
        match state.cloud.call("GET", "/api/v1/me/settings", None).await {
            Ok(v) => {
                state.sync.update_settings(|c| {
                    if let Some(items) = v.get("items") {
                        merge_settings_pull(c, items);
                    }
                });
                let items: Value = state
                    .sync
                    .settings()
                    .items
                    .iter()
                    .map(|(k, e)| (k.clone(), wire_setting(e)))
                    .collect();
                return Json(json!({ "items": items, "source": "cloud" })).into_response();
            }
            Err(e) if e.code == "CLOUD_UNREACHABLE" => {}
            Err(e) => return cloud_err(e),
        }
    }
    let items: Value = cache
        .items
        .iter()
        .map(|(k, e)| (k.clone(), wire_setting(e)))
        .collect();
    Json(json!({ "items": items, "source": "cache" })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PutSettingBody {
    #[serde(default)]
    value: Value,
}

async fn put_setting(
    State(state): State<Arc<AppState>>,
    Path(ns): Path<String>,
    Json(body): Json<PutSettingBody>,
) -> Response {
    if !valid_namespace(&ns) {
        return cloud_err(CloudError::new(
            400,
            "INVALID_NAMESPACE",
            "命名空间需匹配 ^[a-z][a-z0-9_-]{0,31}$",
        ));
    }
    let text = body.value.to_string();
    if body.value.is_null() || text.trim().is_empty() {
        return cloud_err(CloudError::new(400, "INVALID_REQUEST", "value 不能为空"));
    }
    if text.len() > SETTINGS_MAX_BYTES {
        return cloud_err(CloudError::new(
            413,
            "SETTINGS_TOO_LARGE",
            "设置命名空间不能超过 64 KiB",
        ));
    }
    let (base_version, prev_value) = state.sync.update_settings(|c| {
        let base = c.items.get(&ns).map(|e| e.version).unwrap_or(0);
        let prev = c.items.get(&ns).map(|e| e.value.clone());
        let now = crate::events::now_rfc3339();
        c.items.insert(
            ns.clone(),
            CachedSetting {
                value: body.value.clone(),
                version: base,
                updated_at: now,
                pending: true,
            },
        );
        (base, prev)
    });
    let mut pending = true;
    let mut version = base_version;
    let mut updated_at = state
        .sync
        .settings()
        .items
        .get(&ns)
        .map(|e| e.updated_at.clone())
        .unwrap_or_default();
    if state.cloud.is_logged_in() {
        match state
            .cloud
            .call(
                "PUT",
                &format!("/api/v1/me/settings/{ns}"),
                Some(json!({
                    "value": body.value,
                    "baseVersion": base_version,
                    "force": true,
                })),
            )
            .await
        {
            Ok(v) => {
                version = v.get("version").and_then(Value::as_i64).unwrap_or(version);
                updated_at = v
                    .get("updatedAt")
                    .and_then(Value::as_str)
                    .unwrap_or(&updated_at)
                    .to_string();
                pending = false;
                state.sync.update_settings(|c| {
                    if let Some(e) = c.items.get_mut(&ns) {
                        e.version = version;
                        e.updated_at = updated_at.clone();
                        e.pending = false;
                    }
                });
            }
            Err(e) if e.code == "CLOUD_UNREACHABLE" => {}
            Err(e) if e.status == 409 => {
                pending = true;
            }
            Err(e) => return cloud_err(e),
        }
    }
    let _ = prev_value;
    Json(json!({
        "namespace": ns,
        "value": body.value,
        "version": version,
        "updatedAt": updated_at,
        "pending": pending,
    }))
    .into_response()
}

async fn post_sync(State(state): State<Arc<AppState>>) -> Response {
    match run_sync(&state).await {
        Ok(()) => Json(json!({ "ok": true, "sync": sync_status(&state) })).into_response(),
        Err(e) => cloud_err(CloudError::new(502, "SYNC_FAILED", e)),
    }
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/forge/account/settings", get(get_settings))
        .route("/api/forge/account/settings/{ns}", put(put_setting))
        .route("/api/forge/account/sync", post(post_sync))
}

async fn push_pending_settings(state: &AppState) -> Result<(), String> {
    if !state.cloud.sync_enabled(SyncKind::Settings) {
        return Ok(());
    }
    let pending: Vec<(String, CachedSetting)> = state
        .sync
        .settings()
        .items
        .iter()
        .filter(|(_, e)| e.pending)
        .map(|(k, e)| (k.clone(), e.clone()))
        .collect();
    for (ns, entry) in pending {
        match state
            .cloud
            .call(
                "PUT",
                &format!("/api/v1/me/settings/{ns}"),
                Some(json!({
                    "value": entry.value,
                    "baseVersion": entry.version,
                    "force": true,
                })),
            )
            .await
        {
            Ok(v) => {
                let version = v
                    .get("version")
                    .and_then(Value::as_i64)
                    .unwrap_or(entry.version);
                let updated_at = v
                    .get("updatedAt")
                    .and_then(Value::as_str)
                    .unwrap_or(&entry.updated_at)
                    .to_string();
                state.sync.update_settings(|c| {
                    if let Some(e) = c.items.get_mut(&ns) {
                        e.version = version;
                        e.updated_at = updated_at;
                        e.pending = false;
                    }
                });
            }
            Err(e) if e.code == "CLOUD_UNREACHABLE" => return Err(e.message),
            Err(e) if e.status == 409 => {
                // 409 附 current —— call() 已丢附加字段,下轮 GET 拉齐。
                state.sync.nudge();
            }
            Err(e) => return Err(format!("{}: {}", e.code, e.message)),
        }
    }
    Ok(())
}

async fn pull_settings(state: &AppState) -> Result<(), String> {
    if !state.cloud.sync_enabled(SyncKind::Settings) {
        return Ok(());
    }
    let v = state
        .cloud
        .call("GET", "/api/v1/me/settings", None)
        .await
        .map_err(|e| e.message)?;
    state.sync.update_settings(|c| {
        if let Some(items) = v.get("items") {
            merge_settings_pull(c, items);
        }
    });
    Ok(())
}

async fn pull_memories(state: &AppState) -> Result<(), String> {
    if !state.cloud.sync_enabled(SyncKind::Memory) {
        return Ok(());
    }
    let mut cursor = state.sync.state().memory_cursor;
    loop {
        let path = format!("/api/v1/me/memories?since={cursor}&limit=500");
        let v = state
            .cloud
            .call("GET", &path, None)
            .await
            .map_err(|e| e.message)?;
        let remote: Vec<MemoryEntry> = v
            .get("items")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(MemoryEntry::from_cloud)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        state.memory.apply_remote(remote);
        cursor = v.get("cursor").and_then(Value::as_i64).unwrap_or(cursor);
        let has_more = v.get("hasMore").and_then(Value::as_bool).unwrap_or(false);
        state.sync.update_state(|st| st.memory_cursor = cursor);
        if !has_more {
            break;
        }
    }
    Ok(())
}

async fn push_memories(state: &AppState) -> Result<(), String> {
    if !state.cloud.sync_enabled(SyncKind::Memory) {
        return Ok(());
    }
    loop {
        let batch = state.memory.dirty(MEMORY_PUSH_BATCH);
        if batch.is_empty() {
            break;
        }
        let items: Vec<Value> = batch.iter().map(MemoryEntry::to_cloud).collect();
        let v = state
            .cloud
            .call(
                "PUT",
                "/api/v1/me/memories",
                Some(json!({ "items": items })),
            )
            .await
            .map_err(|e| e.message)?;
        let applied: Vec<String> = v
            .get("applied")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let conflicts: Vec<MemoryEntry> = v
            .get("conflicts")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(MemoryEntry::from_cloud)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !conflicts.is_empty() {
            state.memory.apply_remote(conflicts);
        }
        if let Some(c) = v.get("cursor").and_then(Value::as_i64) {
            state
                .sync
                .update_state(|st| st.memory_cursor = st.memory_cursor.max(c));
        }
        state.memory.mark_pushed(&batch, &applied);
    }
    Ok(())
}

fn skill_ts(s: &str) -> i64 {
    crate::memory::parse_rfc3339_micros(s).unwrap_or(i64::MIN)
}

fn pack_personal_skill(name: &str) -> Result<(BTreeMap<String, String>, String, i64), String> {
    let dir = crate::skills::user_skills_root().join(name);
    if !dir.join("SKILL.md").is_file() {
        return Err(format!("个人技能缺少 SKILL.md: {name}"));
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    walk_skill_dir(&dir, &dir, &mut paths)?;
    paths.sort();
    let mut files = BTreeMap::new();
    let mut hasher = forge_util::hashutil::Sha256Stream::new();
    let mut total = 0usize;
    for rel in paths {
        let abs = dir.join(&rel);
        let bytes = std::fs::read(&abs).map_err(|e| e.to_string())?;
        total += bytes.len();
        if total > SKILL_MAX_BYTES {
            return Err("技能包超过 2 MiB".to_string());
        }
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(&bytes);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        files.insert(rel.to_string_lossy().replace('\\', "/"), b64);
    }
    let hash = hasher.finish();
    let updated_at = crate::events::now_rfc3339();
    Ok((files, hash, skill_ts(&updated_at)))
}

fn walk_skill_dir(base: &PathBuf, cur: &PathBuf, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for ent in std::fs::read_dir(cur).map_err(|e| e.to_string())? {
        let ent = ent.map_err(|e| e.to_string())?;
        let path = ent.path();
        let meta = ent.metadata().map_err(|e| e.to_string())?;
        if meta.is_dir() {
            walk_skill_dir(base, &path, out)?;
        } else if meta.is_file() {
            let rel = path.strip_prefix(base).map_err(|e| e.to_string())?;
            let rel_s = rel.to_string_lossy();
            if rel_s.contains("..") {
                return Err("非法路径".to_string());
            }
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}

fn write_personal_skill(name: &str, files: &BTreeMap<String, String>) -> Result<(), String> {
    if !files.contains_key("SKILL.md") {
        return Err("缺少 SKILL.md".to_string());
    }
    let root = crate::skills::user_skills_root().join(name);
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let mut total = 0usize;
    for (rel, b64) in files {
        if rel.contains("..") || rel.starts_with('/') {
            return Err("非法路径".to_string());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| e.to_string())?;
        total += bytes.len();
        if total > SKILL_MAX_BYTES {
            return Err("技能包超过 2 MiB".to_string());
        }
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn copy_skill_conflict(name: &str) -> Result<(), String> {
    let src = crate::skills::user_skills_root().join(name);
    if !src.is_dir() {
        return Ok(());
    }
    let stamp = crate::events::now_rfc3339()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(14)
        .collect::<String>();
    let dst = crate::skills::user_skills_root().join(format!("{name}-conflict-{stamp}"));
    copy_dir_all(&src, &dst).map_err(|e| e.to_string())
}

fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for ent in std::fs::read_dir(src)? {
        let ent = ent?;
        let ty = ent.file_type()?;
        let to = dst.join(ent.file_name());
        if ty.is_dir() {
            copy_dir_all(&ent.path(), &to)?;
        } else {
            std::fs::copy(ent.path(), to)?;
        }
    }
    Ok(())
}

async fn pull_skills(state: &AppState) -> Result<(), String> {
    if !state.cloud.sync_enabled(SyncKind::Skills) {
        return Ok(());
    }
    let mut cursor = state.sync.state().skills_cursor;
    loop {
        let path = format!("/api/v1/me/skills?since={cursor}&limit=50");
        let v = state
            .cloud
            .call("GET", &path, None)
            .await
            .map_err(|e| e.message)?;
        if let Some(items) = v.get("items").and_then(Value::as_array) {
            for raw in items {
                let Some(name) = raw.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let remote_at = raw
                    .get("updatedAt")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let deleted = raw.get("deleted").and_then(Value::as_bool).unwrap_or(false);
                let remote_ts = skill_ts(&remote_at);
                let sync_snap = state.sync.state();
                let local_mark = sync_snap.skill_marks.get(name).cloned();
                let local_ts = local_mark.as_deref().map(skill_ts).unwrap_or(i64::MIN);
                let synced_ts = sync_snap
                    .skills
                    .get(name)
                    .map(|m| skill_ts(&m.updated_at))
                    .unwrap_or(i64::MIN);
                if remote_ts >= local_ts.max(synced_ts) {
                    if deleted {
                        let dir = crate::skills::user_skills_root().join(name);
                        if dir.is_dir() {
                            let _ = std::fs::remove_dir_all(dir);
                        }
                        state.sync.update_state(|st| {
                            st.skills.remove(name);
                            st.skill_marks.remove(name);
                            st.skill_tombstones.remove(name);
                        });
                    } else if let Some(files_obj) = raw.get("files").and_then(Value::as_object) {
                        let mut files = BTreeMap::new();
                        for (k, v) in files_obj {
                            if let Some(s) = v.as_str() {
                                files.insert(k.clone(), s.to_string());
                            }
                        }
                        if local_mark.is_some() && remote_ts > local_ts {
                            let _ = copy_skill_conflict(name);
                        }
                        write_personal_skill(name, &files)?;
                        let hash = raw
                            .get("sha256")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        state.sync.update_state(|st| {
                            st.skills.insert(
                                name.to_string(),
                                SkillSyncMeta {
                                    hash,
                                    updated_at: remote_at.clone(),
                                },
                            );
                            st.skill_marks.remove(name);
                            st.skill_tombstones.remove(name);
                        });
                    }
                }
            }
        }
        cursor = v.get("cursor").and_then(Value::as_i64).unwrap_or(cursor);
        let has_more = v.get("hasMore").and_then(Value::as_bool).unwrap_or(false);
        state.sync.update_state(|st| st.skills_cursor = cursor);
        if !has_more {
            break;
        }
    }
    Ok(())
}

async fn push_skills(state: &AppState) -> Result<(), String> {
    if !state.cloud.sync_enabled(SyncKind::Skills) {
        return Ok(());
    }
    let marks: Vec<(String, String)> = state
        .sync
        .state()
        .skill_marks
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (name, updated_at) in marks {
        let (files, hash, _) = match pack_personal_skill(&name) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[sync] 打包个人技能 {name} 失败: {e}");
                continue;
            }
        };
        let v = state
            .cloud
            .call(
                "PUT",
                &format!("/api/v1/me/skills/{name}"),
                Some(json!({ "files": files, "updatedAt": updated_at })),
            )
            .await
            .map_err(|e| e.message)?;
        let applied = v.get("applied").and_then(Value::as_bool).unwrap_or(false);
        if applied {
            state.sync.update_state(|st| {
                st.skills.insert(
                    name.clone(),
                    SkillSyncMeta {
                        hash,
                        updated_at: updated_at.clone(),
                    },
                );
                st.skill_marks.remove(&name);
            });
        } else if let Some(skill) = v.get("skill") {
            let _ = copy_skill_conflict(&name);
            if let Some(files_obj) = skill.get("files").and_then(Value::as_object) {
                let mut files = BTreeMap::new();
                for (k, v) in files_obj {
                    if let Some(s) = v.as_str() {
                        files.insert(k.clone(), s.to_string());
                    }
                }
                let _ = write_personal_skill(&name, &files);
            }
            let remote_at = skill
                .get("updatedAt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let remote_hash = skill
                .get("sha256")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            state.sync.update_state(|st| {
                st.skills.insert(
                    name.clone(),
                    SkillSyncMeta {
                        hash: remote_hash,
                        updated_at: remote_at,
                    },
                );
                st.skill_marks.remove(&name);
            });
        }
        if let Some(c) = v.get("cursor").and_then(Value::as_i64) {
            state
                .sync
                .update_state(|st| st.skills_cursor = st.skills_cursor.max(c));
        }
    }
    let tombs: Vec<(String, String)> = state
        .sync
        .state()
        .skill_tombstones
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (name, updated_at) in tombs {
        let path = format!(
            "/api/v1/me/skills/{name}?updatedAt={}",
            urlencoding(&updated_at)
        );
        let v = state
            .cloud
            .call("DELETE", &path, None)
            .await
            .map_err(|e| e.message)?;
        if v.get("applied").and_then(Value::as_bool).unwrap_or(false) {
            state.sync.update_state(|st| {
                st.skill_tombstones.remove(&name);
                st.skills.remove(&name);
            });
        }
    }
    Ok(())
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

fn ensure_account(state: &AppState) {
    let key = account_key(&state.cloud);
    state.sync.update_state(|st| {
        if st.account.as_deref() != key.as_deref() {
            st.account = key;
            st.memory_cursor = 0;
            st.skills_cursor = 0;
            st.skills.clear();
            st.skill_marks.clear();
            st.skill_tombstones.clear();
        }
    });
}

/// 全量同步(串行;后台循环与 POST /sync 共用)。
pub async fn run_sync(state: &AppState) -> Result<(), String> {
    if !state.cloud.is_logged_in() {
        return Err("尚未登录".to_string());
    }
    ensure_account(state);
    push_pending_settings(state).await?;
    pull_settings(state).await?;
    pull_memories(state).await?;
    push_memories(state).await?;
    pull_skills(state).await?;
    push_skills(state).await?;
    state.sync.update_state(|st| {
        st.last_sync_at = Some(crate::events::now_rfc3339());
        st.last_error = None;
    });
    Ok(())
}

/// 启动后台同步循环(登录后增量、定时 + nudge);未登录时空转。
pub(crate) fn start(state: &Arc<AppState>) {
    let me = Arc::clone(state);
    tokio::spawn(async move {
        let mut last_gen = me.cloud.login_generation();
        loop {
            if me.cloud.is_logged_in() {
                let _guard = me.sync.running.lock().await;
                let gen = me.cloud.login_generation();
                if gen != last_gen {
                    ensure_account(&me);
                    last_gen = gen;
                }
                if let Err(e) = run_sync(&me).await {
                    me.sync.update_state(|st| st.last_error = Some(e));
                }
            } else {
                last_gen = me.cloud.login_generation();
            }
            tokio::select! {
                _ = tokio::time::sleep(SYNC_INTERVAL) => {}
                _ = me.sync.notify.notified() => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pull_does_not_clobber_pending_local() {
        let mut cache = SettingsCache::default();
        cache.items.insert(
            "agent".into(),
            CachedSetting {
                value: json!({ "mode": "local" }),
                version: 2,
                updated_at: "2026-01-01T00:00:00Z".into(),
                pending: true,
            },
        );
        merge_settings_pull(
            &mut cache,
            &json!({
                "agent": { "value": { "mode": "cloud" }, "version": 1, "updatedAt": "2026-01-02T00:00:00Z" }
            }),
        );
        assert_eq!(cache.items["agent"].value["mode"], "local");
        assert!(cache.items["agent"].pending);
        merge_settings_pull(
            &mut cache,
            &json!({
                "agent": { "value": { "mode": "cloud-v3" }, "version": 3, "updatedAt": "2026-01-03T00:00:00Z" }
            }),
        );
        assert_eq!(cache.items["agent"].value["mode"], "cloud-v3");
        assert!(!cache.items["agent"].pending);
    }

    #[test]
    fn memory_lww_via_apply_remote() {
        let store = MemoryStore::ephemeral();
        let id = "11111111-1111-4111-8111-111111111111";
        store.apply_remote(vec![MemoryEntry {
            id: id.into(),
            scope: "global".into(),
            kind: "fact".into(),
            content: "本地较新".into(),
            tags: vec![],
            created_at: "2026-09-26T10:00:00Z".into(),
            updated_at: "2026-09-26T12:00:00.123Z".into(),
            deleted: false,
            source: "user".into(),
            dirty: false,
        }]);
        store.apply_remote(vec![MemoryEntry {
            id: id.into(),
            scope: "global".into(),
            kind: "fact".into(),
            content: "云端旧".into(),
            tags: vec![],
            created_at: "2026-09-26T10:00:00Z".into(),
            updated_at: "2026-09-26T11:00:00Z".into(),
            deleted: false,
            source: "user".into(),
            dirty: false,
        }]);
        assert_eq!(store.active()[0].content, "本地较新");
        store.apply_remote(vec![MemoryEntry {
            id: id.into(),
            scope: "global".into(),
            kind: "fact".into(),
            content: "云端更新".into(),
            tags: vec![],
            created_at: "2026-09-26T10:00:00Z".into(),
            updated_at: "2026-09-26T13:00:00Z".into(),
            deleted: false,
            source: "user".into(),
            dirty: false,
        }]);
        assert_eq!(store.active()[0].content, "云端更新");
    }
}
