//! graph 三工具(F4 wave.2,10 §6 agent 生成路径):
//! graph_validate(schema + 注册表 + 类型 + 双环)/ graph_create(校验通过落
//! Content/Graphs/<name>.rxgraph,覆盖即更新)/ graph_get(读回)。
//! 校验器在 forge-logic(纯函数);路径一律相对项目根(projects/demo)。

use std::path::Path;

use serde_json::{json, Value};

use forge_logic::graph::GraphDoc;
use forge_logic::validate::{validate_graph_with_project, GRAPH_SCHEMA};

use crate::rxtool::{TResult, ToolError};

fn terr(code: &str, message: impl Into<String>) -> ToolError {
    ToolError { code: code.to_string(), message: message.into() }
}

fn confine_rel(root: &Path, rel: &str) -> Result<std::path::PathBuf, ToolError> {
    forge_util::pathutil::confine_under(&[root], rel).map_err(|e| terr("PATH_OUTSIDE_ROOT", e))
}

/// 解析 GraphDoc;失败 → ok:false + 单条 GRAPH_SCHEMA(校验语义,非工具级错误)。
fn parse_doc(v: &Value) -> Result<GraphDoc, Value> {
    serde_json::from_value(v.clone()).map_err(|e| {
        json!({
            "ok": false,
            "errors": [{ "code": GRAPH_SCHEMA, "message": format!("图 JSON 结构不合法: {e}") }]
        })
    })
}

fn validate_result(doc: &GraphDoc, root: &Path) -> Value {
    // RD-F4-004:项目感校验(八臂 + call_function 第九臂,module 文件经 root 解析)。
    let errors = validate_graph_with_project(doc, root);
    json!({ "ok": errors.is_empty(), "errors": errors })
}

/// graph_validate {graph?: object | path?: string} → {ok, errors[]}(二选一;
/// path 相对项目根)。校验结果为正常返回(isError=false),拒绝体现在 ok:false。
pub fn validate(args: &Value, root: &Path) -> TResult<Value> {
    let has_graph = args.get("graph").is_some();
    let has_path = args.get("path").is_some();
    if has_graph == has_path {
        return Err(terr("USAGE", "graph 与 path 须二选一"));
    }
    let doc = if let Some(g) = args.get("graph") {
        match parse_doc(g) {
            Ok(d) => d,
            Err(rej) => return Ok(rej),
        }
    } else {
        let rel = args["path"].as_str().ok_or_else(|| terr("USAGE", "path 须为字符串"))?;
        let p = confine_rel(root, rel)?;
        let text = std::fs::read_to_string(&p)
            .map_err(|_| terr("GRAPH_NOT_FOUND", format!("图文件不存在: {rel}")))?;
        match parse_doc(&serde_json::from_str::<Value>(&text).map_err(|e| {
            terr("GRAPH_BAD_JSON", format!("{rel} 非合法 JSON: {e}"))
        })?) {
            Ok(d) => d,
            Err(rej) => return Ok(rej),
        }
    };
    Ok(validate_result(&doc, root))
}

/// graph_create {name, graph} → 校验通过落 <root>/Content/Graphs/<name>.rxgraph
/// (确定性 JSON:key 字典序 + 两空格缩进 + 末尾单换行;**已存在则覆盖,即更新语义**)
/// → {ok, path};校验不过 → {ok:false, errors[]} 且不落盘。
pub fn create(args: &Value, root: &Path) -> TResult<Value> {
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| terr("USAGE", "缺 name"))?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        // 400 语义:工具级错误,code 承载 GRAPH_BAD_NAME。
        return Err(terr("GRAPH_BAD_NAME", format!("name 须匹配 [A-Za-z0-9_-]+: {name:?}")));
    }
    let g = args.get("graph").ok_or_else(|| terr("USAGE", "缺 graph"))?;
    let doc: GraphDoc = serde_json::from_value(g.clone())
        .map_err(|e| terr("GRAPH_BAD_GRAPH", format!("图 JSON 结构不合法: {e}")))?;
    let errors = validate_graph_with_project(&doc, root);
    if !errors.is_empty() {
        return Ok(json!({ "ok": false, "errors": errors }));
    }
    let dir = root.join("Content").join("Graphs");
    std::fs::create_dir_all(&dir)
        .map_err(|e| terr("TOOL_ERROR", format!("建目录 {} 失败: {e}", dir.display())))?;
    let path = dir.join(format!("{name}.rxgraph"));
    let mut text = doc
        .to_json()
        .map_err(|e| terr("TOOL_ERROR", format!("序列化图失败: {e}")))?;
    text.push('\n');
    std::fs::write(&path, text)
        .map_err(|e| terr("TOOL_ERROR", format!("写 {} 失败: {e}", path.display())))?;
    Ok(json!({ "ok": true, "path": format!("Content/Graphs/{name}.rxgraph") }))
}

/// graph_get {path} → {graph}(path 相对项目根;不存在 → GRAPH_NOT_FOUND)。
pub fn get(args: &Value, root: &Path) -> TResult<Value> {
    let rel = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| terr("USAGE", "缺 path"))?;
    let p = confine_rel(root, rel)?;
    let text = std::fs::read_to_string(&p)
        .map_err(|_| terr("GRAPH_NOT_FOUND", format!("图文件不存在: {rel}")))?;
    let graph: Value = serde_json::from_str(&text)
        .map_err(|e| terr("GRAPH_BAD_JSON", format!("{rel} 非合法 JSON: {e}")))?;
    Ok(json!({ "graph": graph }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge_graphtool_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn door_opener() -> Value {
        json!({
            "version": 1,
            "id": "g_door",
            "name": "DoorOpener",
            "exposedProps": [ { "name": "openSpeed", "kind": "F32", "default": 90.0 } ],
            "nodes": [
                { "id": "n1", "type": "event.on_trigger_enter", "pos": [40, 80] },
                { "id": "n2", "type": "flow.branch", "pos": [240, 80],
                  "inputs": { "condition": { "node": "n3", "pin": "out" } } },
                { "id": "n3", "type": "entity.has_tag", "pos": [60, 200],
                  "inputs": { "entity": { "node": "n1", "pin": "otherEntity" }, "tag": { "const": "player" } } },
                { "id": "n4", "type": "transform.rotate_tween", "pos": [460, 80],
                  "inputs": { "target": { "const": "$self" }, "angle": { "ref": "openSpeed" }, "duration": { "const": 1.2 } } }
            ],
            "edges": [ { "from": ["n1", "exec"], "to": ["n2", "exec"] }, { "from": ["n2", "then"], "to": ["n4", "exec"] } ]
        })
    }

    #[test]
    fn validate_ok_and_reject() {
        let root = tmp_root("validate");
        let v = validate(&json!({ "graph": door_opener() }), &root).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["errors"].as_array().unwrap().len(), 0);
        // 坏图:branch.condition 悬空 → ok:false + GRAPH_DANGLING_INPUT。
        let mut bad = door_opener();
        bad["nodes"][1].as_object_mut().unwrap().remove("inputs");
        let v = validate(&json!({ "graph": bad }), &root).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["errors"][0]["code"], "GRAPH_DANGLING_INPUT");
        assert_eq!(v["errors"][0]["nodeId"], "n2");
        // 二选一约束。
        assert_eq!(validate(&json!({}), &root).unwrap_err().code, "USAGE");
        assert_eq!(
            validate(&json!({ "graph": {}, "path": "x" }), &root).unwrap_err().code,
            "USAGE"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_path_mode_and_not_found() {
        let root = tmp_root("vpath");
        create(&json!({ "name": "door_opener", "graph": door_opener() }), &root).unwrap();
        let v = validate(&json!({ "path": "Content/Graphs/door_opener.rxgraph" }), &root).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        let e = validate(&json!({ "path": "Content/Graphs/ghost.rxgraph" }), &root).unwrap_err();
        assert_eq!(e.code, "GRAPH_NOT_FOUND");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_writes_deterministic_and_get_reads_back() {
        let root = tmp_root("create");
        let v = create(&json!({ "name": "door_opener", "graph": door_opener() }), &root).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["path"], "Content/Graphs/door_opener.rxgraph");
        let on_disk = std::fs::read_to_string(root.join("Content/Graphs/door_opener.rxgraph")).unwrap();
        assert!(on_disk.ends_with("}\n"), "末尾单换行: {:.20}", &on_disk[on_disk.len() - 8..]);
        assert!(on_disk.contains("\n  \"version\": 1,"), "两空格缩进");
        // graph_get 读回与写入逐字节同义(解析后等值于原图)。
        let g = get(&json!({ "path": "Content/Graphs/door_opener.rxgraph" }), &root).unwrap();
        let sent: Value = door_opener();
        let canon = |v: &Value| serde_json::to_string(&serde_json::from_value::<GraphDoc>(v.clone()).unwrap()).unwrap();
        assert_eq!(canon(&g["graph"]), canon(&sent));
        // 覆盖即更新:同名重写 name 字段。
        let mut updated = door_opener();
        updated["name"] = json!("DoorOpenerV2");
        let v2 = create(&json!({ "name": "door_opener", "graph": updated }), &root).unwrap();
        assert_eq!(v2["ok"], true);
        let g2 = get(&json!({ "path": "Content/Graphs/door_opener.rxgraph" }), &root).unwrap();
        assert_eq!(g2["graph"]["name"], "DoorOpenerV2");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_rejects_bad_name_and_invalid_graph() {
        let root = tmp_root("badname");
        for bad in ["", "a/b", "a b", "门.rxgraph"] {
            let e = create(&json!({ "name": bad, "graph": door_opener() }), &root).unwrap_err();
            assert_eq!(e.code, "GRAPH_BAD_NAME", "{bad}");
        }
        // 校验不过 → ok:false 不落盘。
        let mut invalid = door_opener();
        invalid["version"] = json!(2);
        let v = create(&json!({ "name": "bad_graph", "graph": invalid }), &root).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["errors"][0]["code"], "GRAPH_SCHEMA");
        assert!(!root.join("Content/Graphs/bad_graph.rxgraph").exists(), "校验不过不落盘");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn get_not_found() {
        let root = tmp_root("getnf");
        let e = get(&json!({ "path": "Content/Graphs/ghost.rxgraph" }), &root).unwrap_err();
        assert_eq!(e.code, "GRAPH_NOT_FOUND");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn get_rejects_path_escape() {
        let root = tmp_root("escape");
        let e = get(&json!({ "path": "../secret.rxgraph" }), &root).unwrap_err();
        assert_eq!(e.code, "PATH_OUTSIDE_ROOT");
        let _ = std::fs::remove_dir_all(&root);
    }
}
