//! 运行时原生工具：待办 + 工作区读写/补丁 + task 委派。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::agent::{PatchTodoRequest, TodoStore};
use crate::engine::is_native_tool;
use crate::events::{EventBus, EventDraft};

pub fn is_native(name: &str) -> bool {
    is_native_tool(name)
}

pub fn dispatch_native(
    ws_root: &Path,
    events: &EventBus,
    todos: &TodoStore,
    session_id: &str,
    run_id: &str,
    name: &str,
    args: &Value,
) -> (bool, String) {
    match name {
        "todo_write" | "write_todos" | "plan_write" => {
            (true, handle_todo_write(events, todos, session_id, run_id, args))
        }
        "todo_update" => handle_todo_update(events, todos, args),
        "read_file" => read_file(ws_root, args),
        "list_dir" => list_dir(ws_root, args),
        "glob" => glob_files(ws_root, args),
        "grep" => grep_files(ws_root, args),
        "read_skill" => read_skill(args),
        "write_file" => write_file(ws_root, args),
        "str_replace_edit" => str_replace_edit(ws_root, args),
        "apply_patch" => apply_patch(ws_root, args),
        "task" => (false, "task 须由引擎嵌套循环处理".into()),
        other => (false, format!("未知原生工具: {other}")),
    }
}

fn handle_todo_write(
    events: &EventBus,
    todos: &TodoStore,
    session_id: &str,
    run_id: &str,
    args: &Value,
) -> String {
    let mut n = 0usize;
    if let Some(items) = args.get("todos").and_then(|v| v.as_array()) {
        for item in items {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if title.is_empty() {
                continue;
            }
            let desc = item
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let kind = item
                .get("kind")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            match todos.create(session_id, title, desc, kind) {
                Ok(todo) => {
                    events.emit(
                        EventDraft::new(session_id, "todo.created", "todo").payload(json!({
                            "id": todo.id,
                            "title": todo.title,
                            "kind": todo.kind,
                            "status": todo.status,
                            "description": todo.description,
                            "runId": run_id,
                        })),
                    );
                    n += 1;
                }
                Err(_) => {}
            }
        }
    }
    format!("recorded {n} todos\n提醒：开始任何一项前先用 todo_update 标记 running，完成后立即标记 completed 并附 summary。")
}

fn handle_todo_update(events: &EventBus, todos: &TodoStore, args: &Value) -> (bool, String) {
    let Some(id) = args.get("id").and_then(|v| v.as_str()) else {
        return (false, "id required".into());
    };
    let req = PatchTodoRequest {
        status: args.get("status").and_then(|v| v.as_str()).map(|s| {
            if s == "in_progress" {
                "running".to_string()
            } else {
                s.to_string()
            }
        }),
        title: args.get("title").and_then(|v| v.as_str()).map(str::to_string),
        description: None,
        summary: args.get("summary").and_then(|v| v.as_str()).map(str::to_string),
    };
    match todos.patch(id, &req) {
        Ok(todo) => {
            events.emit(
                EventDraft::new(&todo.session_id, "todo.updated", "todo").payload(json!({
                    "id": todo.id,
                    "title": todo.title,
                    "status": todo.status,
                    "summary": todo.summary,
                })),
            );
            (true, format!("updated todo {}", todo.id))
        }
        Err(e) => (false, format!("{e:?}")),
    }
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn confine_existing(ws_root: &Path, rel: &str) -> Result<PathBuf, String> {
    let root = ws_root
        .canonicalize()
        .map_err(|_| "WORKSPACE_ROOT_UNREADABLE".to_string())?;
    let candidate = if rel.trim().is_empty() {
        root.clone()
    } else {
        root.join(rel)
    };
    let canon = candidate
        .canonicalize()
        .map_err(|_| format!("PATH_NOT_FOUND: {rel}"))?;
    if !canon.starts_with(&root) {
        return Err("PATH_OUTSIDE_ROOT".into());
    }
    Ok(canon)
}

/// 写路径：已存在则 confine；不存在则 confine 父目录后拼接文件名。
fn confine_write(ws_root: &Path, rel: &str) -> Result<PathBuf, String> {
    let root = ws_root
        .canonicalize()
        .map_err(|_| "WORKSPACE_ROOT_UNREADABLE".to_string())?;
    let rel = rel.trim().trim_start_matches(['/', '\\']);
    if rel.is_empty() {
        return Err("path required".into());
    }
    let candidate = root.join(rel);
    if candidate.exists() {
        return confine_existing(ws_root, rel);
    }
    let parent = candidate.parent().ok_or("PATH_OUTSIDE_ROOT")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let parent_canon = parent
        .canonicalize()
        .map_err(|_| "PATH_NOT_FOUND".to_string())?;
    if !parent_canon.starts_with(&root) {
        return Err("PATH_OUTSIDE_ROOT".into());
    }
    let name = candidate.file_name().ok_or("PATH_OUTSIDE_ROOT")?;
    Ok(parent_canon.join(name))
}

fn rel_display(ws_root: &Path, abs: &Path) -> String {
    let Ok(root) = ws_root.canonicalize() else {
        return abs.to_string_lossy().into_owned();
    };
    abs.strip_prefix(&root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs.to_string_lossy().into_owned())
}

/// 读取工作区相对路径的行区间(resource_get 文档腿复用 confine)。
pub fn read_lines(ws_root: &Path, rel: &str, start: usize, end: usize) -> Result<String, String> {
    let p = confine_existing(ws_root, rel)?;
    if !p.is_file() {
        return Err("PATH_NOT_FOUND: not a file".into());
    }
    let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = text.lines().collect();
    let s = start.max(1).saturating_sub(1);
    let e = end.max(start).min(lines.len());
    if s >= lines.len() {
        return Ok(String::new());
    }
    Ok(lines[s..e].join("\n"))
}

fn read_file(ws_root: &Path, args: &Value) -> (bool, String) {
    let Some(path) = arg_str(args, "path") else {
        return (false, "path required".into());
    };
    match confine_existing(ws_root, &path) {
        Ok(p) if p.is_file() => match std::fs::read_to_string(&p) {
            Ok(t) => (true, t),
            Err(e) => (false, e.to_string()),
        },
        Ok(_) => (false, "PATH_NOT_FOUND: not a file".into()),
        Err(e) => (false, e),
    }
}

/// F11 wave.2:技能规程读取(06 §2 read_skill 兑现;经 skills 模块统一定位,支持 extraDirs)。
///
/// 刻意不走 confine_existing:它把路径强行禁锢在会话工作区根内,而 extraDirs 配置的
/// 技能目录本就可以在工作区之外,禁锢会把它们全部误杀。防穿越改由 skills 模块的名字
/// 白名单 `[a-z0-9-]` 承担——`.`/`/`/`\` 一律不合法,拼不出 `../` 这类路径(D-F11-SK2)。
fn read_skill(args: &Value) -> (bool, String) {
    let Some(name) = arg_str(args, "name") else {
        return (false, "name required".into());
    };
    if !crate::skills::is_valid_skill_name(&name) {
        return (
            false,
            format!("SKILL_NAME_INVALID: 技能名须为小写英文+数字+中划线,收到 {name}"),
        );
    }
    let Some(entry) = crate::skills::find_skill(&name) else {
        return (false, format!("SKILL_NOT_FOUND: 无此技能 {name}"));
    };
    // 已禁用的技能如实拒绝,不偷偷把内容喂回去(I-5)。
    if !entry.enabled {
        return (
            false,
            format!("SKILL_DISABLED: 技能 {name} 已在 skills-config.json 中禁用,如需使用请先启用"),
        );
    }
    match std::fs::read_to_string(&entry.file) {
        Ok(text) => (true, text),
        Err(e) => (false, format!("SKILL_READ_FAILED: {name}: {e}")),
    }
}

fn list_dir(ws_root: &Path, args: &Value) -> (bool, String) {
    let path = arg_str(args, "path").unwrap_or_default();
    let dir = match confine_existing(ws_root, &path) {
        Ok(p) => p,
        Err(e) => return (false, e),
    };
    if !dir.is_dir() {
        return (false, "PATH_NOT_FOUND: not a directory".into());
    }
    let mut names: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for ent in rd.flatten() {
            let n = ent.file_name().to_string_lossy().into_owned();
            let suffix = if ent.path().is_dir() { "/" } else { "" };
            names.push(format!("{n}{suffix}"));
        }
    }
    names.sort();
    names.truncate(500);
    (true, names.join("\n"))
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>, cap: usize) {
    if out.len() >= cap {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        if out.len() >= cap {
            return;
        }
        let p = ent.path();
        if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name == ".git" || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            walk_files(&p, out, cap);
        } else if p.is_file() {
            out.push(p);
        }
    }
}

fn glob_match(pattern: &str, rel: &str) -> bool {
    // 极简：* 任意段内字符，** 任意路径。
    let pat: Vec<char> = pattern.replace('\\', "/").chars().collect();
    let text: Vec<char> = rel.replace('\\', "/").chars().collect();
    fn rec(p: &[char], t: &[char]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        if p.len() >= 2 && p[0] == '*' && p[1] == '*' {
            let rest = if p.len() > 2 && p[2] == '/' { &p[3..] } else { &p[2..] };
            if rec(rest, t) {
                return true;
            }
            if !t.is_empty() {
                return rec(p, &t[1..]);
            }
            return false;
        }
        if p[0] == '*' {
            let rest = &p[1..];
            if rec(rest, t) {
                return true;
            }
            if !t.is_empty() && t[0] != '/' {
                return rec(p, &t[1..]);
            }
            return false;
        }
        if !t.is_empty() && (p[0] == '?' || p[0] == t[0]) {
            return rec(&p[1..], &t[1..]);
        }
        false
    }
    rec(&pat, &text)
}

fn glob_files(ws_root: &Path, args: &Value) -> (bool, String) {
    let Some(pattern) = arg_str(args, "pattern") else {
        return (false, "pattern required".into());
    };
    let root = match ws_root.canonicalize() {
        Ok(r) => r,
        Err(_) => return (false, "WORKSPACE_ROOT_UNREADABLE".into()),
    };
    let mut files = Vec::new();
    walk_files(&root, &mut files, 2000);
    let mut hits: Vec<String> = files
        .iter()
        .map(|p| rel_display(ws_root, p))
        .filter(|rel| glob_match(&pattern, rel))
        .collect();
    hits.sort();
    hits.truncate(200);
    (true, hits.join("\n"))
}

fn grep_files(ws_root: &Path, args: &Value) -> (bool, String) {
    let Some(query) = arg_str(args, "query") else {
        return (false, "query required".into());
    };
    let start = arg_str(args, "path").unwrap_or_default();
    let start_path = match confine_existing(ws_root, &start) {
        Ok(p) => p,
        Err(e) => return (false, e),
    };
    let mut files = Vec::new();
    if start_path.is_file() {
        files.push(start_path);
    } else {
        walk_files(&start_path, &mut files, 800);
    }
    let mut lines = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let rel = rel_display(ws_root, &f);
        for (i, line) in text.lines().enumerate() {
            if line.contains(&query) {
                lines.push(format!("{rel}:{}:{line}", i + 1));
                if lines.len() >= 80 {
                    return (true, lines.join("\n"));
                }
            }
        }
    }
    (true, if lines.is_empty() { format!("no matches for {query}") } else { lines.join("\n") })
}

fn write_file(ws_root: &Path, args: &Value) -> (bool, String) {
    let Some(path) = arg_str(args, "path") else {
        return (false, "path required".into());
    };
    let Some(content) = args.get("content").and_then(|v| v.as_str()) else {
        return (false, "content required".into());
    };
    match confine_write(ws_root, &path) {
        Ok(p) => match std::fs::write(&p, content) {
            Ok(()) => (true, format!("wrote {}", rel_display(ws_root, &p))),
            Err(e) => (false, e.to_string()),
        },
        Err(e) => (false, e),
    }
}

fn str_replace(content: &str, old: &str, new: &str, replace_all: bool) -> Result<String, String> {
    if old.is_empty() {
        return Err("old_string 不能为空".into());
    }
    if old == new {
        return Err("old_string 与 new_string 相同".into());
    }
    let count = content.matches(old).count();
    if count == 0 {
        return Err("old_string 在文件中不存在".into());
    }
    if count > 1 && !replace_all {
        return Err(format!("old_string 出现了 {count} 次，无法唯一定位"));
    }
    Ok(if replace_all {
        content.replace(old, new)
    } else {
        content.replacen(old, new, 1)
    })
}

fn str_replace_edit(ws_root: &Path, args: &Value) -> (bool, String) {
    let Some(path) = arg_str(args, "path") else {
        return (false, "path required".into());
    };
    let old = args
        .get("old_string")
        .or_else(|| args.get("old_str"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new = args
        .get("new_string")
        .or_else(|| args.get("new_str"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let replace_all = args
        .get("replace_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let p = match confine_existing(ws_root, &path) {
        Ok(p) => p,
        Err(e) => return (false, e),
    };
    let Ok(text) = std::fs::read_to_string(&p) else {
        return (false, "read failed".into());
    };
    match str_replace(&text, old, new, replace_all) {
        Ok(next) => match std::fs::write(&p, next) {
            Ok(()) => (true, format!("edited {}", rel_display(ws_root, &p))),
            Err(e) => (false, e.to_string()),
        },
        Err(e) => (false, e),
    }
}

#[derive(Debug)]
enum FileOp {
    Add { path: String, content: String },
    Delete { path: String },
    Update { path: String, find: Vec<String>, replace: Vec<String> },
}

fn parse_patch(text: &str) -> Result<Vec<FileOp>, String> {
    let body = text.trim();
    let mut lines = body.lines().peekable();
    match lines.next() {
        Some(l) if l.trim() == "*** Begin Patch" => {}
        _ => return Err("patch 必须以 '*** Begin Patch' 开头".into()),
    }
    let mut ops = Vec::new();
    while let Some(line) = lines.next() {
        let line = line.trim_end();
        if line.trim() == "*** End Patch" {
            return if ops.is_empty() {
                Err("patch 不包含任何文件操作".into())
            } else {
                Ok(ops)
            };
        }
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            let mut content = String::new();
            while let Some(next) = lines.peek() {
                if next.starts_with("*** ") {
                    break;
                }
                let next = lines.next().unwrap();
                let added = next
                    .strip_prefix('+')
                    .ok_or_else(|| format!("Add File 行须以 + 开头: {next:?}"))?;
                content.push_str(added);
                content.push('\n');
            }
            ops.push(FileOp::Add {
                path: path.trim().to_string(),
                content,
            });
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            ops.push(FileOp::Delete {
                path: path.trim().to_string(),
            });
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            let mut find = Vec::new();
            let mut replace = Vec::new();
            while let Some(next) = lines.peek() {
                if next.starts_with("*** ") {
                    break;
                }
                let next = lines.next().unwrap();
                if next.starts_with("@@") {
                    continue;
                }
                if let Some(ctx) = next.strip_prefix(' ') {
                    find.push(ctx.to_string());
                    replace.push(ctx.to_string());
                } else if let Some(del) = next.strip_prefix('-') {
                    find.push(del.to_string());
                } else if let Some(add) = next.strip_prefix('+') {
                    replace.push(add.to_string());
                } else if next.is_empty() {
                    find.push(String::new());
                    replace.push(String::new());
                }
            }
            ops.push(FileOp::Update {
                path: path.trim().to_string(),
                find,
                replace,
            });
        }
    }
    Err("patch 缺少 '*** End Patch' 结尾".into())
}

fn apply_hunk(content: &str, find: &[String], replace: &[String]) -> Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let had_nl = content.ends_with('\n') || content.is_empty();
    if find.is_empty() {
        lines.extend(replace.iter().cloned());
    } else {
        let positions: Vec<usize> = (0..=lines.len().saturating_sub(find.len()))
            .filter(|&start| {
                lines[start..start + find.len()]
                    .iter()
                    .zip(find)
                    .all(|(a, b)| a == b)
            })
            .collect();
        match positions.len() {
            0 => return Err("hunk 上下文不存在".into()),
            1 => {
                let start = positions[0];
                lines.splice(start..start + find.len(), replace.iter().cloned());
            }
            n => return Err(format!("hunk 上下文出现 {n} 次")),
        }
    }
    let mut out = lines.join("\n");
    if had_nl && !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

fn apply_patch(ws_root: &Path, args: &Value) -> (bool, String) {
    let Some(patch) = args.get("patch").and_then(|v| v.as_str()) else {
        return (false, "patch required".into());
    };
    let ops = match parse_patch(patch) {
        Ok(o) => o,
        Err(e) => return (false, e),
    };
    let mut reports = Vec::new();
    for op in ops {
        match op {
            FileOp::Add { path, content } => match confine_write(ws_root, &path) {
                Ok(p) => {
                    if let Some(parent) = p.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&p, content) {
                        return (false, e.to_string());
                    }
                    reports.push(format!("added {}", rel_display(ws_root, &p)));
                }
                Err(e) => return (false, e),
            },
            FileOp::Delete { path } => match confine_existing(ws_root, &path) {
                Ok(p) => {
                    if let Err(e) = std::fs::remove_file(&p) {
                        return (false, e.to_string());
                    }
                    reports.push(format!("deleted {}", rel_display(ws_root, &p)));
                }
                Err(e) => return (false, e),
            },
            FileOp::Update { path, find, replace } => {
                let p = match confine_existing(ws_root, &path) {
                    Ok(p) => p,
                    Err(e) => return (false, e),
                };
                let text = match std::fs::read_to_string(&p) {
                    Ok(t) => t,
                    Err(e) => return (false, e.to_string()),
                };
                match apply_hunk(&text, &find, &replace) {
                    Ok(next) => {
                        if let Err(e) = std::fs::write(&p, next) {
                            return (false, e.to_string());
                        }
                        reports.push(format!("updated {}", rel_display(ws_root, &p)));
                    }
                    Err(e) => return (false, e),
                }
            }
        }
    }
    (true, reports.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    fn with_root<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        let _g = ENV.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "forge-nt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let prev = std::env::var("FORGE_AGENTD_WORKSPACE_ROOT").ok();
        std::env::set_var("FORGE_AGENTD_WORKSPACE_ROOT", &dir);
        let out = f(&dir);
        match prev {
            Some(p) => std::env::set_var("FORGE_AGENTD_WORKSPACE_ROOT", p),
            None => std::env::remove_var("FORGE_AGENTD_WORKSPACE_ROOT"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    #[test]
    fn write_read_replace_and_confine() {
        with_root(|dir| {
            let root = dir.canonicalize().unwrap();
            let (ok, msg) = write_file(&root, &json!({ "path": "a.txt", "content": "hello" }));
            assert!(ok, "{msg}");
            let (ok, t) = read_file(&root, &json!({ "path": "a.txt" }));
            assert!(ok);
            assert_eq!(t, "hello");
            let (ok, msg) = str_replace_edit(&root, &json!({
                "path": "a.txt",
                "old_string": "hello",
                "new_string": "world"
            }));
            assert!(ok, "{msg}");
            let (ok, t) = read_file(&root, &json!({ "path": "a.txt" }));
            assert!(ok && t == "world");
            let (ok, e) = write_file(&root, &json!({ "path": "../escape.txt", "content": "x" }));
            assert!(!ok);
            assert!(e.contains("PATH_OUTSIDE_ROOT"), "{e}");
        });
    }

    #[test]
    fn apply_patch_add_and_grep() {
        with_root(|dir| {
            let root = dir.canonicalize().unwrap();
            let patch = "*** Begin Patch\n*** Add File: notes/hi.txt\n+alpha\n+beta\n*** End Patch\n";
            let (ok, msg) = apply_patch(&root, &json!({ "patch": patch }));
            assert!(ok, "{msg}");
            let (ok, t) = read_file(&root, &json!({ "path": "notes/hi.txt" }));
            assert!(ok);
            assert!(t.contains("alpha"));
            let (ok, hits) = grep_files(&root, &json!({ "query": "beta" }));
            assert!(ok);
            assert!(hits.contains("notes/hi.txt"), "{hits}");
            let (ok, listed) = glob_files(&root, &json!({ "pattern": "notes/**" }));
            assert!(ok);
            assert!(listed.contains("notes/hi.txt"), "{listed}");
        });
    }
}
