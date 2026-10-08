//! D-040:工作区文件名模糊搜索(命令面板「文件」组的事实源)。
//! GET /api/forge/workspace/search?q=&workspaceId=&limit= →
//! `{query, results[{path,name,dir}], total, truncated, source:"git"|"walk", scanned, scanTruncated}`。
//! - 候选:工作区在 git 仓库内时取 `git ls-files --cached --others --exclude-standard`
//!   (遵守 .gitignore,含未跟踪新文件);否则有界遍历,跳过 .git/node_modules/target/dist 等重目录;
//! - 候选按工作区根缓存:30s 内直接复用;过期先返回旧表、后台刷新(大工作区列一次在秒级,
//!   不能让每次敲键都等);空查询只预热缓存、不打分(面板打开时调用);
//! - 打分:查询按空白拆词,每词都须命中;文件名内命中 > 路径内连续子串 > 散落子序列,
//!   词首/分隔符后命中加分,路径越短越靠前;结果只回仍存在的文件(已删未提交的跟踪文件跳过)。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::workspace_git::{resolve_root, run_git};
use crate::AppState;

const CANDIDATE_TTL: Duration = Duration::from_secs(30);
const MAX_CANDIDATES: usize = 200_000;
const WALK_MAX_DEPTH: usize = 16;
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
/// 有界遍历跳过的目录名(构建产物/依赖/版本库内部)。
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".turbo",
    ".next",
    ".cache",
    "__pycache__",
    ".venv",
];

struct Candidate {
    path: String,
    lower: String,
    /// 文件名在 lower 中的起始字节位置。
    name_start: usize,
    /// 文件名字符数(越短越贴近查询,打分时扣分)。
    name_len: usize,
}

#[derive(Clone)]
struct CandidateSet {
    built: Instant,
    items: Arc<Vec<Candidate>>,
    source: &'static str,
    truncated: bool,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, CandidateSet>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CandidateSet>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn refreshing() -> &'static Mutex<HashSet<PathBuf>> {
    static INFLIGHT: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn to_candidates(paths: Vec<String>) -> Vec<Candidate> {
    paths
        .into_iter()
        .map(|path| {
            let lower = path.to_lowercase();
            let name_start = lower.rfind('/').map(|i| i + 1).unwrap_or(0);
            let name_len = lower[name_start..].chars().count();
            Candidate {
                path,
                lower,
                name_start,
                name_len,
            }
        })
        .collect()
}

/// 有界遍历(非仓库回落):不跟随软链接,超深度/超上限即止。
fn walk_candidates(root: &Path) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new(), 0usize)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            let name = ent.file_name().to_string_lossy().into_owned();
            let child = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if ft.is_dir() {
                if depth + 1 > WALK_MAX_DEPTH
                    || SKIP_DIRS.contains(&name.as_str())
                    || child == ".forge/cache"
                {
                    continue;
                }
                stack.push((ent.path(), child, depth + 1));
            } else if ft.is_file() {
                out.push(child);
                if out.len() >= MAX_CANDIDATES {
                    return (out, true);
                }
            }
        }
    }
    (out, false)
}

async fn build_candidates(root: &Path) -> CandidateSet {
    let listed = run_git(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )
    .await;
    let (paths, source, truncated) = match listed {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out);
            let mut seen = HashSet::new();
            let mut paths: Vec<String> = text
                .split('\0')
                .filter(|p| !p.is_empty() && seen.insert(*p))
                .map(str::to_string)
                .collect();
            let truncated = paths.len() > MAX_CANDIDATES;
            paths.truncate(MAX_CANDIDATES);
            (paths, "git", truncated)
        }
        Err(_) => {
            let root = root.to_path_buf();
            let (paths, truncated) = tokio::task::spawn_blocking(move || walk_candidates(&root))
                .await
                .unwrap_or_default();
            (paths, "walk", truncated)
        }
    };
    CandidateSet {
        built: Instant::now(),
        items: Arc::new(to_candidates(paths)),
        source,
        truncated,
    }
}

/// 取候选:新鲜直接用;过期先回旧表并后台刷新;没有则同步建。
async fn candidates_for(root: &Path) -> CandidateSet {
    let cached = cache().lock().unwrap().get(root).cloned();
    if let Some(set) = cached {
        if set.built.elapsed() >= CANDIDATE_TTL
            && refreshing().lock().unwrap().insert(root.to_path_buf())
        {
            let root = root.to_path_buf();
            tokio::spawn(async move {
                let fresh = build_candidates(&root).await;
                cache().lock().unwrap().insert(root.clone(), fresh);
                refreshing().lock().unwrap().remove(&root);
            });
        }
        return set;
    }
    let fresh = build_candidates(root).await;
    cache()
        .lock()
        .unwrap()
        .insert(root.to_path_buf(), fresh.clone());
    fresh
}

fn is_separator(c: char) -> bool {
    matches!(c, '/' | '_' | '-' | '.' | ' ')
}

/// 驼峰词首(`Status|Bar`):小写后丢了大小写,回原路径看;仅在大小写转换不改字节长度时可比。
fn camel_at(c: &Candidate, pos: usize) -> bool {
    if pos == 0 || pos >= c.path.len() || c.path.len() != c.lower.len() {
        return false;
    }
    let b = c.path.as_bytes();
    b[pos - 1].is_ascii_lowercase() && b[pos].is_ascii_uppercase()
}

/// lower 中 pos 处是否词首:开头、分隔符之后或驼峰边界。
fn boundary_at(c: &Candidate, pos: usize) -> bool {
    pos == 0 || c.lower[..pos].chars().next_back().is_some_and(is_separator) || camel_at(c, pos)
}

/// 散落子序列:每个字符按序命中;相邻命中、分隔符后命中、落在文件名内都加分,跳过的字符扣分。
fn subsequence_score(c: &Candidate, tok: &str) -> Option<i64> {
    let mut chars = c.lower.char_indices();
    let mut score = 40i64;
    let mut ci = 0usize;
    let mut prev_hit: Option<usize> = None;
    let mut prev_char: Option<char> = None;
    for tc in tok.chars() {
        loop {
            let (bi, hc) = chars.next()?;
            let idx = ci;
            ci += 1;
            let before = prev_char.replace(hc);
            if hc != tc {
                continue;
            }
            if let Some(p) = prev_hit {
                if idx == p + 1 {
                    score += 6;
                } else {
                    score -= ((idx - p - 1) as i64).min(8);
                }
            }
            if before.map_or(true, is_separator) || camel_at(c, bi) {
                score += 8;
            }
            if bi >= c.name_start {
                score += 4;
            }
            prev_hit = Some(idx);
            break;
        }
    }
    Some(score.max(1))
}

fn token_score(c: &Candidate, tok: &str) -> Option<i64> {
    let name = &c.lower[c.name_start..];
    if name == tok {
        return Some(1000);
    }
    if let Some(pos) = name.find(tok) {
        return Some(if pos == 0 {
            600
        } else if boundary_at(c, c.name_start + pos) {
            450
        } else {
            350
        });
    }
    if let Some(pos) = c.lower.find(tok) {
        return Some(if boundary_at(c, pos) { 250 } else { 180 });
    }
    subsequence_score(c, tok)
}

/// 总分 = 各词得分 − 文件名长度(越贴近查询越好)− 路径长度/8(同名文件浅层优先)。
fn score_candidate(c: &Candidate, tokens: &[String]) -> Option<i64> {
    let mut total = 0i64;
    for tok in tokens {
        total += token_score(c, tok)?;
    }
    Some(total - c.name_len as i64 - (c.path.len() as i64) / 8)
}

fn tokens_of(q: &str) -> Vec<String> {
    q.to_lowercase()
        .replace('\\', "/")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// 打分排序(纯函数):返回 (命中总数, 按分数排好的候选下标)。
fn rank(items: &[Candidate], tokens: &[String]) -> (usize, Vec<usize>) {
    let mut hits: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, c)| score_candidate(c, tokens).map(|s| (s, i)))
        .collect();
    hits.sort_unstable_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| items[a.1].path.len().cmp(&items[b.1].path.len()))
            .then_with(|| items[a.1].path.cmp(&items[b.1].path))
    });
    let total = hits.len();
    (total, hits.into_iter().map(|(_, i)| i).collect())
}

fn result_json(path: &str) -> Value {
    let (dir, name) = match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    };
    json!({ "path": path, "name": name, "dir": dir })
}

/// 工作区文件搜索(显式根入参,测试直测)。
pub(crate) async fn search_in(root: &Path, q: &str, limit: usize) -> Value {
    let set = candidates_for(root).await;
    let tokens = tokens_of(q);
    let mut results = Vec::new();
    let mut total = 0;
    if !tokens.is_empty() {
        let (hits, order) = rank(&set.items, &tokens);
        total = hits;
        for i in order.into_iter().take(limit.saturating_mul(3)) {
            let path = &set.items[i].path;
            if !root.join(path).is_file() {
                total = total.saturating_sub(1);
                continue;
            }
            results.push(result_json(path));
            if results.len() >= limit {
                break;
            }
        }
    }
    json!({
        "query": q,
        "truncated": total > results.len(),
        "total": total,
        "results": results,
        "source": set.source,
        "scanned": set.items.len(),
        "scanTruncated": set.truncated,
    })
}

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default, rename = "workspaceId")]
    workspace_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

/// GET /api/forge/workspace/search?q=&workspaceId=&limit=(纪律见模块头)。
pub async fn workspace_search(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SearchQuery>,
) -> Response {
    let root = match resolve_root(&state, q.workspace_id.as_deref()) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    Json(search_in(&root, q.q.as_deref().unwrap_or(""), limit).await).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands(paths: &[&str]) -> Vec<Candidate> {
        to_candidates(paths.iter().map(|p| p.to_string()).collect())
    }

    fn ranked(paths: &[&str], q: &str) -> Vec<String> {
        let items = cands(paths);
        let (_, order) = rank(&items, &tokens_of(q));
        order.into_iter().map(|i| items[i].path.clone()).collect()
    }

    #[test]
    fn exact_name_beats_prefix_beats_substring_beats_subsequence() {
        let got = ranked(
            &[
                "src/lib/chat_store.ts",
                "src/chat.ts",
                "docs/chatting-guide.md",
                "src/c/h/a/t.ts",
                "src/mychat.ts",
            ],
            "chat.ts",
        );
        assert_eq!(got[0], "src/chat.ts", "文件名完全命中排第一: {got:?}");
        let got = ranked(&["a/xchatx.ts", "a/chat_list.ts", "b/c-h-a-t.ts"], "chat");
        assert_eq!(got[0], "a/chat_list.ts", "文件名前缀命中优先: {got:?}");
        assert_eq!(got[1], "a/xchatx.ts");
        assert_eq!(got[2], "b/c-h-a-t.ts", "散落子序列垫底: {got:?}");
    }

    #[test]
    fn multi_token_requires_all_and_supports_paths() {
        let got = ranked(
            &[
                "client/src/shell/Sidebar.tsx",
                "client/test/sidebar.test.tsx",
                "host/src/side.ts",
            ],
            "shell side",
        );
        assert_eq!(got, vec!["client/src/shell/Sidebar.tsx"]);
        let got = ranked(
            &["client/src/shell/Sidebar.tsx", "host/src/Sidebar.tsx"],
            "shell/sidebar",
        );
        assert_eq!(got, vec!["client/src/shell/Sidebar.tsx"]);
        assert!(ranked(&["a/b.ts"], "zzz").is_empty());
    }

    #[test]
    fn camel_case_words_and_closer_names_win() {
        let got = ranked(
            &[
                "packages/client/test/statusBar.test.tsx",
                "packages/client/src/components/shell/StatusBar.tsx",
            ],
            "status bar",
        );
        assert_eq!(
            got[0], "packages/client/src/components/shell/StatusBar.tsx",
            "{got:?}"
        );
        // 驼峰词首的子串命中高于词中命中
        let got = ranked(&["src/tabbar.ts", "src/TabBar.ts"], "bar");
        assert_eq!(got[0], "src/TabBar.ts", "{got:?}");
    }

    #[test]
    fn shorter_path_breaks_ties_and_case_is_ignored() {
        let got = ranked(&["deep/nested/dir/Readme.md", "README.md"], "readme");
        assert_eq!(got[0], "README.md");
    }

    #[test]
    fn chinese_names_match() {
        let got = ranked(&["docs/设计说明.md", "docs/说明.md"], "说明");
        assert_eq!(got[0], "docs/说明.md");
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-d040-search-{tag}-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn walk_skips_heavy_dirs() {
        let dir = temp_dir("walk");
        for d in [
            "src",
            "node_modules/pkg",
            "target/debug",
            ".git/objects",
            ".forge/cache",
            ".forge/plans",
        ] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("src/main.rs"), "").unwrap();
        std::fs::write(dir.join("node_modules/pkg/index.js"), "").unwrap();
        std::fs::write(dir.join("target/debug/app.exe"), "").unwrap();
        std::fs::write(dir.join(".git/objects/x"), "").unwrap();
        std::fs::write(dir.join(".forge/cache/thumb.png"), "").unwrap();
        std::fs::write(dir.join(".forge/plans/a.plan.md"), "").unwrap();
        let (mut paths, truncated) = walk_candidates(&dir);
        paths.sort();
        assert!(!truncated);
        assert_eq!(paths, vec![".forge/plans/a.plan.md", "src/main.rs"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn search_in_non_repo_uses_walk_and_skips_missing() {
        let dir = temp_dir("route");
        std::fs::create_dir_all(dir.join("src/shell")).unwrap();
        std::fs::write(dir.join("src/shell/Sidebar.tsx"), "").unwrap();
        std::fs::write(dir.join("src/shell/StatusBar.tsx"), "").unwrap();
        let v = search_in(&dir, "sidebar", 10).await;
        assert_eq!(v["source"], "walk");
        assert_eq!(v["results"][0]["path"], "src/shell/Sidebar.tsx");
        assert_eq!(v["results"][0]["name"], "Sidebar.tsx");
        assert_eq!(v["results"][0]["dir"], "src/shell");
        // 候选缓存里还在、磁盘上已删的文件不回。
        std::fs::remove_file(dir.join("src/shell/Sidebar.tsx")).unwrap();
        let v = search_in(&dir, "sidebar", 10).await;
        assert_eq!(v["results"].as_array().unwrap().len(), 0);
        // 空查询只预热,不回结果。
        let v = search_in(&dir, "  ", 10).await;
        assert_eq!(v["results"].as_array().unwrap().len(), 0);
        assert_eq!(v["total"], 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
