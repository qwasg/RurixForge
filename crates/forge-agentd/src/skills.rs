//! F11 wave.2:skill 内核(06 §2 兑现)。
//!
//! 修的断层:06 §2 声称「agentd 启动扫描 skills/ 生成索引注入系统提示,agent 调
//! read_skill(name) 取全文」,但两样此前都不存在——前端把技能名拼成
//! `Use skills: a, b.` 文本前缀,服务端零解析,SKILL.md 全文从未进过 LLM 上下文。
//! 本模块把 main.rs 里的零散 skills 代码收拢成内核并补齐:
//! - frontmatter 结构化解析(name/description/version/license/tags/allowed-tools);
//! - 文档校验(errors/warnings 二分,create/update 据此分派错误码);
//! - 生命周期 CRUD(create/update/delete;delete 为 destructive,走 Proposal 两阶段门);
//! - 扫描与定位(修 list 用 frontmatter name、read 用目录名导致的 404 不一致 bug);
//! - 系统提示索引段 + 选中技能全文 preamble(agent.rs 注入点)。
//!
//! 诚实纪律(I-5):技能不存在/已禁用/注入被截断一律显式如实,不静默降级。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use crate::AppState;

/// 选中技能全文注入硬预算(字符)。超出按请求序截断,并在段尾如实标注截断篇数
/// (I-5:不静默丢弃)。
const SKILLS_PREAMBLE_BUDGET: usize = 60_000;
/// 索引段单条 description 上限(索引只做路由,细节靠 read_skill 取全文)。
const INDEX_DESC_MAX: usize = 200;
/// 正文过短告警阈值(字符)。
const BODY_SHORT_CHARS: usize = 200;

/// 正文必备三节(06 §1 skill 文档骨架;中文子串匹配,不require 精确标题层级)。
const REQUIRED_SECTIONS: &[&str] = &["执行流程", "输出约束", "失败回退"];
/// description 触发时机提示词(缺失只告警不拦截——存量 skill 文风不统一)。
const TRIGGER_HINTS: &[&str] = &[
    "当任务涉及",
    "当任务",
    "当用户",
    "当需要",
    "何时",
    "触发",
    "当",
];

/// skills 状态测试锁:skills/ 目录与 data/skills-config.json 是进程级共享面,
/// 并发读写必互踩(F4 wave.3 教训同源)。本模块与 main.rs 的 skills 测试统一取此锁。
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---------- frontmatter ----------

/// SKILL.md frontmatter(06 §1)。name/description 必填,其余可选。
/// 存量 13 篇只写 name/description 两键,向后兼容是硬要求。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub allowed_tools: Vec<String>,
}

/// 逐键收集的中间态(name/description 可缺;校验侧据此分项报错)。
#[derive(Debug, Default)]
struct RawFront {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    license: Option<String>,
    tags: Vec<String>,
    allowed_tools: Vec<String>,
}

/// 剥两端成对引号(单/双;不成对则原样返回)。
fn strip_quotes(v: &str) -> String {
    let v = v.trim();
    let (Some(first), Some(last)) = (v.chars().next(), v.chars().next_back()) else {
        return v.to_string();
    };
    if v.chars().count() >= 2 && ((first == '"' && last == '"') || (first == '\'' && last == '\''))
    {
        return v[first.len_utf8()..v.len() - last.len_utf8()].to_string();
    }
    v.to_string()
}

/// 行内数组 `[a, b, c]` → Vec;无方括号时按逗号分隔的裸串处理。
fn parse_inline_list(v: &str) -> Vec<String> {
    let v = v.trim();
    let inner = v
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(v);
    inner
        .split(',')
        .map(strip_quotes)
        .filter(|s| !s.is_empty())
        .collect()
}

/// 拆 frontmatter 与正文。
/// None = 首行非 `---`(整篇无 frontmatter)。
/// 缺闭合 `---` 时正文取空:与其把整篇当正文蒙混过关,不如让校验如实报「缺三节」。
fn split_doc(text: &str) -> Option<(Vec<&str>, String)> {
    let mut it = text.lines();
    if it.next()?.trim() != "---" {
        return None;
    }
    let mut front = Vec::new();
    let mut closed = false;
    for line in it.by_ref() {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        front.push(line);
    }
    let body = if closed {
        it.collect::<Vec<_>>().join("\n")
    } else {
        String::new()
    };
    Some((front, body))
}

/// 逐键解析 frontmatter 行;未知键宽容忽略(06 §1 只钉 name/description 两键必填)。
/// 键名归一化(去中划线/下划线 + 小写)后匹配,故 `allowed-tools` / `allowedTools` /
/// `allowed_tools` 三种写法等价。
fn parse_front_raw(text: &str) -> Option<RawFront> {
    let (lines, _) = split_doc(text)?;
    let mut out = RawFront::default();
    for line in lines {
        let line = line.trim();
        // 无冒号的行(列表项/注释/空行)一律跳过,不当成键。
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase().replace(['-', '_'], "");
        match key.as_str() {
            "name" => out.name = Some(strip_quotes(value)).filter(|s| !s.is_empty()),
            "description" => out.description = Some(strip_quotes(value)).filter(|s| !s.is_empty()),
            "version" => out.version = Some(strip_quotes(value)).filter(|s| !s.is_empty()),
            "license" => out.license = Some(strip_quotes(value)).filter(|s| !s.is_empty()),
            "tags" => out.tags = parse_inline_list(value),
            "allowedtools" => out.allowed_tools = parse_inline_list(value),
            _ => {}
        }
    }
    Some(out)
}

/// SKILL.md → frontmatter;无 frontmatter 或缺 name/description → None。
pub fn parse_frontmatter(text: &str) -> Option<SkillFrontmatter> {
    let raw = parse_front_raw(text)?;
    Some(SkillFrontmatter {
        name: raw.name?,
        description: raw.description?,
        version: raw.version,
        license: raw.license,
        tags: raw.tags,
        allowed_tools: raw.allowed_tools,
    })
}

/// skill 名白名单:小写英文 + 数字 + 中划线(06 §1)。
/// 同时充当路径穿越防线——`.`/`/`/`\` 全被挡在外面,故按名拼路径无需再 canonicalize
/// 禁锢(extraDirs 本就在工作区外,禁锢会误杀)。
pub fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ---------- 校验 ----------

/// 文档校验结果(errors 非空 = 拒收;warnings 只提示不拦截)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillValidation {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// 分项校验:(frontmatter 类错误, 正文类错误, 告警)。
/// 二分是为了让 create/update 能分派 SKILL_FRONTMATTER_INVALID / SKILL_BODY_INCOMPLETE
/// 两个错误码——靠字符串猜错误类型太脆。
fn validate_parts(text: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut front_errors = Vec::new();
    let mut body_errors = Vec::new();
    let mut warnings = Vec::new();
    let Some((_, body)) = split_doc(text) else {
        front_errors.push("缺 frontmatter:首行须为 ---".to_string());
        return (front_errors, body_errors, warnings);
    };
    let raw = parse_front_raw(text).unwrap_or_default();
    match &raw.name {
        None => front_errors.push("frontmatter 缺 name".to_string()),
        Some(n) if !is_valid_skill_name(n) => {
            front_errors.push(format!("name 须为小写英文+数字+中划线: {n}"));
        }
        Some(_) => {}
    }
    match &raw.description {
        None => front_errors.push("frontmatter 缺 description".to_string()),
        Some(d) => {
            if !TRIGGER_HINTS.iter().any(|h| d.contains(h)) {
                warnings.push(
                    "description 未写触发时机(建议含「当任务涉及…时使用」),索引段无法路由到本技能"
                        .to_string(),
                );
            }
        }
    }
    for sec in REQUIRED_SECTIONS {
        if !body.contains(sec) {
            body_errors.push(format!("正文缺「{sec}」小节"));
        }
    }
    if body.chars().count() < BODY_SHORT_CHARS {
        warnings.push(format!(
            "正文仅 {} 字符(建议 ≥{BODY_SHORT_CHARS}),规程过简难以照做",
            body.chars().count()
        ));
    }
    (front_errors, body_errors, warnings)
}

/// SKILL.md 校验(errors 非空即 valid=false)。
pub fn validate_skill_doc(text: &str) -> SkillValidation {
    let (front_errors, body_errors, warnings) = validate_parts(text);
    let mut errors = front_errors;
    errors.extend(body_errors);
    SkillValidation {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// 新建 skill 的骨架模板(五节齐备,自身能通过 validate_skill_doc)。
pub fn skill_template(name: &str) -> String {
    format!(
        "---\n\
name: {name}\n\
description: 一句话说明本技能做什么。当任务涉及「触发词A / 触发词B」时使用。\n\
---\n\
\n\
# {name}\n\
\n\
## 目标\n\
\n\
用一两句话写清本技能要达成的最终状态,以及判定「做完了」的客观标准。\n\
\n\
## 必须遵守\n\
\n\
- **先查询后修改**:动手前先用只读工具读现状,禁止盲改。\n\
- **destructive 走 Proposal**:删除/覆盖类操作须先提案并等批准,未批准一律拒绝执行。\n\
- **不得伪造结果**:任一步失败如实报告,不把没做的写成做了。\n\
\n\
## 执行流程\n\
\n\
1. 取证:调用只读工具确认当前状态,记录关键事实。\n\
2. 规划:据事实列出待执行动作清单(含预期影响面)。\n\
3. 执行:逐条执行并记录每步真实返回。\n\
4. 验证:用只读工具复查,确认达成目标章节写的判定标准。\n\
\n\
## 输出约束\n\
\n\
- 报告必含:执行数、跳过数、失败数与验证结论。\n\
- 数字必须实测,禁止估算充数。\n\
\n\
## 失败回退策略\n\
\n\
- 单项失败:记入 failed 清单后继续其余项,报告中标注,可重跑本技能续作。\n\
- 整体失败:零副作用退出,只留取证报告,不留半成品状态。\n"
    )
}

// ---------- 配置与目录 ----------

/// skills 配置(data/skills-config.json):disabled 清单 + extraDirs 追加扫描目录(06 §2)。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(default, rename = "extraDirs")]
    pub extra_dirs: Vec<String>,
}

pub fn skills_config_path() -> PathBuf {
    crate::workspace_root()
        .join("data")
        .join("skills-config.json")
}

pub fn skills_config_load() -> SkillsConfig {
    match std::fs::read_to_string(skills_config_path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("skills-config.json 损坏({e}),按缺省处理");
            SkillsConfig::default()
        }),
        Err(_) => SkillsConfig::default(),
    }
}

/// 原子写:同目录 tmp 全量写 + rename(与 agent.rs write_atomic 同纪律,
/// 避免半截文件被并发读到)。
fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|n| format!("{}.tmp", n.to_string_lossy()))
        .unwrap_or_else(|| "skill.tmp".to_string());
    let tmp = path.with_file_name(name);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

pub fn skills_config_save(cfg: &SkillsConfig) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(cfg).expect("SkillsConfig 序列化失败");
    write_atomic(&skills_config_path(), &text)
}

/// 主 skills 目录(工作区技能;POST scope=workspace 落这里)。
pub fn skills_root() -> PathBuf {
    crate::workspace_root().join("skills")
}

/// 账号个人技能(15 §8.4;随云同步,POST scope=personal 缺省落这里)。
pub fn user_skills_root() -> PathBuf {
    crate::agent_data_root().join("user-skills")
}

/// scope 缺省或 personal → 个人技能;仅显式 workspace 落到仓内 skills/。
pub(crate) fn skill_is_personal(scope: Option<&str>) -> bool {
    !matches!(scope.map(str::trim), Some("workspace"))
}

/// 扫描目录集:主 skills/ → 个人 user-skills/ → config.extraDirs(相对 workspace 根解析)。
/// 顺序即优先级——同名技能取先扫到的。
pub fn skills_dirs(cfg: &SkillsConfig) -> Vec<PathBuf> {
    let root = crate::workspace_root();
    let mut dirs = vec![skills_root(), user_skills_root()];
    for d in &cfg.extra_dirs {
        dirs.push(root.join(d));
    }
    dirs
}

// ---------- 扫描与定位 ----------

/// 单条技能(对外标识一律 front.name,与 find_skill 定位口径统一)。
#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub front: SkillFrontmatter,
    pub dir: PathBuf,
    pub file: PathBuf,
    pub enabled: bool,
    /// 位于主 skills/ 下 = true;extraDirs 内 = false(供 UI 区分与只读保护)。
    pub builtin: bool,
    /// 位于 data/user-skills/ 下(随账号云同步)。
    pub personal: bool,
}

fn is_disabled(cfg: &SkillsConfig, front_name: &str, dir_name: &str) -> bool {
    cfg.disabled
        .iter()
        .any(|d| d == front_name || d == dir_name)
}

fn scan_with(cfg: &SkillsConfig) -> Vec<SkillEntry> {
    let root = skills_root();
    let personal_root = user_skills_root();
    let mut out: Vec<SkillEntry> = Vec::new();
    for dir in skills_dirs(cfg) {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        // read_dir 序依赖文件系统,先排序保证同名去重的「先扫到」是确定的。
        let mut children: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        children.sort();
        for d in children {
            let file = d.join("SKILL.md");
            if !file.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            // frontmatter 不合格的目录不进清单(名字无从确定);修它走 read/update 面。
            let Some(front) = parse_frontmatter(&text) else {
                continue;
            };
            if out.iter().any(|e| e.front.name == front.name) {
                continue;
            }
            let dir_name = d
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let enabled = !is_disabled(cfg, &front.name, &dir_name);
            let personal = d.starts_with(&personal_root);
            let builtin = d.starts_with(&root);
            out.push(SkillEntry {
                front,
                dir: d,
                file,
                enabled,
                builtin,
                personal,
            });
        }
    }
    out.sort_by(|a, b| a.front.name.cmp(&b.front.name));
    out
}

/// 扫全部目录 → 按 name 排序去重的技能清单。
pub fn scan_skills() -> Vec<SkillEntry> {
    scan_with(&skills_config_load())
}

/// 按名定位:先按目录名直查(快路径),未命中再遍历扫描按 frontmatter name 匹配(兜底)。
/// 兜底这条修的是老 bug:list 用 frontmatter name 作标识、read 用目录名拼路径,
/// 两者不一致时 list 里有的项 read 会 404。
pub fn find_skill(name: &str) -> Option<SkillEntry> {
    if !is_valid_skill_name(name) {
        return None;
    }
    let cfg = skills_config_load();
    let root = skills_root();
    let personal_root = user_skills_root();
    for dir in skills_dirs(&cfg) {
        let d = dir.join(name);
        let file = d.join("SKILL.md");
        if !file.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // frontmatter 坏了也要能读回来——否则坏 skill 无法经 read→update 修复。
        // 此时 front.name 退化为目录名,如实标注而非编造。
        let front = parse_frontmatter(&text).unwrap_or(SkillFrontmatter {
            name: name.to_string(),
            ..Default::default()
        });
        let enabled = !is_disabled(&cfg, &front.name, name);
        let personal = d.starts_with(&personal_root);
        let builtin = d.starts_with(&root);
        return Some(SkillEntry {
            front,
            dir: d,
            file,
            enabled,
            builtin,
            personal,
        });
    }
    scan_with(&cfg).into_iter().find(|e| e.front.name == name)
}

/// 路径显示:工作区内转相对(UI 友好),区外保留绝对。
fn display_path(p: &std::path::Path) -> String {
    let root = crate::workspace_root();
    p.strip_prefix(&root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| p.to_string_lossy().replace('\\', "/"))
}

// ---------- 系统提示注入 ----------

fn one_line(s: &str, max: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let flat = flat.trim().to_string();
    if flat.chars().count() <= max {
        return flat;
    }
    flat.chars().take(max).collect::<String>() + "…"
}

/// 「可用技能」索引段(仅启用项;无技能返回 None,不注入空段)。
/// 文案刻意兼容 ask 模式:ask 无工具面,read_skill 不可用,故说明里给出「无该工具时」
/// 的诚实退路,而不是让模型对着一个不存在的工具干瞪眼(D-F11-SK1)。
pub fn skills_index_prompt() -> Option<String> {
    let entries: Vec<SkillEntry> = scan_skills().into_iter().filter(|e| e.enabled).collect();
    if entries.is_empty() {
        return None;
    }
    let mut s = String::from(
        "\n\n## 可用技能(skills)\n\
以下是本工作区沉淀的操作规程。当任务与某条技能的触发时机匹配时,先调用 read_skill(name) \
取回该技能全文,再严格按其执行流程/输出约束/失败回退办事;不匹配则忽略本节。\
若当前模式未提供 read_skill 工具,只可据下表如实说明本工作区具备哪些技能,不得凭名字杜撰其流程细节。\n",
    );
    for e in &entries {
        s.push_str(&format!(
            "- {}: {}\n",
            e.front.name,
            one_line(&e.front.description, INDEX_DESC_MAX)
        ));
    }
    Some(s)
}

/// 选中技能的全文拼装(供 preamble 注入)。返回 (注入文本, 命中名单)。
/// 跳过不存在与已禁用项(命中名单回请求名,调用侧据此做 missing 差集);
/// 超预算按请求序截断并在段尾如实标注篇数,不静默丢弃(I-5)。
pub fn skills_preamble(names: &[String]) -> Option<(String, Vec<String>)> {
    let mut body = String::new();
    let mut used = 0usize;
    let mut hit: Vec<String> = Vec::new();
    let mut truncated = 0usize;
    for name in names {
        if hit.iter().any(|h| h == name) {
            continue;
        }
        let Some(entry) = find_skill(name) else {
            continue;
        };
        if !entry.enabled {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&entry.file) else {
            continue;
        };
        let seg = format!("### 技能: {}\n{}\n\n", entry.front.name, text.trim_end());
        let cost = seg.chars().count();
        if used + cost > SKILLS_PREAMBLE_BUDGET {
            truncated += 1;
            continue;
        }
        used += cost;
        body.push_str(&seg);
        hit.push(name.clone());
    }
    if hit.is_empty() {
        return None;
    }
    let mut out = String::from(
        "## 本次任务指定的技能规程\n\
以下是用户为本次任务指定的技能规程,请严格遵循其执行流程、输出约束与失败回退策略;\
与你的一般习惯冲突时以技能规程为准。\n\n",
    );
    out.push_str(&body);
    if truncated > 0 {
        out.push_str(&format!(
            "(已截断 {truncated} 篇:注入预算 {SKILLS_PREAMBLE_BUDGET} 字符已用尽,\
未注入的技能可按需用 read_skill 单独读取)\n"
        ));
    }
    Some((out, hit))
}

// ---------- HTTP 面 ----------

fn err(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
        .into_response()
}

fn invalid_name(name: &str) -> Response {
    err(
        StatusCode::BAD_REQUEST,
        "SKILL_NAME_INVALID",
        format!("skill 名须为小写英文+数字+中划线(06 §1): {name}"),
    )
}

fn not_found(name: &str) -> Response {
    err(
        StatusCode::NOT_FOUND,
        "SKILL_NOT_FOUND",
        format!("skill 不存在: {name}"),
    )
}

fn io_err(e: std::io::Error) -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, "FORGE_IO", e.to_string())
}

/// 校验失败 → 按错误类型分派错误码(frontmatter 类优先)。
fn validation_reject(text: &str) -> Option<Response> {
    let (front_errors, body_errors, warnings) = validate_parts(text);
    if front_errors.is_empty() && body_errors.is_empty() {
        return None;
    }
    let code = if front_errors.is_empty() {
        "SKILL_BODY_INCOMPLETE"
    } else {
        "SKILL_FRONTMATTER_INVALID"
    };
    let mut errors = front_errors;
    errors.extend(body_errors);
    Some(
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": code,
                    "message": errors.join(";"),
                    "errors": errors,
                    "warnings": warnings,
                }
            })),
        )
            .into_response(),
    )
}

fn entry_json(e: &SkillEntry) -> Value {
    json!({
        "name": e.front.name,
        "description": e.front.description,
        "enabled": e.enabled,
        "version": e.front.version,
        "license": e.front.license,
        "tags": e.front.tags,
        "allowedTools": e.front.allowed_tools,
        "builtin": e.builtin,
        "personal": e.personal,
        "dir": display_path(&e.dir),
    })
}

/// GET /api/forge/skills/list:扫描全部目录的技能清单(06 §2;07 §7.2 skills tab 数据源)。
pub(crate) async fn skills_list() -> Json<Value> {
    let skills: Vec<Value> = scan_skills().iter().map(entry_json).collect();
    Json(json!({ "skills": skills }))
}

/// GET /api/forge/skills/{name}:SKILL.md 全文 + 结构化 frontmatter(read_skill 的 HTTP 面)。
pub(crate) async fn skills_read(Path(name): Path<String>) -> Response {
    if !is_valid_skill_name(&name) {
        return invalid_name(&name);
    }
    let Some(entry) = find_skill(&name) else {
        return not_found(&name);
    };
    match std::fs::read_to_string(&entry.file) {
        Ok(content) => Json(json!({
            "name": name,
            "content": content,
            "front": entry.front,
            "builtin": entry.builtin,
            "path": display_path(&entry.file),
        }))
        .into_response(),
        Err(e) => io_err(e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillCreateRequest {
    #[serde(default)]
    name: String,
    /// 缺省 = 用内置模板生成骨架。
    #[serde(default)]
    content: Option<String>,
    /// personal → data/user-skills/; workspace → skills/(缺省 personal)。
    #[serde(default)]
    scope: Option<String>,
}

/// POST /api/forge/skills:新建技能(scope 缺省 personal → data/user-skills/)。
/// 400 SKILL_NAME_INVALID / 409 SKILL_ALREADY_EXISTS /
/// 400 SKILL_FRONTMATTER_INVALID|SKILL_BODY_INCOMPLETE。
pub(crate) async fn skills_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SkillCreateRequest>,
) -> Response {
    let name = req.name.trim().to_string();
    if !is_valid_skill_name(&name) {
        return invalid_name(&name);
    }
    if find_skill(&name).is_some() {
        return err(
            StatusCode::CONFLICT,
            "SKILL_ALREADY_EXISTS",
            format!("skill 已存在: {name}(改用 PUT 更新)"),
        );
    }
    let content = match req.content {
        Some(c) => {
            if let Some(reject) = validation_reject(&c) {
                return reject;
            }
            c
        }
        None => skill_template(&name),
    };
    let personal = skill_is_personal(req.scope.as_deref());
    let root = if personal {
        user_skills_root()
    } else {
        skills_root()
    };
    let file = root.join(&name).join("SKILL.md");
    if let Err(e) = write_atomic(&file, &content) {
        return io_or_readonly(e, &name);
    }
    if personal {
        state.sync.mark_skill_dirty(&name);
    }
    let warnings = validate_skill_doc(&content).warnings;
    Json(json!({
        "created": true,
        "name": name,
        "path": display_path(&file),
        "personal": personal,
        "warnings": warnings,
    }))
    .into_response()
}

/// 写失败归因:权限类 → 403 SKILL_READONLY_DIR(诚实指出是目录不可写),其余 → 500。
fn io_or_readonly(e: std::io::Error, name: &str) -> Response {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return err(
            StatusCode::FORBIDDEN,
            "SKILL_READONLY_DIR",
            format!("skill 目录不可写: {name}({e})"),
        );
    }
    io_err(e)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillUpdateRequest {
    #[serde(default)]
    content: String,
}

/// PUT /api/forge/skills/{name}:整篇覆盖 SKILL.md(原子写)。
/// extraDirs 内的技能同样允许改;目录只读 → 403 SKILL_READONLY_DIR。
pub(crate) async fn skills_update(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(req): Json<SkillUpdateRequest>,
) -> Response {
    if !is_valid_skill_name(&name) {
        return invalid_name(&name);
    }
    let Some(entry) = find_skill(&name) else {
        return not_found(&name);
    };
    if req.content.trim().is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "FORGE_INVALID_ARGS",
            "content 不可空",
        );
    }
    if let Some(reject) = validation_reject(&req.content) {
        return reject;
    }
    // 先判目录只读属性:与其让 rename 抛个语焉不详的 IO 错,不如提前给明确错误码。
    if let Ok(meta) = std::fs::metadata(&entry.dir) {
        if meta.permissions().readonly() {
            return err(
                StatusCode::FORBIDDEN,
                "SKILL_READONLY_DIR",
                format!("skill 目录只读,拒绝写入: {}", display_path(&entry.dir)),
            );
        }
    }
    if let Err(e) = write_atomic(&entry.file, &req.content) {
        return io_or_readonly(e, &name);
    }
    if entry.personal {
        state.sync.mark_skill_dirty(&name);
    }
    let warnings = validate_skill_doc(&req.content).warnings;
    Json(json!({
        "updated": true,
        "name": name,
        "path": display_path(&entry.file),
        "personal": entry.personal,
        "warnings": warnings,
    }))
    .into_response()
}

/// DELETE /api/forge/skills/{name}:删除整个技能目录。
/// destructive → Proposal 两阶段门(照 mcp_call 的 asset_delete force 模板,I-6):
/// 无 approved Proposal 覆盖时建 pending 并 409 GOV_PROPOSAL_REQUIRED,批准后同一调用放行。
/// impact.assets 必须填技能名——has_approved_covering 读的正是 impact.assets。
pub(crate) async fn skills_delete(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    if !is_valid_skill_name(&name) {
        return invalid_name(&name);
    }
    let Some(entry) = find_skill(&name) else {
        return not_found(&name);
    };
    let paths = vec![name.clone()];
    if !state
        .proposals
        .has_approved_covering("skill.delete", &paths)
    {
        let id = state.proposals.create(
            "skill.delete",
            format!(
                "删除技能 {name}(整目录 {} 连同 SKILL.md 一并移除,不可撤销)",
                display_path(&entry.dir)
            ),
            json!({ "assets": paths, "skills": paths }),
            json!({ "sessionId": "http", "tool": "skills.delete" }),
        );
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": {
                    "code": "GOV_PROPOSAL_REQUIRED",
                    "message": format!("删除技能为 destructive,须先批准 Proposal(I-6): {name}"),
                    "proposalId": id,
                }
            })),
        )
            .into_response();
    }
    // 防误删护栏:只删确实挂着 SKILL.md 的子目录,绝不碰扫描根本身。
    if !entry.file.is_file() || entry.dir == skills_root() {
        return err(
            StatusCode::CONFLICT,
            "SKILL_DELETE_UNSAFE",
            format!("拒绝删除非技能目录: {}", display_path(&entry.dir)),
        );
    }
    match std::fs::remove_dir_all(&entry.dir) {
        Ok(()) => {
            if entry.personal {
                state.sync.mark_skill_deleted(&name);
            }
            Json(json!({ "deleted": true, "name": name, "personal": entry.personal }))
                .into_response()
        }
        Err(e) => io_or_readonly(e, &name),
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillValidateRequest {
    /// 给了 = 校验草稿(未落盘);不给 = 校验磁盘上的现文。
    #[serde(default)]
    content: Option<String>,
}

/// POST /api/forge/skills/{name}:validate:校验草稿(body 带 content)或磁盘现文(body 省略)。
///
/// 冒号动作在 handler 内分派而非写进路由:matchit 0.8 不许参数与静态字面量混排在同一段
/// (详见 main.rs 路由注释),故整段连动作后缀一并作为 {name} 收进来,在这里剥。
/// 保留 `<名字>:<动作>` 这个形状,是为了后续 `:enable` / `:disable` 之类动作能原地扩展。
pub(crate) async fn skills_validate(
    Path(raw): Path<String>,
    body: Option<Json<SkillValidateRequest>>,
) -> Response {
    let Some((name, action)) = raw.rsplit_once(':') else {
        return err(
            StatusCode::BAD_REQUEST,
            "SKILL_ACTION_REQUIRED",
            format!("POST 须带动作后缀,如 {raw}:validate"),
        );
    };
    if action != "validate" {
        return err(
            StatusCode::BAD_REQUEST,
            "SKILL_ACTION_UNKNOWN",
            format!("未知动作 :{action}(当前支持 :validate)"),
        );
    }
    let name = name.to_string();
    if !is_valid_skill_name(&name) {
        return invalid_name(&name);
    }
    let draft = body.and_then(|Json(r)| r.content);
    let text = match draft {
        Some(c) => c,
        None => {
            let Some(entry) = find_skill(&name) else {
                return not_found(&name);
            };
            match std::fs::read_to_string(&entry.file) {
                Ok(t) => t,
                Err(e) => return io_err(e),
            }
        }
    };
    Json(json!(validate_skill_doc(&text))).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillsConfigWriteRequest {
    /// 全量覆盖 disabled 清单;缺省 = 不变。
    #[serde(default)]
    disabled: Option<Vec<String>>,
    #[serde(default)]
    extra_dirs: Option<Vec<String>>,
}

/// POST /api/forge/skills/config/write:写 skills 配置(启用/禁用 + 目录配置,06 §2)。
/// 写盘后立即生效(list 每次读盘无缓存)。
pub(crate) async fn skills_config_write(Json(req): Json<SkillsConfigWriteRequest>) -> Response {
    let mut cfg = skills_config_load();
    if let Some(disabled) = req.disabled {
        for d in &disabled {
            if !is_valid_skill_name(d) {
                return invalid_name(d);
            }
        }
        cfg.disabled = disabled;
    }
    if let Some(dirs) = req.extra_dirs {
        cfg.extra_dirs = dirs;
    }
    match skills_config_save(&cfg) {
        Ok(()) => Json(json!({
            "written": true,
            "disabled": cfg.disabled,
            "extraDirs": cfg.extra_dirs,
        }))
        .into_response(),
        Err(e) => io_err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 存量 13 篇真实 SKILL.md 必须全部解析成功(向后兼容硬要求)。
    #[test]
    fn real_skills_all_parse_frontmatter() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = skills_root();
        let mut n = 0usize;
        for ent in std::fs::read_dir(&root).expect("skills/ 应存在").flatten() {
            let f = ent.path().join("SKILL.md");
            if !f.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap();
            let front =
                parse_frontmatter(&text).unwrap_or_else(|| panic!("解析失败: {}", f.display()));
            assert!(!front.name.is_empty(), "{}", f.display());
            assert!(!front.description.is_empty(), "{}", f.display());
            assert!(
                is_valid_skill_name(&front.name),
                "name 不合白名单: {}",
                front.name
            );
            n += 1;
        }
        assert!(n >= 13, "仓内真实技能应 ≥13 篇,实测 {n}");
    }

    #[test]
    fn parse_quotes_inline_arrays_and_unknown_keys() {
        let text = "---\n\
name: \"my-skill\"\n\
description: '带引号的说明。当任务涉及 X 时使用。'\n\
version: 1.2.0\n\
license: MIT\n\
tags: [alpha, \"beta\", 'gamma']\n\
allowed-tools: [read_file, grep]\n\
unknownKey: 随便写\n\
- 这行没有冒号\n\
---\n\
正文\n";
        let f = parse_frontmatter(text).expect("应解析成功");
        assert_eq!(f.name, "my-skill");
        assert_eq!(f.description, "带引号的说明。当任务涉及 X 时使用。");
        assert_eq!(f.version.as_deref(), Some("1.2.0"));
        assert_eq!(f.license.as_deref(), Some("MIT"));
        assert_eq!(f.tags, vec!["alpha", "beta", "gamma"]);
        assert_eq!(f.allowed_tools, vec!["read_file", "grep"]);
    }

    /// 键名归一化:allowed-tools / allowedTools / allowed_tools 三写法等价。
    #[test]
    fn parse_allowed_tools_key_variants() {
        for key in ["allowed-tools", "allowedTools", "allowed_tools"] {
            let text = format!("---\nname: k\ndescription: d\n{key}: [a, b]\n---\n正文\n");
            let f = parse_frontmatter(&text).expect("应解析成功");
            assert_eq!(f.allowed_tools, vec!["a", "b"], "键写法 {key} 未归一");
        }
    }

    #[test]
    fn parse_rejects_missing_fields_and_no_frontmatter() {
        assert!(
            parse_frontmatter("---\ndescription: d\n---\n正文\n").is_none(),
            "缺 name 应 None"
        );
        assert!(
            parse_frontmatter("---\nname: n\n---\n正文\n").is_none(),
            "缺 description 应 None"
        );
        assert!(
            parse_frontmatter("# 标题\nname: n\ndescription: d\n").is_none(),
            "首行非 --- 应 None"
        );
        assert!(parse_frontmatter("").is_none(), "空文档应 None");
    }

    #[test]
    fn validate_real_skills_all_valid() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for ent in std::fs::read_dir(skills_root()).unwrap().flatten() {
            let f = ent.path().join("SKILL.md");
            if !f.is_file() {
                continue;
            }
            let v = validate_skill_doc(&std::fs::read_to_string(&f).unwrap());
            assert!(v.valid, "{} 校验不过: {:?}", f.display(), v.errors);
            assert!(
                v.warnings.is_empty(),
                "{} 意外告警: {:?}",
                f.display(),
                v.warnings
            );
        }
    }

    #[test]
    fn validate_reports_missing_sections_and_fields() {
        // 缺三节全缺。
        let v = validate_skill_doc(
            "---\nname: x\ndescription: 当任务涉及 X 时使用。\n---\n只有一句话。\n",
        );
        assert!(!v.valid);
        for sec in REQUIRED_SECTIONS {
            assert!(
                v.errors.iter().any(|e| e.contains(sec)),
                "应报缺「{sec}」: {:?}",
                v.errors
            );
        }
        assert!(
            v.warnings.iter().any(|w| w.contains("字符")),
            "短正文应告警: {:?}",
            v.warnings
        );

        // 无 frontmatter。
        let v2 = validate_skill_doc("# 无 frontmatter\n执行流程 输出约束 失败回退\n");
        assert!(
            v2.errors.iter().any(|e| e.contains("缺 frontmatter")),
            "{:?}",
            v2.errors
        );

        // name 不合白名单。
        let v3 = validate_skill_doc(
            "---\nname: Bad_Name\ndescription: 当任务涉及 X。\n---\n执行流程 输出约束 失败回退\n",
        );
        assert!(
            v3.errors.iter().any(|e| e.contains("小写英文")),
            "{:?}",
            v3.errors
        );

        // description 无触发时机 → 只告警不拦截。
        let v4 = validate_skill_doc(&format!(
            "---\nname: x\ndescription: 一个说明\n---\n{}",
            "## 执行流程\n## 输出约束\n## 失败回退\n".to_string() + &"填充。".repeat(120)
        ));
        assert!(v4.valid, "{:?}", v4.errors);
        assert!(
            v4.warnings.iter().any(|w| w.contains("触发时机")),
            "{:?}",
            v4.warnings
        );
    }

    /// 内置模板必须自身合规(create 缺省路径不产出坏文档)。
    #[test]
    fn template_is_self_valid() {
        let v = validate_skill_doc(&skill_template("demo-skill"));
        assert!(v.valid, "模板不合规: {:?}", v.errors);
        assert!(v.warnings.is_empty(), "模板不该告警: {:?}", v.warnings);
        assert_eq!(
            parse_frontmatter(&skill_template("demo-skill"))
                .unwrap()
                .name,
            "demo-skill"
        );
    }

    #[test]
    fn scan_and_find_agree_on_every_skill() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let all = scan_skills();
        assert!(all.len() >= 13, "扫描应 ≥13 篇,实测 {}", all.len());
        // list 与 read 标识必须一致:清单里的每个 name 都要能被 find_skill 定位。
        for e in &all {
            let found = find_skill(&e.front.name)
                .unwrap_or_else(|| panic!("list 有而 find 无: {}", e.front.name));
            assert_eq!(found.file, e.file);
            assert_eq!(found.builtin, e.builtin, "读取与扫描来源应一致: {}", e.front.name);
        }
        assert!(find_skill("no-such-skill").is_none());
        assert!(find_skill("../etc").is_none(), "非法名须被白名单挡住");
    }

    #[test]
    fn index_prompt_lists_enabled_skills() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let s = skills_index_prompt().expect("仓内有技能,索引段不应为 None");
        assert!(s.contains("## 可用技能(skills)"), "{s}");
        assert!(s.contains("read_skill"), "索引段须说明怎么取全文: {s}");
        assert!(s.contains("scene-greybox"), "应含真实技能名: {s}");
    }

    #[test]
    fn personal_scope_is_default() {
        assert!(skill_is_personal(None));
        assert!(skill_is_personal(Some("personal")));
        assert!(skill_is_personal(Some("")));
        assert!(!skill_is_personal(Some("workspace")));
        assert!(!skill_is_personal(Some(" workspace ")));
    }

    #[test]
    fn preamble_injects_full_skill_text() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (text, hit) = skills_preamble(&["scene-greybox".to_string()]).expect("应命中");
        assert_eq!(hit, vec!["scene-greybox".to_string()]);
        assert!(text.contains("### 技能: scene-greybox"), "{text}");
        // 正文片段逐字取自磁盘,确认注入的是全文而非摘要。
        let disk =
            std::fs::read_to_string(skills_root().join("scene-greybox").join("SKILL.md")).unwrap();
        let probe: String = disk.lines().filter(|l| l.contains("执行流程")).collect();
        assert!(!probe.is_empty() && text.contains(probe.trim()), "{text}");
        // 不存在项跳过且不进命中名单(调用侧据此算 missing)。
        let (_, hit2) = skills_preamble(&["scene-greybox".into(), "no-such-skill".into()]).unwrap();
        assert_eq!(hit2, vec!["scene-greybox".to_string()]);
        assert!(
            skills_preamble(&["no-such-skill".into()]).is_none(),
            "全不命中应 None"
        );
        assert!(skills_preamble(&[]).is_none(), "空清单应 None");
    }
}
