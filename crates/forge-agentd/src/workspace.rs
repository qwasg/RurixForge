//! F7 wave.5:工作区文件树只读面(参考 agent-ide ui/inspector.rs 工作区树规格适配)。
//! GET /api/forge/workspace/tree?path= → 单层 entries:
//! - 根 = agentd 仓根(workspace_root();env FORGE_AGENTD_WORKSPACE_ROOT 可覆盖,测试隔离用);
//! - path 必须 confined 在根内(越界 400 PATH_OUTSIDE_ROOT;不存在/非目录 404 PATH_NOT_FOUND);
//! - entries[{name,kind:dir|file,relPath,size,modifiedAt,hidden}],目录优先 + 名称(小写)排序;
//! - 单层超 500 项截断,truncated:true 如实标记(不伪造完整)。
//! tree 只读;hidden = 名称 . 前缀(与参考 inspector 同口径,跨平台一致)。
//!
//! F8 wave.1:GET /api/forge/workspace/file?path= → 只读文本文件:
//! - confined 同 tree 纪律(canonicalize+starts_with):越界 400 PATH_OUTSIDE_ROOT,
//!   不存在/目录当文件 404 PATH_NOT_FOUND;
//! - 尺寸上限 256KB,超限 413 FILE_TOO_LARGE(拒绝,不截断伪造);
//! - 二进制检测(前 8KB 含 NUL)或非 UTF-8 → 415 BINARY_FILE;
//! - 返回 {path,name,size,content,truncated,modifiedAt}(truncated 恒 false:超限即拒;
//!   modifiedAt = 纳秒级 RFC3339 冲突令牌,F9 起)。
//!
//! F9:PUT /api/forge/workspace/file {path,content,baseModifiedAt?} → 文本写回(文件编辑器落盘):
//! - confine 复用只读面严格口径(resolve_confined_file_in):只改已存在文件,不新建;
//! - 写前双闸:新内容 >256KB 413 FILE_TOO_LARGE / 含 NUL 415 BINARY_FILE;
//! - baseModifiedAt(GET 的 modifiedAt 原样回传)与磁盘不符 → 409 FILE_CONFLICT
//!   (缺省跳过检测 = 最后写入胜;agent write_file 可能后台并发改同一文件,前端恒回传);
//! - 同目录 tmp + rename 原子落盘(不留半截文件);返回 {path,name,size,modifiedAt}。

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::events::now_rfc3339;
use crate::workspaces::resolve_workspace_root;
use crate::AppState;

/// 单层条目上限(超出截断 + truncated 标记)。
const MAX_ENTRIES: usize = 500;

/// F8 wave.1:只读文本文件上限 256KB(超限 413 FILE_TOO_LARGE 拒绝,不截断伪造)。
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// 二进制嗅探窗口(前 8KB 含 NUL → 415 BINARY_FILE)。
const SNIFF_BYTES: usize = 8 * 1024;

#[derive(Deserialize)]
pub struct TreeQuery {
    /// 仓根相对路径(缺省/空 = 根)。越界(含 .. 逃逸或绝对路径出根)拒绝。
    #[serde(default)]
    path: Option<String>,
    /// 工作区 id;有则 tree 根 = 该工作区 root,否则进程默认根。
    #[serde(default, rename = "workspaceId")]
    workspace_id: Option<String>,
}

#[derive(Deserialize)]
pub struct FileQuery {
    /// 仓根相对路径(必填,须为根内已存在文件)。
    #[serde(default)]
    path: Option<String>,
    #[serde(default, rename = "workspaceId")]
    workspace_id: Option<String>,
}

/// F9:PUT /api/forge/workspace/file 写请求体。
#[derive(Deserialize)]
pub struct FileWriteReq {
    /// 仓根相对路径(须为根内已存在文件;不新建)。
    path: String,
    /// 新全文(UTF-8 文本;字节原样落盘,不做 EOL 归一)。
    content: String,
    /// 乐观并发基线:GET 返回的 modifiedAt 原样回传;缺省/空 = 跳过冲突检测(最后写入胜)。
    #[serde(default, rename = "baseModifiedAt")]
    base_modified_at: Option<String>,
    #[serde(default, rename = "workspaceId")]
    workspace_id: Option<String>,
}

/// 工作区根:env FORGE_AGENTD_WORKSPACE_ROOT 优先(测试隔离),否则仓根(CARGO_MANIFEST_DIR 上两级)。
pub(crate) fn workspace_root_path() -> std::path::PathBuf {
    root()
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
fn resolve_confined(
    root: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, (StatusCode, &'static str)> {
    let root = root
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

/// F8 wave.1:文件版 confined 解析(显式 root 入参,测试直测免 env 串扰)。
/// 同 tree 纪律:canonicalize + starts_with;越界 400 / 不存在 404;目录当文件 404。
fn resolve_confined_file_in(
    root_canon: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, (StatusCode, &'static str)> {
    let rel = rel.trim();
    let candidate = if rel.is_empty() {
        root_canon.to_path_buf()
    } else {
        root_canon.join(rel)
    };
    let canon = candidate
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "PATH_NOT_FOUND"))?;
    if !canon.starts_with(root_canon) {
        return Err((StatusCode::BAD_REQUEST, "PATH_OUTSIDE_ROOT"));
    }
    if !canon.is_file() {
        return Err((StatusCode::NOT_FOUND, "PATH_NOT_FOUND"));
    }
    Ok(canon)
}

/// F8 wave.1:只读文本文件加载(显式 root 入参)。
/// Ok({path,name,size,content,truncated}) / Err((status, code)):
/// 越界 400 PATH_OUTSIDE_ROOT;不存在/目录 404 PATH_NOT_FOUND;
/// 超 256KB 413 FILE_TOO_LARGE(拒绝,不截断伪造);
/// 前 8KB 含 NUL 或非 UTF-8 415 BINARY_FILE;IO 失败 500 FORGE_IO。
fn load_file_in(
    root_canon: &std::path::Path,
    rel: &str,
) -> Result<Value, (StatusCode, &'static str)> {
    let canon = resolve_confined_file_in(root_canon, rel)?;
    let meta = std::fs::metadata(&canon).map_err(|_| (StatusCode::NOT_FOUND, "PATH_NOT_FOUND"))?;
    let size = meta.len();
    if size > MAX_FILE_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "FILE_TOO_LARGE"));
    }
    let bytes = std::fs::read(&canon).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "FORGE_IO"))?;
    let sniff = &bytes[..bytes.len().min(SNIFF_BYTES)];
    if sniff.contains(&0) {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "BINARY_FILE"));
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| (StatusCode::UNSUPPORTED_MEDIA_TYPE, "BINARY_FILE"))?;
    let name = canon
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(json!({
        "path": rel_path(root_canon, &canon),
        "name": name,
        "size": size,
        "content": content,
        "truncated": false,
        "modifiedAt": mtime_token(&meta),
    }))
}

/// F9:文件 mtime → 乐观并发令牌(RFC3339 + 纳秒小数)。
/// 比 tree 的秒级 modifiedAt 精:同一秒内的外部改写也要能被 409 捕获。
fn mtime_token(meta: &std::fs::Metadata) -> String {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            let secs = format_epoch_rfc3339(d.as_secs() as i64);
            format!("{}.{:09}Z", secs.trim_end_matches('Z'), d.subsec_nanos())
        })
        .unwrap_or_else(now_rfc3339)
}

/// F9:工作区文本文件写回(显式 root 入参,测试直测免 env 串扰)。
/// Ok({path,name,size,modifiedAt}) / Err((status, code)):
/// 越界 400 PATH_OUTSIDE_ROOT;不存在/目录当文件 404 PATH_NOT_FOUND(只改已存在文件,不新建);
/// 新内容超 256KB 413 FILE_TOO_LARGE;含 NUL 415 BINARY_FILE;
/// baseModifiedAt 与磁盘 mtime 令牌不符 409 FILE_CONFLICT(冲突时不落盘);
/// IO 失败 500 FORGE_IO。落盘 = 同目录 tmp + rename(原子,不留半截文件)。
fn save_file_in(
    root_canon: &std::path::Path,
    rel: &str,
    content: &str,
    base_modified_at: Option<&str>,
) -> Result<Value, (StatusCode, &'static str)> {
    let canon = resolve_confined_file_in(root_canon, rel)?;
    if content.len() as u64 > MAX_FILE_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "FILE_TOO_LARGE"));
    }
    if content.contains('\0') {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "BINARY_FILE"));
    }
    let meta = std::fs::metadata(&canon).map_err(|_| (StatusCode::NOT_FOUND, "PATH_NOT_FOUND"))?;
    if let Some(base) = base_modified_at.filter(|s| !s.is_empty()) {
        if base != mtime_token(&meta) {
            return Err((StatusCode::CONFLICT, "FILE_CONFLICT"));
        }
    }
    let name = canon
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = canon
        .parent()
        .ok_or((StatusCode::BAD_REQUEST, "PATH_OUTSIDE_ROOT"))?;
    let tmp = parent.join(format!("{name}.{}.forge-tmp", std::process::id()));
    std::fs::write(&tmp, content).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "FORGE_IO"))?;
    if std::fs::rename(&tmp, &canon).is_err() {
        std::fs::remove_file(&tmp).ok();
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "FORGE_IO"));
    }
    let meta =
        std::fs::metadata(&canon).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "FORGE_IO"))?;
    Ok(json!({
        "path": rel_path(root_canon, &canon),
        "name": name,
        "size": meta.len(),
        "modifiedAt": mtime_token(&meta),
    }))
}

/// F8 wave.1:GET /api/forge/workspace/file?path= → 只读文本文件(纪律见 load_file_in)。
pub async fn workspace_file(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FileQuery>,
) -> Response {
    let rel = q.path.unwrap_or_default();
    if let Some(id) = q.workspace_id.as_deref().filter(|s| !s.is_empty()) {
        if state.workspaces.get(id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "WORKSPACE_NOT_FOUND", "message": format!("工作区不存在: {id}") } })),
            )
                .into_response();
        }
    }
    let root = resolve_workspace_root(&state, q.workspace_id.as_deref());
    let root = match root.canonicalize() {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "WORKSPACE_ROOT_UNREADABLE", "message": "workspace 根不可读" } })),
            )
                .into_response();
        }
    };
    match load_file_in(&root, &rel) {
        Ok(v) => Json(v).into_response(),
        Err((status, code)) => (
            status,
            Json(json!({ "error": { "code": code, "message": format!("path 须为根内 ≤256KB 文本文件(实: {rel})") } })),
        )
            .into_response(),
    }
}

/// F9:PUT /api/forge/workspace/file → 工作区文本文件写回(纪律见 save_file_in)。
pub async fn workspace_file_write(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FileWriteReq>,
) -> Response {
    let rel = req.path;
    if let Some(id) = req.workspace_id.as_deref().filter(|s| !s.is_empty()) {
        if state.workspaces.get(id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "WORKSPACE_NOT_FOUND", "message": format!("工作区不存在: {id}") } })),
            )
                .into_response();
        }
    }
    let root = resolve_workspace_root(&state, req.workspace_id.as_deref());
    let root = match root.canonicalize() {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "WORKSPACE_ROOT_UNREADABLE", "message": "workspace 根不可读" } })),
            )
                .into_response();
        }
    };
    match save_file_in(&root, &rel, &req.content, req.base_modified_at.as_deref()) {
        Ok(v) => Json(v).into_response(),
        Err((status, code)) => {
            let message = if code == "FILE_CONFLICT" {
                format!("文件已被外部修改,请刷新后重试(实: {rel})")
            } else {
                format!("path 须为根内 ≤256KB 已存在文本文件(实: {rel})")
            };
            (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
        }
    }
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
pub async fn workspace_tree(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TreeQuery>,
) -> Response {
    let rel = q.path.unwrap_or_default();
    if let Some(id) = q.workspace_id.as_deref().filter(|s| !s.is_empty()) {
        if state.workspaces.get(id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "WORKSPACE_NOT_FOUND", "message": format!("工作区不存在: {id}") } })),
            )
                .into_response();
        }
    }
    let root = resolve_workspace_root(&state, q.workspace_id.as_deref());
    let root_canon = match root.canonicalize() {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "code": "WORKSPACE_ROOT_UNREADABLE", "message": "workspace 根不可读" } })),
            )
                .into_response();
        }
    };
    let canon = match resolve_confined(&root_canon, &rel) {
        Ok(p) => p,
        Err((status, code)) => {
            return (
                status,
                Json(json!({ "error": { "code": code, "message": format!("path 须为根内已存在目录(实: {rel})") } })),
            )
                .into_response();
        }
    };
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
        .filter_map(|ent| entry_json(&root_canon, &canon, &ent))
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
        "path": rel_path(&root_canon, &canon),
        "entries": entries,
        "total": total,
        "truncated": truncated,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    //! F8 wave.1:workspace/file 纯函数测试(load_file_in 显式 root 入参,
    //! 不触 FORGE_AGENTD_WORKSPACE_ROOT env,免与 main.rs tree 测试互踩)。
    use super::*;

    /// 独立 workspace 根(返回路径;调用方收尾 remove_dir_all)。
    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-f8w1-wsfile-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn f8w1_file_ok_text_read() {
        let root = temp_root("ok");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("hello.txt"), "你好,世界\n第二行").unwrap();
        let v = load_file_in(&root, "sub/hello.txt").expect("正常文本应读取成功");
        assert_eq!(v["path"], "sub/hello.txt");
        assert_eq!(v["name"], "hello.txt");
        assert_eq!(v["size"], "你好,世界\n第二行".len() as u64);
        assert_eq!(v["content"], "你好,世界\n第二行");
        assert_eq!(v["truncated"], false);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn f8w1_file_confined_outside_root_400() {
        let root = temp_root("confined");
        std::fs::write(root.join("f.txt"), "x").unwrap();
        // .. 逃逸 → 400 PATH_OUTSIDE_ROOT(temp 根的父目录恒存在,canonicalize 后出根)。
        let err = load_file_in(&root, "..").unwrap_err();
        assert_eq!(err, (StatusCode::BAD_REQUEST, "PATH_OUTSIDE_ROOT"));
        // 绝对出根路径 → 400(join 整体替换后出根)。
        #[cfg(windows)]
        let abs = "C:/Windows/notepad.exe";
        #[cfg(not(windows))]
        let abs = "/etc/hostname";
        let err2 = load_file_in(&root, abs).unwrap_err();
        assert_eq!(
            err2,
            (StatusCode::BAD_REQUEST, "PATH_OUTSIDE_ROOT"),
            "绝对出根路径须 400"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn f8w1_file_not_found_and_dir_404() {
        let root = temp_root("nf");
        std::fs::create_dir_all(root.join("adir")).unwrap();
        let e1 = load_file_in(&root, "no_such.txt").unwrap_err();
        assert_eq!(e1, (StatusCode::NOT_FOUND, "PATH_NOT_FOUND"));
        // 目录当文件 → 404。
        let e2 = load_file_in(&root, "adir").unwrap_err();
        assert_eq!(e2, (StatusCode::NOT_FOUND, "PATH_NOT_FOUND"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn f8w1_file_too_large_413() {
        let root = temp_root("large");
        let big = "a".repeat((MAX_FILE_BYTES + 1) as usize);
        std::fs::write(root.join("big.txt"), big).unwrap();
        let e = load_file_in(&root, "big.txt").unwrap_err();
        assert_eq!(e, (StatusCode::PAYLOAD_TOO_LARGE, "FILE_TOO_LARGE"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn f8w1_file_binary_rejected_415() {
        let root = temp_root("bin");
        // 前 8KB 含 NUL → 415。
        let mut bytes = b"PNG-like header".to_vec();
        bytes.push(0);
        bytes.extend_from_slice(b"trail");
        std::fs::write(root.join("bin.dat"), &bytes).unwrap();
        let e = load_file_in(&root, "bin.dat").unwrap_err();
        assert_eq!(e, (StatusCode::UNSUPPORTED_MEDIA_TYPE, "BINARY_FILE"));
        // 非 UTF-8(无 NUL 但非法 UTF-8 序列)→ 415。
        std::fs::write(root.join("gbk.txt"), [0xC4u8, 0xE3, 0xBA, 0xC3]).unwrap();
        let e2 = load_file_in(&root, "gbk.txt").unwrap_err();
        assert_eq!(e2, (StatusCode::UNSUPPORTED_MEDIA_TYPE, "BINARY_FILE"));
        std::fs::remove_dir_all(&root).ok();
    }

    // ---------- F9:save_file_in(PUT 写回;显式 root 入参,免 env 串扰) ----------

    #[test]
    fn f9_save_ok_roundtrip_crlf_preserved() {
        let root = temp_root("save-ok");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("a.txt"), "old").unwrap();
        let base = load_file_in(&root, "sub/a.txt").unwrap()["modifiedAt"]
            .as_str()
            .unwrap()
            .to_string();
        let v = save_file_in(&root, "sub/a.txt", "new\r\n第二行", Some(&base))
            .expect("正确基线应写回成功");
        assert_eq!(v["path"], "sub/a.txt");
        assert_eq!(v["name"], "a.txt");
        assert_eq!(v["size"], "new\r\n第二行".len() as u64);
        assert!(v["modifiedAt"].as_str().unwrap().ends_with('Z'));
        // CRLF 字节原样落盘(不做 EOL 归一)。
        assert_eq!(
            std::fs::read(root.join("sub").join("a.txt")).unwrap(),
            "new\r\n第二行".as_bytes()
        );
        // tmp 文件不残留。
        assert!(!std::fs::read_dir(root.join("sub"))
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("forge-tmp")));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn f9_save_outside_root_400() {
        let root = temp_root("save-confined");
        let e = save_file_in(&root, "..", "x", None).unwrap_err();
        assert_eq!(e, (StatusCode::BAD_REQUEST, "PATH_OUTSIDE_ROOT"));
        #[cfg(windows)]
        let abs = "C:/Windows/notepad.exe";
        #[cfg(not(windows))]
        let abs = "/etc/hostname";
        let e2 = save_file_in(&root, abs, "x", None).unwrap_err();
        assert_eq!(e2, (StatusCode::BAD_REQUEST, "PATH_OUTSIDE_ROOT"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn f9_save_not_found_and_dir_404() {
        let root = temp_root("save-nf");
        std::fs::create_dir_all(root.join("adir")).unwrap();
        // 不存在的文件 → 404(只改已存在文件,不新建)。
        let e1 = save_file_in(&root, "ghost.txt", "x", None).unwrap_err();
        assert_eq!(e1, (StatusCode::NOT_FOUND, "PATH_NOT_FOUND"));
        let e2 = save_file_in(&root, "adir", "x", None).unwrap_err();
        assert_eq!(e2, (StatusCode::NOT_FOUND, "PATH_NOT_FOUND"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn f9_save_too_large_413_and_nul_415() {
        let root = temp_root("save-limits");
        std::fs::write(root.join("f.txt"), "ok").unwrap();
        let big = "a".repeat((MAX_FILE_BYTES + 1) as usize);
        let e = save_file_in(&root, "f.txt", &big, None).unwrap_err();
        assert_eq!(e, (StatusCode::PAYLOAD_TOO_LARGE, "FILE_TOO_LARGE"));
        let e2 = save_file_in(&root, "f.txt", "has\0nul", None).unwrap_err();
        assert_eq!(e2, (StatusCode::UNSUPPORTED_MEDIA_TYPE, "BINARY_FILE"));
        // 双拒后原内容不动(闸在写前)。
        assert_eq!(std::fs::read_to_string(root.join("f.txt")).unwrap(), "ok");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn f9_save_stale_base_409_fresh_base_ok() {
        let root = temp_root("save-conflict");
        std::fs::write(root.join("f.txt"), "v1").unwrap();
        let base = load_file_in(&root, "f.txt").unwrap()["modifiedAt"]
            .as_str()
            .unwrap()
            .to_string();
        // 外部改写(sleep 保 mtime 前进,兼容粗粒度文件系统时钟)。
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(root.join("f.txt"), "v2-external").unwrap();
        let e = save_file_in(&root, "f.txt", "v3-mine", Some(&base)).unwrap_err();
        assert_eq!(e, (StatusCode::CONFLICT, "FILE_CONFLICT"));
        // 冲突时不落盘。
        assert_eq!(
            std::fs::read_to_string(root.join("f.txt")).unwrap(),
            "v2-external"
        );
        // 刷新基线后写回成功;缺省基线 = 跳过检测(最后写入胜)。
        let fresh = load_file_in(&root, "f.txt").unwrap()["modifiedAt"]
            .as_str()
            .unwrap()
            .to_string();
        save_file_in(&root, "f.txt", "v3-mine", Some(&fresh)).expect("新基线应写回成功");
        save_file_in(&root, "f.txt", "v4-no-base", None).expect("缺省基线应跳过冲突检测");
        assert_eq!(
            std::fs::read_to_string(root.join("f.txt")).unwrap(),
            "v4-no-base"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
