//! 统一资源检索:当前项目 + 显式只读项目 + 全局库/商店。
//!
//! 模型只看见 workspaceId / locator / 相对路径,不接触磁盘绝对路径。
//! 写操作永远只落当前项目;其他项目只走 context_search / context_get。

use serde_json::{json, Value};

use crate::scope::ScopeContext;
use crate::workspaces::WorkspaceStore;

pub const RESOURCE_TOOLS: &[&str] = &["project_list", "resource_search", "resource_get"];

pub fn is_resource_tool(name: &str) -> bool {
    RESOURCE_TOOLS.contains(&name)
}

fn spec(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": desc,
            "parameters": {
                "type": "object",
                "properties": props,
                "required": required,
            }
        },
        "x-forge": { "access": "read", "approval": false }
    })
}

pub fn tool_specs() -> Vec<Value> {
    vec![
        spec(
            "project_list",
            "列出当前作用域内的项目:current=当前可写项目;readable=已勾选只读项目;registered=已登记但未勾选(不可检索)。",
            json!({}),
            &[],
        ),
        spec(
            "resource_search",
            "跨当前项目、已勾选只读项目、全局素材库/商店检索资产与文档。返回带 locator 的命中;再用 resource_get 取片段。",
            json!({
                "query": { "type": "string" },
                "scope": {
                    "type": "string",
                    "enum": ["current", "selected", "library", "store", "all"],
                    "description": "缺省 all"
                },
                "kinds": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "asset/entity/graph/symbol/doc/library/store"
                },
                "projectId": { "type": "string", "description": "限定单个项目(须在作用域内)" },
                "cursor": { "type": "string" },
                "limit": { "type": "integer" }
            }),
            &["query"],
        ),
        spec(
            "resource_get",
            "按 locator 读取一条资源的摘要或文档片段。locator 来自 resource_search。",
            json!({
                "locator": { "type": "string" },
                "startLine": { "type": "integer" },
                "endLine": { "type": "integer" }
            }),
            &["locator"],
        ),
    ]
}

pub async fn dispatch(
    workspaces: &WorkspaceStore,
    scope: &ScopeContext,
    name: &str,
    args: &Value,
) -> (bool, String) {
    match name {
        "project_list" => (true, project_list(workspaces, scope)),
        "resource_search" => resource_search(scope, args).await,
        "resource_get" => resource_get(scope, args).await,
        other => (false, format!("未知资源工具: {other}")),
    }
}

fn project_list(workspaces: &WorkspaceStore, scope: &ScopeContext) -> String {
    let registered: Vec<Value> = workspaces
        .list()
        .into_iter()
        .map(|w| {
            let readable = scope.find(&w.id).is_some();
            json!({
                "projectId": w.id,
                "name": w.name,
                "current": scope.current.workspace_id.as_deref() == Some(w.id.as_str()),
                "readable": readable,
                "writable": scope.current.workspace_id.as_deref() == Some(w.id.as_str()),
            })
        })
        .collect();
    json!({
        "current": {
            "projectId": scope.current.id(),
            "name": scope.current.name,
            "writable": true,
        },
        "readable": scope.readonly.iter().map(|p| json!({
            "projectId": p.id(),
            "name": p.name,
        })).collect::<Vec<_>>(),
        "registered": registered,
        "includeLibrary": scope.include_library,
    })
    .to_string()
}

async fn resource_search(scope: &ScopeContext, args: &Value) -> (bool, String) {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if query.is_empty() {
        return (false, "query required".into());
    }
    let want = args.get("scope").and_then(Value::as_str).unwrap_or("all");
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| (n as usize).clamp(1, 40))
        .unwrap_or(12);
    let cursor = args
        .get("cursor")
        .and_then(Value::as_str)
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(0);
    let filter_project = args.get("projectId").and_then(Value::as_str);
    if let Some(id) = filter_project {
        if scope.find(id).is_none() {
            return (false, format!("PROJECT_OUT_OF_SCOPE: {id}"));
        }
    }
    let kinds: Vec<String> = args
        .get("kinds")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut hits: Vec<Value> = Vec::new();
    if matches!(want, "all" | "current" | "selected") {
        for p in scope.searchable() {
            if want == "current" && p.id() != scope.current.id() {
                continue;
            }
            if want == "selected" && p.id() == scope.current.id() {
                continue;
            }
            if let Some(id) = filter_project {
                if p.id() != id {
                    continue;
                }
            }
            hits.extend(search_project(p.id(), &p.project_root, &query, &kinds).await);
        }
    }
    if scope.include_library && matches!(want, "all" | "library") {
        hits.extend(search_library(&scope.current.project_root, &query, &kinds).await);
    }
    if scope.include_library && matches!(want, "all" | "store") {
        hits.extend(search_store(&scope.current.project_root, &query, &kinds).await);
    }

    let total = hits.len();
    let end = (cursor + limit).min(total);
    let page = if cursor >= total {
        Vec::new()
    } else {
        hits[cursor..end].to_vec()
    };
    let next = if end < total {
        Some(end.to_string())
    } else {
        None
    };
    (
        true,
        json!({
            "total": total,
            "hits": page,
            "nextCursor": next,
            "currentProjectId": scope.current.id(),
        })
        .to_string(),
    )
}

async fn search_project(
    project_id: &str,
    root: &std::path::Path,
    query: &str,
    kinds: &[String],
) -> Vec<Value> {
    let ctx_kinds: Vec<&str> = kinds
        .iter()
        .map(String::as_str)
        .filter(|k| matches!(*k, "asset" | "entity" | "graph" | "symbol" | "doc"))
        .collect();
    if !kinds.is_empty() && ctx_kinds.is_empty() {
        return Vec::new();
    }
    let args = json!({
        "query": query,
        "topK": 8,
        "kinds": ctx_kinds,
    });
    let Ok(result) =
        crate::mcp::call_tool_in(root, "mcp__context__context_search", Some(args)).await
    else {
        return Vec::new();
    };
    let text = envelope_text(&result);
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    if v.get("error").is_some() {
        return Vec::new();
    }
    v.get("hits")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|h| {
            let kind = h.get("kind").and_then(Value::as_str).unwrap_or("asset");
            let path = h.get("path").and_then(Value::as_str).unwrap_or("");
            let id = h.get("id").and_then(Value::as_str).unwrap_or("");
            json!({
                "corpus": "project",
                "projectId": project_id,
                "kind": kind,
                "title": h.get("title"),
                "path": path,
                "locator": format!("forge://project/{project_id}/{kind}/{id}"),
                "description": h.get("description"),
                "facts": h.get("facts"),
                "score": h.get("score"),
            })
        })
        .collect()
}

async fn search_library(root: &std::path::Path, query: &str, kinds: &[String]) -> Vec<Value> {
    if !kinds.is_empty() && !kinds.iter().any(|k| k == "library") {
        return Vec::new();
    }
    let args = json!({ "query": query, "limit": 8 });
    let Ok(result) = crate::mcp::call_tool_in(root, "mcp__store__library_search", Some(args)).await
    else {
        return Vec::new();
    };
    let text = envelope_text(&result);
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    v.get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|it| {
            let id = it.get("id").and_then(Value::as_str).unwrap_or("");
            json!({
                "corpus": "library",
                "kind": "library",
                "title": it.get("name"),
                "locator": format!("forge://library/{id}"),
                "description": it.get("kind"),
                "tags": it.get("tags"),
            })
        })
        .collect()
}

async fn search_store(root: &std::path::Path, query: &str, kinds: &[String]) -> Vec<Value> {
    if !kinds.is_empty() && !kinds.iter().any(|k| k == "store") {
        return Vec::new();
    }
    let args = json!({ "query": query, "pageSize": 8 });
    let Ok(result) = crate::mcp::call_tool_in(root, "mcp__store__store_search", Some(args)).await
    else {
        return Vec::new();
    };
    let text = envelope_text(&result);
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    v.get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|it| {
            let sid = it.get("sourceId").and_then(Value::as_str).unwrap_or("");
            let pkg = it.get("package").cloned().unwrap_or(Value::Null);
            let pid = pkg.get("id").and_then(Value::as_str).unwrap_or("");
            json!({
                "corpus": "store",
                "kind": "store",
                "title": pkg.get("name").or_else(|| pkg.get("id")),
                "locator": format!("forge://store/{sid}/{pid}"),
                "description": pkg.get("description"),
            })
        })
        .collect()
}

async fn resource_get(scope: &ScopeContext, args: &Value) -> (bool, String) {
    let locator = args
        .get("locator")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if locator.is_empty() {
        return (false, "locator required".into());
    }
    if let Some(rest) = locator.strip_prefix("forge://project/") {
        let mut parts = rest.splitn(3, '/');
        let pid = parts.next().unwrap_or("");
        let kind = parts.next().unwrap_or("");
        let id = parts.next().unwrap_or("");
        let Some(p) = scope.find(pid) else {
            return (false, format!("PROJECT_OUT_OF_SCOPE: {pid}"));
        };
        if kind == "doc" {
            return read_doc_fragment(p, id, args);
        }
        let Ok(result) = crate::mcp::call_tool_in(
            &p.project_root,
            "mcp__context__context_get",
            Some(json!({ "id": id })),
        )
        .await
        else {
            return (false, "context_get failed".into());
        };
        return (true, envelope_text(&result));
    }
    if let Some(id) = locator.strip_prefix("forge://library/") {
        let Ok(result) = crate::mcp::call_tool_in(
            &scope.current.project_root,
            "mcp__store__library_list",
            Some(json!({})),
        )
        .await
        else {
            return (false, "library_list failed".into());
        };
        let text = envelope_text(&result);
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return (false, text);
        };
        let hit = v.get("items").and_then(Value::as_array).and_then(|a| {
            a.iter()
                .find(|i| i.get("id").and_then(Value::as_str) == Some(id))
        });
        return match hit {
            Some(h) => (true, h.to_string()),
            None => (false, format!("LIBRARY_NOT_FOUND: {id}")),
        };
    }
    if let Some(rest) = locator.strip_prefix("forge://store/") {
        let mut parts = rest.splitn(2, '/');
        let sid = parts.next().unwrap_or("");
        let pid = parts.next().unwrap_or("");
        let Ok(result) = crate::mcp::call_tool_in(
            &scope.current.project_root,
            "mcp__store__store_info",
            Some(json!({ "sourceId": sid, "packageId": pid })),
        )
        .await
        else {
            return (false, "store_info failed".into());
        };
        return (true, envelope_text(&result));
    }
    (false, format!("UNKNOWN_LOCATOR: {locator}"))
}

fn read_doc_fragment(
    project: &crate::scope::ScopeProject,
    id: &str,
    args: &Value,
) -> (bool, String) {
    // id 可能是 context 文档 id(doc:rel#n)或相对路径。
    let rel = id
        .strip_prefix("doc:")
        .unwrap_or(id)
        .split('#')
        .next()
        .unwrap_or(id);
    let start = args.get("startLine").and_then(Value::as_u64).unwrap_or(1) as usize;
    let end = args.get("endLine").and_then(Value::as_u64).unwrap_or(80) as usize;
    match crate::native_tools::read_lines(&project.project_root, rel, start, end) {
        Ok(text) => (
            true,
            json!({
                "locator": format!("forge://project/{}/doc/{id}", project.id()),
                "path": rel,
                "startLine": start,
                "endLine": end,
                "text": text,
            })
            .to_string(),
        ),
        Err(e) => (false, e),
    }
}

fn envelope_text(result: &Value) -> String {
    if let Some(t) = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
    {
        return t.to_string();
    }
    result.to_string()
}

/// Studio turn 前确保索引可用。缓存重建不是用户内容写,不走审批。
pub async fn ensure_indexes(scope: &ScopeContext) {
    for p in scope.searchable() {
        let status = crate::mcp::call_tool_in(
            &p.project_root,
            "mcp__context__context_index_status",
            Some(json!({})),
        )
        .await;
        let info = status
            .ok()
            .and_then(|r| serde_json::from_str::<Value>(&envelope_text(&r)).ok());
        let built = info
            .as_ref()
            .and_then(|v| v.get("built").and_then(Value::as_bool))
            .unwrap_or(false);
        let stale = info
            .as_ref()
            .and_then(|v| v.get("stale").and_then(Value::as_bool))
            .unwrap_or(false);
        if !built || stale {
            let _ = crate::mcp::call_tool_in(
                &p.project_root,
                "mcp__context__context_index_build",
                Some(json!({})),
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::ScopeProject;
    use std::path::PathBuf;

    fn scope() -> ScopeContext {
        ScopeContext {
            current: ScopeProject {
                workspace_id: Some("ws_a".into()),
                name: "A".into(),
                workspace_root: PathBuf::from("."),
                project_root: PathBuf::from("."),
                game_mode: assetd::project::GameMode::ThreeD,
            },
            readonly: vec![ScopeProject {
                workspace_id: Some("ws_b".into()),
                name: "B".into(),
                workspace_root: PathBuf::from("."),
                project_root: PathBuf::from("."),
                game_mode: assetd::project::GameMode::ThreeD,
            }],
            include_library: true,
        }
    }

    #[test]
    fn resource_search_rejects_unselected_project() {
        let s = scope();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (ok, text) = rt.block_on(resource_search(
            &s,
            &json!({ "query": "x", "projectId": "ws_c" }),
        ));
        assert!(!ok);
        assert!(text.contains("PROJECT_OUT_OF_SCOPE"), "{text}");
    }

    #[test]
    fn resource_get_rejects_unknown_locator() {
        let s = scope();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (ok, text) = rt.block_on(resource_get(&s, &json!({ "locator": "http://evil" })));
        assert!(!ok);
        assert!(text.contains("UNKNOWN_LOCATOR"), "{text}");
    }

    #[test]
    fn project_list_marks_unselected_not_readable() {
        use crate::workspaces::WorkspaceStore;
        let dir = std::env::temp_dir().join(format!(
            "forge-res-list-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ws = WorkspaceStore::load(dir.join("ws.json"));
        let text = project_list(&ws, &scope());
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["current"]["writable"], true);
        assert_eq!(v["readable"].as_array().unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_list_three_projects_only_selected_readable() {
        use crate::workspaces::WorkspaceStore;
        let dir = std::env::temp_dir().join(format!(
            "forge-res-abc-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::create_dir_all(dir.join("c")).unwrap();
        let ws = WorkspaceStore::load(dir.join("ws.json"));
        let a = ws.create("A", dir.join("a").to_str().unwrap()).unwrap();
        let b = ws.create("B", dir.join("b").to_str().unwrap()).unwrap();
        let c = ws.create("C", dir.join("c").to_str().unwrap()).unwrap();
        let scope = ScopeContext {
            current: ScopeProject {
                workspace_id: Some(a.id.clone()),
                name: "A".into(),
                workspace_root: dir.join("a"),
                project_root: dir.join("a"),
                game_mode: assetd::project::GameMode::ThreeD,
            },
            readonly: vec![ScopeProject {
                workspace_id: Some(b.id.clone()),
                name: "B".into(),
                workspace_root: dir.join("b"),
                project_root: dir.join("b"),
                game_mode: assetd::project::GameMode::ThreeD,
            }],
            include_library: true,
        };
        let v: Value = serde_json::from_str(&project_list(&ws, &scope)).unwrap();
        assert_eq!(v["current"]["projectId"], a.id);
        assert_eq!(v["current"]["writable"], true);
        assert_eq!(v["readable"][0]["projectId"], b.id);
        let registered = v["registered"].as_array().expect("registered");
        let row = |id: &str| {
            registered
                .iter()
                .find(|r| r["projectId"] == id)
                .cloned()
                .unwrap_or(json!(null))
        };
        assert_eq!(row(&a.id)["readable"], true);
        assert_eq!(row(&a.id)["writable"], true);
        assert_eq!(row(&b.id)["readable"], true);
        assert_eq!(row(&b.id)["writable"], false);
        assert_eq!(row(&c.id)["readable"], false);
        assert_eq!(row(&c.id)["writable"], false);
        std::fs::remove_dir_all(&dir).ok();
    }
}
