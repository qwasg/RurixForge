//! Agent harness：Turn 发射点 + 运行时工具规格。

pub mod turn;

pub use turn::{emit_stream_delta, record_item, Turn, TurnItem};

use serde_json::{json, Value};

/// 运行时工具名（非 MCP）。
pub const NATIVE_TOOLS: &[&str] = &[
    "todo_write",
    "write_todos",
    "todo_update",
    "plan_write",
    "task",
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

/// 按 profile/mode 注入的运行时工具 OpenAI spec。
pub fn runtime_tool_specs(kind: &str, mode: &str) -> Vec<Value> {
    let profile = crate::profile::AgentProfile::from_kind_str(kind);
    if mode == "ask" || mode == "multitask" {
        return Vec::new();
    }
    let mut out = Vec::new();
    if profile.wants_todo_tools() {
        if mode == "plan" {
            out.push(spec(
                "plan_write",
                "写入分步实施计划（待办清单）。plan 模式用此工具落计划。",
                obj_props(
                    json!({
                        "todos": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string" },
                                    "description": { "type": "string" },
                                    "kind": { "type": "string", "enum": ["edit", "explore"] }
                                },
                                "required": ["title"]
                            }
                        }
                    }),
                    &["todos"],
                ),
            ));
        } else {
            out.push(spec(
                "todo_write",
                "创建一批待办。开始一项前用 todo_update 标 in_progress/running。",
                obj_props(
                    json!({
                        "todos": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string" },
                                    "description": { "type": "string" },
                                    "kind": { "type": "string", "enum": ["edit", "explore"] }
                                },
                                "required": ["title"]
                            }
                        }
                    }),
                    &["todos"],
                ),
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
    if profile.wants_task_tool() {
        out.push(spec(
            "task",
            "把子任务委派给嵌套子代理。prompt 必填；description 作卡片标题。",
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
    let read_ok = true;
    if read_ok {
        out.push(spec(
            "read_file",
            "读取工作区文本文件。",
            obj_props(json!({ "path": { "type": "string" } }), &["path"]),
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
    if profile.wants_edit_tools() && mode != "plan" {
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
