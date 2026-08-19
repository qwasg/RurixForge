//! F7 wave.5:工作区文件树只读面(参考 agent-ide ui/inspector.rs 工作区树规格适配)。
//! GET /api/forge/workspace/tree?path= → 单层 entries:
//! - 根 = agentd 仓根(workspace_root();env FORGE_AGENTD_WORKSPACE_ROOT 可覆盖,测试隔离用);
//! - path 必须 confined 在根内(越界 400 PATH_OUTSIDE_ROOT;不存在/非目录 404 PATH_NOT_FOUND);
//! - entries[{name,kind:dir|file,relPath,size,modifiedAt,hidden}],目录优先 + 名称(小写)排序;
//! - 单层超 500 项截断,truncated:true 如实标记(不伪造完整)。
//! 只读:无任何写面;hidden = 名称 . 前缀(与参考 inspector 同口径,跨平台一致)。

use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::events::now_rfc3339;

/// 单层条目上限(超出截断 + truncated 标记)。
const MAX_ENTRIES: usize = 500;

#[derive(Deserialize)]
pub struct TreeQuery {
    /// 仓根相对路径(缺省/空 = 根)。越界(含 .. 逃逸或绝对路径出根)拒绝。
    #[serde(default)]
    path: Option<String>,
}

/// 工作区根:env FORGE_AGENTD_WORKSPACE_ROOT 优先(测试隔离),否则仓根(CARGO_MANIFEST_DIR 上两级)。
fn root() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("FORGE_AGENTD_WORKSPACE_ROOT") {
        if !p.is_empty() {
            return std::path::PathBuf::from(p);
        }
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)")
        .to_path_buf()
}

/// 目标路径 confined 解析:root canonicalize + join(path) canonicalize 后 starts_with 校验。
/// Ok(绝对路径) / Err((status, code))。
fn resolve_confined(rel: &str) -> Result<std::path::PathBuf, (StatusCode, &'static str)> {
    let root = root()
        .canonicalize()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "WORKSPACE_ROOT_UNREADABLE"))?;
    // 不裁切斜杠:越界防护全靠 canonicalize + starts_with(带盘符/根的路径 join 整体替换,
    // 裁切反而会把 "C:/" 降级成盘符相对路径)。""/空白 = 根本身。
    let rel = rel.trim();
    let candidate = if rel.is_empty() {
        root.clone()
    } else {
        root.join(rel)
    };
    let canon = candidate
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "PATH_NOT_FOUND"))?;
    if !canon.starts_with(&root) {
        return Err((StatusCode::BAD_REQUEST, "PATH_OUTSIDE_ROOT"));
    }
    if !canon.is_dir() {
        return Err((StatusCode::NOT_FOUND, "PATH_NOT_FOUND"));
    }
    Ok(canon)
}

/// 绝对路径 → 仓根相对(正斜杠分隔;根本身 → "")。
fn rel_path(root: &std::path::Path, abs: &std::path::Path) -> String {
    abs.strip_prefix(root)
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

fn entry_json(root: &std::path::Path, dir: &std::path::Path, ent: &std::fs::DirEntry) -> Option<Value> {
    let name = ent.file_name().to_string_lossy().into_owned();
    // symlink_metadata:不跟随软链接(软链接目录取 dir 亦不越根,读的是链接自身类型)。
    let meta = ent.metadata().ok()?;
    let is_dir = meta.is_dir();
    let size = if is_dir { 0 } else { meta.len() };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            // now_rfc3339 以当前时刻格式化;此处需要任意时刻 → 用同一 civil 算法重建。
            format_epoch_rfc3339(d.as_secs() as i64)
        })
        .unwrap_or_else(now_rfc3339);
    Some(json!({
        "name": name,
        "kind": if is_dir { "dir" } else { "file" },
        "relPath": rel_path(root, &dir.join(ent.file_name())),
        "size": size,
        "modifiedAt": modified,
        "hidden": name.starts_with('.'),
    }))
}

/// 纪元秒 → RFC3339 UTC(复用 events.rs civil_from_days 同算法,避免 pub 面扩张)。
fn format_epoch_rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // civil_from_days(Howard Hinnant 算法,与 events.rs 同式)。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// GET /api/forge/workspace/tree?path=:单层 entries(排序/截断/confined 见模块头)。
pub async fn workspace_tree(Query(q): Query<TreeQuery>) -> Response {
    let rel = q.path.unwrap_or_default();
    let canon = match resolve_confined(&rel) {
        Ok(p) => p,
        Err((status, code)) => {
            return (
                status,
                Json(json!({ "error": { "code": code, "message": format!("path 须为根内已存在目录(实: {rel})") } })),
            )
                .into_response();
        }
    };
    let root = root()
        .canonicalize()
        .unwrap_or_else(|_| root());
    let rd = match std::fs::read_dir(&canon) {
        Ok(rd) => rd,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
            )
                .into_response();
        }
    };
    let mut entries: Vec<Value> = rd
        .flatten()
        .filter_map(|ent| entry_json(&root, &canon, &ent))
        .collect();
    // 目录优先 + 名称小写排序(同长度再原串定序,稳定)。
    entries.sort_by(|a, b| {
        let ad = a["kind"] == "dir";
        let bd = b["kind"] == "dir";
        bd.cmp(&ad).then_with(|| {
            let an = a["name"].as_str().unwrap_or("").to_lowercase();
            let bn = b["name"].as_str().unwrap_or("").to_lowercase();
            an.cmp(&bn)
                .then_with(|| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")))
        })
    });
    let total = entries.len();
    let truncated = total > MAX_ENTRIES;
    if truncated {
        entries.truncate(MAX_ENTRIES);
    }
    Json(json!({
        "path": rel_path(&root, &canon),
        "entries": entries,
        "total": total,
        "truncated": truncated,
    }))
    .into_response()
}
