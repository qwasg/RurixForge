//! 记忆系统(15_CLOUD_SERVICE.md §8.3):本地存储 `data/agent-memory.json`、
//! `memory_write` / `memory_search` / `memory_delete` 工具、每轮 Top-N 注入、`/api/forge/memory` 路由。
//! 云同步(脏标记推送、LWW 合并)在 [crate::cloud::sync],本模块只提供存储侧钩子。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::events::EventDraft;
use crate::scope::ScopeProject;
use crate::AppState;

pub const MEMORY_WRITE_TOOL: &str = "memory_write";
pub const MEMORY_SEARCH_TOOL: &str = "memory_search";
pub const MEMORY_DELETE_TOOL: &str = "memory_delete";
pub const TOOL_NAMES: &[&str] = &[MEMORY_WRITE_TOOL, MEMORY_SEARCH_TOOL, MEMORY_DELETE_TOOL];

/// 单条记忆正文上限(与云端 §3.4 同口径,按 UTF-8 字节)。
pub const CONTENT_MAX_BYTES: usize = 8 * 1024;
/// 每轮注入条数。
pub const INJECT_TOP_N: usize = 8;
const INJECT_ENTRY_MAX_CHARS: usize = 500;
const SEARCH_DEFAULT_LIMIT: usize = 10;
const SEARCH_MAX_LIMIT: usize = 50;
const TAGS_MAX: usize = 16;
const TAG_MAX_CHARS: usize = 32;
/// 云端 scope 上限 200 字符(含 `project:` 前缀)。
const PROJECT_KEY_MAX_CHARS: usize = 120;
pub const KINDS: &[&str] = &["preference", "fact", "convention"];
pub const SCOPE_GLOBAL: &str = "global";

pub fn is_tool(name: &str) -> bool {
    TOOL_NAMES.contains(&name)
}

// ---------- 通用小工具(sync 共用) ----------

/// UUID v4 形态的随机 id(无 uuid/rand 依赖:RandomState 随机种子 + 计数器 + 纳秒)。
pub fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut words = [0u64; 2];
    for w in &mut words {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        *w = h.finish();
    }
    let mut b = ((words[0] as u128) << 64) | words[1] as u128;
    b = (b & !(0xF << 76)) | (0x4 << 76);
    b = (b & !(0x3 << 62)) | (0x2 << 62);
    let hex = format!("{b:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// days since 1970-01-01(公历,Howard Hinnant days_from_civil)。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// RFC3339 → Unix 微秒(接受 Z / ±HH:MM 与任意位小数)。LWW 比较必须按时间而非字符串:
/// 云端回 `…00Z`、本地写 `…00.123Z`,字典序会把前者排在后面。
pub fn parse_rfc3339_micros(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    let num = |from: usize, to: usize| -> Option<i64> {
        let part = s.get(from..to)?;
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let mut i = 19;
    let mut micros = 0i64;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
        let digits = &s[start..(start + 6).min(i)];
        micros = digits.parse::<i64>().ok()? * 10i64.pow(6 - digits.len() as u32);
    }
    let offset = match b.get(i) {
        Some(b'Z') | Some(b'z') => {
            i += 1;
            0
        }
        Some(&c) if c == b'+' || c == b'-' => {
            if b.get(i + 3) != Some(&b':') {
                return None;
            }
            let secs = num(i + 1, i + 3)? * 3600 + num(i + 4, i + 6)? * 60;
            i += 6;
            if c == b'+' {
                secs
            } else {
                -secs
            }
        }
        _ => return None,
    };
    if i != b.len()
        || !(1..=12).contains(&mo)
        || !(1..=31).contains(&d)
        || h > 23
        || mi > 59
        || sec > 60
    {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset;
    Some(secs * 1_000_000 + micros)
}

fn now_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// 原子写:同目录 tmp 全量写 + rename。
pub(crate) fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|n| format!("{}.tmp", n.to_string_lossy()))
        .unwrap_or_else(|| "state.tmp".to_string());
    let tmp = path.with_file_name(name);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

// ---------- 存储 ----------

/// 本地记忆条目(`agent-memory.json` 落盘形态)。`deleted` 为墓碑,`dirty` = 待推送云端。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id: String,
    pub scope: String,
    pub kind: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub dirty: bool,
}

fn default_source() -> String {
    "user".to_string()
}

impl MemoryEntry {
    /// §8.3 LocalMemory(不含内部字段 deleted/dirty)。
    pub fn to_wire(&self) -> Value {
        json!({
            "id": self.id,
            "scope": self.scope,
            "kind": self.kind,
            "content": self.content,
            "tags": self.tags,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
            "source": self.source,
        })
    }

    /// §3.4 PUT /api/v1/me/memories 条目。
    pub fn to_cloud(&self) -> Value {
        json!({
            "id": self.id,
            "scope": self.scope,
            "kind": self.kind,
            "content": self.content,
            "tags": self.tags,
            "updatedAt": self.updated_at,
            "deleted": self.deleted,
        })
    }

    /// 云端 Memory → 本地条目(来源一律记 user:云端不区分来源)。
    pub fn from_cloud(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let updated_at = s("updatedAt")?;
        Some(MemoryEntry {
            id: s("id").filter(|i| !i.is_empty())?,
            scope: s("scope").unwrap_or_else(|| SCOPE_GLOBAL.to_string()),
            kind: s("kind")
                .filter(|k| KINDS.contains(&k.as_str()))
                .unwrap_or_else(|| "fact".to_string()),
            content: s("content").unwrap_or_default(),
            tags: v
                .get("tags")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            created_at: updated_at.clone(),
            updated_at,
            deleted: v.get("deleted").and_then(Value::as_bool).unwrap_or(false),
            source: "user".to_string(),
            dirty: false,
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum MemoryError {
    Invalid(String),
    TooLarge,
    NotFound(String),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::Invalid(m) => write!(f, "MEMORY_INVALID: {m}"),
            MemoryError::TooLarge => write!(f, "MEMORY_TOO_LARGE: 单条记忆不能超过 8 KiB"),
            MemoryError::NotFound(id) => write!(f, "MEMORY_NOT_FOUND: 记忆不存在: {id}"),
        }
    }
}

impl MemoryError {
    fn response(&self) -> Response {
        let (status, code, message) = match self {
            MemoryError::Invalid(m) => (StatusCode::BAD_REQUEST, "MEMORY_INVALID", m.clone()),
            MemoryError::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "MEMORY_TOO_LARGE",
                "单条记忆不能超过 8 KiB".to_string(),
            ),
            MemoryError::NotFound(id) => (
                StatusCode::NOT_FOUND,
                "MEMORY_NOT_FOUND",
                format!("记忆不存在: {id}"),
            ),
        };
        (
            status,
            Json(json!({ "error": { "code": code, "message": message } })),
        )
            .into_response()
    }
}

/// 新建入参(scope 已解析成 `global` / `project:<key>`)。
pub struct NewMemory {
    pub content: String,
    pub kind: Option<String>,
    pub scope: String,
    pub tags: Vec<String>,
    pub source: &'static str,
}

#[derive(Debug, Default)]
pub struct MemoryPatch {
    pub content: Option<String>,
    pub kind: Option<String>,
    pub tags: Option<Vec<String>>,
}

fn validate_content(content: &str) -> Result<String, MemoryError> {
    let c = content.trim();
    if c.is_empty() {
        return Err(MemoryError::Invalid("content 不可空".to_string()));
    }
    if c.len() > CONTENT_MAX_BYTES {
        return Err(MemoryError::TooLarge);
    }
    Ok(c.to_string())
}

fn validate_kind(kind: Option<&str>) -> Result<String, MemoryError> {
    match kind.map(str::trim).filter(|k| !k.is_empty()) {
        None => Ok("fact".to_string()),
        Some(k) if KINDS.contains(&k) => Ok(k.to_string()),
        Some(k) => Err(MemoryError::Invalid(format!(
            "kind 只能是 preference/fact/convention: {k}"
        ))),
    }
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t: String = t.trim().chars().take(TAG_MAX_CHARS).collect();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
        if out.len() == TAGS_MAX {
            break;
        }
    }
    out
}

#[derive(Default, Serialize, Deserialize)]
struct MemoryFile {
    #[serde(default)]
    items: Vec<MemoryEntry>,
}

/// 记忆存储:内存 Vec + 整文件读-改-写(Mutex 串行化)。path=None 为纯内存(单测)。
pub struct MemoryStore {
    path: Option<PathBuf>,
    inner: Mutex<Vec<MemoryEntry>>,
}

impl MemoryStore {
    pub fn load(path: PathBuf) -> Self {
        let items = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| {
                serde_json::from_str::<MemoryFile>(&t)
                    .map_err(|e| eprintln!("agent-memory.json 损坏({e}),按空库处理"))
                    .ok()
            })
            .map(|f| f.items)
            .unwrap_or_default();
        MemoryStore {
            path: Some(path),
            inner: Mutex::new(items),
        }
    }

    pub fn ephemeral() -> Self {
        MemoryStore {
            path: None,
            inner: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<MemoryEntry>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, items: &[MemoryEntry]) {
        let Some(path) = &self.path else { return };
        let file = MemoryFile {
            items: items.to_vec(),
        };
        match serde_json::to_string_pretty(&file) {
            Ok(text) => {
                if let Err(e) = write_atomic(path, &text) {
                    eprintln!("agent-memory.json 写入失败: {e}");
                }
            }
            Err(e) => eprintln!("记忆序列化失败: {e}"),
        }
    }

    /// 全部有效条目(不含墓碑)。
    pub fn active(&self) -> Vec<MemoryEntry> {
        self.lock().iter().filter(|e| !e.deleted).cloned().collect()
    }

    pub fn get(&self, id: &str) -> Option<MemoryEntry> {
        self.lock()
            .iter()
            .find(|e| e.id == id && !e.deleted)
            .cloned()
    }

    pub fn create(&self, new: NewMemory) -> Result<MemoryEntry, MemoryError> {
        let content = validate_content(&new.content)?;
        let kind = validate_kind(new.kind.as_deref())?;
        let now = crate::events::now_rfc3339();
        let entry = MemoryEntry {
            id: new_uuid(),
            scope: new.scope,
            kind,
            content,
            tags: normalize_tags(&new.tags),
            created_at: now.clone(),
            updated_at: now,
            deleted: false,
            source: new.source.to_string(),
            dirty: true,
        };
        let mut items = self.lock();
        items.push(entry.clone());
        self.persist(&items);
        Ok(entry)
    }

    pub fn update(&self, id: &str, patch: MemoryPatch) -> Result<MemoryEntry, MemoryError> {
        let content = patch.content.as_deref().map(validate_content).transpose()?;
        let kind = match patch.kind.as_deref() {
            Some(k) => Some(validate_kind(Some(k))?),
            None => None,
        };
        let mut items = self.lock();
        let entry = items
            .iter_mut()
            .find(|e| e.id == id && !e.deleted)
            .ok_or_else(|| MemoryError::NotFound(id.to_string()))?;
        if let Some(c) = content {
            entry.content = c;
        }
        if let Some(k) = kind {
            entry.kind = k;
        }
        if let Some(t) = patch.tags {
            entry.tags = normalize_tags(&t);
        }
        entry.updated_at = crate::events::now_rfc3339();
        entry.dirty = true;
        let out = entry.clone();
        self.persist(&items);
        Ok(out)
    }

    /// 删除 = 墓碑(正文清空;推送云端后由同步侧清掉)。
    pub fn delete(&self, id: &str) -> Result<MemoryEntry, MemoryError> {
        let mut items = self.lock();
        let entry = items
            .iter_mut()
            .find(|e| e.id == id && !e.deleted)
            .ok_or_else(|| MemoryError::NotFound(id.to_string()))?;
        entry.deleted = true;
        entry.content.clear();
        entry.tags.clear();
        entry.updated_at = crate::events::now_rfc3339();
        entry.dirty = true;
        let out = entry.clone();
        self.persist(&items);
        Ok(out)
    }

    // ---------- 同步钩子 ----------

    /// 待推送条目(含墓碑),最多 limit 条。
    pub fn dirty(&self, limit: usize) -> Vec<MemoryEntry> {
        self.lock()
            .iter()
            .filter(|e| e.dirty)
            .take(limit)
            .cloned()
            .collect()
    }

    /// 推送结果回写:applied 中且推送后未再改动的条目清脏;已推送的墓碑就地清除。
    pub fn mark_pushed(&self, pushed: &[MemoryEntry], applied: &[String]) {
        let mut items = self.lock();
        let mut changed = false;
        for p in pushed.iter().filter(|p| applied.contains(&p.id)) {
            if let Some(e) = items
                .iter_mut()
                .find(|e| e.id == p.id && e.updated_at == p.updated_at)
            {
                e.dirty = false;
                changed = true;
            }
        }
        let before = items.len();
        items.retain(|e| !(e.deleted && !e.dirty));
        if changed || items.len() != before {
            self.persist(&items);
        }
    }

    /// 合并云端条目(LWW:updatedAt 更新者胜,相等以云端为准以便收敛)。返回本地被改动的条数。
    /// 本地更新的条目保持 dirty,等下轮推送;云端墓碑命中本地有效条目时删除本地。
    pub fn apply_remote(&self, remote: Vec<MemoryEntry>) -> usize {
        let mut items = self.lock();
        let mut changed = 0usize;
        for r in remote {
            let r_ts = parse_rfc3339_micros(&r.updated_at).unwrap_or(i64::MIN);
            match items.iter_mut().find(|e| e.id == r.id) {
                Some(local) => {
                    let l_ts = parse_rfc3339_micros(&local.updated_at).unwrap_or(i64::MIN);
                    if l_ts > r_ts {
                        if !local.dirty {
                            local.dirty = true;
                            changed += 1;
                        }
                        continue;
                    }
                    if local.content == r.content
                        && local.kind == r.kind
                        && local.scope == r.scope
                        && local.tags == r.tags
                        && local.deleted == r.deleted
                        && local.updated_at == r.updated_at
                        && !local.dirty
                    {
                        continue;
                    }
                    let created_at = std::mem::take(&mut local.created_at);
                    let source = std::mem::take(&mut local.source);
                    *local = MemoryEntry {
                        created_at: if created_at.is_empty() {
                            r.updated_at.clone()
                        } else {
                            created_at
                        },
                        source,
                        dirty: false,
                        ..r
                    };
                    changed += 1;
                }
                None if r.deleted => {}
                None => {
                    items.push(MemoryEntry { dirty: false, ..r });
                    changed += 1;
                }
            }
        }
        let before = items.len();
        items.retain(|e| !(e.deleted && !e.dirty));
        if changed > 0 || items.len() != before {
            self.persist(&items);
        }
        changed
    }
}

// ---------- 项目键 ----------

/// 项目键归一化:小写、去首尾空白、内部空白折成 `-`,截断到云端 scope 上限内。
pub fn normalize_key(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    let joined = lower.split_whitespace().collect::<Vec<_>>().join("-");
    let key: String = joined.chars().take(PROJECT_KEY_MAX_CHARS).collect();
    if key.is_empty() {
        "default".to_string()
    } else {
        key
    }
}

/// 项目键(跨设备稳定):forge.toml 的项目名优先,没有清单退回工作区名。
/// 不用路径或工作区 id——两台机器上它们天然不同,同一个项目的记忆就对不上了。
pub fn project_key(p: &ScopeProject) -> String {
    let manifest_name = if p.project_root.join("forge.toml").is_file() {
        assetd::project::ForgeProject::load(&p.project_root)
            .ok()
            .map(|f| f.name)
            .filter(|n| !n.trim().is_empty())
    } else {
        None
    };
    normalize_key(&manifest_name.unwrap_or_else(|| p.name.clone()))
}

pub fn project_scope(p: &ScopeProject) -> String {
    format!("project:{}", project_key(p))
}

/// 工具/REST 的 scope 入参(global|project,缺省按 kind:偏好默认全局,其余默认本项目)。
fn resolve_scope(
    arg: Option<&str>,
    kind: &str,
    project_scope: &str,
) -> Result<String, MemoryError> {
    match arg.map(str::trim).filter(|s| !s.is_empty()) {
        Some(SCOPE_GLOBAL) => Ok(SCOPE_GLOBAL.to_string()),
        Some("project") => Ok(project_scope.to_string()),
        Some(s) if s == project_scope => Ok(s.to_string()),
        Some(s) => Err(MemoryError::Invalid(format!(
            "scope 只能是 global 或 project: {s}"
        ))),
        None if kind == "preference" => Ok(SCOPE_GLOBAL.to_string()),
        None => Ok(project_scope.to_string()),
    }
}

// ---------- 相关度 ----------

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

fn flush_word(word: &mut String, out: &mut HashSet<String>) {
    if word.chars().count() >= 2 {
        out.insert(std::mem::take(word));
    } else {
        word.clear();
    }
}

fn flush_cjk(run: &mut Vec<char>, out: &mut HashSet<String>) {
    if run.len() == 1 {
        out.insert(run[0].to_string());
    }
    for pair in run.windows(2) {
        out.insert(pair.iter().collect());
    }
    run.clear();
}

/// 词项:ASCII 词(小写,≥2 字符)+ CJK 字二元组(孤立单字按单字计)。
pub fn terms(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut word = String::new();
    let mut run: Vec<char> = Vec::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            flush_cjk(&mut run, &mut out);
            word.push(c.to_ascii_lowercase());
        } else if is_cjk(c) {
            flush_word(&mut word, &mut out);
            run.push(c);
        } else {
            flush_word(&mut word, &mut out);
            flush_cjk(&mut run, &mut out);
        }
    }
    flush_word(&mut word, &mut out);
    flush_cjk(&mut run, &mut out);
    out
}

/// (词面重合度 0..1, 综合分)。综合分 = 重合度 + 类别权重 + 时近度 + 本项目加成。
fn score(
    entry: &MemoryEntry,
    query: &HashSet<String>,
    now_us: i64,
    project_scope: &str,
) -> (f64, f64) {
    let mut mt = terms(&entry.content);
    for t in &entry.tags {
        mt.extend(terms(t));
    }
    let lexical = if query.is_empty() {
        0.0
    } else {
        query.intersection(&mt).count() as f64 / query.len() as f64
    };
    let kind_weight = match entry.kind.as_str() {
        "preference" => 0.3,
        "convention" => 0.25,
        _ => 0.0,
    };
    let age_days = parse_rfc3339_micros(&entry.updated_at)
        .map(|t| (now_us - t).max(0) as f64 / 86_400e6)
        .unwrap_or(365.0);
    let recency = 0.2 * (-age_days / 30.0).exp();
    let scope_bonus = if entry.scope == project_scope {
        0.1
    } else {
        0.0
    };
    (lexical, lexical + kind_weight + recency + scope_bonus)
}

fn rank(mut scored: Vec<(f64, f64, MemoryEntry)>, by_lexical: bool) -> Vec<MemoryEntry> {
    scored.sort_by(|a, b| {
        let key = |x: &(f64, f64, MemoryEntry)| if by_lexical { (x.0, x.1) } else { (x.1, x.0) };
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.2.updated_at.cmp(&a.2.updated_at))
    });
    scored.into_iter().map(|(_, _, e)| e).collect()
}

/// 本轮注入的 Top-N:仅全局与当前项目两个作用域。
pub fn select_for_turn(
    store: &MemoryStore,
    project_scope: &str,
    user_text: &str,
    n: usize,
) -> Vec<MemoryEntry> {
    let query = terms(user_text);
    let now = now_micros();
    let scored = store
        .active()
        .into_iter()
        .filter(|e| e.scope == SCOPE_GLOBAL || e.scope == project_scope)
        .map(|e| {
            let (lex, total) = score(&e, &query, now, project_scope);
            (lex, total, e)
        })
        .collect();
    rank(scored, false).into_iter().take(n).collect()
}

/// 检索:有查询词时只返回词面命中的条目(按重合度排),无查询词按综合分(近期优先)。
pub fn search(store: &MemoryStore, scopes: &[&str], query: &str, limit: usize) -> Vec<MemoryEntry> {
    let q = terms(query);
    let needle = query.trim().to_lowercase();
    let now = now_micros();
    let project = scopes
        .iter()
        .find(|s| s.starts_with("project:"))
        .copied()
        .unwrap_or("");
    let scored = store
        .active()
        .into_iter()
        .filter(|e| scopes.is_empty() || scopes.contains(&e.scope.as_str()))
        .filter_map(|e| {
            let (lex, total) = score(&e, &q, now, project);
            let hit = needle.is_empty() || lex > 0.0 || e.content.to_lowercase().contains(&needle);
            hit.then_some((lex, total, e))
        })
        .collect();
    rank(scored, !needle.is_empty())
        .into_iter()
        .take(limit)
        .collect()
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "preference" => "偏好",
        "convention" => "约定",
        _ => "事实",
    }
}

fn one_line(s: &str, max: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= max {
        return flat.to_string();
    }
    flat.chars().take(max).collect::<String>() + "…"
}

/// 「## 用户记忆」段(无条目返回 None)。with_tools=false 用于无记忆工具的面(ask 模式 / Codex)。
pub fn format_section(entries: &[MemoryEntry], with_tools: bool) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let mut s = String::from(
        "\n\n## 用户记忆\n以下是跨会话记住的用户偏好、项目约定与事实(按与本轮请求的相关度排序)。\
相关时遵循;与用户本轮的明确要求冲突时,以本轮要求为准。",
    );
    if with_tools {
        s.push_str(
            "发现新的长期偏好/约定时用 memory_write 记下;条目过时或有误时用 memory_delete(id) 删除,\
不要记录一次性的任务细节或密钥。",
        );
    }
    s.push('\n');
    for e in entries {
        let scope = if e.scope == SCOPE_GLOBAL {
            "全局"
        } else {
            "本项目"
        };
        s.push_str(&format!(
            "- [{}·{scope}] {} (id: {})\n",
            kind_label(&e.kind),
            one_line(&e.content, INJECT_ENTRY_MAX_CHARS),
            e.id
        ));
    }
    Some(s)
}

/// 本轮系统提示的记忆段(execute_turn / Codex developerInstructions 共用)。
pub fn prompt_section(
    store: &MemoryStore,
    project: &ScopeProject,
    user_text: &str,
    with_tools: bool,
) -> Option<String> {
    let entries = select_for_turn(store, &project_scope(project), user_text, INJECT_TOP_N);
    format_section(&entries, with_tools)
}

// ---------- 工具 ----------

fn tool_spec(name: &str, desc: &str, params: Value) -> Value {
    json!({ "type": "function", "function": { "name": name, "description": desc, "parameters": params } })
}

/// memory_* 工具 OpenAI spec(engine::runtime_tool_specs 在非 ask 模式追加)。
pub fn tool_specs() -> Vec<Value> {
    vec![
        tool_spec(
            MEMORY_WRITE_TOOL,
            "记住一条跨会话有效的信息:用户偏好(preference)、项目约定(convention)或事实(fact)。\
只记长期有用的内容,不记一次性任务细节,绝不记录密钥/令牌。scope 缺省:偏好记全局,其余记本项目。",
            json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "要记住的内容(一句话讲清楚,≤8KiB)" },
                    "kind": { "type": "string", "enum": KINDS },
                    "scope": { "type": "string", "enum": ["global", "project"] },
                    "tags": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["content"],
            }),
        ),
        tool_spec(
            MEMORY_SEARCH_TOOL,
            "按关键词检索已记住的记忆(全局 + 本项目)。系统提示里只注入了最相关的几条,需要更多时用它查。",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "description": "最多返回条数(缺省 10,上限 50)" }
                },
                "required": ["query"],
            }),
        ),
        tool_spec(
            MEMORY_DELETE_TOOL,
            "删除一条过时或错误的记忆(id 见系统提示「用户记忆」段或 memory_search 结果)。",
            json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
            }),
        ),
    ]
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn tags_arg(args: &Value) -> Vec<String> {
    args.get("tags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn emit(state: &AppState, session_id: &str, event_type: &str, payload: Value) {
    state
        .events
        .emit(EventDraft::new(session_id, event_type, "memory").payload(payload));
}

/// 执行 memory_* 工具(主 agent 执行闭包调用)。返回 (ok, 反馈文本)。
pub fn dispatch_tool(
    state: &AppState,
    project: &ScopeProject,
    session_id: &str,
    run_id: &str,
    name: &str,
    args: &Value,
) -> (bool, String) {
    let project_scope = project_scope(project);
    match name {
        MEMORY_WRITE_TOOL => {
            let content = str_arg(args, "content").unwrap_or("").trim().to_string();
            let kind = match validate_kind(str_arg(args, "kind")) {
                Ok(k) => k,
                Err(e) => return (false, e.to_string()),
            };
            let scope = match resolve_scope(str_arg(args, "scope"), &kind, &project_scope) {
                Ok(s) => s,
                Err(e) => return (false, e.to_string()),
            };
            let tags = tags_arg(args);
            // 同作用域同正文 = 同一条记忆:更新而不是重复堆积。
            let existing = state
                .memory
                .active()
                .into_iter()
                .find(|e| e.scope == scope && e.content == content);
            let result = match existing {
                Some(e) => {
                    let mut merged = e.tags.clone();
                    merged.extend(tags);
                    state
                        .memory
                        .update(
                            &e.id,
                            MemoryPatch {
                                content: None,
                                kind: Some(kind),
                                tags: Some(merged),
                            },
                        )
                        .map(|m| (m, false))
                }
                None => state
                    .memory
                    .create(NewMemory {
                        content,
                        kind: Some(kind),
                        scope,
                        tags,
                        source: "agent",
                    })
                    .map(|m| (m, true)),
            };
            match result {
                Ok((m, created)) => {
                    emit(
                        state,
                        session_id,
                        "memory.written",
                        json!({
                            "runId": run_id, "id": m.id, "scope": m.scope, "kind": m.kind,
                            "source": "agent", "created": created,
                        }),
                    );
                    state.sync.nudge();
                    let verb = if created {
                        "已记住"
                    } else {
                        "已存在相同记忆,已更新"
                    };
                    (
                        true,
                        format!("{verb}(id={}, scope={}, kind={})", m.id, m.scope, m.kind),
                    )
                }
                Err(e) => (false, e.to_string()),
            }
        }
        MEMORY_SEARCH_TOOL => {
            let query = str_arg(args, "query").unwrap_or("");
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|n| (n as usize).clamp(1, SEARCH_MAX_LIMIT))
                .unwrap_or(SEARCH_DEFAULT_LIMIT);
            let hits = search(&state.memory, &[SCOPE_GLOBAL, &project_scope], query, limit);
            let items: Vec<Value> = hits
                .iter()
                .map(|e| {
                    json!({
                        "id": e.id, "scope": e.scope, "kind": e.kind, "content": e.content,
                        "tags": e.tags, "updatedAt": e.updated_at,
                    })
                })
                .collect();
            (
                true,
                json!({ "items": items, "total": items.len() }).to_string(),
            )
        }
        MEMORY_DELETE_TOOL => {
            let id = str_arg(args, "id").unwrap_or("").trim();
            match state.memory.delete(id) {
                Ok(m) => {
                    emit(
                        state,
                        session_id,
                        "memory.deleted",
                        json!({ "runId": run_id, "id": m.id, "scope": m.scope, "source": "agent" }),
                    );
                    state.sync.nudge();
                    (true, format!("已删除记忆 {id}"))
                }
                Err(e) => (false, e.to_string()),
            }
        }
        other => (false, format!("未知记忆工具: {other}")),
    }
}

// ---------- REST ----------

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/forge/memory", get(list_memories).post(create_memory))
        .route(
            "/api/forge/memory/{id}",
            patch(patch_memory).delete(delete_memory),
        )
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    /// 当前项目所属工作区(缺省 = 默认工作区)。
    #[serde(default)]
    workspace_id: Option<String>,
}

/// GET /api/forge/memory?q=&scope=global|project|all&workspaceId=
pub(crate) async fn list_memories(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Response {
    let project = crate::scope::project_of(&state, q.workspace_id.as_deref());
    let key = project_key(&project);
    let project_scope = format!("project:{key}");
    let scopes: Vec<&str> = match q.scope.as_deref().map(str::trim).unwrap_or("all") {
        "global" => vec![SCOPE_GLOBAL],
        "project" => vec![project_scope.as_str()],
        "all" | "" => Vec::new(),
        other => {
            return MemoryError::Invalid(format!("scope 只能是 global|project|all: {other}"))
                .response();
        }
    };
    let items: Vec<Value> = search(
        &state.memory,
        &scopes,
        q.q.as_deref().unwrap_or(""),
        usize::MAX,
    )
    .iter()
    .map(MemoryEntry::to_wire)
    .collect();
    let status = crate::cloud::sync::sync_status(&state);
    Json(json!({
        "items": items,
        "projectKey": key,
        "sync": {
            "enabled": state.cloud.sync_enabled(crate::cloud::SyncKind::Memory),
            "lastSyncAt": status["lastSyncAt"],
        },
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateRequest {
    #[serde(default)]
    content: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    workspace_id: Option<String>,
}

/// POST /api/forge/memory {content, kind, scope: global|project, tags} → LocalMemory
pub(crate) async fn create_memory(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRequest>,
) -> Response {
    let project = crate::scope::project_of(&state, req.workspace_id.as_deref());
    let kind = match validate_kind(req.kind.as_deref()) {
        Ok(k) => k,
        Err(e) => return e.response(),
    };
    // 用户手动新建:scope 缺省全局(不按 kind 猜——界面上看得到作用域选择)。
    let scope_arg = req.scope.as_deref().or(Some(SCOPE_GLOBAL));
    let scope = match resolve_scope(scope_arg, &kind, &project_scope(&project)) {
        Ok(s) => s,
        Err(e) => return e.response(),
    };
    match state.memory.create(NewMemory {
        content: req.content,
        kind: Some(kind),
        scope,
        tags: req.tags,
        source: "user",
    }) {
        Ok(m) => {
            state.sync.nudge();
            Json(m.to_wire()).into_response()
        }
        Err(e) => e.response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PatchRequest {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

/// PATCH /api/forge/memory/{id} {content, kind, tags} → LocalMemory
pub(crate) async fn patch_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchRequest>,
) -> Response {
    match state.memory.update(
        &id,
        MemoryPatch {
            content: req.content,
            kind: req.kind,
            tags: req.tags,
        },
    ) {
        Ok(m) => {
            state.sync.nudge();
            Json(m.to_wire()).into_response()
        }
        Err(e) => e.response(),
    }
}

/// DELETE /api/forge/memory/{id} → {ok:true}
pub(crate) async fn delete_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.memory.delete(&id) {
        Ok(_) => {
            state.sync.nudge();
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => e.response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_micros_orders_subsecond_and_offset() {
        let a = parse_rfc3339_micros("2026-09-26T10:00:00Z").unwrap();
        let b = parse_rfc3339_micros("2026-09-26T10:00:00.123Z").unwrap();
        assert!(b > a);
        let c = parse_rfc3339_micros("2026-09-26T18:00:00+08:00").unwrap();
        assert_eq!(c, a);
    }

    #[test]
    fn memory_scoring_prefers_project_and_query_hits() {
        let store = MemoryStore::ephemeral();
        store
            .create(NewMemory {
                content: "全局偏好:深色主题".into(),
                kind: Some("preference".into()),
                scope: SCOPE_GLOBAL.to_string(),
                tags: vec![],
                source: "user",
            })
            .unwrap();
        store
            .create(NewMemory {
                content: "本项目使用 bevy 0.14".into(),
                kind: Some("convention".into()),
                scope: "project:demo".into(),
                tags: vec![],
                source: "user",
            })
            .unwrap();
        let hits = select_for_turn(&store, "project:demo", "bevy 引擎", INJECT_TOP_N);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].scope, "project:demo");
        let searched = search(&store, &[SCOPE_GLOBAL, "project:demo"], "bevy", 5);
        assert!(searched.iter().any(|e| e.content.contains("bevy")));
    }
}
