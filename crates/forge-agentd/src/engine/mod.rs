//! Agent harness：Turn 发射点 + 运行时工具规格。

pub mod turn;

pub use turn::{emit_stream_delta, record_item, Turn, TurnItem};

use serde_json::{json, Value};

/// D-035:plan 模式唯一产物出口——落 `.forge/plans/<slug>.plan.md` 计划文件。
/// 实现在 crate::plan_doc(要写会话 activePlanPath,由 agent.rs 父执行闭包接管,
/// 不走 native_tools::dispatch_native)。
pub const CREATE_PLAN_TOOL: &str = "create_plan";

/// D-036:multitask 的异步派发工具——起后台子代理 run 后立刻返回受理回执。
/// 与 `task`(同步嵌套子代理,阻塞本轮)是两套语义,故用两个名字,不复用一个带开关的工具:
/// 模型对「调完就返回」与「调完拿结果」的行为差异极其敏感,靠参数区分必然误用。
pub const DISPATCH_TOOL: &str = "dispatch";

/// 运行时工具名（非 MCP）。
pub const NATIVE_TOOLS: &[&str] = &[
    "todo_write",
    "write_todos",
    "todo_update",
    "plan_write",
    CREATE_PLAN_TOOL,
    "task",
    DISPATCH_TOOL,
    "read_file",
    "list_dir",
    "glob",
    "grep",
    "read_skill",
    "write_file",
    "str_replace_edit",
    "apply_patch",
];

pub fn is_native_tool(name: &str) -> bool {
    NATIVE_TOOLS.contains(&name)
}

pub fn is_native_write_tool(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "str_replace_edit" | "apply_patch"
    )
}

fn spec(name: &str, desc: &str, params: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": desc, "parameters": params },
    })
}

fn obj_props(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

/// task 工具描述:基础文案 + 磁盘 profile 工种清单(subagent_type 可选值)。
/// 抽成纯函数以便单测(生产路径由 runtime_tool_specs 传入实际扫盘结果)。
fn task_tool_desc(profiles: &[(String, String)]) -> String {
    let mut desc = String::from(
        "把子任务委派给嵌套子代理。prompt 必填(带全上下文,子代理看不到对话历史);\
         description 作卡片标题;subagent_type 选专职工种(省略 = 通用子代理)。",
    );
    desc.push_str(&profile_list_suffix(profiles));
    desc
}

/// D-036:multitask 的异步派发工具描述。与 task 的语义差别必须写死在描述里——
/// 模型得知道「调完就返回、结果不在本轮」,否则它会傻等或反复追问。
fn dispatch_tool_desc(profiles: &[(String, String)]) -> String {
    let mut desc = String::from(
        "把子任务派给后台子代理,**立即返回受理回执、不等它做完**(本轮不会拿到执行结果)。\
         同一轮可连发多个 dispatch,它们并行执行。子代理跑完后回执自动送达:你若仍在工作,\
         回执在你下一步之前插进上下文;你若已收束,系统会带着回执唤醒你新开一轮。\
         你现在不必也无法等待。\
         prompt 必填(自带全部背景:目标、路径、命名约定、验收标准——子代理看不到对话历史);\
         description 必填,作子代理卡片标题(一句话,用户直接看这行);\
         subagent_type 选专职工种(省略 = 通用子代理)。",
    );
    desc.push_str(&profile_list_suffix(profiles));
    desc
}

fn profile_list_suffix(profiles: &[(String, String)]) -> String {
    if profiles.is_empty() {
        return String::new();
    }
    let mut out = String::from("可用工种:");
    for (name, d) in profiles {
        out.push_str(&format!("\n- {name}: {d}"));
    }
    out
}

/// todo_write / plan_write 共用的条目 schema(F-GAME-4 wave.3:加 Plan DAG 可选五字段;
/// 旧调用只带 title/description/kind 依然合法)。
fn todo_items_schema() -> Value {
    json!({
        "todos": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "kind": { "type": "string", "enum": ["edit", "explore"] },
                    "stage": { "type": "string", "description": "阶段名(素材/场景/逻辑/测试…);team 编排器把同阶段的就绪任务并行派发" },
                    "deps": { "type": "array", "items": { "type": "string" }, "description": "依赖任务:引用已有 todo 的 id,或同批任务的 title;全部 completed 后本任务才就绪" },
                    "role": { "type": "string", "description": "执行工种(subagent_type,见 task 工具描述);有 role 的任务由 team 编排器自动派发" },
                    "prompt": { "type": "string", "description": "完整委派词(目标/路径/命名约定/验收标准;子代理看不到对话历史)" },
                    "verify": { "type": "string", "enum": ["none", "qa", "reviewer"], "description": "qa=完成后自动派 qa-tester 复测;reviewer=纳入终审" }
                },
                "required": ["title"]
            }
        }
    })
}

/// D-035:create_plan 入参 schema(计划文件 = front matter 三字段 + Markdown 正文)。
fn create_plan_schema() -> Value {
    obj_props(
        json!({
            "name": { "type": "string", "description": "计划名(单行;用作 Plan 页签标题与文件名)" },
            "overview": { "type": "string", "description": "一句话概述(单行)" },
            "plan": {
                "type": "string",
                "description": "Markdown 计划正文:现状与差距 / 目标方案 / 分模块改动落点(带真实文件路径与函数名)。写给接手实施的人看,不要在这里重复 todos 清单。"
            },
            "todos": {
                "type": "array",
                "description": "可执行的分步待办(按实施顺序)。用户点 Build 时按此清单物化为待办并逐条执行。",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "短横线小写标识(如 be-plan-prompt);Build 侧按它去重,重复 Build 不会重建同一条" },
                        "content": { "type": "string", "description": "一句话待办,动词开头、可独立验收" }
                    },
                    "required": ["id", "content"]
                }
            }
        }),
        &["name", "plan", "todos"],
    )
}

/// 按 profile/mode 注入的运行时工具 OpenAI spec。
///
/// D-036:multitask 从「空工具面 + 服务端模板链」改为「只读侦察 + 异步派发」——
/// 拿 todo 工具与全部只读工具,但写工具一律不给(写入交子代理),`task` 换成 `dispatch`
/// (同步委派会把主轮拖住,与异步语义冲突)。
pub fn runtime_tool_specs(kind: &str, mode: &str) -> Vec<Value> {
    let profile = crate::profile::AgentProfile::from_kind_str(kind);
    if mode == "ask" {
        return Vec::new();
    }
    let multitask = mode == "multitask";
    let mut out = Vec::new();
    if profile.wants_todo_tools() {
        if mode == "plan" {
            // D-035:plan 模式不再给 plan_write/todo_update——计划(设计正文 + 待办)整体
            // 落成工作区文件,待办到 Build 时才物化进 TodoStore,调研阶段不污染会话待办面。
            out.push(spec(
                CREATE_PLAN_TOOL,
                "写入计划文件(.forge/plans/<名>.plan.md):Markdown 设计正文 + 分步待办。整轮只调一次;\
本会话已有计划时原地覆盖迭代。落盘后前端自动打开 Plan 页签,用户可编辑并点 Build 执行。",
                create_plan_schema(),
            ));
        } else {
            out.push(spec(
                "todo_write",
                "创建一批待办。开始一项前用 todo_update 标 in_progress/running。",
                obj_props(todo_items_schema(), &["todos"]),
            ));
            // F-GAME-4 wave.3:team 模式的 leader 额外拿 plan_write——结构化计划
            // (每任务带 role/deps/prompt/verify)落库后由编排器按依赖分层并行派发。
            if mode == "team" {
                out.push(spec(
                    "plan_write",
                    "写入结构化团队计划:每个任务带 role(执行工种)/prompt(完整委派词)/deps(依赖)/stage(阶段)/verify(qa|reviewer)。落库后编排器自动按依赖分层并行派发执行,无需逐个 task 派单。",
                    obj_props(todo_items_schema(), &["todos"]),
                ));
            }
            out.push(spec(
                "todo_update",
                "更新已有待办的 status/title/summary。",
                obj_props(
                    json!({
                        "id": { "type": "string" },
                        "status": { "type": "string", "enum": ["queued", "running", "completed", "failed"] },
                        "title": { "type": "string" },
                        "summary": { "type": "string" }
                    }),
                    &["id"],
                ),
            ));
        }
    }
    if profile.wants_task_tool() {
        // 动态列磁盘 profile:leader(尤其 team 模式)得知道有哪些工种可派。
        let (profiles, _errs) = crate::subagents::list_subagents(&crate::subagents::agents_dir());
        let pairs: Vec<(String, String)> = profiles
            .into_iter()
            .map(|p| (p.name, p.description))
            .collect();
        if multitask {
            // description 在异步面是必填:卡片标题是用户在子代理跑完前唯一看得到的东西。
            out.push(spec(
                DISPATCH_TOOL,
                &dispatch_tool_desc(&pairs),
                obj_props(
                    json!({
                        "prompt": { "type": "string", "description": "完整委派词:目标、涉及路径、命名约定、验收标准(子代理看不到对话历史)" },
                        "description": { "type": "string", "description": "一句话任务标题(子代理卡片标题,用户直接看这行)" },
                        "subagent_type": { "type": "string", "description": "专职工种名(省略 = 通用子代理)" }
                    }),
                    &["prompt", "description"],
                ),
            ));
        } else {
            out.push(spec(
                "task",
                &task_tool_desc(&pairs),
                obj_props(
                    json!({
                        "prompt": { "type": "string" },
                        "description": { "type": "string" },
                        "subagent_type": { "type": "string" }
                    }),
                    &["prompt"],
                ),
            ));
        }
    }
    let read_ok = true;
    if read_ok {
        out.push(spec(
            "read_file",
            "读取工作区文本文件。可选 offset/limit 按行区间分段读大文件。",
            obj_props(
                json!({
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "description": "起始行(1 基;缺省从头读)" },
                    "limit": { "type": "integer", "description": "最多读取行数(缺省读到文件末尾)" }
                }),
                &["path"],
            ),
        ));
        out.push(spec(
            "list_dir",
            "列出工作区单层目录。",
            obj_props(json!({ "path": { "type": "string" } }), &[]),
        ));
        out.push(spec(
            "glob",
            "按 glob 查找工作区文件（* 与 **）。",
            obj_props(json!({ "pattern": { "type": "string" } }), &["pattern"]),
        ));
        out.push(spec(
            "grep",
            "在工作区文本文件中搜索字面量（非正则）。",
            obj_props(
                json!({
                    "query": { "type": "string" },
                    "path": { "type": "string" }
                }),
                &["query"],
            ),
        ));
        // F11 wave.2:read_skill 兑现 06 §2——系统提示只给「名字+触发时机」索引,
        // 全文按需经本工具取,避免十几篇规程常驻上下文。
        out.push(spec(
            "read_skill",
            "读取工作区技能规程全文(skills/<name>/SKILL.md)。系统提示中列出的技能,匹配到任务时先用本工具取全文再按流程执行。",
            obj_props(
                json!({ "name": { "type": "string", "description": "技能名(小写英文+中划线)" } }),
                &["name"],
            ),
        ));
    }
    if profile.wants_edit_tools() && mode != "plan" && !multitask {
        out.push(spec(
            "write_file",
            "写入或创建工作区文本文件。",
            obj_props(
                json!({
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                }),
                &["path", "content"],
            ),
        ));
        out.push(spec(
            "str_replace_edit",
            "精确替换文件中的一段文本。",
            obj_props(
                json!({
                    "path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean" }
                }),
                &["path", "old_string", "new_string"],
            ),
        ));
        out.push(spec(
            "apply_patch",
            "应用 Codex 风格补丁（*** Begin Patch / Add|Update|Delete File）。",
            obj_props(json!({ "patch": { "type": "string" } }), &["patch"]),
        ));
    } else if matches!(profile.kind, crate::profile::AgentKind::Document | crate::profile::AgentKind::Studio)
        && mode != "plan"
        && !multitask
    {
        out.push(spec(
            "write_file",
            "写入或创建工作区文本文件。",
            obj_props(
                json!({
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                }),
                &["path", "content"],
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(specs: &[Value]) -> Vec<String> {
        specs
            .iter()
            .filter_map(|t| {
                t.pointer("/function/name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect()
    }

    /// team 模式工具面 = build 全量 + plan_write(F-GAME-4 wave.3:leader 结构化计划入口)。
    #[test]
    fn team_mode_tools_are_build_plus_plan_write() {
        let team = names(&runtime_tool_specs("coding", "team"));
        let build = names(&runtime_tool_specs("coding", "build"));
        let team_minus_plan: Vec<String> =
            team.iter().filter(|n| n.as_str() != "plan_write").cloned().collect();
        assert_eq!(team_minus_plan, build, "team 应为 build 全量 + plan_write");
        for need in ["todo_write", "plan_write", "task", "read_file", "write_file", "apply_patch"] {
            assert!(team.iter().any(|n| n == need), "team 缺 {need}");
        }
        assert!(!build.iter().any(|n| n == "plan_write"), "build 不该带 plan_write");
    }

    /// F-GAME-4 wave.3:todo/plan 条目 schema 携带 Plan DAG 五字段(旧字段原样保留)。
    #[test]
    fn todo_items_schema_carries_plan_dag_fields() {
        let schema = todo_items_schema();
        let props = &schema["todos"]["items"]["properties"];
        for k in ["title", "description", "kind", "stage", "deps", "role", "prompt", "verify"] {
            assert!(props.get(k).is_some(), "schema 缺 {k}: {props}");
        }
        assert_eq!(schema["todos"]["items"]["required"][0], "title", "仅 title 必填(旧调用兼容)");
        assert_eq!(props["verify"]["enum"], json!(["none", "qa", "reviewer"]));
        assert_eq!(props["deps"]["type"], "array");
    }

    /// D-036:multitask 工具面 = 只读侦察 + dispatch(异步派发),不给写工具与同步 task。
    #[test]
    fn multitask_mode_tools_are_readonly_plus_dispatch() {
        let mt = names(&runtime_tool_specs("coding", "multitask"));
        for need in [DISPATCH_TOOL, "todo_write", "todo_update", "read_file", "grep", "list_dir"] {
            assert!(mt.iter().any(|n| n == need), "multitask 缺 {need}: {mt:?}");
        }
        for banned in ["task", "plan_write", CREATE_PLAN_TOOL, "write_file", "str_replace_edit", "apply_patch"] {
            assert!(!mt.iter().any(|n| n == banned), "multitask 不该有 {banned}");
        }
        // ask 仍是空工具面(纯对话);build 仍是同步 task,两者不受本波影响。
        assert!(runtime_tool_specs("coding", "ask").is_empty());
        let build = names(&runtime_tool_specs("coding", "build"));
        assert!(build.iter().any(|n| n == "task"));
        assert!(!build.iter().any(|n| n == DISPATCH_TOOL));
    }

    /// dispatch 描述必须写死异步语义:模型得知道调完就返回、结果不在本轮。
    #[test]
    fn dispatch_desc_states_async_semantics_and_profiles() {
        let desc = dispatch_tool_desc(&[("scene-builder".into(), "场景搭建".into())]);
        assert!(desc.contains("立即返回"), "{desc}");
        // D-038:两条送达路径都要写明——工作中插入 / 空闲时唤醒。
        assert!(desc.contains("下一步之前插进上下文"), "{desc}");
        assert!(desc.contains("唤醒你新开一轮"), "{desc}");
        assert!(desc.contains("- scene-builder: 场景搭建"), "{desc}");
        let spec = runtime_tool_specs("coding", "multitask")
            .into_iter()
            .find(|t| t.pointer("/function/name").and_then(Value::as_str) == Some(DISPATCH_TOOL))
            .expect("dispatch spec");
        // description 必填:它是子代理跑完前用户唯一看得到的东西(卡片标题)。
        assert_eq!(
            spec.pointer("/function/parameters/required"),
            Some(&json!(["prompt", "description"]))
        );
    }

    /// task 描述动态列工种(leader 派单指南);空清单不加「可用工种」段。
    #[test]
    fn task_desc_lists_profiles() {
        let desc = task_tool_desc(&[
            ("scene-builder".into(), "场景搭建".into()),
            ("qa-tester".into(), "运行验证".into()),
        ]);
        assert!(desc.contains("可用工种:"));
        assert!(desc.contains("- scene-builder: 场景搭建"));
        assert!(desc.contains("- qa-tester: 运行验证"));
        let empty = task_tool_desc(&[]);
        assert!(!empty.contains("可用工种"));
        // 生产路径(磁盘 profile)也应把工种写进实发 spec。
        let task_spec = runtime_tool_specs("coding", "team")
            .into_iter()
            .find(|t| t.pointer("/function/name").and_then(Value::as_str) == Some("task"))
            .expect("task spec");
        let d = task_spec
            .pointer("/function/description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        assert!(d.contains("qa-tester"), "实发 task 描述应含磁盘工种: {d}");
    }
}
