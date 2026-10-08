//! Local graph identity and bounded subgraph reads, including unsaved shared drafts.
use super::*;

pub(super) fn read(p: &ScopeProject, r: &Value) -> Result<Value, String> {
    let kind = r["kind"].as_str().ok_or("EDITOR_REFERENCE_INVALID")?;
    if r["draft"] == true || r.pointer("/selection/dirty") == Some(&json!(true)) {
        let id = r["resourceId"].as_str().ok_or("EDITOR_GRAPH_ID_REQUIRED")?;
        let mut result = read_document(&p.project_root, kind, id)?;
        if result["document"].is_null() {
            return Err(
                "EDITOR_DRAFT_NOT_FOUND: synchronize the graph draft before sending its annotation"
                    .into(),
            );
        }
        result["draft"] = json!(true);
        project_graph(&mut result["document"], r)?;
        return Ok(result);
    }
    let suffix = if kind == "shaderGraph" {
        ".rxshadergraph"
    } else {
        ".rxgraph"
    };
    let id = r["resourceId"].as_str().filter(|s| !s.is_empty());
    let project = assetd::project::ForgeProject::load(&p.project_root)
        .unwrap_or_else(|_| assetd::project::ForgeProject::with_defaults(p.project_root.clone()));
    let mut candidates = Vec::new();
    // Identity scans detect duplicate graph IDs instead of preferring an arbitrary file or stale path.
    if id.is_some() {
        for rel in project.scan_content().map_err(|e| e.to_string())? {
            if rel.ends_with(suffix) {
                candidates.push(format!("Content/{rel}"));
            }
        }
    } else if let Some(path) = r["path"].as_str() {
        candidates.push(if path.starts_with("Content/") {
            path.into()
        } else {
            format!("Content/{path}")
        });
    }
    let mut found = Vec::new();
    for relative in candidates {
        let path = safe_path(&p.project_root, &relative)?;
        let bytes = std::fs::read(&path).map_err(|e| format!("EDITOR_GRAPH_NOT_FOUND: {e}"))?;
        if bytes.len() > 1024 * 1024 {
            continue;
        }
        let graph: Value = match serde_json::from_slice(&bytes) {
            Ok(graph) => graph,
            Err(_) => continue,
        };
        let meta = assetd::meta::MetaDoc::load(&assetd::meta_path_for(
            &project.content_root(),
            relative.strip_prefix("Content/").unwrap_or(&relative),
        ))
        .ok();
        if id.is_some_and(|id| graph["id"] != id && meta.as_ref().is_none_or(|m| m.guid != id)) {
            continue;
        }
        found.push(json!({"graph":graph,"path":relative,"guid":meta.map(|m|m.guid),"revision":forge_util::hashutil::sha256_hex(&bytes),"sourceHash":forge_util::hashutil::sha256_hex(&bytes)}));
    }
    if found.len() > 1 {
        return Err("EDITOR_DUPLICATE_IDENTITY: multiple graph files carry this identity".into());
    }
    let mut result = found.pop().ok_or(
        "EDITOR_GRAPH_NOT_FOUND: graph is missing; synchronize unsaved drafts before annotating",
    )?;
    project_graph(&mut result["graph"], r)?;
    Ok(result)
}

fn project_graph(graph: &mut Value, r: &Value) -> Result<(), String> {
    let Some(ids) = r
        .pointer("/selection/nodeIds")
        .and_then(Value::as_array)
        .filter(|ids| !ids.is_empty())
    else {
        return Ok(());
    };
    let nodes = graph["nodes"].as_array().ok_or("EDITOR_GRAPH_INVALID")?;
    if ids
        .iter()
        .any(|id| !nodes.iter().any(|node| node["id"] == *id))
    {
        return Err("EDITOR_REFERENCE_NOT_FOUND: graph node was removed".into());
    }
    let mut included = ids.clone();
    for node in nodes.iter().filter(|node| ids.contains(&node["id"])) {
        for input in node["inputs"]
            .as_object()
            .into_iter()
            .flat_map(|v| v.values())
        {
            if let Some(id) = input.get("node") {
                if !included.contains(id) {
                    included.push(id.clone());
                }
            }
        }
    }
    let total = nodes.len();
    let selected = nodes
        .iter()
        .filter(|node| included.contains(&node["id"]))
        .cloned()
        .collect::<Vec<_>>();
    graph["nodes"] = json!(selected);
    if let Some(edges) = graph["edges"].as_array_mut() {
        edges.retain(|edge| {
            included.contains(&edge["from"][0]) || included.contains(&edge["to"][0])
        });
    }
    graph["disclosure"] =
        json!({"partial":true,"totalNodes":total,"selectedNodeIds":ids,"includedNodeIds":included});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subgraph_includes_dependencies_and_rejects_deleted_nodes() {
        let mut graph = json!({"nodes":[{"id":"a","inputs":{}},{"id":"b","inputs":{"value":{"node":"a","pin":"out"}}},{"id":"unrelated"}],"edges":[]});
        project_graph(&mut graph, &json!({"selection":{"nodeIds":["b"]}})).unwrap();
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(graph["disclosure"]["totalNodes"], 3);
        assert!(project_graph(&mut graph, &json!({"selection":{"nodeIds":["deleted"]}})).is_err());
    }
}
