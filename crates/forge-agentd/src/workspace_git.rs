//! D-040:工作区 git 状态只读面(git CLI;不引 git2,与 ffmpeg 同「外部可执行文件」处置)。
//! GET /api/forge/workspace/git?workspaceId= →
//! - 非仓库:`{isRepo:false, reason}`(不算错误,文件树/状态栏据此不画 git 标记);
//! - 仓库:`{isRepo, branch, upstream, ahead, behind, detached, unborn, rootUntracked,
//!   files[{path,status,staged,dir,insertions,deletions,origPath}], counts, insertions, deletions,
//!   total, truncated}`;
//! - 状态字母取 VS Code 口径:M 修改 / A 新增(已暂存)/ D 删除 / R 重命名 / U 未跟踪 / C 冲突;
//! - 路径一律换算为**工作区相对**:porcelain 与 numstat 给的是仓库根相对,按 `rev-parse
//!   --show-prefix` 剥前缀;`-- .` pathspec 把范围限定在工作区子树;
//! - 工作区目录整体未跟踪(仓库里只有一行 `?? <prefix>/`)时 rootUntracked=true、files 为空——
//!   不把子树里成千上万个文件逐一标成未跟踪;
//! - 找不到 git 501 GIT_NOT_FOUND;超时 504 GIT_TIMEOUT;其余失败 500 GIT_FAILED(stderr 首行)。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::workspaces::resolve_workspace_root;
use crate::AppState;

/// 单条 git 命令超时(大仓库冷缓存 status 在秒级,留足余量)。
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// 逐文件条目上限(超出截断 + truncated 标记,计数仍按全量)。
const MAX_FILES: usize = 2000;

#[derive(Debug, PartialEq)]
pub(crate) enum GitError {
    /// 找不到 git 可执行文件。
    NotFound,
    /// 超过 GIT_TIMEOUT。
    Timeout,
    /// 非零退出(stderr 首行)或 IO 失败。
    Failed(String),
}

impl GitError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            GitError::NotFound => (
                StatusCode::NOT_IMPLEMENTED,
                "GIT_NOT_FOUND",
                "未找到 git 可执行文件(请安装 git 并加入 PATH)".to_string(),
            ),
            GitError::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "GIT_TIMEOUT",
                format!("git 命令超过 {}s 未返回", GIT_TIMEOUT.as_secs()),
            ),
            GitError::Failed(msg) => (StatusCode::INTERNAL_SERVER_ERROR, "GIT_FAILED", msg),
        };
        (
            status,
            Json(json!({ "error": { "code": code, "message": message } })),
        )
            .into_response()
    }
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("git 执行失败")
        .to_string()
}

/// 在 root 下跑一条只读 git 命令,返回 stdout 原始字节。
/// 关掉可选锁与凭据提示:只读查询不抢 index.lock,也不会挂起等人输入。
pub(crate) async fn run_git(root: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(GitError::NotFound),
        Err(e) => return Err(GitError::Failed(e.to_string())),
    };
    match tokio::time::timeout(GIT_TIMEOUT, child.wait_with_output()).await {
        Err(_) => Err(GitError::Timeout),
        Ok(Err(e)) => Err(GitError::Failed(e.to_string())),
        Ok(Ok(out)) if out.status.success() => Ok(out.stdout),
        Ok(Ok(out)) => Err(GitError::Failed(first_line(&out.stderr))),
    }
}

/// `## ` 头行解析结果。
#[derive(Debug, Default, PartialEq)]
struct BranchInfo {
    branch: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    detached: bool,
    unborn: bool,
}

/// porcelain v1 `-b` 头行(不含 `## `):
/// `main...origin/main [ahead 1, behind 2]` / `main` / `HEAD (no branch)` /
/// `No commits yet on main` / `Initial commit on main`(旧版 git)。
fn parse_branch_header(h: &str) -> BranchInfo {
    let mut info = BranchInfo::default();
    for prefix in ["No commits yet on ", "Initial commit on "] {
        if let Some(name) = h.strip_prefix(prefix) {
            info.branch = Some(name.trim().to_string());
            info.unborn = true;
            return info;
        }
    }
    if h.starts_with("HEAD (no branch)") {
        info.detached = true;
        return info;
    }
    let (names, tracking) = match h.find(" [") {
        Some(i) => (&h[..i], Some(h[i + 2..].trim_end_matches(']'))),
        None => (h, None),
    };
    match names.split_once("...") {
        Some((local, upstream)) => {
            info.branch = Some(local.to_string());
            info.upstream = Some(upstream.to_string());
        }
        None => info.branch = Some(names.trim().to_string()),
    }
    if let Some(t) = tracking {
        for part in t.split(',').map(str::trim) {
            if let Some(n) = part.strip_prefix("ahead ") {
                info.ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                info.behind = n.parse().unwrap_or(0);
            }
        }
    }
    info
}

/// porcelain v1 条目(仓库根相对路径)。
#[derive(Debug, PartialEq)]
struct StatusEntry {
    x: u8,
    y: u8,
    path: String,
    orig: Option<String>,
}

/// `git status --porcelain=v1 -b -z`:记录以 NUL 分隔;重命名/复制条目后跟一条原路径记录。
fn parse_porcelain_z(buf: &[u8]) -> (BranchInfo, Vec<StatusEntry>) {
    let text = String::from_utf8_lossy(buf);
    let mut records = text.split('\0').filter(|r| !r.is_empty());
    let mut branch = BranchInfo::default();
    let mut entries = Vec::new();
    while let Some(rec) = records.next() {
        if let Some(h) = rec.strip_prefix("## ") {
            branch = parse_branch_header(h);
            continue;
        }
        let bytes = rec.as_bytes();
        if bytes.len() < 4 {
            continue;
        }
        let (x, y) = (bytes[0], bytes[1]);
        let path = rec[3..].to_string();
        let orig = if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            records.next().map(str::to_string)
        } else {
            None
        };
        entries.push(StatusEntry { x, y, path, orig });
    }
    (branch, entries)
}

/// XY → VS Code 口径状态字母。
fn classify(x: u8, y: u8) -> &'static str {
    match (x, y) {
        (b'?', b'?') => "U",
        (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => "C",
        (b'A', _) => "A",
        (b'R', _) | (_, b'R') | (b'C', _) | (_, b'C') => "R",
        (b'D', _) | (_, b'D') => "D",
        _ => "M",
    }
}

/// `git diff --numstat -z HEAD`:`增\t删\t路径\0`;重命名为 `增\t删\t\0原\0新\0`;二进制为 `-\t-`。
/// 返回 仓库根相对路径 → (增, 删)(二进制为 None)。
fn parse_numstat_z(buf: &[u8]) -> Vec<(String, Option<(u64, u64)>)> {
    let text = String::from_utf8_lossy(buf);
    let mut tokens = text.split('\0');
    let mut out = Vec::new();
    while let Some(tok) = tokens.next() {
        if tok.is_empty() {
            continue;
        }
        let mut parts = tok.splitn(3, '\t');
        let (Some(a), Some(d)) = (parts.next(), parts.next()) else {
            continue;
        };
        let counts = match (a.parse::<u64>(), d.parse::<u64>()) {
            (Ok(a), Ok(d)) => Some((a, d)),
            _ => None,
        };
        let path = match parts.next() {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => {
                let _src = tokens.next();
                match tokens.next() {
                    Some(dst) => dst.to_string(),
                    None => continue,
                }
            }
        };
        out.push((path, counts));
    }
    out
}

/// 仓库根相对 → 工作区相对;不在工作区子树内返回 None,工作区根本身返回 Some("")。
fn strip_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }
    if let Some(rest) = path.strip_prefix(prefix) {
        return Some(rest);
    }
    // 目录条目 `projects/x/` 与前缀 `projects/x/` 等长时上面已命中;无尾斜杠形态兜底。
    (path == prefix.trim_end_matches('/')).then_some("")
}

/// 工作区 git 状态(显式根入参,测试直测)。
pub(crate) async fn git_status_in(root: &Path) -> Result<Value, GitError> {
    let inside = match run_git(root, &["rev-parse", "--is-inside-work-tree"]).await {
        Ok(out) => String::from_utf8_lossy(&out).trim() == "true",
        Err(GitError::Failed(reason)) => return Ok(json!({ "isRepo": false, "reason": reason })),
        Err(e) => return Err(e),
    };
    if !inside {
        return Ok(json!({ "isRepo": false, "reason": "不在 git 工作树内" }));
    }
    let prefix_out = run_git(root, &["rev-parse", "--show-prefix"]).await?;
    let prefix = String::from_utf8_lossy(&prefix_out)
        .trim()
        .replace('\\', "/");

    let status = run_git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-b",
            "-z",
            "--untracked-files=normal",
            "--",
            ".",
        ],
    )
    .await?;
    let (branch, entries) = parse_porcelain_z(&status);

    // 未诞生分支(无提交)没有 HEAD 可比:增删行如实缺省,不因此报错。
    let numstat = if branch.unborn {
        Vec::new()
    } else {
        match run_git(root, &["diff", "--numstat", "-z", "HEAD", "--", "."]).await {
            Ok(out) => parse_numstat_z(&out),
            Err(GitError::Failed(_)) => Vec::new(),
            Err(e) => return Err(e),
        }
    };
    let lines: std::collections::HashMap<&str, Option<(u64, u64)>> =
        numstat.iter().map(|(p, c)| (p.as_str(), *c)).collect();

    let mut root_untracked = false;
    let mut files: Vec<Value> = Vec::new();
    let (mut modified, mut added, mut deleted, mut renamed, mut untracked, mut conflicted) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut insertions, mut deletions) = (0u64, 0u64);
    for e in &entries {
        let Some(rel) = strip_prefix(&e.path, &prefix) else {
            continue;
        };
        let status = classify(e.x, e.y);
        if rel.is_empty() || rel == "/" {
            if status == "U" {
                root_untracked = true;
            }
            continue;
        }
        match status {
            "M" => modified += 1,
            "A" => added += 1,
            "D" => deleted += 1,
            "R" => renamed += 1,
            "U" => untracked += 1,
            _ => conflicted += 1,
        }
        let counts = lines.get(e.path.as_str()).copied().flatten();
        if let Some((a, d)) = counts {
            insertions += a;
            deletions += d;
        }
        let dir = rel.ends_with('/');
        files.push(json!({
            "path": rel.trim_end_matches('/'),
            "status": status,
            "staged": !matches!(e.x, b' ' | b'?' | b'!'),
            "dir": dir,
            "insertions": counts.map(|c| c.0),
            "deletions": counts.map(|c| c.1),
            "origPath": e.orig.as_deref().and_then(|o| strip_prefix(o, &prefix)).map(str::to_string),
        }));
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let total = files.len();
    let truncated = total > MAX_FILES;
    files.truncate(MAX_FILES);
    Ok(json!({
        "isRepo": true,
        "branch": branch.branch,
        "upstream": branch.upstream,
        "ahead": branch.ahead,
        "behind": branch.behind,
        "detached": branch.detached,
        "unborn": branch.unborn,
        "rootUntracked": root_untracked,
        "files": files,
        "counts": {
            "modified": modified,
            "added": added,
            "deleted": deleted,
            "renamed": renamed,
            "untracked": untracked,
            "conflicted": conflicted,
        },
        "insertions": insertions,
        "deletions": deletions,
        "total": total,
        "truncated": truncated,
    }))
}

#[derive(Deserialize)]
pub struct GitQuery {
    #[serde(default, rename = "workspaceId")]
    workspace_id: Option<String>,
}

/// 工作区根解析(与 workspace/tree 同口径):未知 workspaceId 404,根不可读 500。
pub(crate) fn resolve_root(
    state: &AppState,
    workspace_id: Option<&str>,
) -> Result<PathBuf, Response> {
    if let Some(id) = workspace_id.filter(|s| !s.is_empty()) {
        if state.workspaces.get(id).is_none() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "WORKSPACE_NOT_FOUND", "message": format!("工作区不存在: {id}") } })),
            )
                .into_response());
        }
    }
    resolve_workspace_root(state, workspace_id).canonicalize().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "code": "WORKSPACE_ROOT_UNREADABLE", "message": "workspace 根不可读" } })),
        )
            .into_response()
    })
}

/// GET /api/forge/workspace/git?workspaceId=(纪律见模块头)。
pub async fn workspace_git(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GitQuery>,
) -> Response {
    let root = match resolve_root(&state, q.workspace_id.as_deref()) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match git_status_in(&root).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_header_variants() {
        assert_eq!(
            parse_branch_header("main...origin/main [ahead 2, behind 1]"),
            BranchInfo {
                branch: Some("main".into()),
                upstream: Some("origin/main".into()),
                ahead: 2,
                behind: 1,
                ..Default::default()
            }
        );
        assert_eq!(
            parse_branch_header("feat/x").branch.as_deref(),
            Some("feat/x")
        );
        assert!(parse_branch_header("HEAD (no branch)").detached);
        let unborn = parse_branch_header("No commits yet on main");
        assert!(unborn.unborn);
        assert_eq!(unborn.branch.as_deref(), Some("main"));
        let gone = parse_branch_header("dev...origin/dev [gone]");
        assert_eq!(gone.upstream.as_deref(), Some("origin/dev"));
        assert_eq!((gone.ahead, gone.behind), (0, 0));
    }

    #[test]
    fn porcelain_z_with_rename_and_untracked_dir() {
        let buf = b"## main...origin/main [ahead 1]\0 M src/a.ts\0R  src/new.ts\0src/old.ts\0?? assets/\0UU c.txt\0";
        let (branch, entries) = parse_porcelain_z(buf);
        assert_eq!(branch.ahead, 1);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[1].path, "src/new.ts");
        assert_eq!(entries[1].orig.as_deref(), Some("src/old.ts"));
        assert_eq!(classify(entries[0].x, entries[0].y), "M");
        assert_eq!(classify(entries[1].x, entries[1].y), "R");
        assert_eq!(classify(entries[2].x, entries[2].y), "U");
        assert_eq!(classify(entries[3].x, entries[3].y), "C");
        assert_eq!(classify(b'A', b' '), "A");
        assert_eq!(classify(b' ', b'D'), "D");
    }

    #[test]
    fn numstat_z_with_rename_and_binary() {
        let buf = b"3\t1\tsrc/a.ts\0-\t-\timg.png\0" as &[u8];
        let mut v = buf.to_vec();
        v.extend_from_slice(b"5\t0\t\0src/old.ts\0src/new.ts\0");
        let rows = parse_numstat_z(&v);
        assert_eq!(rows[0], ("src/a.ts".to_string(), Some((3, 1))));
        assert_eq!(rows[1], ("img.png".to_string(), None));
        assert_eq!(rows[2], ("src/new.ts".to_string(), Some((5, 0))));
    }

    #[test]
    fn prefix_stripping() {
        assert_eq!(strip_prefix("a/b.ts", ""), Some("a/b.ts"));
        assert_eq!(strip_prefix("proj/x/a.ts", "proj/x/"), Some("a.ts"));
        assert_eq!(strip_prefix("proj/x/", "proj/x/"), Some(""));
        assert_eq!(strip_prefix("proj/x", "proj/x/"), Some(""));
        assert_eq!(strip_prefix("other/a.ts", "proj/x/"), None);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-d040-git-{tag}-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    async fn git(root: &Path, args: &[&str]) {
        let mut full = vec![
            "-c",
            "user.name=forge",
            "-c",
            "user.email=forge@test",
            "-c",
            "core.autocrlf=false",
        ];
        full.extend_from_slice(args);
        run_git(root, &full).await.expect("测试 git 命令应成功");
    }

    #[tokio::test]
    async fn non_repo_reports_is_repo_false() {
        let dir = temp_dir("plain");
        match git_status_in(&dir).await {
            Ok(v) => assert_eq!(v["isRepo"], false),
            Err(GitError::NotFound) => {}
            Err(e) => panic!("非仓库不应报错: {e:?}"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn repo_counts_statuses_and_subdir_prefix() {
        let dir = temp_dir("repo");
        if run_git(&dir, &["init", "-q", "-b", "main"]).await == Err(GitError::NotFound) {
            return;
        }
        std::fs::create_dir_all(dir.join("proj").join("src")).unwrap();
        std::fs::write(dir.join("proj/src/a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("proj/gone.txt"), "bye\n").unwrap();
        std::fs::write(dir.join("outside.txt"), "x\n").unwrap();
        git(&dir, &["add", "."]).await;
        git(&dir, &["commit", "-q", "-m", "init"]).await;
        // 修改 1 + 删除 1 + 暂存新增 1 + 未跟踪 1;仓库根的改动不应出现在子目录工作区里。
        std::fs::write(dir.join("proj/src/a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::remove_file(dir.join("proj/gone.txt")).unwrap();
        std::fs::write(dir.join("proj/staged.txt"), "s\n").unwrap();
        git(&dir, &["add", "proj/staged.txt"]).await;
        std::fs::write(dir.join("proj/src/new.txt"), "n\n").unwrap();
        std::fs::write(dir.join("outside.txt"), "changed\n").unwrap();

        let v = git_status_in(&dir.join("proj"))
            .await
            .expect("仓库子目录应可查询");
        assert_eq!(v["isRepo"], true);
        assert_eq!(v["branch"], "main");
        assert_eq!(v["rootUntracked"], false);
        let files = v["files"].as_array().unwrap();
        let find = |p: &str| files.iter().find(|f| f["path"] == p).cloned();
        assert_eq!(find("src/a.txt").unwrap()["status"], "M");
        assert_eq!(find("src/a.txt").unwrap()["insertions"], 2);
        assert_eq!(find("src/a.txt").unwrap()["deletions"], 1);
        assert_eq!(find("gone.txt").unwrap()["status"], "D");
        let staged = find("staged.txt").unwrap();
        assert_eq!(staged["status"], "A");
        assert_eq!(staged["staged"], true);
        assert_eq!(find("src/new.txt").unwrap()["status"], "U");
        assert!(find("outside.txt").is_none(), "工作区外的改动不应出现");
        assert_eq!(v["counts"]["modified"], 1);
        assert_eq!(v["counts"]["deleted"], 1);
        assert_eq!(v["counts"]["added"], 1);
        assert_eq!(v["counts"]["untracked"], 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn untracked_workspace_root_is_flagged_not_expanded() {
        let dir = temp_dir("rootu");
        if run_git(&dir, &["init", "-q", "-b", "main"]).await == Err(GitError::NotFound) {
            return;
        }
        std::fs::write(dir.join("tracked.txt"), "t\n").unwrap();
        git(&dir, &["add", "."]).await;
        git(&dir, &["commit", "-q", "-m", "init"]).await;
        std::fs::create_dir_all(dir.join("game").join("assets")).unwrap();
        std::fs::write(dir.join("game/assets/a.png"), "x").unwrap();
        std::fs::write(dir.join("game/main.txt"), "y").unwrap();
        let v = git_status_in(&dir.join("game"))
            .await
            .expect("未跟踪子目录应可查询");
        assert_eq!(v["isRepo"], true);
        assert_eq!(v["rootUntracked"], true);
        assert_eq!(v["files"].as_array().unwrap().len(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
