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
        .filter_map(|spec| {
            let f = spec.get("function")?;
            let name = f.get("name")?.as_str()?;
            Some(json!({
                "name": format!("{NAMESPACE}.{name}"),
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
        .filter(|n| crate::resources::is_resource_tool(n))
}

/// 执行一次 dynamicTool 调用,返回 Codex `item/tool/call` 的响应体。
pub async fn call(
    state: &crate::AppState,
    scope: &crate::scope::ScopeContext,
    name: &str,
    args: &Value,
) -> Value {
    let Some(tool) = strip_namespace(name) else {
        return error_result(&format!("未知工具: {name}"));
    };
    let (ok, text) = crate::resources::dispatch(&state.workspaces, scope, tool, args).await;
    if !ok {
        return error_result(&text);
    }
    json!({
        "contentItems": [{ "type": "text", "text": text }],
        "isError": false,
    })
}

fn error_result(message: &str) -> Value {
    json!({
        "contentItems": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 声明表覆盖三个资源工具,且带命名空间前缀与可用的 inputSchema。
    #[test]
    fn declarations_cover_resource_tools() {
        let decls = declarations();
        let names: Vec<&str> = decls
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"forge.project_list"), "{names:?}");
        assert!(names.contains(&"forge.resource_search"), "{names:?}");
        assert!(names.contains(&"forge.resource_get"), "{names:?}");
        for d in &decls {
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
}
