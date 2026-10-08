//! D-035:Plan 模式产物 = 工作区计划文件 `.forge/plans/<slug>.plan.md`。
//!
//! 事实源是文件本身,不是聊天记录里的一段文本——用户可在 Plan 页签直接编辑后再 Build,
//! 刷新/换会话/重启都不丢。文件形态 = YAML front matter(name/overview/todos)+ Markdown 正文。
//!
//! 为什么落在 `.forge/`:与 `.forge/cache`、`.forge/tmp` 同一「项目系统目录」约定,
//! forge-index / context-mcp 的抽取器都跳过该目录,计划文本不会污染语义检索。
//!
//! 解析用 serde_yaml(工作区已有依赖,assetd 的 `.meta` 在用);写盘前把 name/overview/
//! todo 文本压成单行,让前端那份手写行级解析器(client `lib/planFile.ts`)只需处理
//! 平铺标量,两侧不会因 YAML 高级语法(块标量/锚点)分叉。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::events::{now_rfc3339, EventBus, EventDraft};
use crate::sessions::SessionStore;

/// 计划文件目录(工作区根相对,正斜杠——与前端文件 API 的 path 同形)。
pub const PLAN_DIR: &str = ".forge/plans";
/// 计划文件后缀。
pub const PLAN_EXT: &str = ".plan.md";
/// slug 字符数上限(避免超长文件名撞 Windows MAX_PATH)。
const SLUG_MAX_CHARS: usize = 48;
/// 单份计划的待办条数上限(超出截断,如实告知模型)。
const MAX_TODOS: usize = 64;

fn default_status() -> String {
    "pending".to_string()
}

/// front matter 里的一条待办(Build 时按 id 物化进 TodoStore)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanTodo {
    pub id: String,
    pub content: String,
    #[serde(default = "default_status")]
    pub status: String,
}

/// 计划文件的 front matter。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanFront {
    pub name: String,
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub todos: Vec<PlanTodo>,
}

/// 解析后的计划文件。
#[derive(Debug, Clone, PartialEq)]
pub struct PlanDoc {
    pub front: PlanFront,
    /// Markdown 正文(front matter 之后的全部内容)。
    pub body: String,
}

/// 多行/多空白压成单行(front matter 只放平铺标量,见模块注释)。
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 计划名 → 文件 slug。
///
/// 保留中文(前端与文件 API 全程 UTF-8);路径分隔符、Windows 非法字符与点号一律换 `-`
/// ——点号换掉顺带根除 `..` 逃逸,且我们自己负责补 `.plan.md` 后缀。
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.trim().chars() {
        if ch.is_control() {
            continue;
        }
        let is_sep = ch.is_whitespace()
            || matches!(
                ch,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '.'
            );
        if is_sep {
            if !out.is_empty() && !prev_dash {
                out.push('-');
                prev_dash = true;
            }
            continue;
        }
        out.push(ch);
        prev_dash = false;
        if out.chars().count() >= SLUG_MAX_CHARS {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        // 名字全是分隔符/控制字符:回落时间戳,不静默丢计划。
        format!("plan-{}", now_rfc3339().replace([':', '-', '.'], ""))
    } else {
        out
    }
}

/// slug → 工作区相对路径(`.forge/plans/<slug>.plan.md`)。
pub fn plan_rel_path(slug: &str) -> String {
    format!("{PLAN_DIR}/{slug}{PLAN_EXT}")
}

/// 工作区相对路径是否为合法计划文件路径(Build 入口的 confine 前置判据)。
///
/// 只认 `.forge/plans/` 下、以 `.plan.md` 结尾、不含 `..` 与绝对路径形态的单层文件。
pub fn is_plan_path(rel: &str) -> bool {
    let norm = rel.replace('\\', "/");
    let Some(tail) = norm.strip_prefix(&format!("{PLAN_DIR}/")) else {
        return false;
    };
    !tail.is_empty()
        && tail.ends_with(PLAN_EXT)
        && !tail.contains('/')
        && !tail.contains("..")
        && !tail.contains([':', '\0'])
        && tail.len() > PLAN_EXT.len()
}

/// Reject links and Windows reparse points before reading or creating a plan.
/// The workspace itself may be a user-selected linked checkout; descendants may not escape it.
pub fn confined_path(ws_root: &Path, rel: &str) -> Result<PathBuf, String> {
    if !is_plan_path(rel) {
        return Err("非法计划文件路径".into());
    }
    let mut path = ws_root.to_path_buf();
    for part in rel.split(['/', '\\']) {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => {
                #[cfg(windows)]
                let reparse = {
                    use std::os::windows::fs::MetadataExt;
                    meta.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let reparse = false;
                if meta.file_type().is_symlink() || reparse {
                    return Err(format!("计划路径不能经过链接: {}", path.display()));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("检查计划路径失败: {e}")),
        }
    }
    Ok(path)
}

/// 工作区相对路径 → 绝对路径(仅在 `is_plan_path` 通过后调用)。
pub fn abs_path(ws_root: &Path, rel: &str) -> PathBuf {
    let mut p = ws_root.to_path_buf();
    for seg in rel.replace('\\', "/").split('/').filter(|s| !s.is_empty()) {
        p.push(seg);
    }
    p
}

/// front matter + 正文 → 文件全文。
pub fn render(front: &PlanFront, body: &str) -> String {
    let fm = serde_yaml::to_string(front)
        .unwrap_or_else(|e| format!("name: {}\n# front matter 序列化失败: {e}\n", front.name));
    format!("---\n{fm}---\n\n{}\n", body.trim_end())
}

/// 文件全文 → 计划文档(front matter 缺失/损坏时如实报错,不猜)。
pub fn parse(text: &str) -> Result<PlanDoc, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err("缺 front matter 起始 ---".to_string());
    }
    let mut fm = String::new();
    let mut body_start: Option<usize> = None;
    for (i, line) in text.lines().enumerate().skip(1) {
        if line.trim() == "---" {
            body_start = Some(i + 1);
            break;
        }
        fm.push_str(line);
        fm.push('\n');
    }
    let Some(body_start) = body_start else {
        return Err("缺 front matter 结束 ---".to_string());
    };
    let front: PlanFront =
        serde_yaml::from_str(&fm).map_err(|e| format!("front matter 解析失败: {e}"))?;
    if front.name.trim().is_empty() {
        return Err("front matter 缺 name".to_string());
    }
    let body = text
        .lines()
        .skip(body_start)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    Ok(PlanDoc { front, body })
}

/// create_plan 入参归一:压单行、补/去重 id、截断超量、丢空条目。
fn normalize_todos(raw: Option<&Vec<Value>>) -> Vec<PlanTodo> {
    let mut out: Vec<PlanTodo> = Vec::new();
    let Some(items) = raw else {
        return out;
    };
    for (i, item) in items.iter().enumerate() {
        let content = one_line(item.get("content").and_then(Value::as_str).unwrap_or(""));
        if content.is_empty() {
            continue;
        }
        let raw_id = one_line(item.get("id").and_then(Value::as_str).unwrap_or(""));
        let mut id = slugify(&raw_id);
        if raw_id.is_empty() {
            id = format!("task-{}", i + 1);
        }
        // 同名 id 会让 Build 侧的物化去重把两条待办并成一条,这里就地消歧。
        if out.iter().any(|t| t.id == id) {
            id = format!("{id}-{}", i + 1);
        }
        out.push(PlanTodo {
            id,
            content,
            status: default_status(),
        });
        if out.len() >= MAX_TODOS {
            break;
        }
    }
    out
}

/// 落盘结果。
pub struct WriteOutcome {
    /// 工作区相对路径。
    pub rel_path: String,
    /// true = 该路径此前无文件(发 plan.created);false = 覆盖(发 plan.updated)。
    pub created: bool,
    pub front: PlanFront,
}

/// 原子写:同目录 tmp + rename(半截文件不可见;与 sessions/todos 同纪律)。
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(format!(".{}.tmp", crate::events::new_id("plan")));
    let tmp = PathBuf::from(tmp);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    let result = std::fs::rename(&tmp, path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// 写入计划文件。`existing_rel` = 会话已有的计划路径(有则原地迭代,保住页签与文件身份)。
pub fn write_plan(
    ws_root: &Path,
    existing_rel: Option<&str>,
    name: &str,
    overview: &str,
    body: &str,
    todos: Vec<PlanTodo>,
) -> Result<WriteOutcome, String> {
    let name = one_line(name);
    if name.is_empty() {
        return Err("name 不可为空".to_string());
    }
    let rel = match existing_rel.filter(|r| is_plan_path(r)) {
        Some(r) => r.to_string(),
        None => plan_rel_path(&slugify(&name)),
    };
    let abs = confined_path(ws_root, &rel)?;
    let created = !abs.exists();
    let front = PlanFront {
        name,
        overview: one_line(overview),
        todos,
    };
    write_atomic(&abs, &render(&front, body))
        .map_err(|e| format!("计划文件写盘失败({}): {e}", abs.display()))?;
    Ok(WriteOutcome {
        rel_path: rel,
        created,
        front,
    })
}

/// 读取并解析计划文件(Build 入口与 plan 模式迭代注入共用)。
pub fn load(ws_root: &Path, rel: &str) -> Result<PlanDoc, String> {
    if !is_plan_path(rel) {
        return Err(format!(
            "planPath 须为 {PLAN_DIR}/<名>{PLAN_EXT} 形态(实: {rel})"
        ));
    }
    let abs = confined_path(ws_root, rel)?;
    let text =
        std::fs::read_to_string(&abs).map_err(|e| format!("计划文件读取失败({rel}): {e}"))?;
    parse(&text)
}

/// 计划全文 → 注入模型的上下文段。`heading` 区分「当前计划(迭代)」与「本次要实施的计划」。
pub fn preamble_section(heading: &str, rel: &str, doc: &PlanDoc) -> String {
    let mut s = format!("【{heading}】{}({rel})\n", doc.front.name);
    if !doc.front.overview.is_empty() {
        s.push_str(&format!("概述:{}\n", doc.front.overview));
    }
    if !doc.front.todos.is_empty() {
        s.push_str("待办清单:\n");
        for t in &doc.front.todos {
            s.push_str(&format!("- [{}] {} :: {}\n", t.status, t.id, t.content));
        }
    }
    s.push_str("\n计划正文:\n");
    s.push_str(&doc.body);
    s
}

/// create_plan 工具实现(plan 模式唯一产物出口)。
///
/// 写盘 + 会话 activePlanPath 落库 + plan.created|updated / session.updated 事件。
/// 走 agent.rs 的父执行闭包而非 native_tools::dispatch_native——它要写会话存贮,
/// 而 dispatch_native 拿不到 SessionStore。
pub fn handle_create_plan(
    ws_root: &Path,
    events: &EventBus,
    sessions: &Arc<SessionStore>,
    session_id: &str,
    run_id: &str,
    args: &Value,
) -> (bool, String) {
    let name = args.get("name").and_then(Value::as_str).unwrap_or("");
    let overview = args.get("overview").and_then(Value::as_str).unwrap_or("");
    let body = args.get("plan").and_then(Value::as_str).unwrap_or("");
    if body.trim().is_empty() {
        return (false, "plan(Markdown 正文)不可为空".to_string());
    }
    let todos = normalize_todos(args.get("todos").and_then(Value::as_array));
    if todos.is_empty() {
        return (
            false,
            "todos 不可为空:计划必须给出可执行的分步待办(每条带 id 与 content)".to_string(),
        );
    }
    let existing = sessions.get(session_id).and_then(|s| s.active_plan_path);
    let outcome = match write_plan(ws_root, existing.as_deref(), name, overview, body, todos) {
        Ok(o) => o,
        Err(e) => return (false, e),
    };
    events.emit(
        EventDraft::new(
            session_id,
            if outcome.created {
                "plan.created"
            } else {
                "plan.updated"
            },
            "plan",
        )
        .payload(json!({
            "runId": run_id,
            "path": outcome.rel_path,
            "name": outcome.front.name,
            "overview": outcome.front.overview,
            "todoCount": outcome.front.todos.len(),
        })),
    );
    // 会话指针:前端据此在快照回放后仍能定位当前计划(刷新不丢)。
    if let Some(mut s) = sessions.get(session_id) {
        if s.active_plan_path.as_deref() != Some(outcome.rel_path.as_str()) {
            s.active_plan_path = Some(outcome.rel_path.clone());
            s.touch();
            sessions.save(&s);
            events.emit(
                EventDraft::new(session_id, "session.updated", "session")
                    .payload(json!({ "sessionId": session_id })),
            );
        }
    }
    (
        true,
        format!(
            "计划已写入 {}({} 条待办)。已在 Plan 页签打开;最终消息只给两三句摘要,不要复述计划全文。",
            outcome.rel_path,
            outcome.front.todos.len()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo(id: &str, content: &str) -> PlanTodo {
        PlanTodo {
            id: id.to_string(),
            content: content.to_string(),
            status: default_status(),
        }
    }

    #[test]
    fn slugify_strips_path_and_windows_illegal_chars() {
        assert_eq!(slugify("敌人波次系统"), "敌人波次系统");
        assert_eq!(slugify("  Wave  System  "), "Wave-System");
        assert_eq!(slugify("a/b\\c:d*e?f\"g<h>i|j"), "a-b-c-d-e-f-g-h-i-j");
        // 点号一并换掉 → `..` 逃逸根除。
        assert_eq!(slugify("../../etc/passwd"), "etc-passwd");
        assert!(!slugify("...").starts_with('.'));
        assert!(slugify("...").starts_with("plan-"), "全分隔符名回落时间戳");
        assert!(slugify(&"长".repeat(200)).chars().count() <= SLUG_MAX_CHARS);
    }

    #[test]
    fn is_plan_path_only_accepts_confined_single_level_files() {
        assert!(is_plan_path(".forge/plans/x.plan.md"));
        assert!(is_plan_path(".forge\\plans\\x.plan.md"), "反斜杠归一");
        assert!(!is_plan_path(".forge/plans/sub/x.plan.md"), "不许多层");
        assert!(!is_plan_path(".forge/plans/../../secret.plan.md"));
        assert!(!is_plan_path(".forge/plans/x.md"), "后缀须 .plan.md");
        assert!(!is_plan_path(".forge/plans/.plan.md"), "空名不认");
        assert!(!is_plan_path("plans/x.plan.md"));
        assert!(!is_plan_path("/etc/x.plan.md"));
        assert!(!is_plan_path(".forge/plans/x:stream.plan.md"));
        assert!(confined_path(std::path::Path::new("."), "../../x.plan.md").is_err());
    }

    #[test]
    fn render_parse_roundtrip() {
        let front = PlanFront {
            name: "敌人波次系统".into(),
            overview: "一句话概述".into(),
            todos: vec![
                todo("wave-config", "新增 WaveConfig 组件"),
                todo("spawner", "写生成器"),
            ],
        };
        let text = render(&front, "# 标题\n\n## 现状\n正文");
        assert!(text.starts_with("---\n"));
        let doc = parse(&text).expect("解析");
        assert_eq!(doc.front, front);
        assert!(doc.body.contains("## 现状"));
    }

    /// 冒号/引号等 YAML 敏感字符必须能原样往返(serde_yaml 负责引号,手搓拼串会炸)。
    #[test]
    fn render_parse_survives_yaml_special_chars() {
        let front = PlanFront {
            name: "计划: 带冒号 \"引号\" 与 #井号".into(),
            overview: "- 以横杠开头".into(),
            todos: vec![todo("a-b", "改 foo: bar 字段")],
        };
        let doc = parse(&render(&front, "body")).expect("解析");
        assert_eq!(doc.front, front);
    }

    #[test]
    fn parse_reports_broken_front_matter() {
        assert!(parse("no front matter").unwrap_err().contains("起始"));
        assert!(parse("---\nname: x\n").unwrap_err().contains("结束"));
        assert!(parse("---\noverview: x\n---\nbody")
            .unwrap_err()
            .contains("name"));
    }

    #[test]
    fn normalize_todos_fills_ids_dedupes_and_collapses_lines() {
        let raw = json!([
            { "content": "无 id 的一条" },
            { "id": "a b", "content": "多行\n内容  压成\n单行" },
            { "id": "a-b", "content": "撞 id" },
            { "id": "empty", "content": "   " },
        ]);
        let out = normalize_todos(raw.as_array());
        assert_eq!(out.len(), 3, "空 content 条目丢弃: {out:?}");
        assert_eq!(out[0].id, "task-1");
        assert_eq!(out[1].id, "a-b");
        assert_eq!(out[1].content, "多行 内容 压成 单行");
        assert_eq!(out[2].id, "a-b-3", "撞 id 就地消歧");
        assert!(out.iter().all(|t| t.status == "pending"));
    }

    #[test]
    fn write_plan_creates_then_overwrites_same_path() {
        let dir = std::env::temp_dir().join(format!(
            "forge-plan-doc-{}-{}",
            std::process::id(),
            crate::events::new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let first = write_plan(
            &dir,
            None,
            "波次系统",
            "概述",
            "正文",
            vec![todo("a", "甲")],
        )
        .unwrap();
        assert_eq!(first.rel_path, ".forge/plans/波次系统.plan.md");
        assert!(first.created);
        assert!(dir
            .join(".forge")
            .join("plans")
            .join("波次系统.plan.md")
            .is_file());
        // 迭代:带 existing 路径 → 原地覆盖(名字变了也不换文件,页签身份稳定)。
        let second = write_plan(
            &dir,
            Some(&first.rel_path),
            "波次系统 v2",
            "",
            "新正文",
            vec![todo("a", "甲"), todo("b", "乙")],
        )
        .unwrap();
        assert_eq!(second.rel_path, first.rel_path);
        assert!(!second.created, "覆盖既有文件应报 updated");
        let doc = load(&dir, &second.rel_path).unwrap();
        assert_eq!(doc.front.name, "波次系统 v2");
        assert_eq!(doc.front.todos.len(), 2);
        assert_eq!(doc.body, "新正文");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_rejects_out_of_dir_path() {
        let dir = std::env::temp_dir();
        let err = load(&dir, "../../secret.md").unwrap_err();
        assert!(err.contains("planPath"), "{err}");
    }

    #[test]
    fn preamble_section_carries_name_todos_and_body() {
        let doc = PlanDoc {
            front: PlanFront {
                name: "计划甲".into(),
                overview: "概述".into(),
                todos: vec![todo("a", "第一步")],
            },
            body: "# 设计\n细节".into(),
        };
        let s = preamble_section("本次要实施的计划", ".forge/plans/x.plan.md", &doc);
        assert!(s.contains("【本次要实施的计划】计划甲"));
        assert!(s.contains("- [pending] a :: 第一步"));
        assert!(s.contains("# 设计"));
    }
}
