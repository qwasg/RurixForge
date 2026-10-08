//! D-044:UltraPlan —— 第七个 composer 模式,把「一句游戏设想」一路带到可玩的 MVP:
//! 设想 → 项目理解(已有内容时并行 explore)→ 问卷 → 单个子代理做网页 Demo → 用户试玩确认 →
//! 制作计划 → Team 模式一次制作 → 人工验收清单。
//!
//! 为什么是「会话上的一台阶段机」而不是一轮里的阻塞审批卡:既有的审批卡随轮结束失效、进程
//! 重启即丢,而这条流程的每一道闸(填问卷、试玩 Demo、审计划、验收)都可能隔夜。所以每道闸
//! 是独立的一轮,阶段落在 `DebugSession.ultraplan`(sessions.json),产物落在工作区
//! `.forge/ultraplan/<slug>/`——与 D-035「文件才是事实源」同一纪律:刷新、换会话、重启都不丢,
//! 用户也能直接打开文件看。计划文档仍写 `.forge/plans/<slug>.plan.md`,既有 Plan 页签原样可见。
//!
//! 本模块的分层:
//! - 状态 [`UltraPlanState`] 与 slug/token 分配(I-6:永不覆盖另一条流程或既有计划的文件);
//! - 纯函数路由 [`route`]:(阶段, 模式, 动作) → 本轮做什么,契约 §2 的表逐行对应;
//! - 项目事实 [`project_facts`]:只靠扫盘,不触 MCP/引擎(mock 渠道同样可用);
//! - 问卷校验 [`validate_questionnaire`]:模型写的 JSON 先过这一关才会落盘/下发;
//! - 预算化上下文段 [`assemble`]:超限截头并给出「全文在哪」的指针,不静默丢内容(I-5);
//! - 轮次接入:[`resolve_request`](起 run 之前的全部校验)→ [`revalidate`](认领 run 之后
//!   再核一次)→ [`begin_turn`](副作用只在这里发生)→ 出口工具 [`handle_exit_tool`] →
//!   [`finish_turn`](收尾只改相位,阶段只经出口工具推进);
//! - REST:`GET …/ultraplan` 与 `POST …/ultraplan/{action}`(不产生轮次的操作,同 goal/{action} 形态)。
//!
//! 写状态一律走 `SessionStore::update_ultraplan`(存贮锁内读-改-写);`get` 再 `save` 是整结构
//! 覆盖,并发下后写者会吃掉先写者的阶段推进。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::events::{new_id, now_rfc3339, EventDraft};
use crate::AppState;

pub mod evidence;
mod lifecycle;
pub use lifecycle::initialize_production_project;
pub use lifecycle::record_review;
pub mod verify;
pub use lifecycle::{
    complete_demo, complete_production, discovery_tasks, prepare_demo, production_context,
    record_check, validate_answers, validation_summary, verification_tool_spec, VERIFY_TOOL,
};

// ---------------- 常量 ----------------

/// composer 模式 id(讨论 → 问卷 → Demo → 计划三类轮次都用它;制作轮用 `team`)。
pub const MODE: &str = "ultraplan";
/// 制作轮所用模式(契约 §2:start/resume/fix_production 要求 mode=team)。
pub const PRODUCTION_MODE: &str = "team";
/// 流程产物根目录(工作区根相对,正斜杠——与 `.forge/plans` 同一「项目系统目录」约定)。
pub const ROOT_DIR: &str = ".forge/ultraplan";

/// 阶段 = 流程正停在哪道闸前。
pub const STAGE_DISCOVERY: &str = "discovery";
pub const STAGE_QUESTIONNAIRE: &str = "questionnaire";
pub const STAGE_DEMO_REVIEW: &str = "demo_review";
pub const STAGE_PLAN_REVIEW: &str = "plan_review";
pub const STAGE_PRODUCTION: &str = "production";
pub const STAGE_ACCEPTANCE: &str = "acceptance";
pub const STAGE_DONE: &str = "done";
/// 阶段全集(推进顺序)。目前只有路由表单测穷举用它。
#[cfg_attr(not(test), allow(dead_code))]
pub const STAGES: [&str; 7] = [
    STAGE_DISCOVERY,
    STAGE_QUESTIONNAIRE,
    STAGE_DEMO_REVIEW,
    STAGE_PLAN_REVIEW,
    STAGE_PRODUCTION,
    STAGE_ACCEPTANCE,
    STAGE_DONE,
];

/// 相位 = 这道闸此刻的状态(UI 刷新后据此显示「在跑 / 等你 / 上一轮失败」)。
pub const PHASE_WAITING: &str = "waiting";
pub const PHASE_RUNNING: &str = "running";
pub const PHASE_FAILED: &str = "failed";

/// 契约 §2 的轮次失败码表(`agent.failed.error` / `lastError.code` 里只许出现这几个 `ULTRAPLAN_*` 码;
/// 渠道 / 云端自带的错误码如 `OPENAI_COMPAT_NOT_CONFIGURED` 照原样透传,不在此列)。
pub const TURN_FAILURE_CODES: [&str; 6] = [
    ERR_TURN_INVALID,
    "ULTRAPLAN_SPEC_MISSING",
    "ULTRAPLAN_DEMO_BUILD_FAILED",
    "ULTRAPLAN_DEMO_MISSING",
    "ULTRAPLAN_FIX_NO_TASKS",
    "ULTRAPLAN_PRODUCTION_INCOMPLETE",
];
/// 失败码表里的通用码:写盘失败、流程目录不可用、进程重启打断(启动清扫写入,见
/// `SessionStore::sweep_stale_ultraplan_runs`)、渠道报错不带可识别错误码……一切没有专属码的
/// 轮次失败都归到它,具体原因写在 message 里。断在哪一步由 `running`(失败时保留)说明。
pub const ERR_TURN_INVALID: &str = "ULTRAPLAN_TURN_INVALID";

/// `running` 字段取值(正在跑的轮次种类)。
pub const RUNNING_DISCOVERY: &str = "discovery";
pub const RUNNING_SPEC_DEMO: &str = "spec_demo";
pub const RUNNING_PLANNING: &str = "planning";
pub const RUNNING_PRODUCTION: &str = "production";

/// leader 的阶段出口工具(每类轮次只给它自己的那一个/一组)。
pub const QUESTIONNAIRE_TOOL: &str = "ultraplan_questionnaire";
pub const SPEC_TOOL: &str = "ultraplan_spec";
pub const PLAN_DOC_TOOL: &str = "ultraplan_plan_doc";
pub const PLAN_TASKS_TOOL: &str = "ultraplan_plan_tasks";
/// 子代理工具:无头浏览器探测网页 Demo(仅 web-demo-builder)。
#[allow(dead_code)] // D-044 W2(单子代理做 Demo)接线
pub const WEB_DEMO_PROBE_TOOL: &str = "web_demo_probe";
/// 出口工具全集。
pub const EXIT_TOOLS: [&str; 4] = [
    QUESTIONNAIRE_TOOL,
    SPEC_TOOL,
    PLAN_DOC_TOOL,
    PLAN_TASKS_TOOL,
];

/// 制作阶段的返修轮数上限(普通 team 轮是 3;一次成型的 MVP 链路更长,给 5)。
#[allow(dead_code)] // D-044 W4(制作)接线
pub const MAX_FIX_ROUNDS: usize = 5;
/// 每轮注入的流程上下文字符预算(实际取 min(本值, 上下文窗口/4),见 [`effective_budget`])。
pub const PREAMBLE_BUDGET_CHARS: usize = 24_000;

/// 流程目录内的产物文件名。标 allow 的几项由 W2–W4 的轮次写出,W1 只占名。
pub const BRIEF_FILE: &str = "brief.md";
pub const UNDERSTANDING_FILE: &str = "understanding.md";
pub const QUESTIONNAIRE_FILE: &str = "questionnaire.json";
pub const ANSWERS_FILE: &str = "answers.json";
#[allow(dead_code)] // D-044 W2
pub const ANSWERS_MD_FILE: &str = "answers.md";
#[allow(dead_code)] // D-044 W2
pub const SPEC_FILE: &str = "spec.md";
#[allow(dead_code)] // D-044 W3
pub const TEAM_PLAN_FILE: &str = "team-plan.json";
pub const CHECKS_FILE: &str = "checks.json";
#[allow(dead_code)] // D-044 W4
pub const PRODUCTION_FILE: &str = "production.json";
pub const ACCEPTANCE_FILE: &str = "acceptance.json";
pub const EXPLORE_DIR: &str = "explore";
#[allow(dead_code)] // D-044 W2
pub const DEMO_DIR: &str = "demo";
#[allow(dead_code)] // D-044 W2
pub const PROBE_DIR: &str = "probe";

/// 标题上限(字符):超出截断,只影响展示与 slug,原始设想全文在 brief.md。
const TITLE_MAX_CHARS: usize = 80;
/// slug 取设想开头的字符数(再经 `plan_doc::slugify` 清洗)。
const SLUG_BRIEF_CHARS: usize = 24;
/// slug 随机重掷次数上限(之后改用计数后缀,保证一定收敛)。
const SLUG_REROLLS: usize = 32;

pub fn is_exit_tool(name: &str) -> bool {
    EXIT_TOOLS.contains(&name)
}

// ---------------- 状态 ----------------

fn default_stage() -> String {
    STAGE_DISCOVERY.to_string()
}
fn default_phase() -> String {
    PHASE_WAITING.to_string()
}

/// 上一轮失败的原因(`lastError`;失败不改阶段,重发同一动作即重试)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowError {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

/// 会话上的 UltraPlan 流程状态(契约 §1;`DebugSession.ultraplan`)。
///
/// 每个字段都带 serde 缺省:后续波次加字段时,旧 sessions.json 里的半截状态仍能读回,
/// 不会因为一个字段缺失让整条会话解析失败(见 [`deserialize_lenient`])。
/// Option 字段不 skip:契约写的是 `string|null`,前端按「键恒在」取值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UltraPlanState {
    /// `up_…`。
    #[serde(default)]
    pub id: String,
    /// 32 位小写 hex;Demo 托管的路径令牌(**不是** id/slug——id 会进事件与 URL 之外的各处,
    /// 拿它当路径等于把托管地址公开)。
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub slug: String,
    /// `.forge/ultraplan/<slug>`(工作区根相对,正斜杠)。
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub title: String,
    /// 流程创建时会话绑定的工作区;流程进行中不许换(产物路径是工作区相对的)。
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default = "default_stage")]
    pub stage: String,
    #[serde(default = "default_phase")]
    pub phase: String,
    /// 正在跑的轮次种类(`phase == running` 时有值)。`phase == failed` 时保留断掉的那一轮的种类
    /// (前端据此说清断在哪一步:同在 questionnaire 关口,可能是重出问卷断了,也可能是定稿轮断了);
    /// `waiting` 时为 null。
    #[serde(default)]
    pub running: Option<String>,
    #[serde(default)]
    pub last_error: Option<FlowError>,
    /// 首份问卷落盘前为 0,之后 1,2,…
    #[serde(default)]
    pub questionnaire_rev: u32,
    /// 首个 Demo 就绪前为 0。
    #[serde(default)]
    pub demo_iteration: u32,
    /// 只来自服务端自己跑的探测,绝不取子代理自报(I-5)。
    #[serde(default)]
    pub demo_verified: bool,
    #[serde(default)]
    pub demo_note: Option<String>,
    /// `.forge/plans/<slug>.plan.md`;首份计划落盘前为 None(预留路径见 [`Self::reserved_plan_path`])。
    #[serde(default)]
    pub plan_path: Option<String>,
    #[serde(default)]
    pub plan_rev: u32,
    /// 计划文件落盘时的 sha256 hex;制作前比对,用户在 Plan 页签改过就拦下(不静默采用)。
    #[serde(default)]
    pub plan_hash: Option<String>,
    #[serde(default)]
    pub production_run_id: Option<String>,
    /// 首次 acceptance.ready 前为 0。
    #[serde(default)]
    pub acceptance_round: u32,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl UltraPlanState {
    /// 开一条新流程:分配 id、托管令牌与**唯一** slug。不写盘、不落会话——
    /// 调用方在认领 run 之后才把它塞进会话并写 brief.md(认领前的副作用会在 SESSION_BUSY 时留垃圾)。
    ///
    /// `title` 可直接传用户设想原文:取首个非空行压成单行并截到 80 字。
    pub fn new_flow(title: &str, workspace_id: Option<&str>, ws_root: &Path) -> Self {
        let id = new_id("up");
        let title = derive_title(title);
        let slug = allocate_slug(ws_root, &title, &id);
        let ts = now_rfc3339();
        UltraPlanState {
            token: new_token(),
            dir: flow_dir_rel(&slug),
            slug,
            title,
            id,
            workspace_id: workspace_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            stage: default_stage(),
            phase: default_phase(),
            running: None,
            last_error: None,
            questionnaire_rev: 0,
            demo_iteration: 0,
            demo_verified: false,
            demo_note: None,
            plan_path: None,
            plan_rev: 0,
            plan_hash: None,
            production_run_id: None,
            acceptance_round: 0,
            created_at: ts.clone(),
            updated_at: ts,
        }
    }

    /// 流程是否仍在进行(`done` 之外的任何阶段)。工作区锁、回退拦截、目标暂停都以它为准。
    pub fn is_active(&self) -> bool {
        self.stage != STAGE_DONE
    }

    /// 流程目录绝对路径。`dir` 不是本模块写出的形态(sessions.json 被手改)→ None,
    /// 调用方按「产物不存在」处理,不拿一个任意路径去读写。
    pub fn dir_abs(&self, ws_root: &Path) -> Option<PathBuf> {
        if !is_flow_dir(&self.dir) {
            return None;
        }
        let mut path = ws_root.to_path_buf();
        for component in self.dir.split(['/', '\\']) {
            path.push(component);
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                if meta.file_type().is_symlink() {
                    return None;
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return None;
                    }
                }
            }
        }
        Some(path)
    }

    /// 分配 slug 时一并占下的计划文件路径(首份计划就写这里;`plan_path` 此前为 None)。
    /// 计划生成与修订使用同一路径，正文和任务图通过版本哈希绑定。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn reserved_plan_path(&self) -> String {
        crate::plan_doc::plan_rel_path(&self.slug)
    }

    pub fn touch(&mut self) {
        self.updated_at = now_rfc3339();
    }
}

/// `DebugSession.ultraplan` 的宽松反序列化:状态块读不出来(类型不符等)→ 当作没有流程。
///
/// sessions.json 逐条解析,任何一条失败整条会话就从列表里消失(`read_sessions_file`);
/// 不能让一块附属状态的损坏把用户的整段对话弄丢。流程产物在磁盘上,重新开始即可接回。
pub fn deserialize_lenient<'de, D>(de: D) -> Result<Option<UltraPlanState>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<Value>::deserialize(de)?;
    Ok(
        raw.and_then(|v| match serde_json::from_value::<UltraPlanState>(v) {
            Ok(s) if !s.id.is_empty() => Some(s),
            Ok(_) => None,
            Err(e) => {
                eprintln!("[ultraplan] 会话流程状态解析失败,按无流程处理: {e}");
                None
            }
        }),
    )
}

/// 设想原文 → 标题:首个非空行压成单行,截到 [`TITLE_MAX_CHARS`]。
pub fn derive_title(brief: &str) -> String {
    let line = brief
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let title: String = line.chars().take(TITLE_MAX_CHARS).collect();
    if title.is_empty() {
        "未命名游戏".to_string()
    } else {
        title
    }
}

/// slug → 流程目录(工作区根相对)。
pub fn flow_dir_rel(slug: &str) -> String {
    format!("{ROOT_DIR}/{slug}")
}

/// 是否为本模块写出的流程目录形态:`.forge/ultraplan/<单层名>`。
///
/// 单层名只认 slug 能产出的字符:不含分隔符、`..`、控制字符与 Windows 非法字符。
/// 冒号尤其要拦——Windows 上 `PathBuf::push("C:x")` 会把整条路径换到那个盘符下,
/// 一个被手改过的 `dir` 就能把读写带出工作区。
pub fn is_flow_dir(rel: &str) -> bool {
    let norm = rel.replace('\\', "/");
    let Some(tail) = norm.strip_prefix(&format!("{ROOT_DIR}/")) else {
        return false;
    };
    !tail.is_empty()
        && !tail.contains("..")
        && !tail
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// 无 slug 可用字符时的兜底名(设想全是标点/空白)。
const SLUG_FALLBACK: &str = "game";

/// 标题 → slug 主体:取前 [`SLUG_BRIEF_CHARS`] 字,交 `plan_doc::slugify` 清洗
/// (路径分隔符、Windows 非法字符、点号一律换 `-`,`..` 逃逸随之根除),再把其余标点也收成 `-`,
/// 只留字母数字(含中文)与下划线。这个目录名会被模型在 read_file 里逐字照抄,
/// 逗号括号之类留着只会招来抄错。
pub fn slug_base(title: &str) -> String {
    let head: String = title.trim().chars().take(SLUG_BRIEF_CHARS).collect();
    // slugify 对「没有可用字符」的输入回落「plan-时间戳」,那是计划文件的口径;
    // 流程目录用固定兜底名,唯一性交给后缀。
    if !head.chars().any(|c| c.is_alphanumeric() || c == '_') {
        return SLUG_FALLBACK.to_string();
    }
    let mut out = String::new();
    for ch in crate::plan_doc::slugify(&head).chars() {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// 该 slug 是否已被占:流程目录或同名计划文件任一存在即算(两处都不能覆盖,I-6)。
fn slug_taken(ws_root: &Path, slug: &str) -> bool {
    crate::plan_doc::abs_path(ws_root, &flow_dir_rel(slug)).exists()
        || crate::plan_doc::abs_path(ws_root, &crate::plan_doc::plan_rel_path(slug)).exists()
}

/// 分配唯一 slug:`<主体>-<id 末 4 位>`;撞了就重掷 4 位随机 hex,再不行挂计数后缀。
///
/// 只查不建:目录由随后的 brief.md 写入创建。两条流程同毫秒、同标题、又掷出同一组 4 位 hex
/// 才会并发撞上,概率可忽略;真撞上也只是共用目录,后写的 brief 覆盖先写的——记作已知边界。
pub fn allocate_slug(ws_root: &Path, title: &str, id: &str) -> String {
    let base = slug_base(title);
    let tail = id_tail(id);
    let mut suffix = tail.clone();
    for _ in 0..SLUG_REROLLS {
        let slug = format!("{base}-{suffix}");
        if !slug_taken(ws_root, &slug) {
            return slug;
        }
        suffix = random_hex(4);
    }
    let mut n = 2usize;
    loop {
        let slug = format!("{base}-{tail}-{n}");
        if !slug_taken(ws_root, &slug) {
            return slug;
        }
        n += 1;
    }
}

/// id 末 4 个字母数字(`up_<毫秒>_<8hex>` 的尾巴);形态不符时现掷。
fn id_tail(id: &str) -> String {
    let tail: String = id
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if tail.len() == 4 {
        tail.to_ascii_lowercase()
    } else {
        random_hex(4)
    }
}

/// n 位小写 hex 随机串(n ≤ 64)。
///
/// 本仓没有 rand/uuid 依赖(events::new_id 同样是手搓),这一波也不为它加依赖:熵取自
/// `RandomState`——其 SipHash 密钥由操作系统随机源播种、对外不可见——取四个独立实例的输出,
/// 连同纳秒时钟、pid 与进程内计数一起过 SHA-256。对「本机回环上的 Demo 路径令牌」这个用途
/// (防同机其它页面猜路径)强度足够;它不是通用密码学随机源,别拿去做密钥。
fn random_hex(n: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut seed: Vec<u8> = Vec::with_capacity(64);
    for _ in 0..4 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        seed.extend_from_slice(&h.finish().to_le_bytes());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    seed.extend_from_slice(&now.as_nanos().to_le_bytes());
    seed.extend_from_slice(&u64::from(std::process::id()).to_le_bytes());
    let hex = forge_util::hashutil::sha256_hex(&seed);
    hex.chars().take(n.min(hex.len())).collect()
}

/// Demo 托管路径令牌:32 位小写 hex(128 bit)。
pub fn new_token() -> String {
    random_hex(32)
}

/// `ultraplan.stage` 事件载荷(契约 §4;`effort` / `thinkingForced` 由调用方按需追加)。
pub fn stage_payload(up: &UltraPlanState, run_id: Option<&str>) -> Value {
    let mut v = json!({
        "id": up.id,
        "stage": up.stage,
        "phase": up.phase,
        "running": up.running,
    });
    if let Some(r) = run_id {
        v["runId"] = json!(r);
    }
    if let Some(e) = &up.last_error {
        v["lastError"] = json!(e);
    }
    v
}

// ---------------- 路由 ----------------

/// `ask:execute` 里 `ultraplan.action` 的七个取值(契约 §2)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Answer,
    ApproveDemo,
    ReviseDemo,
    RevisePlan,
    StartProduction,
    ResumeProduction,
    FixProduction,
}

impl Action {
    pub const ALL: [Action; 7] = [
        Action::Answer,
        Action::ApproveDemo,
        Action::ReviseDemo,
        Action::RevisePlan,
        Action::StartProduction,
        Action::ResumeProduction,
        Action::FixProduction,
    ];

    /// wire 字符串 → 动作;未知值 → None(调用方回 400 INVALID_INPUT)。
    pub fn parse(s: &str) -> Option<Self> {
        Action::ALL.into_iter().find(|a| a.as_str() == s.trim())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Action::Answer => "answer",
            Action::ApproveDemo => "approve_demo",
            Action::ReviseDemo => "revise_demo",
            Action::RevisePlan => "revise_plan",
            Action::StartProduction => "start_production",
            Action::ResumeProduction => "resume_production",
            Action::FixProduction => "fix_production",
        }
    }

    /// 该动作必须搭配的 composer 模式。
    pub fn required_mode(self) -> &'static str {
        match self {
            Action::Answer | Action::ApproveDemo | Action::ReviseDemo | Action::RevisePlan => MODE,
            Action::StartProduction | Action::ResumeProduction | Action::FixProduction => {
                PRODUCTION_MODE
            }
        }
    }

    /// 该动作唯一合法的阶段。
    pub fn required_stage(self) -> &'static str {
        match self {
            Action::Answer => STAGE_QUESTIONNAIRE,
            Action::ApproveDemo | Action::ReviseDemo => STAGE_DEMO_REVIEW,
            Action::RevisePlan | Action::StartProduction => STAGE_PLAN_REVIEW,
            Action::ResumeProduction => STAGE_PRODUCTION,
            Action::FixProduction => STAGE_ACCEPTANCE,
        }
    }

    /// 请求里的 `rev` 必须等于状态上的哪个计数(契约 §2 表末列;None = 该动作不带 rev)。
    pub fn expected_rev(self, up: &UltraPlanState) -> Option<u32> {
        match self {
            Action::Answer => Some(up.questionnaire_rev),
            Action::ApproveDemo | Action::ReviseDemo => Some(up.demo_iteration),
            Action::RevisePlan | Action::StartProduction => Some(up.plan_rev),
            Action::ResumeProduction => None,
            Action::FixProduction => Some(up.acceptance_round),
        }
    }

    /// 点按钮触发的动作没有用户正文时,`composer.user.message` 里显示的那句话(契约 §2:
    /// 动作在场时 userInput 可空,由服务端写展示文案)。
    pub fn display_text(self) -> &'static str {
        match self {
            Action::Answer => "已提交问卷答案",
            Action::ApproveDemo => "Demo 通过,请开始制定制作计划",
            Action::ReviseDemo => "请按反馈修改 Demo",
            Action::RevisePlan => "请按反馈修改制作计划",
            Action::StartProduction => "确认制作计划,开始制作",
            Action::ResumeProduction => "继续制作",
            Action::FixProduction => "人工验收未通过,请修复",
        }
    }

    /// 该动作是否必须带用户正文(修改意见就是 userInput,没有单独的 feedback 字段)。
    pub fn requires_text(self) -> bool {
        matches!(self, Action::ReviseDemo | Action::RevisePlan)
    }

    /// 本动作对应的轮次种类(阶段已由 [`route`] 校验过)。
    fn turn_kind(self) -> TurnKind {
        match self {
            Action::Answer => TurnKind::SpecAndDemo { revise: false },
            Action::ApproveDemo => TurnKind::Planning { revise: false },
            Action::ReviseDemo => TurnKind::SpecAndDemo { revise: true },
            Action::RevisePlan => TurnKind::Planning { revise: true },
            Action::StartProduction => TurnKind::Production(ProductionPhase::Start),
            Action::ResumeProduction => TurnKind::Production(ProductionPhase::Resume),
            Action::FixProduction => TurnKind::Production(ProductionPhase::Fix),
        }
    }
}

/// 制作轮的三种入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionPhase {
    Start,
    Resume,
    Fix,
}

impl ProductionPhase {
    /// `ultraplan.production.started.phase` 的取值。
    #[allow(dead_code)] // D-044 W4(制作)接线
    pub fn as_str(self) -> &'static str {
        match self {
            ProductionPhase::Start => "start",
            ProductionPhase::Resume => "resume",
            ProductionPhase::Fix => "fix",
        }
    }
}

/// 一轮 UltraPlan 轮次做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    /// 讨论设想并出问卷。`fresh` = 开一条新流程;否则是补充说明 / 要求重出问卷。
    Discovery { fresh: bool },
    /// 定稿需求并派单个子代理做网页 Demo。`revise` = 带反馈重做。
    SpecAndDemo { revise: bool },
    /// 写制作计划。`revise` = 带反馈改计划。
    Planning { revise: bool },
    /// Team 模式制作(开始 / 续跑 / 验收未过的返修)。
    Production(ProductionPhase),
}

impl TurnKind {
    /// 对应状态 `running` 字段的取值。
    pub fn running(self) -> &'static str {
        match self {
            TurnKind::Discovery { .. } => RUNNING_DISCOVERY,
            TurnKind::SpecAndDemo { .. } => RUNNING_SPEC_DEMO,
            TurnKind::Planning { .. } => RUNNING_PLANNING,
            TurnKind::Production(_) => RUNNING_PRODUCTION,
        }
    }

    /// 本轮所跑的 composer 模式。
    pub fn mode(self) -> &'static str {
        match self {
            TurnKind::Production(_) => PRODUCTION_MODE,
            _ => MODE,
        }
    }

    /// 中文环节名(错误消息与提示用)。
    pub fn label(self) -> &'static str {
        match self {
            TurnKind::Discovery { .. } => "立项讨论与问卷",
            TurnKind::SpecAndDemo { .. } => "需求定稿与 Demo",
            TurnKind::Planning { .. } => "制作计划",
            TurnKind::Production(_) => "制作",
        }
    }

    /// 本轮是否带多轮对话历史。只有 Discovery 带:用户的设想与补充说明就在对话里;
    /// 其余轮次以流程产物(brief / answers / spec …)为唯一来源,再带历史等于把设想喂两遍。
    pub fn wants_history(self) -> bool {
        matches!(self, TurnKind::Discovery { .. })
    }

    /// leader 是否用「深度规划」规格(思考强制开 + 最强 effort)。制作轮是调度不是规划,不用。
    pub fn deep_planning(self) -> bool {
        !matches!(self, TurnKind::Production(_))
    }
}

/// 阶段不匹配(409 `ULTRAPLAN_STAGE_MISMATCH` 的 `details`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageMismatch {
    /// 当前阶段;无流程 → None(wire 上为 null)。
    pub stage: Option<String>,
    /// 当前阶段可用的操作(见 [`allowed_actions`])。
    pub allowed: Vec<String>,
}

impl StageMismatch {
    pub fn at(stage: Option<&str>) -> Self {
        StageMismatch {
            stage: stage.map(str::to_string),
            allowed: allowed_actions(stage)
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn details(&self) -> Value {
        json!({ "stage": self.stage, "allowed": self.allowed })
    }
}

/// [`route`] 的拒绝原因。两种都不产生轮次、不触状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// 动作发在了错的模式上(如 `answer` 却是 team 模式)。
    WrongMode {
        action: Action,
        required: &'static str,
        mismatch: StageMismatch,
    },
    /// 当前阶段不接受这个动作 / 这段自由文本。
    Mismatch(StageMismatch),
    /// 请求指向的流程或版本已经不是当前这个(id / rev 对不上,或认领 run 之后发现状态变了)。
    /// 与阶段不配同码同体,只是消息说清「请刷新」——前端据 details 重新取状态即可。
    Stale {
        reason: String,
        mismatch: StageMismatch,
    },
}

impl RouteError {
    /// HTTP 错误码。契约 §2 明文:「Wrong id / rev / stage / mode → 409 ULTRAPLAN_STAGE_MISMATCH」,
    /// 所以模式不配与阶段不配同码(早期设计稿把模式不配写成 400 INVALID_INPUT,以冻结的契约为准;
    /// 变体仍分开,便于消息说清是哪一种)。400 INVALID_INPUT 留给「action 字符串本身不认识」。
    pub fn code(&self) -> &'static str {
        "ULTRAPLAN_STAGE_MISMATCH"
    }

    pub fn status(&self) -> StatusCode {
        StatusCode::CONFLICT
    }

    pub fn mismatch(&self) -> &StageMismatch {
        match self {
            RouteError::WrongMode { mismatch, .. } => mismatch,
            RouteError::Mismatch(m) => m,
            RouteError::Stale { mismatch, .. } => mismatch,
        }
    }

    pub fn message(&self) -> String {
        let at = |m: &StageMismatch| match m.stage.as_deref() {
            Some(s) => format!("当前阶段 {s}"),
            None => "本会话没有进行中的 UltraPlan 流程".to_string(),
        };
        match self {
            RouteError::WrongMode {
                action,
                required,
                mismatch,
            } => format!(
                "操作 {} 需在 {required} 模式下发送({})",
                action.as_str(),
                at(mismatch)
            ),
            RouteError::Mismatch(m) => format!(
                "{},不接受该操作;可用操作: {}",
                at(m),
                if m.allowed.is_empty() {
                    "无".to_string()
                } else {
                    m.allowed.join(" / ")
                }
            ),
            RouteError::Stale { reason, mismatch } => {
                format!("{reason}({});请刷新后重试", at(mismatch))
            }
        }
    }
}

impl IntoResponse for RouteError {
    fn into_response(self) -> Response {
        err_response(
            self.status(),
            self.code(),
            self.message(),
            Some(self.mismatch().details()),
        )
    }
}

/// 自由文本在 `allowed` 清单里的名字(不是契约动作,仅供 409 details 告诉前端「这一步可以直接发消息」)。
pub const ALLOWED_FREE_TEXT: &str = "free_text";

/// 某阶段可用的操作:契约动作名,外加 [`ALLOWED_FREE_TEXT`](该阶段接受 ultraplan 模式下的自由文本时)。
pub fn allowed_actions(stage: Option<&str>) -> Vec<&'static str> {
    match stage {
        None | Some(STAGE_DONE) | Some(STAGE_DISCOVERY) => vec![ALLOWED_FREE_TEXT],
        Some(STAGE_QUESTIONNAIRE) => vec![Action::Answer.as_str(), ALLOWED_FREE_TEXT],
        Some(STAGE_DEMO_REVIEW) => vec![
            Action::ApproveDemo.as_str(),
            Action::ReviseDemo.as_str(),
            ALLOWED_FREE_TEXT,
        ],
        Some(STAGE_PLAN_REVIEW) => vec![
            Action::RevisePlan.as_str(),
            Action::StartProduction.as_str(),
            ALLOWED_FREE_TEXT,
        ],
        Some(STAGE_PRODUCTION) => vec![Action::ResumeProduction.as_str()],
        Some(STAGE_ACCEPTANCE) => vec![Action::FixProduction.as_str()],
        Some(_) => Vec::new(),
    }
}

/// 纯路由:(当前阶段, 请求模式, 请求动作) → 本轮种类(契约 §2 的表)。
///
/// - `Ok(None)`:普通轮次(其它模式、且没带动作),不碰流程状态;
/// - 带动作的请求**永不**开新流程:没有流程(`stage == None`)时带任何动作都是不匹配;
/// - ultraplan 模式下的自由文本:无流程 / `done` → 新流程;`discovery` → 补充说明;
///   `questionnaire` → 重出问卷;`demo_review` → 改 Demo;`plan_review` → 改计划;
///   `production` / `acceptance` → 不匹配(这两步没有「说句话」的入口,只有续跑 / 返修动作)。
///
/// id / rev 的核对不在这里(需要完整状态),由调用方用 [`Action::expected_rev`] 做。
pub fn route(
    stage: Option<&str>,
    mode: &str,
    action: Option<Action>,
) -> Result<Option<TurnKind>, RouteError> {
    match action {
        Some(a) => {
            if mode != a.required_mode() {
                return Err(RouteError::WrongMode {
                    action: a,
                    required: a.required_mode(),
                    mismatch: StageMismatch::at(stage),
                });
            }
            if stage == Some(a.required_stage()) {
                Ok(Some(a.turn_kind()))
            } else {
                Err(RouteError::Mismatch(StageMismatch::at(stage)))
            }
        }
        None if mode != MODE => Ok(None),
        None => match stage {
            None | Some(STAGE_DONE) => Ok(Some(TurnKind::Discovery { fresh: true })),
            Some(STAGE_DISCOVERY) | Some(STAGE_QUESTIONNAIRE) => {
                Ok(Some(TurnKind::Discovery { fresh: false }))
            }
            Some(STAGE_DEMO_REVIEW) => Ok(Some(TurnKind::SpecAndDemo { revise: true })),
            Some(STAGE_PLAN_REVIEW) => Ok(Some(TurnKind::Planning { revise: true })),
            Some(_) => Err(RouteError::Mismatch(StageMismatch::at(stage))),
        },
    }
}

/// 契约 §2 末条:流程停在 plan_review..acceptance 时,不许用普通 build/plan 轮去动流程自己的计划
/// (那会绕过 planHash 校验与制作阶段机)。
pub fn blocks_plain_plan_turn(
    up: Option<&UltraPlanState>,
    mode: &str,
    plan_path: Option<&str>,
) -> bool {
    let requested = plan_path.map(str::trim).filter(|p| !p.is_empty());
    let (Some(up), Some(requested)) = (up, requested) else {
        return false;
    };
    matches!(mode, "build" | "plan")
        && matches!(
            up.stage.as_str(),
            STAGE_PLAN_REVIEW | STAGE_PRODUCTION | STAGE_ACCEPTANCE
        )
        && up
            .plan_path
            .as_deref()
            .is_some_and(|p| p.replace('\\', "/") == requested.replace('\\', "/"))
}

// ---------------- 项目事实 ----------------

/// 扫 docs/ 的深度与篇数上限(只为判断「有没有文档」与给个量级,不做全量统计)。
const DOCS_MAX_DEPTH: usize = 6;
const DOCS_MAX_COUNT: usize = 500;

/// 服务端注入给 leader 的项目事实(只靠文件系统扫描,不触 MCP/引擎)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFacts {
    /// 工作区里有没有**真的** Forge 项目:项目根就是工作区根且有 forge.toml。
    /// 工作区没有项目时 scope 会悄悄退到 `projects/demo`——那不是用户的项目(哪怕它在工作区目录里)。
    pub has_project: bool,
    /// `2d` | `3d`;没有项目或清单读不出来 → None(由问卷向用户确认)。
    pub game_mode: Option<String>,
    /// 项目实际使用的后端(含显式环境覆盖);在问卷中由用户确认，不能留到定稿时由模型选择。
    pub render_backend: Option<String>,
    /// Content 下的资产数(不含场景、脚本、.meta 与点文件)。
    pub assets: usize,
    pub scenes: usize,
    /// 脚本 / 逻辑图(.rx / .rs / .rxgraph)。
    pub scripts: usize,
    /// 项目根 README*.md 与 docs/**/*.md。
    pub docs: usize,
    /// 项目是否已有值得先摸清的内容:为真则出问卷前必须先派 explore。
    pub has_content: bool,
    /// 扫描失败的原因。失败**不**等于空项目:此时 has_content 置真,保住 explore 要求。
    pub scan_error: Option<String>,
}

impl ProjectFacts {
    fn none() -> Self {
        ProjectFacts {
            has_project: false,
            game_mode: None,
            render_backend: None,
            assets: 0,
            scenes: 0,
            scripts: 0,
            docs: 0,
            has_content: false,
            scan_error: None,
        }
    }

    /// 注入上下文的事实段(中文;leader 据此决定要不要派 explore、要不要问 2D/3D)。
    /// 轮次注入走 [`assemble`](段头由它加,所以用 [`Self::render_body`]);带段头的整段
    /// 目前只有单测直接断言。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn render(&self) -> String {
        format!(
            "【项目事实】(服务端扫描所得,以此为准)\n{}",
            self.render_body()
        )
    }

    /// 事实段正文(不带段头;[`assemble`] 会自己加段头,避免出现两行标题)。
    pub fn render_body(&self) -> String {
        let counts = format!(
            "资产 {} 个(不含场景与脚本)、场景 {} 个、脚本/逻辑图 {} 个、文档 {} 篇",
            self.assets, self.scenes, self.scripts, self.docs
        );
        let mut s = String::new();
        if let Some(err) = &self.scan_error {
            s.push_str(&format!(
                "- 项目扫描失败:{err}。无法确认项目是否为空,按「已有内容」处理。\n"
            ));
        }
        if self.has_project {
            match &self.game_mode {
                Some(m) => s.push_str(&format!(
                    "- 当前工作区已有 Forge 项目,维度模式 {m}(保持既有维度,在问卷中确认后端技术栈)。\n"
                )),
                None => s.push_str(
                    "- 当前工作区已有 Forge 项目,但 forge.toml 读不出维度模式,需先修复项目配置再确认技术栈。\n",
                ),
            }
            if let Some(backend) = &self.render_backend {
                s.push_str(&format!(
                    "- 当前正式实现后端为 {backend};问卷必须明确确认沿用此技术栈。\n"
                ));
            }
            s.push_str(&format!("- 现有内容:{counts}。\n"));
        } else if self.has_content {
            s.push_str(
                "- 当前工作区没有 forge.toml(项目尚未初始化),但已有 Content 内容;2D/3D 需在问卷里向用户确认。\n",
            );
            s.push_str(&format!("- 现有内容:{counts}。\n"));
        } else {
            s.push_str(
                "- 当前工作区还没有 Forge 项目(工作区根没有 forge.toml)。这是一个全新的游戏:\
                 2D/3D 需在问卷里向用户确认;进入制作阶段时系统会先初始化项目。\n",
            );
        }
        if self.has_content {
            s.push_str(
                "- 项目已有内容:出问卷前必须先在**同一轮**并行派发 2–4 个 task{subagent_type:\"explore\"} \
                 摸清现状(资产与美术风格 / 场景与实体 / 脚本与逻辑 / 文档与约定),再据实出问卷。\n",
            );
        } else {
            s.push_str(
                "- 项目没有需要先摸底的既有内容,无需派发 explore 子代理,直接构思并出问卷。\n",
            );
        }
        s
    }
}

/// 项目根是不是工作区**自己**的项目(D-044)。
///
/// `scope::project_root_of` 只有在工作区根本身像项目(有 forge.toml 或 Content/)时才返回工作区根;
/// 其余结果——`<ws>/projects/demo` 或编译期默认根——全是退回腿,不是用户的项目。退回腿可能就落在
/// 工作区**里面**(未绑定工作区的会话以仓根为工作区,仓内 demo 就在它下面),所以不能按「在不在
/// 工作区内」判,只能判「是不是工作区根本身」。canonicalize 失败(路径不存在)按不是处理。
fn is_workspace_project(workspace_root: &Path, project_root: &Path) -> bool {
    match (workspace_root.canonicalize(), project_root.canonicalize()) {
        (Ok(ws), Ok(project)) => ws == project,
        _ => false,
    }
}

/// 计算项目事实。`workspace_root` / `project_root` 取自 `scope::ScopeProject`。
///
/// 只有项目根就是工作区根才扫描(见 [`is_workspace_project`]):退回仓内 `projects/demo` 的那条腿
/// 不是用户的项目,既不算「有项目」,也不去数它的资产(否则一个全新工作区会被要求先 explore
/// 别人的 demo,还会把 demo 的 2D/3D 当成已定)。
pub fn project_facts(workspace_root: &Path, project_root: &Path) -> ProjectFacts {
    if !is_workspace_project(workspace_root, project_root) {
        return ProjectFacts::none();
    }
    let mut facts = ProjectFacts::none();
    facts.has_project = project_root.join("forge.toml").is_file();
    let project = match assetd::project::ForgeProject::load(project_root) {
        Ok(p) => {
            if facts.has_project {
                facts.game_mode = Some(p.mode.as_str().to_string());
                let (backend, method, driver) = p.render.as_strs();
                let env = |name| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
                let eb = env("FORGE_RENDER_BACKEND");
                let em = env("FORGE_RENDER_METHOD");
                let ed = env("FORGE_RENDER_DRIVER");
                match assetd::project::RenderConfig::from_parts(
                    eb.as_deref().or(Some(backend)),
                    em.as_deref().or(method),
                    ed.as_deref().or(driver),
                ) {
                    Ok((render, _)) => {
                        facts.render_backend = Some(render.as_strs().0.to_string());
                    }
                    Err(e) => {
                        facts.scan_error = Some(format!("正式实现后端配置无效: {e}"));
                    }
                }
            }
            p
        }
        Err(e) => {
            // 清单坏了:用缺省目录布局接着数,但把失败如实记下。
            facts.scan_error = Some(format!("forge.toml 解析失败: {e}"));
            assetd::project::ForgeProject::with_defaults(project_root.to_path_buf())
        }
    };
    match project.scan_content() {
        Ok(files) => {
            for rel in files {
                let name = rel.rsplit('/').next().unwrap_or(&rel);
                if name.starts_with('.') || is_os_junk(name) {
                    continue;
                }
                let ext = name
                    .rsplit_once('.')
                    .map(|(_, e)| e.to_ascii_lowercase())
                    .unwrap_or_default();
                match ext.as_str() {
                    "rxscene" => facts.scenes += 1,
                    "rx" | "rs" | "rxgraph" => facts.scripts += 1,
                    _ => facts.assets += 1,
                }
            }
        }
        Err(e) => {
            let msg = format!("Content 扫描失败: {e}");
            facts.scan_error = Some(match facts.scan_error.take() {
                Some(prev) => format!("{prev};{msg}"),
                None => msg,
            });
        }
    }
    facts.docs = count_docs(project_root);
    // 起始脚手架(project/init:一张场景、零资产)算空项目;扫描失败则宁可多探一次。
    facts.has_content = facts.scan_error.is_some()
        || facts.assets >= 1
        || facts.scenes >= 2
        || facts.scripts >= 1
        || facts.docs >= 1;
    facts
}

fn is_os_junk(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "thumbs.db" | "desktop.ini"
    )
}

/// 项目根 README*.md + docs/**/*.md 的篇数(封顶 [`DOCS_MAX_COUNT`])。
fn count_docs(project_root: &Path) -> usize {
    let mut n = 0usize;
    if let Ok(rd) = std::fs::read_dir(project_root) {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with("readme") && name.ends_with(".md") && ent.path().is_file() {
                n += 1;
            }
        }
    }
    count_md(&project_root.join("docs"), 0, &mut n);
    n
}

fn count_md(dir: &Path, depth: usize, n: &mut usize) {
    if depth > DOCS_MAX_DEPTH || *n >= DOCS_MAX_COUNT {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        if *n >= DOCS_MAX_COUNT {
            return;
        }
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
        if path.is_dir() {
            if !name.starts_with('.') {
                count_md(&path, depth + 1, n);
            }
        } else if name.ends_with(".md") {
            *n += 1;
        }
    }
}

// ---------------- 问卷 ----------------

pub const MAX_SECTIONS: usize = 8;
pub const MAX_QUESTIONS: usize = 40;
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 6;
/// scale 题量程上限(max - min):与前端分段按钮上限(21 个)对齐。
pub const MAX_SCALE_SPAN: i64 = 20;
const MAX_ID_CHARS: usize = 64;
const MAX_TITLE_CHARS: usize = 120;
const MAX_UNDERSTANDING_CHARS: usize = 20_000;
const MAX_QUESTION_CHARS: usize = 400;
const MAX_HELP_CHARS: usize = 600;
const MAX_LABEL_CHARS: usize = 120;
const MAX_DESCRIPTION_CHARS: usize = 400;
const MAX_SCALE_LABEL_CHARS: usize = 40;

pub const KIND_SINGLE: &str = "single";
pub const KIND_MULTI: &str = "multi";
pub const KIND_TEXT: &str = "text";
pub const KIND_SCALE: &str = "scale";
/// 服务端生成的正式实现技术栈确认题;禁止委托或用自由文本绕过确认。
pub const IMPLEMENTATION_STACK_QUESTION: &str = "implementation_stack";

/// 选择题的一个选项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub recommended: bool,
}

/// 一道题(契约 §5)。校验后是**归一形态**:缺省值已填实,前端与后续轮次都不必再猜缺省。
/// Deserialize 只用于读回已归一的 questionnaire.json;模型给的原始入参一律走
/// [`validate_questionnaire`](字段级 serde 缺省表达不了「text 选答、其余必答」这类按题型的缺省)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    /// 全问卷唯一(答案以它为键)。
    pub id: String,
    /// single | multi | text | scale
    pub kind: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// single / multi:2..=6 个;既有技术栈确认题可为 1 个;text / scale 无。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOption>>,
    /// 是否允许自填「其他」(仅 single / multi;缺省 false)。
    #[serde(default)]
    pub allow_other: bool,
    /// 是否允许「交给你决定」(缺省 true)。
    #[serde(default = "default_true")]
    pub allow_delegate: bool,
    /// 缺省:single / multi / scale 必答,text 选答。
    #[serde(default)]
    pub required: bool,
    /// multi:可选数量下界;scale:量程下界(缺省 1)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    /// multi:可选数量上界;scale:量程上界(缺省 5)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    /// scale 两端的文字(低端, 高端)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_labels: Option<[String; 2]>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionnaireSection {
    pub id: String,
    pub title: String,
    pub questions: Vec<Question>,
}

/// 问卷(契约 §5)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Questionnaire {
    pub title: String,
    /// Markdown:对设想的理解、所作假设、项目摸底所得。
    pub understanding: String,
    pub sections: Vec<QuestionnaireSection>,
}

impl Questionnaire {
    pub fn question_count(&self) -> usize {
        self.sections.iter().map(|s| s.questions.len()).sum()
    }

    /// 按题目 id 取题(W2 校验答案时用;目前只有单测用)。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn find(&self, question_id: &str) -> Option<&Question> {
        self.sections
            .iter()
            .flat_map(|s| s.questions.iter())
            .find(|q| q.id == question_id)
    }
}

fn implementation_stack(choice: &str) -> Option<(&'static str, &'static str)> {
    match choice {
        "2d_godot" => Some(("2d", "godot")),
        "3d_godot" => Some(("3d", "godot")),
        "3d_rurix" => Some(("3d", "rurix")),
        // A legacy project can be confirmed without silently migrating it.
        "2d_rurix" => Some(("2d", "rurix")),
        _ => None,
    }
}

/// The server owns this question so a model cannot omit backend confirmation,
/// allow delegation, or introduce an unsupported new-project combination.
fn ensure_implementation_stack(
    mut questionnaire: Questionnaire,
    facts: &ProjectFacts,
) -> Result<Questionnaire, String> {
    let option = |id: &str, label: &str, description: &str, recommended| QuestionOption {
        id: id.into(),
        label: label.into(),
        description: Some(description.into()),
        recommended,
    };
    let (question, help, options) = if facts.has_project {
        let mode = facts
            .game_mode
            .as_deref()
            .ok_or("现有项目的维度无法读取，请先修复 forge.toml 后重新生成问卷")?;
        let backend = facts
            .render_backend
            .as_deref()
            .ok_or("现有项目的后端无法读取，请先修复后端配置后重新生成问卷")?;
        let id = format!("{mode}_{backend}");
        if implementation_stack(&id).is_none() {
            return Err("现有项目的维度或后端不受支持".into());
        }
        (
            "请确认正式游戏沿用当前项目的后端技术栈。",
            "确认现有项目技术栈后再定稿；迁移后端需先单独修改项目并重新生成问卷。",
            vec![option(
                &id,
                &format!("确认沿用 {} · {}", mode.to_uppercase(), backend),
                "保留当前项目的维度、后端与既有内容。",
                true,
            )],
        )
    } else {
        (
            "正式游戏采用哪种维度与后端技术栈？",
            "2D 默认使用 Godot；3D 可选择 Godot 或 Rurix。此选择用于正式游戏实现，并在需求、计划和制作中保持一致。",
            vec![
                option(
                    "2d_godot",
                    "2D · Godot（默认）",
                    "使用 Godot 实现 2D 游戏。",
                    true,
                ),
                option("3d_godot", "3D · Godot", "使用 Godot 实现 3D 游戏。", false),
                option("3d_rurix", "3D · Rurix", "使用 Rurix 后端实现 3D 游戏。", false),
            ],
        )
    };
    let stack_question = Question {
        id: IMPLEMENTATION_STACK_QUESTION.into(),
        kind: KIND_SINGLE.into(),
        question: question.into(),
        help: Some(help.into()),
        options: Some(options),
        allow_other: false,
        allow_delegate: false,
        required: true,
        min: None,
        max: None,
        scale_labels: None,
    };
    for section in &mut questionnaire.sections {
        for q in &mut section.questions {
            if q.id == IMPLEMENTATION_STACK_QUESTION {
                *q = stack_question;
                return Ok(questionnaire);
            }
        }
    }
    if questionnaire.question_count() >= MAX_QUESTIONS {
        return Err(format!(
            "需为正式实现技术栈确认题预留 1 题，请将自定义问题减少至 {} 题以内",
            MAX_QUESTIONS - 1
        ));
    }
    questionnaire.sections[0]
        .questions
        .insert(0, stack_question);
    Ok(questionnaire)
}

/// Resolve only a user's explicit answer to the server's stack question. The
/// caller checks the result against the current project before persisting it.
pub fn confirmed_implementation_stack(
    questionnaire: &Value,
    answers: &Value,
) -> Result<(&'static str, &'static str), String> {
    let questionnaire = validate_questionnaire(questionnaire)?;
    let q = questionnaire
        .find(IMPLEMENTATION_STACK_QUESTION)
        .ok_or("问卷缺少正式实现技术栈确认题，请重新生成问卷并确认 2D/3D 与后端")?;
    if q.kind != KIND_SINGLE || !q.required || q.allow_delegate || q.allow_other {
        return Err("正式实现技术栈必须为必答单选题，且不能委托或自填其他答案".into());
    }
    let options = q.options.as_deref().unwrap_or_default();
    if !matches!(options.len(), 1 | 3)
        || options
            .iter()
            .any(|o| implementation_stack(&o.id).is_none())
        || (options.len() == 3 && options.iter().any(|o| o.id == "2d_rurix"))
    {
        return Err("正式实现技术栈确认题的选项无效，请重新生成问卷".into());
    }
    let answer = answers
        .get(IMPLEMENTATION_STACK_QUESTION)
        .and_then(Value::as_object)
        .ok_or("尚未明确确认正式实现的维度和后端技术栈")?;
    let selected = answer.get("choice").and_then(Value::as_array);
    if answer.len() != 1 || selected.is_none_or(|v| v.len() != 1) {
        return Err("必须明确选择一个正式实现技术栈，不能委托或混用答案".into());
    }
    let choice = selected
        .and_then(|v| v[0].as_str())
        .filter(|id| options.iter().any(|o| o.id == *id))
        .ok_or("正式实现技术栈选项无效")?;
    implementation_stack(choice).ok_or_else(|| "正式实现技术栈选项无效".into())
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// 必填字符串:缺失/非字符串/空白 → Err;超长 → Err(不静默截断模型写的内容)。
fn req_str(obj: &Value, key: &str, at: &str, max: usize) -> Result<String, String> {
    let s = match obj.get(key) {
        Some(Value::String(s)) => s.trim(),
        Some(Value::Null) | None => return Err(format!("{at}.{key} 必填(非空字符串)")),
        Some(_) => return Err(format!("{at}.{key} 须为字符串")),
    };
    if s.is_empty() {
        return Err(format!("{at}.{key} 不可为空"));
    }
    if char_len(s) > max {
        return Err(format!(
            "{at}.{key} 过长({} 字,上限 {max} 字),请精简",
            char_len(s)
        ));
    }
    Ok(s.to_string())
}

/// 可选字符串:缺失/null/空白 → None。
fn opt_str(obj: &Value, key: &str, at: &str, max: usize) -> Result<Option<String>, String> {
    let s = match obj.get(key) {
        Some(Value::String(s)) => s.trim(),
        Some(Value::Null) | None => return Ok(None),
        Some(_) => return Err(format!("{at}.{key} 须为字符串")),
    };
    if s.is_empty() {
        return Ok(None);
    }
    if char_len(s) > max {
        return Err(format!(
            "{at}.{key} 过长({} 字,上限 {max} 字),请精简",
            char_len(s)
        ));
    }
    Ok(Some(s.to_string()))
}

fn opt_bool(obj: &Value, key: &str, at: &str) -> Result<Option<bool>, String> {
    match obj.get(key) {
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(format!("{at}.{key} 须为布尔值 true/false")),
    }
}

fn opt_int(obj: &Value, key: &str, at: &str) -> Result<Option<i64>, String> {
    match obj.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            .map(Some)
            .ok_or_else(|| format!("{at}.{key} 须为整数")),
    }
}

/// id:必填、≤64 字、不含空白与控制字符(它是答案 JSON 的键,也会出现在文件里)。
fn req_id(obj: &Value, at: &str) -> Result<String, String> {
    let id = req_str(obj, "id", at, MAX_ID_CHARS)?;
    if id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!(
            "{at}.id 不可含空白字符(建议小写英文加下划线,如 core_loop)"
        ));
    }
    Ok(id)
}

/// 校验并归一 `ultraplan_questionnaire` 的入参(契约 §5)。
///
/// 错误信息是写给模型看的:指明哪一节哪一题的哪个字段、该改成什么,模型照着改即可重新提交。
/// 归一:补全 allowOther/allowDelegate/required/scale 量程/multi 数量界的缺省;
/// text / scale 题上多写的 options 直接丢弃(无害的多余字段不值得打回一轮)。
pub fn validate_questionnaire(v: &Value) -> Result<Questionnaire, String> {
    if !v.is_object() {
        return Err("参数须为对象:{ title, understanding, sections }".to_string());
    }
    let title = req_str(v, "title", "问卷", MAX_TITLE_CHARS)?;
    let understanding = req_str(v, "understanding", "问卷", MAX_UNDERSTANDING_CHARS)?;
    let Some(raw_sections) = v.get("sections").and_then(Value::as_array) else {
        return Err("问卷.sections 必填(数组,1–8 节)".to_string());
    };
    if raw_sections.is_empty() || raw_sections.len() > MAX_SECTIONS {
        return Err(format!(
            "问卷.sections 须为 1–{MAX_SECTIONS} 节(实 {} 节);请合并或拆分章节",
            raw_sections.len()
        ));
    }
    let mut section_ids: HashSet<String> = HashSet::new();
    let mut question_ids: HashSet<String> = HashSet::new();
    let mut sections: Vec<QuestionnaireSection> = Vec::new();
    let mut total = 0usize;
    for (si, rs) in raw_sections.iter().enumerate() {
        let at = format!("sections[{si}]");
        if !rs.is_object() {
            return Err(format!("{at} 须为对象:{{ id, title, questions }}"));
        }
        let id = req_id(rs, &at)?;
        if !section_ids.insert(id.clone()) {
            return Err(format!("{at}.id「{id}」与前面的章节重复,章节 id 须唯一"));
        }
        let stitle = req_str(rs, "title", &at, MAX_TITLE_CHARS)?;
        let Some(raw_questions) = rs.get("questions").and_then(Value::as_array) else {
            return Err(format!("{at}.questions 必填(数组,至少 1 题)"));
        };
        if raw_questions.is_empty() {
            return Err(format!(
                "{at}.questions 不可为空:每节至少 1 题,否则删掉这一节"
            ));
        }
        total += raw_questions.len();
        if total > MAX_QUESTIONS {
            return Err(format!(
                "问卷题目总数超过 {MAX_QUESTIONS} 题;请删去次要问题或把能自行决定的事项写进 understanding"
            ));
        }
        let mut questions: Vec<Question> = Vec::new();
        for (qi, rq) in raw_questions.iter().enumerate() {
            let qat = format!("{at}.questions[{qi}]");
            let q = validate_question(rq, &qat)?;
            if !question_ids.insert(q.id.clone()) {
                return Err(format!(
                    "{qat}.id「{}」与前面的题目重复,题目 id 须在整份问卷内唯一",
                    q.id
                ));
            }
            questions.push(q);
        }
        sections.push(QuestionnaireSection {
            id,
            title: stitle,
            questions,
        });
    }
    Ok(Questionnaire {
        title,
        understanding,
        sections,
    })
}

fn validate_question(rq: &Value, at: &str) -> Result<Question, String> {
    if !rq.is_object() {
        return Err(format!("{at} 须为对象:{{ id, kind, question, … }}"));
    }
    let id = req_id(rq, at)?;
    let kind = req_str(rq, "kind", at, 16)?;
    if !matches!(
        kind.as_str(),
        KIND_SINGLE | KIND_MULTI | KIND_TEXT | KIND_SCALE
    ) {
        return Err(format!(
            "{at}.kind「{kind}」不支持;只能是 single(单选)| multi(多选)| text(填空)| scale(打分)"
        ));
    }
    let question = req_str(rq, "question", at, MAX_QUESTION_CHARS)?;
    let help = opt_str(rq, "help", at, MAX_HELP_CHARS)?;
    let is_choice = matches!(kind.as_str(), KIND_SINGLE | KIND_MULTI);
    let allow_delegate = opt_bool(rq, "allowDelegate", at)?.unwrap_or(true);
    let required = opt_bool(rq, "required", at)?.unwrap_or(kind != KIND_TEXT);
    let allow_other = is_choice && opt_bool(rq, "allowOther", at)?.unwrap_or(false);

    let mut options: Option<Vec<QuestionOption>> = None;
    let mut min: Option<i64> = None;
    let mut max: Option<i64> = None;
    let mut scale_labels: Option<[String; 2]> = None;

    if is_choice {
        // Existing projects still require an explicit click to confirm their
        // established stack; only this server-owned single question may have
        // one option. Ordinary choice questions retain the 2-option minimum.
        let min_options = if id == IMPLEMENTATION_STACK_QUESTION && kind == KIND_SINGLE {
            1
        } else {
            MIN_OPTIONS
        };
        let Some(raw_opts) = rq.get("options").and_then(Value::as_array) else {
            return Err(format!(
                "{at}.options 必填:{kind} 题需要 {min_options}–{MAX_OPTIONS} 个选项"
            ));
        };
        if raw_opts.len() < min_options || raw_opts.len() > MAX_OPTIONS {
            return Err(format!(
                "{at}.options 须为 {min_options}–{MAX_OPTIONS} 个(实 {} 个);\
                 选项太多请合并,或开 allowOther 让用户自填",
                raw_opts.len()
            ));
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut opts: Vec<QuestionOption> = Vec::new();
        for (oi, ro) in raw_opts.iter().enumerate() {
            let oat = format!("{at}.options[{oi}]");
            if !ro.is_object() {
                return Err(format!(
                    "{oat} 须为对象:{{ id, label, description?, recommended? }}"
                ));
            }
            let oid = req_id(ro, &oat)?;
            if !seen.insert(oid.clone()) {
                return Err(format!("{oat}.id「{oid}」在本题内重复,选项 id 须唯一"));
            }
            opts.push(QuestionOption {
                id: oid,
                label: req_str(ro, "label", &oat, MAX_LABEL_CHARS)?,
                description: opt_str(ro, "description", &oat, MAX_DESCRIPTION_CHARS)?,
                recommended: opt_bool(ro, "recommended", &oat)?.unwrap_or(false),
            });
        }
        let recommended = opts.iter().filter(|o| o.recommended).count();
        if kind == KIND_SINGLE && recommended > 1 {
            return Err(format!(
                "{at} 是单选题,最多只能有 1 个 recommended 选项(实 {recommended} 个)"
            ));
        }
        if kind == KIND_MULTI {
            // 「其他」算一项,计入可选数量(与前端 multiBounds 同口径)。
            let slots = opts.len() as i64 + i64::from(allow_other);
            let lo = opt_int(rq, "min", at)?.unwrap_or(i64::from(required));
            let hi = opt_int(rq, "max", at)?.unwrap_or(slots);
            if lo < 0 || hi < 1 || lo > hi || hi > slots {
                return Err(format!(
                    "{at} 多选数量界不合法:须满足 0 ≤ min ≤ max ≤ {slots}(可选项数)且 max ≥ 1,实 min={lo} max={hi}"
                ));
            }
            min = Some(lo);
            max = Some(hi);
        }
        options = Some(opts);
    } else if kind == KIND_SCALE {
        let lo = opt_int(rq, "min", at)?.unwrap_or(1);
        let hi = opt_int(rq, "max", at)?.unwrap_or(5);
        if lo >= hi {
            return Err(format!(
                "{at} 打分量程须 min < max(实 min={lo} max={hi});缺省为 1..5"
            ));
        }
        if hi - lo > MAX_SCALE_SPAN {
            return Err(format!(
                "{at} 打分量程过宽(max - min 上限 {MAX_SCALE_SPAN},实 {});请用 1..5 或 0..10 这类量程",
                hi - lo
            ));
        }
        min = Some(lo);
        max = Some(hi);
        match rq.get("scaleLabels") {
            Some(Value::Null) | None => {}
            Some(Value::Array(a)) if a.len() == 2 && a.iter().all(Value::is_string) => {
                let pick = |i: usize| a[i].as_str().unwrap_or("").trim().to_string();
                let (l, h) = (pick(0), pick(1));
                if l.is_empty() || h.is_empty() {
                    return Err(format!("{at}.scaleLabels 两端文字都不可为空"));
                }
                if char_len(&l) > MAX_SCALE_LABEL_CHARS || char_len(&h) > MAX_SCALE_LABEL_CHARS {
                    return Err(format!(
                        "{at}.scaleLabels 每端上限 {MAX_SCALE_LABEL_CHARS} 字,请精简"
                    ));
                }
                scale_labels = Some([l, h]);
            }
            Some(_) => {
                return Err(format!(
                    "{at}.scaleLabels 须为恰好两个字符串的数组:[低端文字, 高端文字]"
                ))
            }
        }
    }

    Ok(Question {
        id,
        kind,
        question,
        help,
        options,
        allow_other,
        allow_delegate,
        required,
        min,
        max,
        scale_labels,
    })
}

// ---------------- 预算化上下文段 ----------------

/// 注入上下文的一段。
#[derive(Debug, Clone)]
pub struct Section {
    /// 段名(进事件留痕与段头)。
    pub name: String,
    pub text: String,
    /// 本段字符上限。
    pub cap: usize,
    /// 全文所在的工作区相对路径;截断时写进指针,模型可用 read_file 取全文。
    pub rel: Option<String>,
}

impl Section {
    pub fn new(name: &str, text: impl Into<String>, cap: usize) -> Self {
        Section {
            name: name.to_string(),
            text: text.into(),
            cap,
            rel: None,
        }
    }

    pub fn with_source(mut self, rel: &str) -> Self {
        self.rel = Some(rel.to_string());
        self
    }
}

/// 一段的注入结果(`ultraplan.context.injected.sections[]`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SectionReport {
    pub name: String,
    /// 实际注入的正文字符数(不含段头与截断指针)。
    pub chars: usize,
    pub truncated: bool,
}

/// 本轮流程上下文的有效预算:不超过 [`PREAMBLE_BUDGET_CHARS`],也不超过上下文窗口的约四分之一
/// (64K 窗口 → 16 384 字符;小窗口模型上别把问答空间挤没)。
pub fn effective_budget(context_tokens: u64) -> usize {
    PREAMBLE_BUDGET_CHARS.min((context_tokens / 4) as usize)
}

/// 截断指针文案。
fn truncation_pointer(rel: Option<&str>) -> String {
    match rel {
        Some(r) => format!("…(已截断,全文见 {r},用 read_file 读取)"),
        None => "…(已截断)".to_string(),
    }
}

/// 按优先序(入参顺序)贪心装配:每段取 min(本段 cap, 剩余预算) 个字符,保头截尾,
/// 截断处追加指针;预算用尽后的段整段不注入,但仍留一行指针,不让内容无声消失(I-5)。
///
/// 预算只计正文字符;段头与指针是固定小开销(每段几十字),不计入——否则「还剩 10 个字」
/// 时连指针都写不下,模型反而不知道有这份材料。空段跳过且不入报告。
pub fn assemble(sections: &[Section], budget: usize) -> (String, Vec<SectionReport>) {
    let mut out = String::new();
    let mut reports: Vec<SectionReport> = Vec::new();
    let mut remaining = budget;
    for sec in sections {
        let text = sec.text.trim();
        if text.is_empty() {
            continue;
        }
        let total = char_len(text);
        let allow = sec.cap.min(remaining);
        let (body, truncated): (String, bool) = if total <= allow {
            (text.to_string(), false)
        } else {
            (text.chars().take(allow).collect(), true)
        };
        let chars = char_len(&body);
        remaining -= chars;
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("【{}】\n", sec.name));
        out.push_str(body.trim_end());
        if truncated {
            if chars > 0 {
                out.push('\n');
            }
            out.push_str(&truncation_pointer(sec.rel.as_deref()));
        }
        reports.push(SectionReport {
            name: sec.name.clone(),
            chars,
            truncated,
        });
    }
    (out, reports)
}

// ---------------- 文件 ----------------

/// 原子写:同目录 tmp + rename,缺父目录则建(半截文件不可见;同 plan_doc::write_atomic 纪律)。
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// JSON 产物原子写(pretty:这些文件用户会直接打开看)。
pub fn write_json_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, &text)
}

/// 读 JSON 产物;不存在/读不出/不是合法 JSON → None(REST 面如实回 null,不报错)。
pub fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

// ---------------- 轮次接入:请求校验 ----------------

/// `ask:execute` 请求体里的 `ultraplan` 对象(契约 §2)。对象在场即表示「这是一个流程动作」。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UltraplanReq {
    /// 必须等于 `session.ultraplan.id`。
    #[serde(default)]
    pub id: String,
    /// 契约 §2 表末列。用 Value 收:类型写错也走 409 `ULTRAPLAN_STAGE_MISMATCH`
    /// (契约:wrong rev → 409),而不是被 axum 的反序列化拒成 422。
    #[serde(default)]
    pub rev: Option<Value>,
    #[serde(default)]
    pub action: String,
    /// 仅 `answer` 用;重试时可省(服务端复用本 rev 的 answers.json)。
    #[serde(default)]
    pub answers: Option<Value>,
    /// 权限模式为 auto 时开始/续跑/返修制作的显式确认。
    #[serde(default)]
    pub acknowledge_approvals: bool,
}

impl UltraplanReq {
    fn rev_u32(&self) -> Option<u32> {
        self.rev
            .as_ref()
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    }
}

/// 「深度规划」在本轮的实发情况:由 ask_execute 按**实际渠道**算好递进来,
/// 原样进 `ultraplan.stage` 事件(I-5:没强制成就如实说没强制成)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepPlanning {
    /// leader 步进实发的 reasoning_effort(None = 该渠道不收 / 没有可用档)。
    pub effort: Option<String>,
    /// 思考是否真的被强制打开了(模型没有思考可开 → false,并发 THINKING_UNAVAILABLE 提示)。
    pub thinking_forced: bool,
    /// leader 规格的上下文窗口(算注入预算用)。
    pub context_tokens: u64,
}

impl Default for DeepPlanning {
    fn default() -> Self {
        DeepPlanning {
            effort: None,
            thinking_forced: false,
            context_tokens: crate::modelspec::DEFAULT_CONTEXT_TOKENS,
        }
    }
}

/// D-044「深度规划」的退路。强制的思考档(openai-compat 的 `reasoning_effort=max`、云端最强档、
/// deepseek 换 reasoner)可能被端点按参数错误拒收:非推理模型不认这个字段,有的渠道只收
/// low/medium/high。4xx 不重试(`llm::is_transient_llm_error`),不兜一下整轮立项就失败了——
/// 同一会话的其它模式却照常能用。ask_execute 包 leader 步进时建它(见 [`with_deep_fallback`]),
/// 随 [`UltraTurn`] / [`UltraRuntime`] 进本轮:退过之后 `ultraplan.stage` 如实报「没强制成」。
#[derive(Debug, Default)]
pub struct DeepFallback {
    used: AtomicBool,
    /// 退回后实际生效的 effort(会话自己的规格;None = 请求体不带 reasoning_effort)。
    plain_effort: Option<String>,
}

impl DeepFallback {
    pub fn new(plain_effort: Option<String>) -> Self {
        DeepFallback {
            used: AtomicBool::new(false),
            plain_effort,
        }
    }

    /// 本轮是否已退回会话规格。
    pub fn used(&self) -> bool {
        self.used.load(Ordering::SeqCst)
    }

    /// 上报用的深度规划实发情况:没退过原样返回;退过 = 没强制成,effort 是会话自己的那一档。
    pub fn apply(&self, deep: &DeepPlanning) -> DeepPlanning {
        if !self.used() {
            return deep.clone();
        }
        DeepPlanning {
            effort: self.plain_effort.clone(),
            thinking_forced: false,
            context_tokens: deep.context_tokens,
        }
    }
}

/// 端点拒收强制思考档的错误形态:HTTP 400 / 422(参数错误),且正文点到推理 / 思考档位、不支持的
/// 参数或非法取值。别的 4xx(鉴权、额度、模型不存在)退回会话规格也救不了,不在此列。
pub fn is_effort_rejection(error: &str) -> bool {
    let status = error
        .split("HTTP ")
        .nth(1)
        .and_then(|rest| rest.get(..3))
        .and_then(|s| s.parse::<u16>().ok());
    if !matches!(status, Some(400 | 422)) {
        return false;
    }
    let lower = error.to_lowercase();
    [
        "reasoning",
        "reasoner",
        "effort",
        "thinking",
        "unsupported",
        "not support",
        "unknown parameter",
        "unrecognized",
        "invalid value",
        "invalid_value",
        "extra_forbidden",
        "extra inputs",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

/// leader 步进包装:先按深度规划规格 `deep` 发;端点拒收强制的思考档(见 [`is_effort_rejection`])→
/// 这一步改用会话规格 `plain` 重发,本轮余下的步进也一律走 `plain`(只退一次),并调一次
/// `on_fallback(原因)` 如实上报。其它错误原样返回(瞬时错误照常由工具循环重试)。
pub fn with_deep_fallback(
    deep: Box<crate::llm::StepFn>,
    plain: Box<crate::llm::StepFn>,
    fb: Arc<DeepFallback>,
    on_fallback: impl Fn(&str) + Send + Sync + 'static,
) -> Box<crate::llm::StepFn> {
    let deep: Arc<crate::llm::StepFn> = Arc::from(deep);
    let plain: Arc<crate::llm::StepFn> = Arc::from(plain);
    let on_fallback = Arc::new(on_fallback);
    Box::new(move |messages, tools, stream| {
        let deep = deep.clone();
        let plain = plain.clone();
        let fb = fb.clone();
        let on_fallback = on_fallback.clone();
        Box::pin(async move {
            if fb.used() {
                return plain(messages, tools, stream).await;
            }
            match deep(messages.clone(), tools.clone(), stream.clone()).await {
                Err(e) if is_effort_rejection(&e.to_string()) => {
                    if !fb.used.swap(true, Ordering::SeqCst) {
                        on_fallback(&e.to_string());
                    }
                    plain(messages, tools, stream).await
                }
                other => other,
            }
        })
    })
}

/// 深度规划退回会话规格时的上报([`with_deep_fallback`] 的回调;此刻本轮 run 仍挂在会话上):
/// 发 `ultraplan.notice {THINKING_UNAVAILABLE}`,再补一条更正了 effort / thinkingForced 的
/// `ultraplan.stage`(流程条上的实发档位跟着改)。会话不在 running 相位 → 什么都不发。
pub fn report_deep_fallback(
    state: &AppState,
    session_id: &str,
    deep: &DeepPlanning,
    fb: &DeepFallback,
    reason: &str,
) {
    let Some(session) = state.sessions.get(session_id) else {
        return;
    };
    let (Some(run_id), Some(up)) = (session.active_run_id.as_deref(), session.ultraplan.as_ref())
    else {
        return;
    };
    if up.phase != PHASE_RUNNING {
        return;
    }
    let reason: String = reason.chars().take(LAST_ERROR_MAX_CHARS).collect();
    emit_notice(
        state,
        session_id,
        run_id,
        &up.id,
        "THINKING_UNAVAILABLE",
        &format!(
            "模型渠道不接受深度规划的思考档位,本轮已改用会话自己的模型设置继续(未启用深度规划)。渠道返回:{reason}"
        ),
    );
    emit(
        state,
        session_id,
        "ultraplan.stage",
        stage_payload_with(up, Some(run_id), &fb.apply(deep)),
    );
}

/// 一轮 UltraPlan 轮次的输入([`resolve_request`] 产出,经 `TurnInput.ultraplan` 递给 execute_turn)。
///
/// 只是「已校验的意图」:此刻还没有任何副作用——没建状态、没写文件。副作用在认领 run、
/// 再核一次阶段之后才由 [`begin_turn`] 执行(认领前的副作用会在 SESSION_BUSY 时留下垃圾)。
#[derive(Debug, Clone)]
pub struct UltraTurn {
    pub kind: TurnKind,
    /// 请求带的动作;None = ultraplan 模式下的自由文本。
    pub action: Option<Action>,
    /// 请求带的 rev(已核对;进 `composer.user.message` 载荷)。
    pub rev: Option<u32>,
    /// 路由时刻的流程快照(None = 会话没有流程)。[`revalidate`] 据此判断状态是否被别处改过。
    pub flow: Option<UltraPlanState>,
    /// `answer` 动作带的答案(W2 的需求定稿轮消费)。
    #[allow(dead_code)] // D-044 W2 接线
    pub answers: Option<Value>,
    /// 权限模式 auto 下开始/续跑/返修制作的显式确认(W4 的制作轮消费)。
    #[allow(dead_code)] // D-044 W4 接线
    pub acknowledge_approvals: bool,
    pub facts: ProjectFacts,
    pub deep: DeepPlanning,
    /// 深度规划的退路(ask_execute 包了 leader 步进时才有;见 [`with_deep_fallback`])。
    pub deep_fallback: Option<Arc<DeepFallback>>,
}

impl UltraTurn {
    /// 此刻该上报的深度规划实发情况(中途退回过会话规格就如实报「没强制成」)。
    pub fn reported_deep(&self) -> DeepPlanning {
        match &self.deep_fallback {
            Some(fb) => fb.apply(&self.deep),
            None => self.deep.clone(),
        }
    }

    /// 用户卡上显示的正文:用户写了就用用户的;没写(点按钮触发的动作)用动作的展示文案。
    pub fn display_text<'a>(&self, user_input: &'a str) -> &'a str {
        match self.action {
            Some(action) if user_input.trim().is_empty() => action.display_text(),
            _ => user_input,
        }
    }

    /// `composer.user.message` 载荷里的 `ultraplan` 键(契约 §2:仅动作在场时有)。
    pub fn user_message_tag(&self) -> Option<Value> {
        let action = self.action?;
        Some(json!({
            "id": self.flow.as_ref().map(|f| f.id.as_str()),
            "action": action.as_str(),
            "rev": self.rev,
        }))
    }
}

/// 项目根是否就是工作区自己的项目(与 [`project_facts`] 同一判据,见 [`is_workspace_project`])。
/// 否 = scope 退到了 `projects/demo` 之类的退回腿(哪怕它就在工作区目录里):那不是用户的项目,
/// MCP 工具面与预检索此刻指向的都是它,UltraPlan 轮次不该让 leader 看到那份内容。
pub fn project_in_workspace(scope: &crate::scope::ScopeProject) -> bool {
    is_workspace_project(&scope.workspace_root, &scope.project_root)
}

/// 请求核对的纯核心:[`resolve_request`](起 run 前)与 [`revalidate`](认领 run 后)共用,
/// 两次判定同一口径。带动作的请求永不开新流程;id / rev 对不上与阶段不配同为 409。
fn check_request(
    up: Option<&UltraPlanState>,
    mode: &str,
    action: Option<Action>,
    req_id: Option<&str>,
    rev: Option<u32>,
) -> Result<Option<TurnKind>, RouteError> {
    let stage = up.map(|u| u.stage.as_str());
    let Some(a) = action else {
        return route(stage, mode, None);
    };
    let Some(up) = up else {
        return Err(RouteError::Mismatch(StageMismatch::at(None)));
    };
    if req_id.map(str::trim) != Some(up.id.as_str()) {
        return Err(RouteError::Stale {
            reason: "请求指向的不是本会话当前的 UltraPlan 流程(id 不一致)".to_string(),
            mismatch: StageMismatch::at(stage),
        });
    }
    let kind = route(stage, mode, Some(a))?;
    if let Some(expected) = a.expected_rev(up) {
        if rev != Some(expected) {
            return Err(RouteError::Stale {
                reason: format!(
                    "操作 {} 针对的版本已过期(请求 rev={},当前 rev={expected})",
                    a.as_str(),
                    rev.map(|r| r.to_string())
                        .unwrap_or_else(|| "缺失".to_string())
                ),
                mismatch: StageMismatch::at(stage),
            });
        }
    }
    Ok(kind)
}

/// `ask:execute` 的 UltraPlan 入口:在 mode / agentKind / Codex 三道门之后、**任何 run 创建之前**调用。
///
/// - `Ok(None)`:普通轮次(其它模式、没带 `ultraplan` 对象),不碰流程状态;
/// - `Ok(Some(turn))`:本轮是 UltraPlan 轮次,种类与校验结果都在里面,尚无任何副作用;
/// - `Err(response)`:当场 4xx——
///   400 `INVALID_INPUT`(action 不认识 / 修改类动作没写意见);
///   409 `ULTRAPLAN_STAGE_MISMATCH`(id / rev / 阶段 / 模式不配,`details {stage, allowed}`;
///   以及流程停在 plan_review..acceptance 时拿普通 build/plan 轮去动流程自己的计划);
/// 前端遇到版本或阶段不匹配时刷新流程状态，不重放已提交动作。
pub fn resolve_request(
    session: &crate::sessions::DebugSession,
    scope: &crate::scope::ScopeProject,
    mode: &str,
    req: Option<&UltraplanReq>,
    user_input: &str,
    plan_path: Option<&str>,
) -> Result<Option<UltraTurn>, Response> {
    let up = session.ultraplan.as_ref();
    let stage = up.map(|u| u.stage.as_str());
    let action = match req {
        Some(r) => match Action::parse(&r.action) {
            Some(a) => Some(a),
            None => {
                return Err(err_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_INPUT",
                    format!(
                        "未知 ultraplan.action: {:?}(支持 {})",
                        r.action,
                        Action::ALL.map(Action::as_str).join(" | ")
                    ),
                    None,
                ))
            }
        },
        None => None,
    };
    if action.is_none() && blocks_plain_plan_turn(up, mode, plan_path) {
        return Err(err_response(
            StatusCode::CONFLICT,
            "ULTRAPLAN_STAGE_MISMATCH",
            "该计划由 UltraPlan 流程管理,不能用普通的 build / plan 轮次直接实施或改写;\
             请在流程里确认并开始制作,或提出修改意见",
            Some(StageMismatch::at(stage).details()),
        ));
    }
    let rev = req.and_then(UltraplanReq::rev_u32);
    let kind = match check_request(up, mode, action, req.map(|r| r.id.as_str()), rev) {
        Ok(Some(kind)) => kind,
        Ok(None) => return Ok(None),
        Err(e) => return Err(e.into_response()),
    };
    if user_input.trim().is_empty() && action.map_or(true, Action::requires_text) {
        return Err(err_response(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            match action {
                Some(a) => format!("操作 {} 需要在 userInput 里写明修改意见", a.as_str()),
                None => "userInput 不可空".to_string(),
            },
            None,
        ));
    }
    Ok(Some(UltraTurn {
        kind,
        action,
        rev,
        flow: up.cloned(),
        answers: req.and_then(|r| r.answers.clone()),
        acknowledge_approvals: req.is_some_and(|r| r.acknowledge_approvals),
        facts: project_facts(&scope.workspace_root, &scope.project_root),
        deep: DeepPlanning::default(),
        deep_fallback: None,
    }))
}

/// 认领 run 之后的再核对:ask_execute 在路由之后、认领之前还要 await 好几步(拉工具面、预检索),
/// 这段时间里另一条请求可能已经把流程推进或重开了。拿最新会话状态按同一口径重判一次,
/// 对不上就让本轮放弃(释放 run、不发事件、不做任何副作用)。
pub fn revalidate(
    now: Option<&UltraPlanState>,
    mode: &str,
    ut: &UltraTurn,
) -> Result<(), RouteError> {
    let stage = now.map(|u| u.stage.as_str());
    let routed_on = ut.flow.as_ref().map(|u| u.id.as_str());
    if now.map(|u| u.id.as_str()) != routed_on {
        return Err(RouteError::Stale {
            reason: "流程在请求处理期间被重开或清除".to_string(),
            mismatch: StageMismatch::at(stage),
        });
    }
    // 动作请求的 id 已在 resolve_request 核对过,等于路由时的流程 id。
    match check_request(now, mode, ut.action, ut.action.and(routed_on), ut.rev) {
        Ok(Some(kind)) if kind == ut.kind => Ok(()),
        Ok(_) => Err(RouteError::Stale {
            reason: "流程阶段在请求处理期间发生了变化".to_string(),
            mismatch: StageMismatch::at(stage),
        }),
        Err(e) => Err(e),
    }
}

/// 会话是否有一条进行中的 UltraPlan 流程(stage != done)。目标续跑的暂停、PUT goal 的拒绝都以它为准。
pub fn flow_active(state: &AppState, session_id: &str) -> bool {
    state
        .sessions
        .get(session_id)
        .and_then(|s| s.ultraplan)
        .is_some_and(|u| u.is_active())
}

/// 流程进行中暂停目标时写进 `goal.note` 的说明(用户在 GoalBar 上直接看到这行)。
pub const GOAL_PAUSED_NOTE: &str = "UltraPlan 流程进行中,目标已暂停;流程完成或重新开始后可恢复";

/// 流程进行中拒绝设定 / 恢复目标:409 `ULTRAPLAN_GOAL_BLOCKED`。
/// 目标的自动续跑轮没有阶段路由,只会在流程的闸前空转,或者绕过阶段机去动项目。
pub fn goal_blocked_response() -> Response {
    err_response(
        StatusCode::CONFLICT,
        "ULTRAPLAN_GOAL_BLOCKED",
        "UltraPlan 流程进行中,不能设定或恢复自动推进的目标;请先完成流程,或在流程条上重新开始",
        None,
    )
}

/// 轮次级错误串 `CODE: 消息` → HTTP 拒绝(execute_turn 在认领后再核对失败时返回的就是这种串;
/// 那条分支在发任何事件之前返回,所以回 409 而不是「一轮失败的 turn」)。
/// 只认 `ULTRAPLAN_STAGE_MISMATCH`;`details` 按**此刻**的会话状态现算。
pub fn mismatch_response(now: Option<&UltraPlanState>, error: &str) -> Option<Response> {
    let message = error.strip_prefix("ULTRAPLAN_STAGE_MISMATCH:")?;
    Some(err_response(
        StatusCode::CONFLICT,
        "ULTRAPLAN_STAGE_MISMATCH",
        message.trim(),
        Some(StageMismatch::at(now.map(|u| u.stage.as_str())).details()),
    ))
}

// ---------------- 轮次接入:运行时 ----------------

/// explore 工种名(`agents/explore.md`)。
pub const EXPLORE_PROFILE: &str = "explore";
/// run_nested_task 对未知工种的报错片段;据此识别「explore 工种根本不存在」,不让门原地打转。
const UNKNOWN_PROFILE_MARK: &str = "未知 subagent_type";
/// 单份 explore 报告回注给 leader 的正文上限(字符)。工具反馈在 llm.rs 按 4000 字截断
/// (TOOL_FEEDBACK_MAX),「全文存在哪」的指针接在末尾——正文不先收到这个数以内,指针会被一并截掉。
const EXPLORE_FEEDBACK_BODY_MAX: usize = 3600;
/// explore 门拒绝这么多次之后放行(模型执意不探 / 探不成时,不让它打转到迭代上限)。
const EXPLORE_GATE_MAX_REJECTIONS: usize = 2;
/// `lastError.message` 上限(字符)。
const LAST_ERROR_MAX_CHARS: usize = 300;

/// Discovery 注入段的字符上限(design §2(j);事实段比设计稿的 800 放宽到 1200——
/// 「必须先派 explore」那句在事实段末尾,扫描报错文案一长就会把它截掉)。
const SECTION_FACTS_CAP: usize = 1200;
const SECTION_FLOW_CAP: usize = 800;
const SECTION_UNDERSTANDING_CAP: usize = 3000;
const SECTION_OUTLINE_CAP: usize = 1500;

/// 本轮的过程记录(父执行闭包与出口工具共享;只活一轮)。
#[derive(Debug, Default)]
pub struct TurnTracker {
    /// 串行化 explore 报告的编号分配与写盘(同一轮的 explore 是并发跑的)。
    explore_lock: Mutex<()>,
    /// 本轮落盘的 explore 报告数。
    explore_saved: AtomicUsize,
    /// 本轮有 explore 任务因「工种不存在」失败:这道门再拒也满足不了。
    explore_profile_missing: AtomicBool,
    /// 问卷因「还没 explore」被拒的次数。
    explore_rejections: AtomicUsize,
    /// EXPLORE_SKIPPED 提示是否已发(一轮只发一次)。
    explore_skip_noticed: AtomicBool,
    /// 本轮已分配的问卷 rev(同一轮内第二次提交覆盖同一 rev,见 [`handle_exit_tool`])。
    questionnaire_rev: Mutex<Option<u32>>,
    plan_document: Mutex<Option<Value>>,
    plan_tasks: Mutex<Option<Value>>,
}

/// 计数读取只给单测断言用(生产代码里门按落盘的报告判,不按计数判)。
#[cfg(test)]
impl TurnTracker {
    pub fn explore_saved(&self) -> usize {
        self.explore_saved.load(Ordering::Relaxed)
    }

    pub fn explore_rejections(&self) -> usize {
        self.explore_rejections.load(Ordering::Relaxed)
    }
}

/// 一轮 UltraPlan 轮次的运行时([`begin_turn`] 产出):提示词后缀、出口工具、注入段、过程记录。
#[derive(Debug)]
pub struct UltraRuntime {
    pub kind: TurnKind,
    pub flow_id: String,
    /// 流程目录(工作区根相对 / 绝对)。
    pub dir_rel: String,
    pub dir_abs: PathBuf,
    pub facts: ProjectFacts,
    pub deep: DeepPlanning,
    /// 深度规划的退路(同 [`UltraTurn::deep_fallback`]);出口工具发的 `ultraplan.stage` 据此如实报档位。
    pub deep_fallback: Option<Arc<DeepFallback>>,
    pub tracker: TurnTracker,
    /// 接在 SYSTEM_PROMPT 之后的阶段提示词。
    pub prompt_suffix: String,
    /// 本轮的出口工具 spec(每类轮次只有自己的那一个/一组)。
    pub exit_tool_specs: Vec<Value>,
    /// 预算化的流程上下文段(可能为空串)。
    pub preamble: String,
    pub sections: Vec<SectionReport>,
}

impl UltraRuntime {
    /// `ultraplan.stage` 载荷:状态面 + 本轮「深度规划」的实发情况(中途退回过会话规格就如实报)。
    fn stage_event(&self, up: &UltraPlanState, run_id: &str) -> Value {
        let deep = match &self.deep_fallback {
            Some(fb) => fb.apply(&self.deep),
            None => self.deep.clone(),
        };
        stage_payload_with(up, Some(run_id), &deep)
    }

    /// `ultraplan.context.injected` 载荷(回放要能查清本轮给模型注入了哪几段、各多长、截没截)。
    pub fn context_injected_payload(&self, run_id: &str) -> Value {
        json!({
            "runId": run_id,
            "id": self.flow_id,
            "kind": self.kind.running(),
            "sections": self.sections,
        })
    }

    /// 父执行闭包 `task` 分支的后处理:explore 子代理的**全文**落 `<dir>/explore/<n>.md`。
    ///
    /// 为什么要落盘:回给 leader 的工具反馈按 4000 字截断,下一轮的历史里更是只剩工具名——
    /// 不存下来,摸底结果出了这一轮就没了。返回改写后的反馈:正文收到
    /// [`EXPLORE_FEEDBACK_BODY_MAX`] 以内,末尾接「(完整报告已存 <rel>)」。
    /// 同一轮并行派发的 task 也走这里(llm.rs 的并行分支调的是同一个执行闭包)。
    pub fn after_task(&self, args: &Value, ok: bool, text: String) -> String {
        let sub_type = args
            .get("subagent_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if sub_type != EXPLORE_PROFILE {
            return text;
        }
        if !ok {
            if text.contains(UNKNOWN_PROFILE_MARK) {
                self.tracker
                    .explore_profile_missing
                    .store(true, Ordering::Relaxed);
            }
            return text;
        }
        if text.trim().is_empty() {
            return text;
        }
        let arg = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").trim();
        match self.save_explore_report(arg("description"), arg("prompt"), &text) {
            Ok(rel) => clip_with_tail(&text, &format!("(完整报告已存 {rel})")),
            // 落盘失败不拦调研本身:如实说没存成,这一份不计入「已探」。
            Err(e) => clip_with_tail(
                &text,
                &format!("(调研报告落盘失败: {e};以上即全部可用内容)"),
            ),
        }
    }

    /// 写一份 explore 报告,返回其工作区相对路径。编号接着目录里已有的往下排(补充说明轮、
    /// 计划轮再探时不覆盖先前的报告)。
    fn save_explore_report(
        &self,
        description: &str,
        prompt: &str,
        text: &str,
    ) -> Result<String, String> {
        let _guard = self
            .tracker
            .explore_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let n = explore_reports(&self.dir_abs)
            .last()
            .map(|(n, _)| n + 1)
            .unwrap_or(1);
        let title = if description.is_empty() {
            "调研报告".to_string()
        } else {
            description.to_string()
        };
        let mut doc = format!("# 调研报告 {n}:{title}\n\n");
        if !prompt.is_empty() {
            doc.push_str("## 调研问题\n\n");
            doc.push_str(prompt);
            doc.push_str("\n\n## 调研结果\n\n");
        }
        doc.push_str(text.trim_end());
        doc.push('\n');
        let file = format!("{n}.md");
        write_atomic(&self.dir_abs.join(EXPLORE_DIR).join(&file), &doc)
            .map_err(|e| e.to_string())?;
        self.tracker.explore_saved.fetch_add(1, Ordering::Relaxed);
        Ok(format!("{}/{EXPLORE_DIR}/{file}", self.dir_rel))
    }
}

/// 正文收到 [`EXPLORE_FEEDBACK_BODY_MAX`] 字以内,再接一行尾注。
fn clip_with_tail(text: &str, tail: &str) -> String {
    let body = text.trim_end();
    if char_len(body) <= EXPLORE_FEEDBACK_BODY_MAX {
        return format!("{body}\n\n{tail}");
    }
    let head: String = body.chars().take(EXPLORE_FEEDBACK_BODY_MAX).collect();
    format!("{head}\n…(以上为节选)\n\n{tail}")
}

/// 流程目录下已落盘的 explore 报告:`explore/<n>.md`(非空文件),按编号升序。
pub fn explore_reports(dir_abs: &Path) -> Vec<(u32, PathBuf)> {
    let mut out: Vec<(u32, PathBuf)> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir_abs.join(EXPLORE_DIR)) else {
        return out;
    };
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let Some(n) = name
            .strip_suffix(".md")
            .and_then(|stem| stem.parse::<u32>().ok())
        else {
            continue;
        };
        let path = ent.path();
        if std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() > 0) {
            out.push((n, path));
        }
    }
    out.sort_by_key(|(n, _)| *n);
    out
}

/// `ultraplan.stage` 载荷 + 「深度规划」实发情况(契约 §4:effort / thinkingForced)。
pub fn stage_payload_with(up: &UltraPlanState, run_id: Option<&str>, deep: &DeepPlanning) -> Value {
    let mut v = stage_payload(up, run_id);
    v["effort"] = json!(deep.effort);
    v["thinkingForced"] = json!(deep.thinking_forced);
    v
}

fn emit(state: &AppState, session_id: &str, event_type: &str, payload: Value) {
    state
        .events
        .emit(EventDraft::new(session_id, event_type, "ultraplan").payload(payload));
}

fn emit_session_updated(state: &AppState, session_id: &str) {
    state.events.emit(
        EventDraft::new(session_id, "session.updated", "session")
            .payload(json!({ "sessionId": session_id })),
    );
}

fn emit_notice(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    id: &str,
    code: &str,
    message: &str,
) {
    emit(
        state,
        session_id,
        "ultraplan.notice",
        json!({ "runId": run_id, "id": id, "code": code, "message": message }),
    );
}

/// 轮次开场:**唯一**做副作用的地方,且只在 execute_turn 认领 run、[`revalidate`] 通过之后调用。
///
/// Discovery:
/// - 新流程:分配状态(id / token / 唯一 slug)→ 把用户设想**逐字**写进 `brief.md` → 状态落会话;
/// - 补充说明 / 重出问卷:把这段话追加到 `brief.md`(设想与历次补充都在这一个文件里);
/// - 置 `phase=running, running=discovery`,发 `ultraplan.started`(仅新流程)与 `ultraplan.stage`;
///   模型没有思考可开时发 `ultraplan.notice {THINKING_UNAVAILABLE}`,不谎称深度规划;
/// - 装配提示词后缀、出口工具与预算化上下文段。
///
/// 先写文件再落状态:文件写失败就什么都没发生(状态没建、相位没动);反过来的顺序会留下一条
/// 没有 brief 的流程。`Err` 为 `CODE: 消息` 形态,execute_turn 据此让本轮如实失败。
pub fn begin_turn(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    scope: &crate::scope::ScopeProject,
    user_text: &str,
    ut: &UltraTurn,
) -> Result<UltraRuntime, String> {
    match ut.kind {
        TurnKind::Discovery { fresh } => {
            begin_discovery(state, session_id, run_id, scope, user_text, ut, fresh)
        }
        _ => lifecycle::begin_followup(state, session_id, run_id, scope, user_text, ut),
    }
}

fn begin_discovery(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    scope: &crate::scope::ScopeProject,
    user_text: &str,
    ut: &UltraTurn,
    fresh: bool,
) -> Result<UltraRuntime, String> {
    let ws_root = scope.workspace_root.as_path();
    let routed_id = ut.flow.as_ref().map(|f| f.id.clone());
    // 开场失败都用契约 §2 的通用失败码(它也原样进 agent.failed.error),具体原因写在消息里。
    let io_err = |rel: &str, e: std::io::Error| format!("{ERR_TURN_INVALID}: 写入 {rel} 失败: {e}");
    // 认领 run 之后状态还会变,只可能是存贮被绕过了(revalidate 刚核对过)。此时用户卡与
    // agent.started 已发,不能再当「起 run 前的拒绝」回 409,按本轮失败如实收场。
    let lost = || "ULTRAPLAN_TURN_INVALID: 流程状态在本轮开始时发生了变化,请刷新后重试".to_string();

    let (up, prior_stage) = if fresh {
        let mut new = UltraPlanState::new_flow(user_text, scope.workspace_id.as_deref(), ws_root);
        let dir = new
            .dir_abs(ws_root)
            .ok_or_else(|| format!("{ERR_TURN_INVALID}: 流程目录分配失败"))?;
        let brief_rel = format!("{}/{BRIEF_FILE}", new.dir);
        write_atomic(&dir.join(BRIEF_FILE), user_text).map_err(|e| io_err(&brief_rel, e))?;
        new.phase = PHASE_RUNNING.to_string();
        new.running = Some(RUNNING_DISCOVERY.to_string());
        // CAS:槽位仍是路由时看到的样子(空,或同一条已 done 的流程)才装入新流程。
        let installed = state.sessions.update_ultraplan(session_id, |slot| {
            let unchanged = slot.as_ref().map(|u| u.id.clone()) == routed_id
                && slot.as_ref().map_or(true, |u| !u.is_active());
            if unchanged {
                *slot = Some(new.clone());
            }
            unchanged
        });
        match installed {
            Some((session, true)) => (session.ultraplan.ok_or_else(lost)?, None),
            _ => return Err(lost()),
        }
    } else {
        let snap = ut.flow.as_ref().ok_or_else(lost)?;
        let dir = snap.dir_abs(ws_root).ok_or_else(|| {
            format!("{ERR_TURN_INVALID}: 流程目录不可用(状态里的 dir 不是流程目录形态)")
        })?;
        let brief_rel = format!("{}/{BRIEF_FILE}", snap.dir);
        append_brief(&dir.join(BRIEF_FILE), user_text).map_err(|e| io_err(&brief_rel, e))?;
        let expected_stage = snap.stage.clone();
        let updated = state
            .sessions
            .update_ultraplan(session_id, |slot| match slot.as_mut() {
                Some(u) if Some(&u.id) == routed_id.as_ref() && u.stage == expected_stage => {
                    u.phase = PHASE_RUNNING.to_string();
                    u.running = Some(RUNNING_DISCOVERY.to_string());
                    u.last_error = None;
                    true
                }
                _ => false,
            });
        match updated {
            Some((session, true)) => (session.ultraplan.ok_or_else(lost)?, Some(expected_stage)),
            _ => return Err(lost()),
        }
    };
    let dir_abs = up.dir_abs(ws_root).ok_or_else(lost)?;

    if fresh {
        emit(
            state,
            session_id,
            "ultraplan.started",
            json!({
                "runId": run_id, "id": up.id, "slug": up.slug, "dir": up.dir, "title": up.title,
            }),
        );
    }
    emit(
        state,
        session_id,
        "ultraplan.stage",
        stage_payload_with(&up, Some(run_id), &ut.deep),
    );
    if !ut.deep.thinking_forced {
        emit_notice(
            state,
            session_id,
            run_id,
            &up.id,
            "THINKING_UNAVAILABLE",
            "当前模型没有可开启的思考 / 推理档位,本轮按普通方式规划(未启用深度规划)",
        );
    }

    // 预算化上下文段(design §2(j)):事实 → 流程产物位置 →(仅非新流程)上一版理解与问卷提纲。
    let reports = explore_reports(&dir_abs);
    let mut sections = vec![
        Section::new(
            "项目事实(服务端扫描所得,以此为准)",
            ut.facts.render_body(),
            SECTION_FACTS_CAP,
        ),
        Section::new(
            "本流程的产物",
            flow_section(&up, &reports),
            SECTION_FLOW_CAP,
        ),
    ];
    if !fresh {
        let understanding_rel = format!("{}/{UNDERSTANDING_FILE}", up.dir);
        if let Ok(text) = std::fs::read_to_string(dir_abs.join(UNDERSTANDING_FILE)) {
            sections.push(
                Section::new("上一版理解", text, SECTION_UNDERSTANDING_CAP)
                    .with_source(&understanding_rel),
            );
        }
        let questionnaire_rel = format!("{}/{QUESTIONNAIRE_FILE}", up.dir);
        if let Some(q) = read_json(&dir_abs.join(QUESTIONNAIRE_FILE)) {
            sections.push(
                Section::new(
                    "上一版问卷提纲",
                    questionnaire_outline(&q),
                    SECTION_OUTLINE_CAP,
                )
                .with_source(&questionnaire_rel),
            );
        }
    }
    let (preamble, reports_out) = assemble(&sections, effective_budget(ut.deep.context_tokens));

    let mut prompt_suffix = DISCOVERY_PROMPT_SUFFIX.to_string();
    match prior_stage.as_deref() {
        Some(STAGE_QUESTIONNAIRE) => prompt_suffix.push_str(DISCOVERY_REGENERATE_ADDENDUM),
        Some(_) => prompt_suffix.push_str(DISCOVERY_CLARIFY_ADDENDUM),
        None => {}
    }

    Ok(UltraRuntime {
        kind: ut.kind,
        flow_id: up.id.clone(),
        dir_rel: up.dir.clone(),
        dir_abs,
        facts: ut.facts.clone(),
        deep: ut.deep.clone(),
        deep_fallback: ut.deep_fallback.clone(),
        tracker: TurnTracker::default(),
        prompt_suffix,
        exit_tool_specs: vec![questionnaire_tool_spec()],
        preamble,
        sections: reports_out,
    })
}

/// 把一段补充说明追加到 brief.md(文件不在就以它为正文新建——用户可能手删了)。
fn append_brief(path: &Path, text: &str) -> std::io::Result<()> {
    let merged = match std::fs::read_to_string(path) {
        Ok(prev) if !prev.trim().is_empty() => format!(
            "{}\n\n---\n\n## 补充说明({})\n\n{}\n",
            prev.trim_end(),
            now_rfc3339(),
            text
        ),
        _ => text.to_string(),
    };
    write_atomic(path, &merged)
}

/// 「本流程的产物」段:告诉 leader 东西都在哪、已有哪些调研报告(有就不必重探)。
fn flow_section(up: &UltraPlanState, reports: &[(u32, PathBuf)]) -> String {
    let mut s = format!(
        "流程目录 {dir}/(工作区相对路径,可用 read_file 读取):\n\
         - {dir}/{BRIEF_FILE}:用户的设想原文与历次补充说明;\n\
         - {dir}/{UNDERSTANDING_FILE}、{dir}/{QUESTIONNAIRE_FILE}:由 {QUESTIONNAIRE_TOOL} 落盘,不需要也不能手写;\n\
         - {dir}/{EXPLORE_DIR}/<n>.md:explore 子代理的完整调研报告(自动保存)。\n",
        dir = up.dir
    );
    if !reports.is_empty() {
        s.push_str("已有调研报告(无需重复调研,需要细节时直接读):");
        for (n, _) in reports {
            s.push_str(&format!(" {}/{EXPLORE_DIR}/{n}.md", up.dir));
        }
        s.push('\n');
    }
    s
}

/// 问卷 JSON → 提纲(章节标题 + 每题一行),给「重出问卷」轮看上一版问了什么。
fn questionnaire_outline(q: &Value) -> String {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut s = String::new();
    let title = text(q, "title");
    if !title.is_empty() {
        s.push_str(&format!("{title}\n"));
    }
    for sec in q
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        s.push_str(&format!("## {}\n", text(sec, "title")));
        for question in sec
            .get("questions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            s.push_str(&format!(
                "- [{}|{}] {}\n",
                text(question, "id"),
                text(question, "kind"),
                text(question, "question")
            ));
        }
    }
    s
}

/// 轮次错误串 → `lastError`。`CODE: 消息` 形态取前缀当码;否则认云端 / 渠道错误码;
/// 都不是就用通用码 [`ERR_TURN_INVALID`]。`ULTRAPLAN_*` 码只认契约 §2 失败码表里的
/// ([`TURN_FAILURE_CODES`]),表外的一律归通用码(原因留在 message 里)。消息截到 [`LAST_ERROR_MAX_CHARS`]。
pub fn flow_error(error: &str) -> FlowError {
    let is_code = |s: &str| {
        s.len() >= 3
            && s.contains('_')
            && s.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    let error = error.trim();
    let (code, message) = match error.split_once(':') {
        Some((head, tail)) if is_code(head.trim()) && !tail.trim().is_empty() => {
            (head.trim().to_string(), tail.trim())
        }
        _ => (
            crate::llm::failure_code_from_error(error)
                .unwrap_or(ERR_TURN_INVALID)
                .to_string(),
            error,
        ),
    };
    let code = if code.starts_with("ULTRAPLAN_") && !TURN_FAILURE_CODES.contains(&code.as_str()) {
        ERR_TURN_INVALID.to_string()
    } else {
        code
    };
    let message = if message.is_empty() {
        "本轮失败(未给出原因)".to_string()
    } else if char_len(message) > LAST_ERROR_MAX_CHARS {
        let head: String = message.chars().take(LAST_ERROR_MAX_CHARS).collect();
        format!("{head}…")
    } else {
        message.to_string()
    };
    FlowError { code, message }
}

/// 轮次收尾:只改相位,**不改阶段**(阶段只经出口工具推进;失败后重发同一动作即重试)。
///
/// completed / cancelled → `waiting`(取消不算错,不记 lastError,`running` 清空);
/// failed → `failed` + `lastError`,`running` 保留为断掉的那一轮的种类(前端据此说清断在哪一步)。
/// 发 `ultraplan.stage` + `session.updated`。在终态事件与释放 run **之前**调用:
/// run 还挂着时 REST 的重新开始会被 SESSION_BUSY 挡住,不会与这次写相位交错。
///
/// 只认 `phase == running`:那只可能是本轮在 [`begin_turn`] 里置的(同一会话同一时刻只有一条 run)。
/// 开场就失败、状态还没动过的轮次到这里是空操作,不发事件。
pub fn finish_turn(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    deep: &DeepPlanning,
    status: &str,
    error: Option<&str>,
) {
    let failed = status == "failed";
    let last_error = failed.then(|| flow_error(error.unwrap_or("")));
    let updated = state.sessions.update_ultraplan(session_id, |slot| {
        let Some(up) = slot.as_mut() else {
            return false;
        };
        if up.phase != PHASE_RUNNING {
            return false;
        }
        if !failed {
            up.running = None;
        }
        up.phase = if failed { PHASE_FAILED } else { PHASE_WAITING }.to_string();
        up.last_error = last_error;
        true
    });
    let Some((session, true)) = updated else {
        return;
    };
    if let Some(up) = session.ultraplan.as_ref() {
        emit(
            state,
            session_id,
            "ultraplan.stage",
            stage_payload_with(up, Some(run_id), deep),
        );
        emit_session_updated(state, session_id);
    }
}

// ---------------- 轮次接入:出口工具 ----------------

/// 阶段出口工具的统一入口(父执行闭包在**权限门之前**调它,与 create_plan 同列:
/// 产物路径固定在流程目录内、不接受模型给的路径,permission=plan 的会话也得能出问卷)。
///
/// 没有 UltraPlan 运行时(普通轮次里模型幻觉出这个工具名),或工具不属于本轮种类 → TOOL_FORBIDDEN。
pub fn handle_exit_tool(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    rt: Option<&UltraRuntime>,
    name: &str,
    args: &Value,
) -> (bool, String) {
    let Some(rt) = rt else {
        return (
            false,
            format!("TOOL_FORBIDDEN: {name} 只在 UltraPlan 流程的对应轮次可用"),
        );
    };
    match (name, rt.kind) {
        (QUESTIONNAIRE_TOOL, TurnKind::Discovery { .. }) => {
            handle_questionnaire(state, session_id, run_id, rt, args)
        }
        (SPEC_TOOL, TurnKind::SpecAndDemo { .. }) => {
            lifecycle::handle_spec(state, session_id, run_id, rt, args)
        }
        (PLAN_DOC_TOOL | PLAN_TASKS_TOOL, TurnKind::Planning { .. }) => {
            lifecycle::handle_plan(state, session_id, run_id, rt, name, args)
        }
        _ => (
            false,
            format!(
                "TOOL_FORBIDDEN: 本轮是「{}」环节,不提供 {name}",
                rt.kind.label()
            ),
        ),
    }
}

/// `ultraplan_questionnaire`:校验 → 落 `understanding.md` + `questionnaire.json` → 阶段推到
/// `questionnaire` → 发 `ultraplan.questionnaire`(带整份问卷 JSON,卡片回放靠它)。
///
/// explore 门:项目已有内容、流程目录里一份调研报告都没有、且这是第一份问卷时,先打回去要求
/// 并行派 explore。打回 [`EXPLORE_GATE_MAX_REJECTIONS`] 次之后,或 explore 工种根本不存在时放行,
/// 同时发 `ultraplan.notice {EXPLORE_SKIPPED}` 并在 understanding 开头如实标注「未经摸底」(I-5)。
/// 门按**落盘的报告**判,不按「本轮有没有探」判:重出问卷不必再探一遍。
///
/// 同一轮内再次调用:整份覆盖上一次的文件,**rev 不再递增**(一轮最多 +1)——rev 是「用户面前
/// 第几版问卷」,模型在一轮里自我修正不算新的一版;事件照发,卡片按同一 rev 原地替换。
fn handle_questionnaire(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    rt: &UltraRuntime,
    args: &Value,
) -> (bool, String) {
    let current = state
        .sessions
        .get(session_id)
        .and_then(|s| s.ultraplan)
        .filter(|u| u.id == rt.flow_id);
    let Some(current) = current else {
        return (
            false,
            "ULTRAPLAN_STAGE_MISMATCH: 本流程已被重开或清除,问卷未保存".to_string(),
        );
    };
    if !matches!(
        current.stage.as_str(),
        STAGE_DISCOVERY | STAGE_QUESTIONNAIRE
    ) {
        return (
            false,
            format!(
                "ULTRAPLAN_STAGE_MISMATCH: 流程已在 {} 阶段,不能再提交问卷",
                current.stage
            ),
        );
    }

    // The coordinator must finish at least two independent project reports.
    if rt.facts.has_content && explore_reports(&rt.dir_abs).len() < 2 {
        rt.tracker
            .explore_rejections
            .fetch_add(1, Ordering::Relaxed);
        return (false, "ULTRAPLAN_EXPLORE_REQUIRED: 已有项目必须先完成 2–4 个并行 explore 子任务，不能跳过失败的探索。请恢复探索后重试。".into());
    }
    let questionnaire = match validate_questionnaire(args)
        .and_then(|q| ensure_implementation_stack(q, &rt.facts))
    {
        Ok(q) => q,
        Err(e) => {
            return (
                false,
                format!("ULTRAPLAN_QUESTIONNAIRE_INVALID: {e}。请修正后重新提交完整问卷。"),
            )
        }
    };

    let understanding_rel = format!("{}/{UNDERSTANDING_FILE}", rt.dir_rel);
    let questionnaire_rel = format!("{}/{QUESTIONNAIRE_FILE}", rt.dir_rel);
    let wire = match serde_json::to_value(&questionnaire) {
        Ok(v) => v,
        Err(e) => return (false, format!("ULTRAPLAN_IO: 问卷序列化失败: {e}")),
    };
    let understanding_doc = format!(
        "# {}\n\n{}\n",
        questionnaire.title,
        questionnaire.understanding.trim_end()
    );
    if let Err(e) = write_atomic(&rt.dir_abs.join(UNDERSTANDING_FILE), &understanding_doc) {
        return (
            false,
            format!("ULTRAPLAN_IO: 写入 {understanding_rel} 失败: {e}"),
        );
    }
    if let Err(e) = write_json_atomic(&rt.dir_abs.join(QUESTIONNAIRE_FILE), &wire) {
        return (
            false,
            format!("ULTRAPLAN_IO: 写入 {questionnaire_rel} 失败: {e}"),
        );
    }

    // 阶段推进:CAS on (id, stage ∈ {discovery, questionnaire})。本轮已分配过 rev 就沿用。
    let mut turn_rev = rt
        .tracker
        .questionnaire_rev
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let assigned = *turn_rev;
    let advanced = state.sessions.update_ultraplan(session_id, |slot| {
        let up = slot.as_mut().filter(|u| {
            u.id == rt.flow_id && matches!(u.stage.as_str(), STAGE_DISCOVERY | STAGE_QUESTIONNAIRE)
        })?;
        let rev = assigned.unwrap_or(up.questionnaire_rev + 1);
        up.stage = STAGE_QUESTIONNAIRE.to_string();
        up.questionnaire_rev = rev;
        Some(rev)
    });
    let Some((session, Some(rev))) = advanced else {
        return (
            false,
            "ULTRAPLAN_STAGE_MISMATCH: 流程状态已变化,问卷未生效".to_string(),
        );
    };
    *turn_rev = Some(rev);
    drop(turn_rev);

    emit(
        state,
        session_id,
        "ultraplan.questionnaire",
        json!({
            "runId": run_id,
            "id": rt.flow_id,
            "rev": rev,
            "path": questionnaire_rel,
            "questionnaire": wire,
        }),
    );
    if let Some(up) = session.ultraplan.as_ref() {
        emit(
            state,
            session_id,
            "ultraplan.stage",
            rt.stage_event(up, run_id),
        );
    }
    emit_session_updated(state, session_id);
    (
        true,
        format!(
            "问卷已保存并展示给用户({} 节 {} 题,第 {rev} 版;文件 {questionnaire_rel})。\
             现在停止调用工具:最终消息只用两三句中文概括你的理解并请用户填写问卷,不要在正文里复述题目。",
            questionnaire.sections.len(),
            questionnaire.question_count()
        ),
    )
}

/// `ultraplan_questionnaire` 的 OpenAI 工具 spec(schema 对应契约 §5;描述写给模型看)。
/// 只用各渠道都认的 schema 关键字(type / properties / items / enum / required / description),
/// 数量界写在描述里、由 [`validate_questionnaire`] 把关。
pub fn questionnaire_tool_spec() -> Value {
    let option = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "选项 id(本题内唯一;小写英文加下划线)" },
            "label": { "type": "string", "description": "选项文字(具体、可直接选,≤120 字)" },
            "description": { "type": "string", "description": "补充说明;推荐项在这里写明推荐理由" },
            "recommended": { "type": "boolean", "description": "是否为你推荐的选项(单选题最多 1 个)" }
        },
        "required": ["id", "label"]
    });
    let question = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "题目 id(整份问卷内唯一;小写英文加下划线,如 core_loop)" },
            "kind": {
                "type": "string",
                "enum": [KIND_SINGLE, KIND_MULTI, KIND_TEXT, KIND_SCALE],
                "description": "single 单选 | multi 多选 | text 填空 | scale 打分"
            },
            "question": { "type": "string", "description": "题干(一句话问清楚,≤400 字)" },
            "help": { "type": "string", "description": "可选:这道题为什么重要 / 会影响什么" },
            "options": {
                "type": "array",
                "items": option,
                "description": "single / multi 必填,2–6 个(建议 3–5 个具体选项);text / scale 不要给"
            },
            "allowOther": { "type": "boolean", "description": "是否允许用户自填「其他」(仅 single / multi;缺省 false)" },
            "allowDelegate": { "type": "boolean", "description": "是否允许「交给你决定」(缺省 true)" },
            "required": { "type": "boolean", "description": "是否必答(缺省:single / multi / scale 必答,text 选答)" },
            "min": { "type": "integer", "description": "multi:最少选几项;scale:量程下界(缺省 1)" },
            "max": { "type": "integer", "description": "multi:最多选几项;scale:量程上界(缺省 5)" },
            "scaleLabels": {
                "type": "array",
                "items": { "type": "string" },
                "description": "仅 scale:恰好两项 [低端文字, 高端文字]"
            }
        },
        "required": ["id", "kind", "question"]
    });
    let section = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "章节 id(唯一;小写英文加下划线)" },
            "title": { "type": "string", "description": "章节标题,如「核心玩法」" },
            "questions": { "type": "array", "items": question, "description": "本节题目(至少 1 题)" }
        },
        "required": ["id", "title", "questions"]
    });
    json!({
        "type": "function",
        "function": {
            "name": QUESTIONNAIRE_TOOL,
            "description": "提交给用户填写的需求问卷(UltraPlan 立项讨论的唯一产物出口)。整轮只调一次;\
    调用成功后问卷会以卡片形式展示给用户,流程停在「等待用户作答」。\
    限制:1–8 个章节、全卷不超过 40 题、每道选择题 2–6 个选项;id 不可含空白。\
    服务端会加入必答的 implementation_stack 技术栈确认题，不能委托或自填其他答案；为它预留 1 题。\
    新项目提供 2D/Godot（默认）、3D/Godot、3D/Rurix，已有项目明确确认沿用当前技术栈。\
    项目已有内容时,必须先并行派发 explore 子代理摸清现状,否则本工具会拒绝。",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "问卷标题(≤120 字),如「塔防小游戏 · 需求确认」" },
                    "understanding": {
                        "type": "string",
                        "description": "Markdown:你理解的游戏是什么、你替用户做了哪些假设、项目摸底发现了什么(带文件路径)。用户会在问卷顶部看到它"
                    },
                    "sections": { "type": "array", "items": section, "description": "问卷章节(建议 4–8 节,共 12–30 题)" }
                },
                "required": ["title", "understanding", "sections"]
            }
        }
    })
}

// 留痕:ultraplan · discovery = 只读 leader(同 plan 模式的双侧写门)+ 同轮并行 explore +
// 唯一出口 ultraplan_questionnaire(D-044)。产物是一份问卷,不是聊天里的一串问题。
pub const DISCOVERY_PROMPT_SUFFIX: &str = "\n当前为 UltraPlan 模式 · 立项讨论:你是这款游戏的制作总监,正在做前期策划。\
本轮唯一的产物是一份给用户填写的需求问卷;写工具已被禁用(调用将被拒绝 TOOL_FORBIDDEN),也没有 create_plan。\
按下面四步推进,不要跳步:\n\
1. 读事实:先看上方「项目事实」段(服务端扫描所得,以它为准)。项目已存在时,用 list_dir 看根目录、\
读 README 与设计文档,涉及场景时用 scene_summary 看当前场景。「项目事实」说工作区还没有项目时,\
不要去查场景或资产——那里没有属于这款游戏的内容。\n\
2. 并行摸底:「项目事实」写明项目已有内容时,必须在**同一轮**里一次性发出 2–4 个 \
task{subagent_type:\"explore\"}(它们会并发执行),分头覆盖:资产与美术风格 / 场景与实体 / \
脚本与玩法逻辑 / 文档与既有约定,互不重叠。每个 task 的 prompt 必须自足(子代理看不到对话历史:\
写清查什么、去哪查、带回什么证据),并用 description 写一句中文说明它在查什么。项目为空则跳过本步。\
每份调研报告的全文会自动存进流程目录的 explore/ 下,回给你的只是节选,需要细节时用 read_file 读全文。\n\
3. 想透:结合用户的设想与摸底结果,逐项想清楚——游戏类型与核心循环、操作方式、镜头与视角、\
关卡与进度、美术方向、UI 与反馈、音频、首版(MVP)范围与取舍、技术约束与风险。\
正式实现技术栈必须在问卷阶段由用户明确确认：2D 默认 Godot；3D 可选 Godot 或 Rurix。\
已有项目须确认沿用当前维度与后端，不能等到需求定稿或制作时自行切换。\
能从设想或项目现状直接确定的事自己定下来并写进 understanding;真正需要用户拍板的才出成题目。\n\
4. 出问卷:调用**一次** ultraplan_questionnaire。understanding 写清三件事:你理解的游戏是什么、\
你替用户做了哪些假设、项目摸底发现了什么(带文件路径)。问卷要**全面**:4–8 个章节\
(如 核心玩法 / 操作与镜头 / 关卡与进度 / 美术风格 / UI 与反馈 / 音频 / 范围与 MVP 取舍 / 技术约束),\
共 12–30 题。选择题每题给 3–5 个**具体**选项,其中一个标 recommended 并在它的 description 里写明推荐理由;\
合适的题目开 allowOther 让用户自填;游戏名、必须有的内容、参考作品这类问题用 text 题;\
程度与偏好类用 scale 题。系统会固定加入 implementation_stack 必答单选确认题，\
同时确认正式游戏的维度与后端，禁止委托或「其他」；自定义问题最多 39 题，为它预留位置。\
不要另出重复的维度或后端题；设想里已经回答的其他问题不要再问。\n\
纪律:设想含糊到连选项都拟不出来时,不要硬出问卷——直接用一两句话反问**一个**最关键的澄清问题,然后结束本轮。\
问卷提交成功后,最终消息只用两三句话概括你的理解并请用户填写问卷,不要在正文里复述题目。";

/// 补充说明轮(阶段仍是 discovery:上一轮反问了用户,或没出成问卷)。
const DISCOVERY_CLARIFY_ADDENDUM: &str =
    "\n本轮是用户对上一轮的补充说明(设想原文与历次补充都在流程目录的 brief.md,\
此前的对话见历史)。把补充内容并入你的理解后继续:信息已经足够,就按上面四步出问卷;\
仍然不足,再反问一个最关键的问题。";

/// 重出问卷轮(阶段是 questionnaire:用户没作答,而是对问卷本身提了意见)。
const DISCOVERY_REGENERATE_ADDENDUM: &str = "\n本轮是重出问卷:用户没有提交上一版问卷,而是发来了修改意见(见本轮用户消息);\
上方已附上一版的理解与问卷提纲。按用户的意见调整后,重新调用一次 ultraplan_questionnaire 提交**完整**的新问卷\
(它会整份替换旧版,不要只交改动的部分):用户点到的地方要改到位,仍然值得问的题目原样保留、id 不变。\
流程目录的 explore/ 下已有的调研报告可以直接读,除非用户指出了新的范围,否则不必重新调研。";

// ---------------- REST ----------------

/// 统一错误体:`{ error: { code, message, details? } }`(契约首段)。
pub(crate) fn err_response(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    details: Option<Value>,
) -> Response {
    let mut error = json!({ "code": code, "message": message.into() });
    if let Some(d) = details {
        error["details"] = d;
    }
    (status, Json(json!({ "error": error }))).into_response()
}

fn session_not_found(id: &str) -> Response {
    err_response(
        StatusCode::NOT_FOUND,
        "SESSION_NOT_FOUND",
        format!("会话不存在: {id}"),
        None,
    )
}

/// 流程所在工作区根。用状态里记下的 workspaceId,而不是会话当前的:
/// 流程结束(done)后会话可以换工作区,产物仍在原处。
fn flow_ws_root(state: &AppState, up: &UltraPlanState) -> PathBuf {
    crate::scope::workspace_root_for(state, up.workspace_id.as_deref())
}

/// answers.json → 契约 `Answers`。文件若是 `{ rev, answers }` 包一层的形态就取内层,
/// 否则原样返回(两种落盘形态都读得出,W2 定稿时不必回头改这里)。
fn answers_face(raw: Option<Value>) -> Value {
    match raw {
        Some(Value::Object(mut o)) => {
            if o.get("rev").is_some_and(Value::is_number)
                && o.get("answers").is_some_and(Value::is_object)
            {
                o.remove("answers").unwrap_or(Value::Null)
            } else {
                Value::Object(o)
            }
        }
        _ => Value::Null,
    }
}

fn object_or_null(raw: Option<Value>) -> Value {
    raw.filter(Value::is_object).unwrap_or(Value::Null)
}

/// `GET /api/forge/sessions/{id}/ultraplan` 的响应体(契约 §3)。
///
/// 问卷/答案/检查项/验收记录直接读流程目录里的文件(文件是事实源);文件不在即 null。
/// Demo 链接由独立源宿主提供；产物缺失时返回 null。
pub fn rest_face(state: &AppState, up: Option<&UltraPlanState>) -> Value {
    let dir = up.and_then(|u| u.dir_abs(&flow_ws_root(state, u)));
    let read = |name: &str| dir.as_ref().and_then(|d| read_json(&d.join(name)));
    json!({
        "ultraplan": up,
        "demo": lifecycle::demo_face(up, dir.as_deref()),
        "questionnaire": object_or_null(read(QUESTIONNAIRE_FILE)),
        "answers": answers_face(read(ANSWERS_FILE)),
        "checks": object_or_null(read(CHECKS_FILE)),
        "acceptance": object_or_null(read(ACCEPTANCE_FILE)),
        "production": object_or_null(read(PRODUCTION_FILE)),
        "target": object_or_null(read("target.json")),
        "delivery": object_or_null(read("delivery.json")),
    })
}

/// GET /api/forge/sessions/{id}/ultraplan →
/// `{ ultraplan|null, demo|null, questionnaire|null, answers|null, checks|null, acceptance|null }`。
pub async fn get_ultraplan(
    State(state): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let Some(session) = state.sessions.get(&id) else {
        return session_not_found(&id);
    };
    if let Some(up) = session.ultraplan.as_ref() {
        if let Some(dir) = up.dir_abs(&flow_ws_root(&state, up)) {
            if up.demo_iteration > 0 && dir.join(DEMO_DIR).join("index.html").is_file() {
                let registry = crate::demo_host::global();
                registry.register(&up.token, &dir.join(DEMO_DIR));
                if let Err(e) = registry.ensure_started().await {
                    return err_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "ULTRAPLAN_DEMO_HOST_FAILED",
                        e,
                        None,
                    );
                }
            }
        }
    }
    Json(rest_face(&state, session.ultraplan.as_ref())).into_response()
}

/// POST /api/forge/sessions/{id}/ultraplan/{action}:不产生轮次的流程操作。
///
/// - `restart` → `{ ok: true }`:清掉会话上的流程状态(产物文件保留,用户可以翻看旧流程),
///   发 `ultraplan.cleared` + `session.updated`。没有流程时幂等返回 ok,不发事件。
/// - `acceptance` / `rollback_demo`:携带流程 id 与对应版本，由 lifecycle 校验并原子推进。
/// - 其它 → 400 `INVALID_INPUT`。
///
/// 会话有运行中的 run 时一律 409 `SESSION_BUSY`:正在跑的那一轮收尾时要写阶段,
/// 此刻改状态等于在它脚下抽地板。忙判定与清状态在存贮锁内一次完成,不留窗口。
pub async fn post_ultraplan_action(
    State(state): State<Arc<AppState>>,
    UrlPath((id, action)): UrlPath<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    let body = if body.is_empty() {
        None
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(value) => Some(value),
            Err(_) => {
                return err_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_INPUT",
                    "请求正文须为 JSON",
                    None,
                )
            }
        }
    };
    let Some(session) = state.sessions.get(&id) else {
        return session_not_found(&id);
    };
    match action.as_str() {
        "restart" => {}
        "acceptance" | "rollback_demo" => {
            if let Some(run) = &session.active_run_id {
                return session_busy(run);
            }
            return lifecycle::rest_action(&state, &id, &action, body.as_ref());
        }
        other => {
            return err_response(
                StatusCode::BAD_REQUEST,
                "INVALID_INPUT",
                format!("未知 UltraPlan 操作: {other}(支持 restart | acceptance | rollback_demo)"),
                None,
            )
        }
    }
    match state
        .sessions
        .update_ultraplan_idle(&id, |slot| slot.take())
    {
        Err(crate::sessions::IdleUpdateError::NotFound) => session_not_found(&id),
        Err(crate::sessions::IdleUpdateError::Busy(run)) => session_busy(&run),
        Ok((_, None)) => Json(json!({ "ok": true })).into_response(),
        Ok((_, Some(cleared))) => {
            crate::demo_host::global().unregister(&cleared.token);
            state.events.emit(
                EventDraft::new(&id, "ultraplan.cleared", "ultraplan")
                    .payload(json!({ "id": cleared.id })),
            );
            state.events.emit(
                EventDraft::new(&id, "session.updated", "session")
                    .payload(json!({ "sessionId": id })),
            );
            Json(json!({ "ok": true })).into_response()
        }
    }
}

fn session_busy(run_id: &str) -> Response {
    err_response(
        StatusCode::CONFLICT,
        "SESSION_BUSY",
        format!("会话已有运行中的 run({run_id}),请等待完成或中止后再操作"),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-ultraplan-{tag}-{}-{}",
            std::process::id(),
            new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 不存在的工作区根:new_flow 只查不建,不需要落盘的用例用它,免得留下空临时目录。
    fn phantom_root() -> PathBuf {
        std::env::temp_dir().join(format!("agentd-ultraplan-none-{}", new_id("t")))
    }

    async fn body_json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("读响应体失败");
        serde_json::from_slice(&bytes).expect("响应应为 JSON")
    }

    // ---------- 路由 ----------

    /// 契约 §2 的表,独立于实现再写一遍:(阶段, 模式, 动作) → 轮次种类。不在表里的组合一律应被拒
    /// (带动作)或当普通轮(其它模式的自由文本)。
    fn contract_row(stage: Option<&str>, mode: &str, action: Option<Action>) -> Option<TurnKind> {
        use ProductionPhase::*;
        use TurnKind::*;
        match (stage, mode, action) {
            (None | Some("done"), "ultraplan", None) => Some(Discovery { fresh: true }),
            (Some("discovery"), "ultraplan", None) => Some(Discovery { fresh: false }),
            (Some("questionnaire"), "ultraplan", None) => Some(Discovery { fresh: false }),
            (Some("demo_review"), "ultraplan", None) => Some(SpecAndDemo { revise: true }),
            (Some("plan_review"), "ultraplan", None) => Some(Planning { revise: true }),
            (Some("questionnaire"), "ultraplan", Some(Action::Answer)) => {
                Some(SpecAndDemo { revise: false })
            }
            (Some("demo_review"), "ultraplan", Some(Action::ApproveDemo)) => {
                Some(Planning { revise: false })
            }
            (Some("demo_review"), "ultraplan", Some(Action::ReviseDemo)) => {
                Some(SpecAndDemo { revise: true })
            }
            (Some("plan_review"), "ultraplan", Some(Action::RevisePlan)) => {
                Some(Planning { revise: true })
            }
            (Some("plan_review"), "team", Some(Action::StartProduction)) => Some(Production(Start)),
            (Some("production"), "team", Some(Action::ResumeProduction)) => {
                Some(Production(Resume))
            }
            (Some("acceptance"), "team", Some(Action::FixProduction)) => Some(Production(Fix)),
            _ => None,
        }
    }

    #[test]
    fn route_table_covers_every_stage_action_pair() {
        let stages: Vec<Option<&str>> = std::iter::once(None)
            .chain(STAGES.iter().map(|s| Some(*s)))
            .collect();
        let modes = [
            "build",
            "plan",
            "team",
            "debug",
            "ask",
            "multitask",
            "ultraplan",
        ];
        let actions: Vec<Option<Action>> = std::iter::once(None)
            .chain(Action::ALL.into_iter().map(Some))
            .collect();
        let mut turns = 0usize;
        let mut ordinary = 0usize;
        for stage in &stages {
            for mode in modes {
                for action in &actions {
                    let got = route(*stage, mode, *action);
                    let ctx = format!("stage={stage:?} mode={mode} action={action:?}");
                    match (contract_row(*stage, mode, *action), action) {
                        (Some(kind), _) => {
                            assert_eq!(got, Ok(Some(kind)), "{ctx}");
                            assert_eq!(kind.mode(), mode, "轮次模式应与请求模式一致: {ctx}");
                            turns += 1;
                        }
                        // 其它模式的自由文本 = 普通轮,永不碰流程。
                        (None, None) if mode != MODE => {
                            assert_eq!(got, Ok(None), "{ctx}");
                            ordinary += 1;
                        }
                        // ultraplan 模式自由文本只在 production / acceptance 被拒。
                        (None, None) => {
                            assert!(
                                matches!(*stage, Some("production") | Some("acceptance")),
                                "{ctx}"
                            );
                            let Err(RouteError::Mismatch(m)) = got else {
                                panic!("应为阶段不匹配: {ctx} → {got:?}");
                            };
                            assert_eq!(m.stage.as_deref(), *stage, "{ctx}");
                            assert_eq!(m.allowed, allowed_actions(*stage), "{ctx}");
                        }
                        // 带动作却不在表里:模式不配 → WrongMode;模式对但阶段不对 → Mismatch。
                        (None, Some(a)) => {
                            let err = got.expect_err(&ctx);
                            assert_eq!(err.code(), "ULTRAPLAN_STAGE_MISMATCH", "{ctx}");
                            assert_eq!(err.status(), StatusCode::CONFLICT, "{ctx}");
                            assert_eq!(err.mismatch().stage.as_deref(), *stage, "{ctx}");
                            assert_eq!(err.mismatch().allowed, allowed_actions(*stage), "{ctx}");
                            if mode == a.required_mode() {
                                assert!(matches!(err, RouteError::Mismatch(_)), "{ctx}");
                                assert_ne!(*stage, Some(a.required_stage()), "{ctx}");
                            } else {
                                assert!(
                                    matches!(err, RouteError::WrongMode { action, required, .. }
                                        if action == *a && required == a.required_mode()),
                                    "{ctx}"
                                );
                            }
                        }
                    }
                }
            }
        }
        // 表里恰 13 行会起轮次:自由文本 6(无流程/done/discovery/questionnaire/demo_review/plan_review)+ 动作 7。
        assert_eq!(turns, 13);
        // 六个其它模式 × 八种阶段态的自由文本全是普通轮。
        assert_eq!(ordinary, 6 * 8);
        // 带动作的请求永不开新流程。
        for a in Action::ALL {
            assert!(route(None, a.required_mode(), Some(a)).is_err(), "{a:?}");
            assert!(route(Some(STAGE_DONE), a.required_mode(), Some(a)).is_err());
        }
    }

    #[test]
    fn action_parse_roundtrip_and_allowed_lists() {
        for a in Action::ALL {
            assert_eq!(Action::parse(a.as_str()), Some(a));
            // 每个动作都列在它所属阶段的 allowed 里(前端据此给出可点的操作)。
            assert!(
                allowed_actions(Some(a.required_stage())).contains(&a.as_str()),
                "{a:?}"
            );
        }
        assert_eq!(Action::parse(" answer "), Some(Action::Answer));
        assert_eq!(
            Action::parse("restart"),
            None,
            "restart 是 REST 操作不是轮次动作"
        );
        assert_eq!(Action::parse(""), None);
        assert!(allowed_actions(Some("乱写的阶段")).is_empty());
        assert_eq!(allowed_actions(None), vec![ALLOWED_FREE_TEXT]);
        // 错误体:409 + details{stage, allowed}。
        let err = route(Some(STAGE_PRODUCTION), MODE, None).unwrap_err();
        assert_eq!(
            err.mismatch().details(),
            json!({ "stage": "production", "allowed": ["resume_production"] })
        );
        assert!(err.message().contains("production"), "{}", err.message());
        let none = route(None, MODE, Some(Action::Answer)).unwrap_err();
        assert_eq!(none.mismatch().details()["stage"], Value::Null);
    }

    /// 拒绝的 wire 形态:409 + `{ error: { code, message, details: { stage, allowed } } }`;
    /// 模式不配与阶段不配同码(契约 §2),消息各说各的。
    #[tokio::test]
    async fn route_error_response_is_409_with_details() {
        let resp = route(Some(STAGE_DISCOVERY), MODE, Some(Action::Answer))
            .unwrap_err()
            .into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = body_json(resp).await;
        assert_eq!(v["error"]["code"], "ULTRAPLAN_STAGE_MISMATCH");
        assert_eq!(
            v["error"]["details"],
            json!({ "stage": "discovery", "allowed": ["free_text"] })
        );
        assert!(v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("当前阶段 discovery"));

        let resp = route(Some(STAGE_QUESTIONNAIRE), "team", Some(Action::Answer))
            .unwrap_err()
            .into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = body_json(resp).await;
        assert_eq!(v["error"]["code"], "ULTRAPLAN_STAGE_MISMATCH");
        assert_eq!(v["error"]["details"]["stage"], "questionnaire");
        assert_eq!(
            v["error"]["details"]["allowed"],
            json!(["answer", "free_text"])
        );
        let msg = v["error"]["message"].as_str().unwrap();
        assert!(
            msg.contains("answer") && msg.contains("ultraplan 模式"),
            "{msg}"
        );

        // err_response 不带 details 时不出该键。
        let v = body_json(err_response(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "坏",
            None,
        ))
        .await;
        assert_eq!(
            v,
            json!({ "error": { "code": "INVALID_INPUT", "message": "坏" } })
        );
    }

    #[test]
    fn expected_rev_follows_contract_columns() {
        let mut up = UltraPlanState::new_flow("x", None, &phantom_root());
        up.questionnaire_rev = 2;
        up.demo_iteration = 3;
        up.plan_rev = 4;
        up.acceptance_round = 5;
        assert_eq!(Action::Answer.expected_rev(&up), Some(2));
        assert_eq!(Action::ApproveDemo.expected_rev(&up), Some(3));
        assert_eq!(Action::ReviseDemo.expected_rev(&up), Some(3));
        assert_eq!(Action::RevisePlan.expected_rev(&up), Some(4));
        assert_eq!(Action::StartProduction.expected_rev(&up), Some(4));
        assert_eq!(Action::ResumeProduction.expected_rev(&up), None);
        assert_eq!(Action::FixProduction.expected_rev(&up), Some(5));
    }

    #[test]
    fn plain_plan_turn_blocked_only_on_flow_plan_mid_flow() {
        let mut up = UltraPlanState::new_flow("塔防", None, &phantom_root());
        let plan = up.reserved_plan_path();
        up.plan_path = Some(plan.clone());
        for (stage, blocked) in [
            (STAGE_DEMO_REVIEW, false),
            (STAGE_PLAN_REVIEW, true),
            (STAGE_PRODUCTION, true),
            (STAGE_ACCEPTANCE, true),
            (STAGE_DONE, false),
        ] {
            up.stage = stage.to_string();
            assert_eq!(
                blocks_plain_plan_turn(Some(&up), "build", Some(&plan)),
                blocked,
                "{stage}"
            );
        }
        up.stage = STAGE_PLAN_REVIEW.to_string();
        assert!(blocks_plain_plan_turn(Some(&up), "plan", Some(&plan)));
        assert!(
            !blocks_plain_plan_turn(Some(&up), "team", Some(&plan)),
            "team + 动作是制作入口,由 route 管"
        );
        assert!(!blocks_plain_plan_turn(
            Some(&up),
            "build",
            Some(".forge/plans/别的.plan.md")
        ));
        assert!(!blocks_plain_plan_turn(Some(&up), "build", None));
        assert!(!blocks_plain_plan_turn(None, "build", Some(&plan)));
    }

    // ---------- 状态 / slug / token ----------

    #[test]
    fn new_flow_allocates_contract_shaped_state() {
        let ws = temp_dir("newflow");
        let up = UltraPlanState::new_flow(
            "\n  做一个 2D 塔防:向日葵产阳光,豌豆射手打僵尸  \n第二行细节",
            Some(" ws_1 "),
            &ws,
        );
        assert!(up.id.starts_with("up_"), "{}", up.id);
        assert_eq!(up.token.len(), 32);
        assert!(up
            .token
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        assert!(!up.token.contains(&up.slug) && !up.id.contains(&up.token));
        assert_eq!(up.title, "做一个 2D 塔防:向日葵产阳光,豌豆射手打僵尸");
        assert_eq!(up.dir, format!(".forge/ultraplan/{}", up.slug));
        assert!(is_flow_dir(&up.dir));
        assert_eq!(up.workspace_id.as_deref(), Some("ws_1"));
        assert_eq!(up.stage, STAGE_DISCOVERY);
        assert_eq!(up.phase, PHASE_WAITING);
        assert!(up.is_active());
        assert!(up.plan_path.is_none(), "首份计划落盘前 planPath 为 null");
        assert!(crate::plan_doc::is_plan_path(&up.reserved_plan_path()));
        assert_eq!(
            up.dir_abs(&ws).unwrap(),
            ws.join(".forge").join("ultraplan").join(&up.slug)
        );
        // wire:camelCase,Option 字段恒在(null),计数从 0 起。
        let wire = serde_json::to_value(&up).unwrap();
        let mut keys: Vec<&str> = wire
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "acceptanceRound",
                "createdAt",
                "demoIteration",
                "demoNote",
                "demoVerified",
                "dir",
                "id",
                "lastError",
                "phase",
                "planHash",
                "planPath",
                "planRev",
                "productionRunId",
                "questionnaireRev",
                "running",
                "slug",
                "stage",
                "title",
                "token",
                "updatedAt",
                "workspaceId",
            ],
            "字段面须与契约 §1 逐一对上"
        );
        assert_eq!(wire["running"], Value::Null);
        assert_eq!(wire["lastError"], Value::Null);
        assert_eq!(wire["planPath"], Value::Null);
        assert_eq!(wire["questionnaireRev"], 0);
        assert_eq!(wire["demoVerified"], false);
        // 两次分配的 token 不同。
        assert_ne!(new_token(), new_token());
        // 手改过的 dir 不被当成流程目录。
        let mut bad = up.clone();
        bad.dir = "../../etc".to_string();
        assert!(bad.dir_abs(&ws).is_none());
        for dir in [
            ".forge/ultraplan/a/b",
            ".forge/ultraplan/",
            ".forge/ultraplan/..",
            ".forge/ultraplan/C:evil",
            ".forge\\ultraplan\\a\\b",
            ".forge/plans/x",
            "C:/Windows",
        ] {
            bad.dir = dir.to_string();
            assert!(bad.dir_abs(&ws).is_none(), "{dir}");
        }
        assert!(is_flow_dir(".forge\\ultraplan\\塔防-0a1b"), "反斜杠归一");
        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn slug_is_unique_against_existing_dirs() {
        let ws = temp_dir("slug");
        let id = "up_1700000000000_0a1b2c3d";
        let first = allocate_slug(&ws, "塔防 小游戏", id);
        assert_eq!(first, "塔防-小游戏-2c3d", "主体 + id 末 4 位");
        // 1) 同名流程目录已存在 → 换一个。
        std::fs::create_dir_all(ws.join(".forge").join("ultraplan").join(&first)).unwrap();
        let second = allocate_slug(&ws, "塔防 小游戏", id);
        assert_ne!(second, first);
        assert!(second.starts_with("塔防-小游戏-"), "{second}");
        assert!(!slug_taken(&ws, &second));
        // 2) 同名计划文件已存在(别的会话用 create_plan 写的)→ 也不能占。
        let other_id = "up_1700000000001_00009999";
        let planned = "塔防-小游戏-9999";
        let plan_abs = crate::plan_doc::abs_path(&ws, &crate::plan_doc::plan_rel_path(planned));
        std::fs::create_dir_all(plan_abs.parent().unwrap()).unwrap();
        std::fs::write(&plan_abs, "---\nname: x\n---\n").unwrap();
        let third = allocate_slug(&ws, "塔防 小游戏", other_id);
        assert_ne!(third, planned, "既有计划文件不得被新流程占用(I-6)");
        assert!(!slug_taken(&ws, &third));
        // 3) new_flow 走同一条分配:连开多条同标题流程,目录互不相同。
        let mut seen = HashSet::new();
        for _ in 0..8 {
            let up = UltraPlanState::new_flow("塔防 小游戏", None, &ws);
            assert!(!slug_taken(&ws, &up.slug));
            std::fs::create_dir_all(up.dir_abs(&ws).unwrap()).unwrap();
            assert!(seen.insert(up.slug.clone()), "slug 重复: {}", up.slug);
        }
        // 4) 主体只取前 24 字,路径字符被清洗;全标点标题用兜底名。
        let long = allocate_slug(&ws, &"长".repeat(100), id);
        assert_eq!(long.chars().count(), SLUG_BRIEF_CHARS + 5);
        let dirty = allocate_slug(&ws, "../../etc/passwd: a*b?", id);
        assert_eq!(
            dirty, "etc-passwd-a-b-2c3d",
            "路径字符一律清洗,`..` 逃逸根除"
        );
        assert_eq!(
            allocate_slug(&ws, "做个 2D 塔防:向日葵,豌豆(射手)!", id),
            "做个-2D-塔防-向日葵-豌豆-射手-2c3d",
            "标点一律收成短横线:目录名要经得起模型逐字照抄"
        );
        assert_eq!(allocate_slug(&ws, " ./?* -- ,。!", id), "game-2c3d");
        for slug in [&first, &second, &third, &long, &dirty] {
            assert!(is_flow_dir(&flow_dir_rel(slug)), "{slug}");
            assert!(
                crate::plan_doc::is_plan_path(&crate::plan_doc::plan_rel_path(slug)),
                "{slug}"
            );
        }
        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn lenient_deserialize_tolerates_partial_and_broken_state() {
        #[derive(Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "deserialize_lenient")]
            ultraplan: Option<UltraPlanState>,
        }
        // 缺字段的旧状态:补缺省。
        let h: Holder =
            serde_json::from_value(json!({ "ultraplan": { "id": "up_1", "slug": "s" } })).unwrap();
        let up = h.ultraplan.expect("半截状态应读回");
        assert_eq!(up.stage, STAGE_DISCOVERY);
        assert_eq!(up.phase, PHASE_WAITING);
        assert_eq!(up.questionnaire_rev, 0);
        assert!(up.workspace_id.is_none());
        // 类型不符 / 没有 id / null / 缺键:都当作没有流程,而不是让宿主结构解析失败。
        for raw in [
            json!({ "ultraplan": { "id": "up_1", "stage": 7 } }),
            json!({ "ultraplan": { "slug": "无 id" } }),
            json!({ "ultraplan": "坏掉了" }),
            json!({ "ultraplan": null }),
            json!({}),
        ] {
            let h: Holder = serde_json::from_value(raw.clone()).expect("宿主结构不得解析失败");
            assert!(h.ultraplan.is_none(), "{raw}");
        }
    }

    #[test]
    fn stage_payload_shape() {
        let mut up = UltraPlanState::new_flow("x", None, &phantom_root());
        up.phase = PHASE_RUNNING.to_string();
        up.running = Some(RUNNING_DISCOVERY.to_string());
        let v = stage_payload(&up, Some("run_1"));
        assert_eq!(v["id"], up.id);
        assert_eq!(v["runId"], "run_1");
        assert_eq!(v["stage"], "discovery");
        assert_eq!(v["phase"], "running");
        assert_eq!(v["running"], "discovery");
        assert!(v.get("lastError").is_none());
        up.phase = PHASE_FAILED.to_string();
        up.running = None;
        up.last_error = Some(FlowError {
            code: "ULTRAPLAN_TURN_INVALID".into(),
            message: "坏了".into(),
        });
        let v = stage_payload(&up, None);
        assert!(v.get("runId").is_none());
        assert_eq!(v["running"], Value::Null);
        assert_eq!(v["lastError"]["code"], "ULTRAPLAN_TURN_INVALID");
    }

    /// lastError 的码:`ULTRAPLAN_*` 只许出现契约 §2 失败码表里的;表外的(写盘失败、产物目录不可用、
    /// 旧的兜底码…)归通用码,原因留在 message。渠道 / 云端自带的码照原样透传;不带码的错误归通用码。
    #[test]
    fn flow_error_only_emits_contract_turn_failure_codes() {
        for code in TURN_FAILURE_CODES {
            let e = flow_error(&format!("{code}: 出了点问题"));
            assert_eq!(e.code, code);
            assert_eq!(e.message, "出了点问题");
        }
        for raw in [
            "ULTRAPLAN_IO: 写入 .forge/ultraplan/x/brief.md 失败: 拒绝访问",
            "ULTRAPLAN_ARTIFACTS_MISSING: 流程目录不可用",
            "ULTRAPLAN_TURN_FAILED: 旧兜底码",
            "ULTRAPLAN_TURN_INTERRUPTED: agentd 进程重启",
            "ULTRAPLAN_SOMETHING_NEW: 以后新加的码没进契约",
        ] {
            let e = flow_error(raw);
            assert_eq!(e.code, ERR_TURN_INVALID, "{raw}");
            assert!(
                !e.message.is_empty() && raw.ends_with(&e.message),
                "{raw} → {}",
                e.message
            );
        }
        let provider = flow_error("OPENAI_COMPAT_NOT_CONFIGURED: openai-compat 渠道未配齐");
        assert_eq!(
            provider.code, "OPENAI_COMPAT_NOT_CONFIGURED",
            "渠道错误码照原样透传"
        );
        let bare = flow_error("openai-compat HTTP 500: upstream boom");
        assert_eq!(bare.code, ERR_TURN_INVALID);
        assert_eq!(bare.message, "openai-compat HTTP 500: upstream boom");
        let empty = flow_error("");
        assert_eq!(empty.code, ERR_TURN_INVALID);
        assert!(!empty.message.is_empty());
    }

    // ---------- 深度规划的退路 ----------

    /// 固定结果的步进(计调用次数)。
    fn fixed_step(
        result: Result<&'static str, &'static str>,
        calls: Arc<AtomicUsize>,
    ) -> Box<crate::llm::StepFn> {
        Box::new(move |_m, _t, _s| {
            calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                match result {
                    Ok(text) => Ok(crate::llm::StepOutcome {
                        message: json!({ "role": "assistant", "content": text }),
                        usage: None,
                    }),
                    Err(e) => Err(crate::llm::LlmError::new(e)),
                }
            })
        })
    }

    /// 只认「参数类 4xx + 点到推理档位 / 不支持的参数」;鉴权、额度、上下文超长、5xx、网络错误都不算。
    #[test]
    fn effort_rejection_only_matches_parameter_errors() {
        for e in [
            "openai-compat HTTP 400: Unsupported value: 'reasoning_effort' does not support 'max' with this model.",
            "openai-compat HTTP 400: Unrecognized request argument supplied: reasoning_effort",
            "openai-compat HTTP 422: extra_forbidden",
            "cloud HTTP 400: invalid reasoning effort",
            "DeepSeek HTTP 400: deepseek-reasoner does not support Function Calling",
        ] {
            assert!(is_effort_rejection(e), "{e}");
        }
        for e in [
            "openai-compat HTTP 401: invalid api key",
            "openai-compat HTTP 402: insufficient balance",
            "openai-compat HTTP 400: context length exceeded",
            "openai-compat HTTP 500: reasoning backend crashed",
            "openai-compat 连接失败: timed out",
            "OPENAI_COMPAT_NOT_CONFIGURED: openai-compat 渠道未配齐",
        ] {
            assert!(!is_effort_rejection(e), "{e}");
        }
    }

    /// 深度规格被拒 → 这一步改用会话规格重发、本轮余下也走会话规格(深度规格只试一次),只上报一次;
    /// 上报的档位改成会话自己的。别的错误原样返回、不退回;成功原样通过。
    #[tokio::test]
    async fn deep_step_falls_back_once_on_effort_rejection() {
        let deep_calls = Arc::new(AtomicUsize::new(0));
        let plain_calls = Arc::new(AtomicUsize::new(0));
        let reasons = Arc::new(Mutex::new(Vec::<String>::new()));
        let fb = Arc::new(DeepFallback::new(Some("medium".to_string())));
        let deep = DeepPlanning {
            effort: Some("max".to_string()),
            thinking_forced: true,
            context_tokens: 1_048_576,
        };
        assert_eq!(fb.apply(&deep), deep, "没退过原样上报");
        let step = {
            let reasons = reasons.clone();
            with_deep_fallback(
                fixed_step(
                    Err("openai-compat HTTP 400: Unsupported value for reasoning_effort: 'max'"),
                    deep_calls.clone(),
                ),
                fixed_step(Ok("plain"), plain_calls.clone()),
                fb.clone(),
                move |r| reasons.lock().unwrap().push(r.to_string()),
            )
        };
        for _ in 0..3 {
            let out = step(Vec::new(), Vec::new(), None)
                .await
                .expect("退回会话规格后照常完成");
            assert_eq!(out.message["content"], "plain");
        }
        assert_eq!(deep_calls.load(Ordering::SeqCst), 1, "深度规格只试一次");
        assert_eq!(plain_calls.load(Ordering::SeqCst), 3);
        assert!(fb.used());
        {
            let reasons = reasons.lock().unwrap();
            assert_eq!(reasons.len(), 1, "只上报一次");
            assert!(reasons[0].contains("reasoning_effort"), "{reasons:?}");
        }
        let reported = fb.apply(&deep);
        assert_eq!(
            reported.effort.as_deref(),
            Some("medium"),
            "报会话自己的档位"
        );
        assert!(!reported.thinking_forced);
        assert_eq!(reported.context_tokens, deep.context_tokens);

        // 鉴权错误:原样返回,不退回、不上报。
        let fb2 = Arc::new(DeepFallback::new(None));
        let plain2 = Arc::new(AtomicUsize::new(0));
        let step2 = with_deep_fallback(
            fixed_step(
                Err("openai-compat HTTP 401: invalid api key"),
                Arc::new(AtomicUsize::new(0)),
            ),
            fixed_step(Ok("plain"), plain2.clone()),
            fb2.clone(),
            |_| panic!("非参数错误不该上报"),
        );
        let err = step2(Vec::new(), Vec::new(), None)
            .await
            .err()
            .expect("鉴权错误原样返回");
        assert!(err.to_string().contains("401"));
        assert_eq!(plain2.load(Ordering::SeqCst), 0);
        assert!(!fb2.used());
        assert_eq!(fb2.apply(&deep), deep);

        // 深度规格被接受:原样通过,会话规格不被调用。
        let plain3 = Arc::new(AtomicUsize::new(0));
        let fb3 = Arc::new(DeepFallback::new(None));
        let step3 = with_deep_fallback(
            fixed_step(Ok("deep"), Arc::new(AtomicUsize::new(0))),
            fixed_step(Ok("plain"), plain3.clone()),
            fb3.clone(),
            |_| panic!("没被拒不该上报"),
        );
        let out = step3(Vec::new(), Vec::new(), None)
            .await
            .expect("深度规格可用");
        assert_eq!(out.message["content"], "deep");
        assert_eq!(plain3.load(Ordering::SeqCst), 0);
        assert!(!fb3.used());
    }

    // ---------- 项目事实 ----------

    #[test]
    fn project_facts_fresh_project_is_empty_and_assets_count() {
        let root = temp_dir("facts");
        crate::project::init_project(&root, "新游戏", "2d").expect("脚手架");
        let ws = root.canonicalize().unwrap();
        // 起始脚手架:一张场景、零资产 → 空项目,不要求 explore。
        let fresh = project_facts(&ws, &ws);
        assert!(fresh.has_project);
        assert_eq!(fresh.game_mode.as_deref(), Some("2d"));
        assert_eq!(
            (fresh.assets, fresh.scenes, fresh.scripts, fresh.docs),
            (0, 1, 0, 0)
        );
        assert!(!fresh.has_content);
        assert!(fresh.scan_error.is_none());
        let text = fresh.render();
        assert!(
            text.contains("维度模式 2d") && text.contains("无需派发 explore"),
            "{text}"
        );

        // 一张贴图(+ 它的 .meta 与点文件不计)→ 有内容。
        let tex = ws.join("Content").join("Textures");
        std::fs::write(tex.join("a.png"), b"x").unwrap();
        std::fs::write(tex.join("a.png.meta"), b"guid: 1").unwrap();
        std::fs::write(tex.join(".gitkeep"), b"").unwrap();
        let with_asset = project_facts(&ws, &ws);
        assert_eq!(with_asset.assets, 1);
        assert!(with_asset.has_content);
        let text = with_asset.render();
        assert!(
            text.contains("资产 1 个") && text.contains("subagent_type:\"explore\""),
            "{text}"
        );
        std::fs::remove_file(tex.join("a.png")).unwrap();
        assert!(!project_facts(&ws, &ws).has_content, "删掉后回到空项目");

        // 脚本 / 逻辑图、文档、第二张场景各自都足以判「有内容」。
        let script = ws.join("Content").join("Scripts").join("player.rx");
        std::fs::write(&script, "fn main() {}").unwrap();
        let f = project_facts(&ws, &ws);
        assert_eq!((f.scripts, f.assets), (1, 0));
        assert!(f.has_content);
        std::fs::remove_file(&script).unwrap();

        std::fs::write(ws.join("README.md"), "# 说明").unwrap();
        std::fs::create_dir_all(ws.join("docs").join("design")).unwrap();
        std::fs::write(ws.join("docs").join("design").join("gdd.md"), "# GDD").unwrap();
        let f = project_facts(&ws, &ws);
        assert_eq!(f.docs, 2);
        assert!(f.has_content);
        std::fs::remove_file(ws.join("README.md")).unwrap();
        std::fs::remove_dir_all(ws.join("docs")).unwrap();

        std::fs::write(
            ws.join("Content").join("Scenes").join("Level2.rxscene"),
            "{}",
        )
        .unwrap();
        let f = project_facts(&ws, &ws);
        assert_eq!(f.scenes, 2);
        assert!(f.has_content, "两张场景 = 已经动过工");
        std::fs::remove_file(ws.join("Content").join("Scenes").join("Level2.rxscene")).unwrap();

        // 清单坏了:扫描失败不等于空项目——如实记原因,并保住 explore 要求。
        std::fs::write(ws.join("forge.toml"), "[project]\nmode = \"5d\"\n").unwrap();
        let broken = project_facts(&ws, &ws);
        assert!(broken.has_project);
        assert!(broken.game_mode.is_none());
        assert!(broken.scan_error.as_deref().unwrap().contains("forge.toml"));
        assert!(broken.has_content, "扫描失败须按「已有内容」处理");
        assert!(broken.render().contains("项目扫描失败"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn project_facts_fallback_root_is_no_project() {
        // 工作区是个空目录;另有一个带资产的完整项目在它**外面**(相当于 scope 退到的仓内 demo)。
        let ws = temp_dir("facts-ws").canonicalize().unwrap();
        let elsewhere = temp_dir("facts-demo");
        crate::project::init_project(&elsewhere, "demo", "3d").unwrap();
        let elsewhere = elsewhere.canonicalize().unwrap();
        std::fs::write(
            elsewhere.join("Content").join("Textures").join("a.png"),
            b"x",
        )
        .unwrap();
        let facts = project_facts(&ws, &elsewhere);
        assert_eq!(facts, ProjectFacts::none(), "工作区外的项目不是用户的项目");
        assert!(!facts.has_project && !facts.has_content);
        assert!(
            facts.game_mode.is_none(),
            "别人项目的 2D/3D 不能当成本项目的"
        );
        let text = facts.render();
        assert!(
            text.contains("还没有 Forge 项目") && text.contains("无需派发 explore"),
            "{text}"
        );
        // 真实解析路径:空工作区经 scope 退回仓内 projects/demo,同样判「没有项目」。
        let fallback = crate::scope::project_root_of(&ws);
        assert_ne!(fallback, ws);
        assert_eq!(project_facts(&ws, &fallback), ProjectFacts::none());
        // 工作区自带 Content 但没 forge.toml:不算有项目,但内容要数(不能装作空白)。
        std::fs::create_dir_all(ws.join("Content").join("Sprites")).unwrap();
        std::fs::write(ws.join("Content").join("Sprites").join("hero.png"), b"x").unwrap();
        let loose = project_facts(&ws, &crate::scope::project_root_of(&ws));
        assert!(!loose.has_project);
        assert!(loose.game_mode.is_none());
        assert_eq!(loose.assets, 1);
        assert!(loose.has_content);
        assert!(loose.render().contains("尚未初始化"), "{}", loose.render());
        std::fs::remove_dir_all(&ws).ok();
        std::fs::remove_dir_all(&elsewhere).ok();
    }

    /// 退回腿落在工作区**里面**:工作区根没有 forge.toml / Content,但有一个完整的 projects/demo
    /// (2D、带资产)。scope 会退到这个 demo——它在工作区内,却仍不是用户的项目:事实为空,
    /// 2D/3D 不算已定,不要求 explore,ultra_foreign_project 成立(MCP 与预检索不指向它)。
    #[test]
    fn project_facts_demo_fallback_inside_workspace_is_no_project() {
        let ws = temp_dir("facts-ws-demo").canonicalize().unwrap();
        let demo = ws.join("projects").join("demo");
        std::fs::create_dir_all(&demo).unwrap();
        crate::project::init_project(&demo, "demo", "2d").unwrap();
        std::fs::write(demo.join("Content").join("Textures").join("a.png"), b"x").unwrap();
        let project_root = crate::scope::project_root_of(&ws);
        assert_eq!(
            project_root,
            demo.canonicalize().unwrap(),
            "scope 退到工作区内的 demo"
        );
        assert!(
            forge_util::pathutil::is_inside(&ws, &project_root),
            "前提:退回腿就在工作区目录里(旧判据会把它当成用户的项目)"
        );
        // 对照:demo 本身是个有内容的 2D 项目。
        let demo_facts = project_facts(&project_root, &project_root);
        assert!(demo_facts.has_project && demo_facts.has_content);
        assert_eq!(demo_facts.game_mode.as_deref(), Some("2d"));

        let facts = project_facts(&ws, &project_root);
        assert_eq!(
            facts,
            ProjectFacts::none(),
            "工作区内的退回 demo 不是用户的项目"
        );
        let text = facts.render();
        assert!(
            text.contains("还没有 Forge 项目") && !text.contains("已定"),
            "{text}"
        );
        let scope = crate::scope::ScopeProject {
            workspace_id: Some("ws_demo_inside".to_string()),
            name: "带 demo 的工作区".to_string(),
            workspace_root: ws.clone(),
            project_root: project_root.clone(),
            game_mode: crate::scope::game_mode_of(&project_root),
        };
        assert!(
            !project_in_workspace(&scope),
            "ultra_foreign_project 须成立"
        );

        // 工作区根自己像项目时照旧算用户的项目(两条判据口径一致)。
        crate::project::init_project(&ws, "真项目", "3d").unwrap();
        let own_root = crate::scope::project_root_of(&ws);
        assert_eq!(own_root, ws);
        let own = project_facts(&ws, &own_root);
        assert!(own.has_project);
        assert_eq!(own.game_mode.as_deref(), Some("3d"));
        assert!(project_in_workspace(&crate::scope::ScopeProject {
            project_root: own_root,
            ..scope
        }));
        std::fs::remove_dir_all(&ws).ok();
    }

    /// 未绑定工作区的会话:工作区 = 进程默认根(仓根),scope 退到仓内 projects/demo(它就在仓根下面)。
    /// 走真实的 resolve_request:事实为空、不算有项目、项目不在工作区(MCP / 预检索不碰仓内 demo)。
    /// 仓根本身像项目时(有 forge.toml / Content)这条退回腿不存在,用例只核对口径一致后返回。
    #[test]
    fn project_facts_no_workspace_session_ignores_repo_demo() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()
            .unwrap();
        let project_root = crate::scope::project_root_of(&repo);
        let scope = crate::scope::ScopeProject {
            workspace_id: None,
            name: "默认工作区".to_string(),
            workspace_root: repo.clone(),
            project_root: project_root.clone(),
            game_mode: crate::scope::game_mode_of(&project_root),
        };
        if project_root == repo {
            assert!(project_in_workspace(&scope));
            return;
        }
        assert!(
            !project_in_workspace(&scope),
            "默认根的退回腿不是用户的项目"
        );
        assert_eq!(project_facts(&repo, &project_root), ProjectFacts::none());
        let (state, dir) = crate::test_app_state("up-facts-nows");
        let session = state.sessions.create("t", "coding", None, true, None);
        assert!(session.workspace_id.is_none());
        let ut = resolve_request(&session, &scope, MODE, None, "做一个塔防", None)
            .ok()
            .flatten()
            .expect("无流程的会话 + 自由文本 = 新流程的立项讨论");
        assert_eq!(ut.facts, ProjectFacts::none());
        assert!(
            ut.facts.game_mode.is_none(),
            "仓内 demo 的维度不能当成本项目的"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 问卷 ----------

    fn sample_questionnaire() -> Value {
        json!({
            "title": "塔防小游戏 · 需求确认",
            "understanding": "## 我的理解\n一个 2D 塔防。",
            "sections": [
                {
                    "id": "core", "title": "核心玩法",
                    "questions": [
                        {
                            "id": "loop", "kind": "single", "question": "核心循环是哪一种?",
                            "help": "决定关卡节奏",
                            "options": [
                                { "id": "wave", "label": "波次防守", "description": "经典", "recommended": true },
                                { "id": "endless", "label": "无尽模式" }
                            ],
                            "allowOther": true
                        },
                        {
                            "id": "towers", "kind": "multi", "question": "首批要哪些塔?",
                            "options": [
                                { "id": "shooter", "label": "射手", "recommended": true },
                                { "id": "slow", "label": "减速", "recommended": true },
                                { "id": "aoe", "label": "范围" }
                            ],
                            "min": 1, "max": 2
                        }
                    ]
                },
                {
                    "id": "feel", "title": "手感",
                    "questions": [
                        { "id": "difficulty", "kind": "scale", "question": "难度?", "scaleLabels": ["轻松", "硬核"] },
                        { "id": "name", "kind": "text", "question": "游戏叫什么?" }
                    ]
                }
            ]
        })
    }

    /// 在样例上改一处,断言被拒且错误信息点到要害。
    fn rejects(mutate: impl FnOnce(&mut Value), needle: &str) {
        let mut v = sample_questionnaire();
        mutate(&mut v);
        let err = validate_questionnaire(&v).expect_err(&format!("应被拒(期望含「{needle}」)"));
        assert!(err.contains(needle), "错误信息应含「{needle}」,实得: {err}");
    }

    #[test]
    fn implementation_stack_is_required_and_resolves_explicit_choices() {
        let q = ensure_implementation_stack(
            validate_questionnaire(&sample_questionnaire()).unwrap(),
            &ProjectFacts::none(),
        )
        .unwrap();
        assert_eq!(q.question_count(), 5);
        let stack = q.find(IMPLEMENTATION_STACK_QUESTION).unwrap();
        assert!(stack.required && !stack.allow_delegate && !stack.allow_other);
        let options = stack.options.as_ref().unwrap();
        assert_eq!(options[0].id, "2d_godot");
        assert!(options[0].recommended);
        let wire = serde_json::to_value(&q).unwrap();
        for (id, expected) in [
            ("2d_godot", ("2d", "godot")),
            ("3d_godot", ("3d", "godot")),
            ("3d_rurix", ("3d", "rurix")),
        ] {
            let answers = json!({IMPLEMENTATION_STACK_QUESTION: {"choice": [id]}});
            assert_eq!(
                confirmed_implementation_stack(&wire, &answers),
                Ok(expected)
            );
        }
        for answers in [
            json!({}),
            json!({IMPLEMENTATION_STACK_QUESTION: {"delegate": true}}),
            json!({IMPLEMENTATION_STACK_QUESTION: {"choice": ["2d_rurix"]}}),
            json!({IMPLEMENTATION_STACK_QUESTION: {"choice": ["3d_rurix"], "other": "godot"}}),
            json!({IMPLEMENTATION_STACK_QUESTION: {"choice": ["3d_rurix", "3d_godot"]}}),
        ] {
            assert!(confirmed_implementation_stack(&wire, &answers).is_err());
        }
        assert!(confirmed_implementation_stack(&sample_questionnaire(), &json!({})).is_err());
    }

    #[test]
    fn implementation_stack_replaces_model_overrides_and_confirms_legacy_projects() {
        let mut input = sample_questionnaire();
        input["sections"][0]["questions"][0]["id"] = json!(IMPLEMENTATION_STACK_QUESTION);
        // Model-supplied delegation and options must not change the server policy.
        let q = ensure_implementation_stack(
            validate_questionnaire(&input).unwrap(),
            &ProjectFacts::none(),
        )
        .unwrap();
        assert_eq!(q.question_count(), 4);
        let stack = q.find(IMPLEMENTATION_STACK_QUESTION).unwrap();
        assert!(!stack.allow_delegate && !stack.allow_other);
        assert_eq!(stack.options.as_ref().unwrap().len(), 3);

        for (mode, backend) in [("2d", "godot"), ("3d", "rurix"), ("2d", "rurix")] {
            let mut facts = ProjectFacts::none();
            facts.has_project = true;
            facts.game_mode = Some(mode.into());
            facts.render_backend = Some(backend.into());
            let q = ensure_implementation_stack(
                validate_questionnaire(&sample_questionnaire()).unwrap(),
                &facts,
            )
            .unwrap();
            let stack = q.find(IMPLEMENTATION_STACK_QUESTION).unwrap();
            let options = stack.options.as_ref().unwrap();
            assert_eq!(options.len(), 1);
            let wire = serde_json::to_value(&q).unwrap();
            assert!(
                validate_questionnaire(&wire).is_ok(),
                "persisted schema must roundtrip"
            );
            let answers = json!({IMPLEMENTATION_STACK_QUESTION: {"choice": [options[0].id]}});
            assert_eq!(
                confirmed_implementation_stack(&wire, &answers),
                Ok(implementation_stack(&options[0].id).unwrap())
            );
            let mut ordinary_single = wire;
            ordinary_single["sections"][0]["questions"][0]["id"] = json!("ordinary_confirmation");
            assert!(validate_questionnaire(&ordinary_single).is_err());
        }

        let mut facts = ProjectFacts::none();
        facts.has_project = true;
        assert!(ensure_implementation_stack(q, &facts).is_err());
    }

    #[test]
    fn implementation_stack_reserves_a_question_without_dropping_requirements() {
        let mut input = sample_questionnaire();
        input["sections"] = Value::Array(
            (0..MAX_SECTIONS)
                .map(|section| {
                    json!({"id": format!("s{section}"), "title": "需要确认", "questions":
                        (0..5).map(|question| json!({"id":format!("q{section}_{question}"),"kind":"text","question":"需求？"})).collect::<Vec<_>>()})
                })
                .collect(),
        );
        let full = validate_questionnaire(&input).unwrap();
        assert_eq!(full.question_count(), MAX_QUESTIONS);
        assert!(ensure_implementation_stack(full, &ProjectFacts::none())
            .unwrap_err()
            .contains("预留"));
        input["sections"][MAX_SECTIONS - 1]["questions"]
            .as_array_mut()
            .unwrap()
            .pop();
        let q = ensure_implementation_stack(
            validate_questionnaire(&input).unwrap(),
            &ProjectFacts::none(),
        )
        .unwrap();
        assert_eq!(q.question_count(), MAX_QUESTIONS);
        assert_eq!(q.sections.len(), MAX_SECTIONS);
        assert!(q.find("q0_0").is_some() && q.find("q7_3").is_some());
        assert!(q.find(IMPLEMENTATION_STACK_QUESTION).is_some());
    }

    #[test]
    fn questionnaire_exit_persists_and_displays_backend_confirmation() {
        let (state, dir) = crate::test_app_state("up-stack-questionnaire");
        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let scope = crate::scope::ScopeProject {
            workspace_id: None,
            name: "新项目".into(),
            workspace_root: ws.clone(),
            project_root: ws.clone(),
            game_mode: crate::scope::game_mode_of(&ws),
        };
        let session = state.sessions.create("新项目", "coding", None, true, None);
        let ut = resolve_request(&session, &scope, MODE, None, "制作游戏", None)
            .unwrap()
            .unwrap();
        let rt = begin_turn(&state, &session.id, "run_stack", &scope, "制作游戏", &ut).unwrap();
        let (ok, message) = handle_exit_tool(
            &state,
            &session.id,
            "run_stack",
            Some(&rt),
            QUESTIONNAIRE_TOOL,
            &sample_questionnaire(),
        );
        assert!(ok, "{message}");
        let stored = read_json(&rt.dir_abs.join(QUESTIONNAIRE_FILE)).unwrap();
        assert_eq!(
            stored["sections"][0]["questions"][0]["id"],
            IMPLEMENTATION_STACK_QUESTION
        );
        assert_eq!(
            confirmed_implementation_stack(
                &stored,
                &json!({IMPLEMENTATION_STACK_QUESTION: {"choice": ["3d_rurix"]}}),
            ),
            Ok(("3d", "rurix"))
        );
        let event = state
            .events
            .persisted(&session.id)
            .into_iter()
            .find(|e| e.event_type == "ultraplan.questionnaire")
            .unwrap();
        assert_eq!(event.payload["questionnaire"], stored);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn questionnaire_validation_rejects_bad_shapes() {
        // 合法样例:通过并归一缺省。
        let q = validate_questionnaire(&sample_questionnaire()).expect("样例应通过");
        assert_eq!(q.sections.len(), 2);
        assert_eq!(q.question_count(), 4);
        let single = q.find("loop").unwrap();
        assert!(single.allow_other && single.allow_delegate && single.required);
        assert_eq!((single.min, single.max), (None, None));
        let multi = q.find("towers").unwrap();
        assert_eq!((multi.min, multi.max), (Some(1), Some(2)));
        assert!(!multi.allow_other);
        let scale = q.find("difficulty").unwrap();
        assert_eq!(
            (scale.min, scale.max),
            (Some(1), Some(5)),
            "scale 缺省 1..5"
        );
        assert_eq!(
            scale.scale_labels,
            Some(["轻松".to_string(), "硬核".to_string()])
        );
        assert!(scale.options.is_none());
        let text = q.find("name").unwrap();
        assert!(!text.required, "text 缺省选答");
        assert!(text.allow_delegate);
        // 归一后的 wire 是 camelCase 且可原样读回(事件载荷与 questionnaire.json 同形)。
        let wire = serde_json::to_value(&q).unwrap();
        assert_eq!(wire["sections"][0]["questions"][0]["allowOther"], true);
        assert_eq!(
            wire["sections"][0]["questions"][0]["options"][0]["recommended"],
            true
        );
        assert_eq!(
            wire["sections"][1]["questions"][0]["scaleLabels"],
            json!(["轻松", "硬核"])
        );
        assert!(wire["sections"][1]["questions"][1].get("options").is_none());
        assert_eq!(serde_json::from_value::<Questionnaire>(wire).unwrap(), q);
        // 已归一的问卷再校验一遍结果不变(重出问卷/回读文件走同一入口)。
        assert_eq!(
            validate_questionnaire(&serde_json::to_value(&q).unwrap()).unwrap(),
            q
        );

        // 顶层形态。
        assert!(validate_questionnaire(&json!([]))
            .unwrap_err()
            .contains("须为对象"));
        assert!(validate_questionnaire(&json!({}))
            .unwrap_err()
            .contains("title"));
        rejects(|v| v["title"] = json!("  "), "title 不可为空");
        rejects(|v| v["understanding"] = Value::Null, "understanding 必填");
        rejects(|v| v["title"] = json!("长".repeat(121)), "过长");
        rejects(|v| v["sections"] = json!([]), "1–8 节");
        rejects(|v| v["sections"] = json!("不是数组"), "sections 必填");
        rejects(
            |v| {
                let s = v["sections"][0].clone();
                v["sections"] = Value::Array((0..9).map(|_| s.clone()).collect());
            },
            "1–8 节",
        );
        // 章节。
        rejects(|v| v["sections"][1]["id"] = json!("core"), "章节 id 须唯一");
        rejects(
            |v| v["sections"][0]["questions"] = json!([]),
            "每节至少 1 题",
        );
        rejects(
            |v| v["sections"][0]["title"] = json!(""),
            "sections[0].title",
        );
        rejects(|v| v["sections"][0]["id"] = json!("有 空格"), "不可含空白");
        // 题目总数上限 40。
        rejects(
            |v| {
                let qs: Vec<Value> = (0..41)
                    .map(|i| json!({ "id": format!("q{i}"), "kind": "text", "question": "问?" }))
                    .collect();
                v["sections"][0]["questions"] = Value::Array(qs);
            },
            "超过 40 题",
        );
        // 题目 id 跨章节唯一。
        rejects(
            |v| v["sections"][1]["questions"][1]["id"] = json!("loop"),
            "整份问卷内唯一",
        );
        rejects(
            |v| v["sections"][0]["questions"][0]["kind"] = json!("dropdown"),
            "不支持",
        );
        rejects(
            |v| {
                v["sections"][0]["questions"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("question");
            },
            "sections[0].questions[0].question 必填",
        );
        rejects(
            |v| v["sections"][0]["questions"][0]["required"] = json!("yes"),
            "须为布尔值",
        );
        // 选项:2..=6、id 唯一、单选至多一个推荐。
        rejects(
            |v| {
                v["sections"][0]["questions"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("options");
            },
            "options 必填",
        );
        rejects(
            |v| v["sections"][0]["questions"][0]["options"] = json!([{ "id": "a", "label": "甲" }]),
            "2–6 个",
        );
        rejects(
            |v| {
                let opts: Vec<Value> = (0..7)
                    .map(|i| json!({ "id": format!("o{i}"), "label": "项" }))
                    .collect();
                v["sections"][0]["questions"][0]["options"] = Value::Array(opts);
            },
            "2–6 个",
        );
        rejects(
            |v| v["sections"][0]["questions"][0]["options"][1]["id"] = json!("wave"),
            "选项 id 须唯一",
        );
        rejects(
            |v| v["sections"][0]["questions"][0]["options"][1]["label"] = json!(""),
            "options[1].label",
        );
        rejects(
            |v| v["sections"][0]["questions"][0]["options"][1]["recommended"] = json!(true),
            "最多只能有 1 个 recommended",
        );
        // 多选数量界。
        rejects(
            |v| v["sections"][0]["questions"][1]["max"] = json!(4),
            "多选数量界不合法",
        );
        rejects(
            |v| v["sections"][0]["questions"][1]["min"] = json!(3),
            "多选数量界不合法",
        );
        rejects(
            |v| v["sections"][0]["questions"][1]["min"] = json!(1.5),
            "须为整数",
        );
        // 打分量程。
        rejects(
            |v| {
                v["sections"][1]["questions"][0]["min"] = json!(5);
                v["sections"][1]["questions"][0]["max"] = json!(5);
            },
            "min < max",
        );
        rejects(
            |v| v["sections"][1]["questions"][0]["max"] = json!(100),
            "量程过宽",
        );
        rejects(
            |v| v["sections"][1]["questions"][0]["scaleLabels"] = json!(["只有一端"]),
            "恰好两个字符串",
        );

        // 宽容项:多选可有多个推荐;allowOther 让多选上界多一格;text/scale 多写的 options 被丢弃。
        let mut v = sample_questionnaire();
        v["sections"][0]["questions"][1]["allowOther"] = json!(true);
        v["sections"][0]["questions"][1]["max"] = json!(4);
        v["sections"][1]["questions"][1]["options"] = json!([{ "id": "x", "label": "多余" }]);
        v["sections"][1]["questions"][1]["allowOther"] = json!(true);
        let q = validate_questionnaire(&v).expect("宽容项应通过");
        assert_eq!(q.find("towers").unwrap().max, Some(4));
        assert!(q.find("name").unwrap().options.is_none());
        assert!(!q.find("name").unwrap().allow_other, "填空题没有「其他」");
    }

    // ---------- 预算 ----------

    #[test]
    fn preamble_budget_truncates_with_pointer() {
        let sections = vec![
            Section::new("项目事实", "事实甲乙丙", 800),
            Section::new("设想原文", "设".repeat(50), 20)
                .with_source(".forge/ultraplan/s/brief.md"),
            Section::new("空段", "   ", 100),
            Section::new("项目理解", "理".repeat(30), 100)
                .with_source(".forge/ultraplan/s/understanding.md"),
            Section::new("无出处", "无".repeat(30), 100),
        ];
        // 预算 40:事实 5 字全进;设想被自己的 cap(20)截;理解被剩余预算(15)截;最后一段预算用尽。
        let (text, reports) = assemble(&sections, 40);
        assert_eq!(
            reports,
            vec![
                SectionReport {
                    name: "项目事实".into(),
                    chars: 5,
                    truncated: false
                },
                SectionReport {
                    name: "设想原文".into(),
                    chars: 20,
                    truncated: true
                },
                SectionReport {
                    name: "项目理解".into(),
                    chars: 15,
                    truncated: true
                },
                SectionReport {
                    name: "无出处".into(),
                    chars: 0,
                    truncated: true
                },
            ],
            "空段不入报告;其余按序"
        );
        assert_eq!(
            reports.iter().map(|r| r.chars).sum::<usize>(),
            40,
            "正文恰好用满预算"
        );
        assert!(text.starts_with("【项目事实】\n事实甲乙丙"), "{text}");
        assert!(
            text.contains(&format!(
                "【设想原文】\n{}\n…(已截断,全文见 .forge/ultraplan/s/brief.md,用 read_file 读取)",
                "设".repeat(20)
            )),
            "保头截尾并给出全文指针: {text}"
        );
        assert!(!text.contains(&"设".repeat(21)));
        assert!(text.contains(&format!(
            "{}\n…(已截断,全文见 .forge/ultraplan/s/understanding.md",
            "理".repeat(15)
        )));
        // 预算用尽的段不注入正文,但留下指针(无出处时至少说明被截断),不让内容无声消失。
        assert!(text.ends_with("【无出处】\n…(已截断)"), "{text}");
        assert_eq!(
            text.matches('无').count(),
            1,
            "只剩段头,正文一字未进: {text}"
        );
        assert!(!text.contains("空段"));

        // 预算充足:原样、无指针。
        let (full, reports) = assemble(&sections, PREAMBLE_BUDGET_CHARS);
        assert!(reports
            .iter()
            .filter(|r| r.name != "设想原文")
            .all(|r| !r.truncated));
        assert!(full.contains(&"理".repeat(30)) && full.contains(&"无".repeat(30)));
        // 多字节字符按字符计,不会切在字节中间(上面全是中文已覆盖);有效预算随窗口收紧。
        assert_eq!(effective_budget(1_048_576), PREAMBLE_BUDGET_CHARS);
        assert_eq!(effective_budget(65_536), 16_384);
        assert_eq!(assemble(&[], 100), (String::new(), Vec::new()));
    }

    // ---------- 文件 ----------

    #[test]
    fn write_atomic_creates_parents_and_overwrites() {
        let dir = temp_dir("write");
        let path = dir.join("a").join("b").join("brief.md");
        write_atomic(&path, "第一版").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "第一版");
        write_atomic(&path, "第二版").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "第二版");
        assert!(
            !dir.join("a").join("b").join("brief.md.tmp").exists(),
            "tmp 不残留"
        );
        let jpath = dir.join("q.json");
        write_json_atomic(&jpath, &json!({ "a": 1 })).unwrap();
        assert_eq!(read_json(&jpath), Some(json!({ "a": 1 })));
        assert_eq!(read_json(&dir.join("不存在.json")), None);
        std::fs::write(&jpath, "{ 坏 json").unwrap();
        assert_eq!(read_json(&jpath), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- REST ----------

    /// 在独立工作区里给会话塞一条流程(返回会话 id、落库后的状态、工作区根)。
    fn seed_flow(
        state: &Arc<AppState>,
        dir: &Path,
        stage: &str,
    ) -> (String, UltraPlanState, PathBuf) {
        let ws_root = dir.join("ws");
        std::fs::create_dir_all(&ws_root).unwrap();
        let Ok(ws) = state
            .workspaces
            .create("测试工作区", ws_root.to_str().unwrap())
        else {
            panic!("注册工作区失败");
        };
        let session = state
            .sessions
            .create("流程会话", "coding", None, true, Some(ws.id.clone()));
        let ws_root = crate::scope::workspace_root_for(state, Some(&ws.id));
        let mut up = UltraPlanState::new_flow("塔防小游戏", Some(&ws.id), &ws_root);
        up.stage = stage.to_string();
        let (stored, ()) = state
            .sessions
            .update_ultraplan(&session.id, move |slot| *slot = Some(up))
            .expect("会话存在");
        (session.id, stored.ultraplan.expect("已落库"), ws_root)
    }

    fn event_types(state: &AppState, sid: &str) -> Vec<String> {
        state
            .events
            .persisted(sid)
            .iter()
            .map(|e| e.event_type.clone())
            .collect()
    }

    #[tokio::test]
    async fn rest_get_reads_artifacts_from_flow_dir() {
        let (state, dir) = crate::test_app_state("up-get");
        // 无流程:全 null。
        let plain = state
            .sessions
            .create("普通会话", "coding", None, true, None);
        let r = get_ultraplan(State(state.clone()), UrlPath(plain.id.clone())).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            body_json(r).await,
            json!({
                "ultraplan": null, "demo": null, "questionnaire": null,
                "answers": null, "checks": null, "acceptance": null, "production": null, "target": null, "delivery": null
            })
        );
        // 404。
        let r = get_ultraplan(State(state.clone()), UrlPath("sess_none".to_string())).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(r).await["error"]["code"], "SESSION_NOT_FOUND");

        // 有流程、产物未落盘:状态在,产物 null。
        let (sid, up, ws_root) = seed_flow(&state, &dir, STAGE_QUESTIONNAIRE);
        let v = body_json(get_ultraplan(State(state.clone()), UrlPath(sid.clone())).await).await;
        assert_eq!(v["ultraplan"]["id"], up.id);
        assert_eq!(v["ultraplan"]["stage"], "questionnaire");
        assert_eq!(v["ultraplan"]["token"], up.token);
        assert_eq!(v["questionnaire"], Value::Null);
        assert_eq!(v["demo"], Value::Null);

        // 产物落盘后按文件回;answers.json 两种形态都读得出;坏文件回 null 不报错。
        let flow = up.dir_abs(&ws_root).unwrap();
        let q = validate_questionnaire(&sample_questionnaire()).unwrap();
        write_json_atomic(
            &flow.join(QUESTIONNAIRE_FILE),
            &serde_json::to_value(&q).unwrap(),
        )
        .unwrap();
        write_json_atomic(
            &flow.join(ANSWERS_FILE),
            &json!({ "rev": 1, "answers": { "loop": { "choice": ["wave"] } } }),
        )
        .unwrap();
        write_json_atomic(
            &flow.join(CHECKS_FILE),
            &json!({ "automated": [], "manual": [] }),
        )
        .unwrap();
        write_atomic(&flow.join(ACCEPTANCE_FILE), "{ 坏").unwrap();
        let v = body_json(get_ultraplan(State(state.clone()), UrlPath(sid.clone())).await).await;
        assert_eq!(
            v["questionnaire"]["sections"][0]["questions"][0]["id"],
            "loop"
        );
        assert_eq!(v["answers"], json!({ "loop": { "choice": ["wave"] } }));
        assert_eq!(v["checks"], json!({ "automated": [], "manual": [] }));
        assert_eq!(v["acceptance"], Value::Null);
        write_json_atomic(
            &flow.join(ANSWERS_FILE),
            &json!({ "name": { "text": "守卫" } }),
        )
        .unwrap();
        let v = body_json(get_ultraplan(State(state.clone()), UrlPath(sid)).await).await;
        assert_eq!(v["answers"], json!({ "name": { "text": "守卫" } }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn rest_restart_clears_state_keeps_files_and_respects_busy() {
        let (state, dir) = crate::test_app_state("up-restart");
        let (sid, up, ws_root) = seed_flow(&state, &dir, STAGE_DEMO_REVIEW);
        let flow = up.dir_abs(&ws_root).unwrap();
        write_atomic(&flow.join(BRIEF_FILE), "做个塔防").unwrap();
        let post = |action: &str| {
            post_ultraplan_action(
                State(state.clone()),
                UrlPath((sid.clone(), action.to_string())),
                axum::body::Bytes::new(),
            )
        };

        // 未知操作 400;未接入的操作 409(不碰状态)。
        let r = post("explode").await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(r).await["error"]["code"], "INVALID_INPUT");
        for action in ["acceptance", "rollback_demo"] {
            let r = post(action).await;
            assert_eq!(r.status(), StatusCode::CONFLICT, "{action}");
            assert_eq!(
                body_json(r).await["error"]["code"],
                if action == "acceptance" {
                    "ULTRAPLAN_ACCEPTANCE_INVALID"
                } else {
                    "ULTRAPLAN_NO_DEMO_SNAPSHOT"
                }
            );
        }

        // 有运行中的 run:三个操作都是 SESSION_BUSY,状态原样、零事件。
        state.sessions.claim_active_run(&sid, "run_x").unwrap();
        for action in ["restart", "acceptance", "rollback_demo"] {
            let r = post(action).await;
            assert_eq!(r.status(), StatusCode::CONFLICT, "{action}");
            assert_eq!(
                body_json(r).await["error"]["code"],
                "SESSION_BUSY",
                "{action}"
            );
        }
        assert_eq!(
            state.sessions.get(&sid).unwrap().ultraplan,
            Some(up.clone())
        );
        assert!(event_types(&state, &sid).is_empty());
        state.sessions.release_active_run(&sid, "run_x");

        // 空闲:清状态、留文件、发 cleared + session.updated。
        let r = post("restart").await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(body_json(r).await, json!({ "ok": true }));
        assert!(state.sessions.get(&sid).unwrap().ultraplan.is_none());
        assert_eq!(
            std::fs::read_to_string(flow.join(BRIEF_FILE)).unwrap(),
            "做个塔防"
        );
        assert_eq!(
            event_types(&state, &sid),
            ["ultraplan.cleared", "session.updated"]
        );
        let cleared = &state.events.persisted(&sid)[0];
        assert_eq!(cleared.payload, json!({ "id": up.id }));
        assert_eq!(cleared.channel(), "ultraplan");
        // 清掉的状态不留在盘上(重启后不会复活)。
        let on_disk = std::fs::read_to_string(state.sessions.path()).unwrap();
        assert!(!on_disk.contains("ultraplan"), "{on_disk}");

        // 已无流程:幂等 ok,不再发事件。
        let r = post("restart").await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(event_types(&state, &sid).len(), 2);
        // 404。
        let r = post_ultraplan_action(
            State(state.clone()),
            UrlPath(("sess_none".to_string(), "restart".to_string())),
            axum::body::Bytes::new(),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        // 清掉之后同标题重开:旧目录还在,新流程必须换 slug(I-6)。
        let again = UltraPlanState::new_flow("塔防小游戏", up.workspace_id.as_deref(), &ws_root);
        assert_ne!(again.slug, up.slug);
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 轮次运行时(explore 落盘与门) ----------

    /// 在「已有内容」的工作区里开一轮新流程的 Discovery,返回运行时与会话 id。
    fn discovery_runtime(state: &Arc<AppState>, ws: &Path) -> (UltraRuntime, String) {
        std::fs::create_dir_all(ws).unwrap();
        crate::project::init_project(ws, "旧项目", "2d").expect("脚手架");
        std::fs::write(ws.join("Content").join("Textures").join("a.png"), b"x").unwrap();
        let ws = ws.canonicalize().unwrap();
        let scope = crate::scope::ScopeProject {
            workspace_id: None,
            name: "测试工作区".to_string(),
            workspace_root: ws.clone(),
            project_root: ws.clone(),
            game_mode: crate::scope::game_mode_of(&ws),
        };
        let session = state.sessions.create("t", "coding", None, true, None);
        let ut = resolve_request(&session, &scope, MODE, None, "在现有项目上做塔防", None)
            .ok()
            .flatten()
            .expect("应路由为 Discovery");
        assert!(ut.facts.has_content);
        let rt = begin_turn(
            state,
            &session.id,
            "run_t",
            &scope,
            "在现有项目上做塔防",
            &ut,
        )
        .expect("开场");
        (rt, session.id)
    }

    /// explore 的全文落 `explore/<n>.md`,回给 leader 的是节选 + 文件位置;非 explore 的 task 原样放过。
    #[test]
    fn explore_report_saved_in_full_and_feedback_clipped() {
        let (state, dir) = crate::test_app_state("up-explore-save");
        let (rt, _) = discovery_runtime(&state, &dir.join("ws"));
        let args =
            json!({ "subagent_type": "explore", "description": "摸底资产", "prompt": "盘点资产" });
        let long = format!("{}尾巴", "报".repeat(EXPLORE_FEEDBACK_BODY_MAX + 500));
        let fb = rt.after_task(&args, true, long.clone());
        let rel = format!("{}/{EXPLORE_DIR}/1.md", rt.dir_rel);
        assert!(fb.ends_with(&format!("(完整报告已存 {rel})")), "{fb}");
        assert!(
            fb.contains("…(以上为节选)") && !fb.contains("尾巴"),
            "反馈只给节选"
        );
        assert!(char_len(&fb) < 4000, "指针不能被 4000 字的工具反馈截断");
        let saved = std::fs::read_to_string(rt.dir_abs.join(EXPLORE_DIR).join("1.md")).unwrap();
        assert!(saved.contains("尾巴") && saved.contains("盘点资产") && saved.contains("摸底资产"));
        assert_eq!(rt.tracker.explore_saved(), 1);
        // 编号接着往下排;通用子代理 / 失败的 explore / 空报告都不落盘。
        let short = rt.after_task(&args, true, "第二份".to_string());
        assert!(
            short.starts_with("第二份") && short.contains("/explore/2.md"),
            "{short}"
        );
        let plain = json!({ "prompt": "随便查查" });
        assert_eq!(rt.after_task(&plain, true, "通用".into()), "通用");
        assert_eq!(rt.after_task(&args, false, "超时".into()), "超时");
        assert_eq!(rt.after_task(&args, true, "  ".into()), "  ");
        assert_eq!(explore_reports(&rt.dir_abs).len(), 2);
        assert_eq!(rt.tracker.explore_saved(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// explore 工种根本不存在(未知 subagent_type)时这道门满足不了:第一次交问卷就放行,
    /// 发 EXPLORE_SKIPPED 并在理解开头如实标注,不让模型原地打转。
    #[test]
    fn explore_gate_rejects_missing_profile() {
        let (state, dir) = crate::test_app_state("up-explore-missing");
        let (rt, sid) = discovery_runtime(&state, &dir.join("ws"));
        let args = json!({ "subagent_type": "explore", "prompt": "盘点资产" });
        let text = "未知 subagent_type: explore(可用工种见 task 工具描述;或省略走通用子代理)";
        assert_eq!(rt.after_task(&args, false, text.to_string()), text);
        let q = json!({
            "title": "塔防 · 需求确认",
            "understanding": "## 我的理解\n塔防。",
            "sections": [{ "id": "core", "title": "核心玩法", "questions": [
                { "id": "name", "kind": "text", "question": "游戏叫什么?" }
            ]}]
        });
        let (ok, msg) = handle_exit_tool(&state, &sid, "run_t", Some(&rt), QUESTIONNAIRE_TOOL, &q);
        assert!(
            !ok && msg.starts_with("ULTRAPLAN_EXPLORE_REQUIRED"),
            "{msg}"
        );
        assert_eq!(rt.tracker.explore_rejections(), 1);
        let up = state.sessions.get(&sid).unwrap().ultraplan.unwrap();
        assert_eq!(up.stage, STAGE_DISCOVERY);
        assert!(!rt.dir_abs.join(QUESTIONNAIRE_FILE).exists());
        // 没有运行时(普通轮次)/ 本轮种类不提供的出口工具 → TOOL_FORBIDDEN。
        let (ok, msg) = handle_exit_tool(&state, &sid, "run_t", None, QUESTIONNAIRE_TOOL, &q);
        assert!(!ok && msg.starts_with("TOOL_FORBIDDEN"), "{msg}");
        let (ok, msg) = handle_exit_tool(&state, &sid, "run_t", Some(&rt), SPEC_TOOL, &q);
        assert!(!ok && msg.starts_with("TOOL_FORBIDDEN"), "{msg}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
