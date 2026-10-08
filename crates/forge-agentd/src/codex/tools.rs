//! 反向暴露给 Codex 的 Forge 原生工具(`thread/start.dynamicTools`)。
//!
//! MCP 注入([mcp_config](super::mcp_config))把「引擎能力」交给了 Codex,但本仓还有一族
//! 不在 MCP 里的工具:跨项目作用域的资源检索([resources.rs](crates/forge-agentd/src/resources.rs))。
//! 它依赖会话的作用域上下文(当前可写项目 + 勾选的只读项目 + 个人库),那是 Forge 进程内
//! 的状态,做不成独立 MCP 子进程 —— 于是走 dynamicTools:Codex 发 `item/tool/call`,
//! Forge 在进程内执行后回结果。

use serde_json::{json, Value};

/// dynamicTools 命名空间。带前缀是为了在 Codex 的工具面里一眼看出「这是宿主给的」。
pub const NAMESPACE: &str = "forge";

/// `thread/start.dynamicTools` 的声明表。
pub fn declarations() -> Vec<Value> {
    crate::resources::tool_specs()
        .into_iter()
        .chain(crate::editor::tool_specs())
        .filter_map(|spec| {
            let f = spec.get("function")?;
            let name = f.get("name")?.as_str()?;
            Some(json!({
                "type": "function",
                // app-server 要求动态工具名匹配 ^[a-zA-Z0-9_-]+$，点号会让 thread/start 失败。
                "name": format!("{NAMESPACE}__{name}"),
                "description": f.get("description").cloned().unwrap_or(Value::Null),
                "inputSchema": f
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object" })),
            }))
        })
        .collect()
}

/// `item/tool/call` 的工具名 → 本仓工具名。不是本命名空间的返回 `None`。
pub fn strip_namespace(name: &str) -> Option<&str> {
    name.strip_prefix(NAMESPACE)
        .and_then(|rest| rest.strip_prefix('.').or_else(|| rest.strip_prefix("__")))
        .filter(|n| crate::resources::is_resource_tool(n) || crate::editor::is_tool(n))
}

/// 执行一次 dynamicTool 调用,返回 Codex `item/tool/call` 的响应体。
pub async fn call(
    state: &crate::AppState,
    scope: &crate::scope::ScopeContext,
    session_id: &str,
    run_id: &str,
    name: &str,
    args: &Value,
) -> Value {
    let Some(tool) = strip_namespace(name) else {
        return error_result(&format!("未知工具: {name}"));
    };
    if crate::editor::is_tool(tool) {
        let Some((mode, current_scope)) = state.team_runtime.native_context(session_id, run_id) else {
            return error_result("EDITOR_RUN_STALE");
        };
        let write = crate::editor::is_write_tool(tool);
        if write && crate::editor::readonly_mode(&mode) {
            return error_result("EDITOR_WRITE_FORBIDDEN");
        }
        match state.permissions.authorize(&state.events, session_id, run_id, tool, write).await {
            Ok(true) => {},
            Ok(false) => return error_result("EDITOR_WRITE_FORBIDDEN"),
            Err(e) => return error_result(&e),
        }
        if state.runs.is_cancelled(run_id)||state.team_runtime.native_context(session_id,run_id).is_none(){return error_result("EDITOR_RUN_STALE");}
        return match crate::editor::dispatch(&current_scope, tool, &crate::editor::attributed(args,session_id,run_id)).await {
            Ok(value) => {
                let feedback=crate::editor::feedback(&current_scope,value);
                let mut items=vec![json!({"type":"inputText","text":feedback.text})];
                items.extend(feedback.images.into_iter().map(|url|json!({"type":"inputImage","imageUrl":url})));
                json!({"contentItems":items,"success":true})
            },
            Err(e) => error_result(&e),
        };
    }
    let (ok, text) = crate::resources::dispatch(&state.workspaces, scope, tool, args).await;
    if !ok {
        return error_result(&text);
    }
    json!({
        "contentItems": [{ "type": "inputText", "text": text }],
        "success": true,
    })
}

fn error_result(message: &str) -> Value {
    json!({
        "contentItems": [{ "type": "inputText", "text": message }],
        "success": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 声明表覆盖三个资源工具,且带命名空间前缀与可用的 inputSchema。
    #[test]
    fn declarations_cover_resource_tools() {
        let decls = declarations();
        let names: Vec<&str> = decls.iter().map(|d| d["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"forge__project_list"), "{names:?}");
        assert!(names.contains(&"forge__resource_search"), "{names:?}");
        assert!(names.contains(&"forge__resource_get"), "{names:?}");
        for name in ["forge__editor_overview","forge__editor_read","forge__editor_apply","forge__editor_capture","forge__editor_undo"]{assert!(names.contains(&name),"{names:?}");}
        for d in &decls {
            assert_eq!(d["type"], "function");
            assert!(
                d["inputSchema"].is_object(),
                "inputSchema 必须是对象(Codex 会拿它校验参数): {d}"
            );
        }
    }

    /// 命名空间剥离:两种分隔符都认;不认的名字不放行(免得把任意工具名当资源工具执行)。
    #[test]
    fn namespace_stripping_is_strict() {
        assert_eq!(strip_namespace("forge.project_list"), Some("project_list"));
        assert_eq!(strip_namespace("forge__resource_get"), Some("resource_get"));
        assert_eq!(strip_namespace("forge.rm_rf"), None);
        assert_eq!(strip_namespace("project_list"), None);
        assert_eq!(strip_namespace("other.project_list"), None);
    }

    #[test]
    fn dynamic_tool_response_matches_app_server_schema() {
        let bad = error_result("未知工具");
        assert_eq!(bad["success"], false);
        assert_eq!(bad["contentItems"][0]["type"], "inputText");
        assert!(bad.get("isError").is_none());
    }
}
