//! MCP stdio 服务:initialize / tools/list / tools/call。复刻 engine-scene-mcp 骨架。

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use assetd::import::{import_assets, ImportOutcome};
use assetd::meta::MetaDoc;
use assetd::project::ForgeProject;
use assetd::{meta_path_for, normalize_rel, AssetType, Result as AssetResult};
use serde_json::{json, Value};

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "asset_import",
                "description": "导入源文件到 Content/目录;幂等(覆盖源文件但保留 .meta GUID)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sourcePaths": { "type": "array", "items": { "type": "string" }, "description": "源文件路径列表(绝对或相对项目根)" },
                        "destFolder": { "type": "string", "description": "目标文件夹(相对 Content/,如 \"Meshes\")" },
                        "importSettings": { "type": "object", "description": "导入选项覆盖(可选)" }
                    },
                    "required": ["sourcePaths", "destFolder"]
                }
            },
            {
                "name": "asset_list",
                "description": "列出 Content/ 下全部资产及元信息",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "asset_get_meta",
                "description": "读 .meta 内容",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPath": { "type": "string" } },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "asset_build_status",
                "description": "查构建状态(current/stale/building/failed);空参数 = 全项目",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPaths": { "type": "array", "items": { "type": "string" } } }
                }
            },
            {
                "name": "asset_refs",
                "description": "查资产引用边(refs=出边 / referencedBy=入边)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "direction": { "type": "string", "enum": ["refs", "referencedBy"] },
                        "depth": { "type": "integer", "description": "递归深度(缺省 1)" }
                    },
                    "required": ["assetPath", "direction"]
                }
            },
            {
                "name": "asset_delete",
                "description": "删除资产(默认引用阻断;force=true 跳过阻断,但 MCP 层须先 Proposal)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPaths": { "type": "array", "items": { "type": "string" } },
                        "force": { "type": "boolean", "description": "跳过引用阻断(危险)" }
                    },
                    "required": ["assetPaths"]
                }
            },
            {
                "name": "asset_move",
                "description": "移动资产到新目录(自动留 redirector;GUID 引用不断链);可选 newName 同步改名(清洗命名)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "destFolder": { "type": "string" },
                        "newName": { "type": "string", "description": "新文件名(可选;清洗命名用)" }
                    },
                    "required": ["assetPath", "destFolder"]
                }
            },
            {
                "name": "asset_fix_redirectors",
                "description": "收敛 redirector(重写路径引用并清除)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "folder": { "type": "string", "description": "限定目录(可选)" } }
                }
            },
            {
                "name": "asset_reimport",
                "description": "重新构建资产(改 importSettings 后)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPaths": { "type": "array", "items": { "type": "string" } } },
                    "required": ["assetPaths"]
                }
            },
            {
                "name": "asset_set_meta",
                "description": "打补丁到 .meta importSettings",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "patch": { "type": "object" }
                    },
                    "required": ["assetPath", "patch"]
                }
            },
            {
                "name": "asset_thumbnail",
                "description": "贴图缩略图(原图直出 data URL,前端 CSS 缩放);非贴图 → NO_THUMBNAIL(网格离屏渲染 = RD-F2-002)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPath": { "type": "string" } },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "material_create",
                "description": "创建材质资产(.rxmat JSON:version/shader/params/textures);textures 槽位值须为已存在资产的 GUID,建 material→texture 引用边",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "材质名(不含扩展名/路径分隔符)" },
                        "destFolder": { "type": "string", "description": "目标文件夹(缺省 Materials)" },
                        "shader": { "type": "string", "description": "shader/closure id(缺省 pbr-default)" },
                        "params": { "type": "object", "description": "材质参数(baseColor/roughness/metallic 等)" },
                        "textures": { "type": "object", "description": "纹理槽位 → 贴图资产 GUID" }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "texture_process",
                "description": "贴图处理(image crate 解码):ops.resize={width,height} 精确 | {max} 等比缩小;ops.format=png|jpg 转格式;输出同目录 <stem>@<w>x<h>.<ext> 新资产(幂等),原资产不动",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "ops": {
                            "type": "object",
                            "properties": {
                                "resize": { "type": "object", "description": "{width,height} 或 {max}" },
                                "format": { "type": "string", "enum": ["png", "jpg"] }
                            }
                        }
                    },
                    "required": ["assetPath", "ops"]
                }
            },
            {
                "name": "mesh_inspect",
                "description": "网格统计(读缓存 .rxmesh 真实字节):vertices/triangles/meshlets/lods/materials/bounds;缓存缺失 → MESH_NOT_BUILT",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPath": { "type": "string" } },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "asset_cleanup_scan",
                "description": "asset-cleanup dryRun:扫描全项目,产出整理提案(misplaced 错放/naming 命名混乱/orphan 孤儿),不写任何文件;执行经 asset_move(移动/改名)或 asset_delete(孤儿,须 Proposal)",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]
    })
}

fn ok(id: Value, result: Value) -> Value { json!({ "jsonrpc": "2.0", "id": id, "result": result }) }
fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_wrap(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error { out["isError"] = json!(true); }
    out
}

fn lock<'a>(m: &'a Mutex<ForgeProject>) -> MutexGuard<'a, ForgeProject> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn call_tool(proj: &Arc<Mutex<ForgeProject>>, params: &Value) -> Result<Value, Value> {
    let name = params.get("name").and_then(Value::as_str)
        .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺工具名"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() { return Err(err(Value::Null, -32602, "invalid params: arguments 须为对象")); }

    match name {
        "asset_import" => {
            let sources: Vec<String> = args.get("sourcePaths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let settings: Option<&serde_json::Map<String, Value>> = args.get("importSettings")
                .and_then(|v| v.as_object());
            let p = lock(proj);
            match import_assets(&p, &sources, dest, settings) {
                Ok(out) => Ok(json!({
                    "imported": out.imported.iter().map(|i| json!({
                        "assetPath": i.asset_path, "guid": i.guid,
                        "type": i.atype.as_str(), "cacheHit": i.cache_hit,
                        "artifact": i.artifact, "vertexCount": i.vertex_count,
                        "triangleCount": i.triangle_count,
                        "width": i.width, "height": i.height
                    })).collect::<Vec<_>>(),
                    "failed": out.failed.iter().map(|f| json!({
                        "source": f.source, "error": f.error
                    })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.message, "code": e.code })),
            }
        }
        "asset_list" => {
            let p = lock(proj);
            let items = p.scan_content()
                .map_err(|e| err(Value::Null, -32000, &e.message))?
                .into_iter()
                .map(|rel| {
                    let meta_path = meta_path_for(&p.content_root(), &rel);
                    let (guid, atype) = if meta_path.is_file() {
                        match MetaDoc::load(&meta_path) {
                            Ok(m) => (m.guid, m.atype),
                            Err(_) => (String::new(), "unknown".into()),
                        }
                    } else { (String::new(), "unknown".into()) };
                    let size = p.content_root().join(&rel).metadata().ok().map(|m| m.len()).unwrap_or(0);
                    json!({ "path": rel, "guid": guid, "type": atype, "size": size })
                })
                .collect::<Vec<_>>();
            Ok(json!({ "assets": items }))
        }
        "asset_get_meta" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let p = lock(proj);
            let rel = normalize_rel(asset_path).map_err(|e| err(Value::Null, -32602, &e.message))?;
            let meta_path = meta_path_for(&p.content_root(), &rel);
            if !meta_path.is_file() {
                return Ok(json!({ "error": "NO_META", "message": format!("缺 .meta: {rel}") }));
            }
            match MetaDoc::load(&meta_path) {
                Ok(m) => Ok(json!({ "meta": serde_json::to_value(m).unwrap_or(Value::Null) })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_build_status" => {
            let paths: Vec<String> = args.get("assetPaths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let p = lock(proj);
            match assetd::status::build_status(&p, &paths) {
                Ok(items) => Ok(json!({
                    "items": items.iter().map(|i| json!({
                        "path": i.path, "state": i.state.as_str(), "hash": i.hash
                    })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_refs" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let direction = args.get("direction").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 direction"))?;
            let p = lock(proj);
            let rel = normalize_rel(asset_path).map_err(|e| err(Value::Null, -32602, &e.message))?;
            let meta_path = meta_path_for(&p.content_root(), &rel);
            if !meta_path.is_file() {
                return Ok(json!({ "error": "NO_META", "message": format!("缺 .meta: {rel}") }));
            }
            let meta = MetaDoc::load(&meta_path)
                .map_err(|e| err(Value::Null, -32000, &e.message))?;
            // 查询前实时重建引用图(小项目性能可接受;大项目后续改增量索引)。
            let graph = assetd::refs::RefGraph::rebuild(&p)
                .map_err(|e| err(Value::Null, -32000, &e.message))?;
            let edges = if direction == "refs" {
                graph.refs(&meta.guid)
            } else {
                graph.referenced_by(&meta.guid)
            };
            Ok(json!({
                "edges": edges.iter().map(|e| json!({
                    "from": e.from_guid, "to": e.to_guid, "type": e.edge_type
                })).collect::<Vec<_>>()
            }))
        }
        "asset_delete" => {
            let paths: Vec<String> = args.get("assetPaths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let p = lock(proj);
            match assetd::ops::delete_assets(&p, &paths, force) {
                Ok(out) => Ok(json!({
                    "deleted": out.deleted,
                    "blockedByRefs": out.blocked_by_refs.iter().map(|(p, refs)| json!({
                        "assetPath": p, "referencedBy": refs
                    })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_move" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let dest = args.get("destFolder").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 destFolder"))?;
            let new_name = args.get("newName").and_then(Value::as_str);
            let p = lock(proj);
            match assetd::ops::move_asset(&p, asset_path, dest, new_name) {
                Ok(out) => {
                    let red = out.redirector.map(|(g, old, new)| json!({
                        "guid": g, "oldPath": old, "newPath": new
                    }));
                    Ok(json!({ "moved": out.moved, "redirector": red }))
                }
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_fix_redirectors" => {
            let folder = args.get("folder").and_then(Value::as_str);
            let p = lock(proj);
            match assetd::ops::fix_redirectors(&p, folder) {
                Ok(fixed) => Ok(json!({ "fixed": fixed })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_reimport" => {
            let paths: Vec<String> = args.get("assetPaths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let p = lock(proj);
            match assetd::ops::reimport_assets(&p, &paths) {
                Ok(rebuilt) => Ok(json!({ "rebuilt": rebuilt })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_set_meta" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let patch = args.get("patch").and_then(|v| v.as_object())
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 patch"))?;
            let p = lock(proj);
            match assetd::ops::set_meta(&p, asset_path, patch) {
                Ok(()) => Ok(json!({})),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_thumbnail" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let p = lock(proj);
            match assetd::thumb::thumbnail_data_url(&p, asset_path) {
                Ok((url, bytes)) => Ok(json!({ "dataUrl": url, "bytes": bytes })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "material_create" => {
            let name = args.get("name").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 name"))?;
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let shader = args.get("shader").and_then(Value::as_str);
            let params = args.get("params").and_then(|v| v.as_object());
            let textures = args.get("textures").and_then(|v| v.as_object());
            let p = lock(proj);
            match assetd::material::create_material(&p, dest, name, shader, params, textures) {
                Ok(c) => Ok(json!({
                    "assetPath": c.asset_path, "guid": c.guid,
                    "textureRefs": c.texture_refs.iter().map(|(s, g)| json!({ "slot": s, "guid": g })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "texture_process" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let ops = args.get("ops").cloned().unwrap_or(json!({}));
            if !ops.is_object() { return Err(err(Value::Null, -32602, "invalid params: ops 须为对象")); }
            let p = lock(proj);
            match assetd::texture::process_texture(&p, asset_path, &ops) {
                Ok(o) => Ok(json!({
                    "outputAssetPath": o.output_rel, "guid": o.guid,
                    "width": o.width, "height": o.height, "bytes": o.bytes
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "mesh_inspect" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let p = lock(proj);
            match assetd::inspect::inspect_mesh(&p, asset_path) {
                Ok(r) => Ok(json!({
                    "vertices": r.vertices, "triangles": r.triangles,
                    "meshlets": r.meshlets, "lods": r.lods,
                    "materials": r.materials,
                    "bounds": r.bounds.map(|(mn, mx)| json!({ "min": mn, "max": mx })),
                    "artifact": r.artifact
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_cleanup_scan" => {
            let p = lock(proj);
            match assetd::cleanup::scan_cleanup(&p) {
                Ok(r) => Ok(json!({
                    "scanned": r.scanned,
                    "proposals": r.proposals.iter().map(|p| json!({
                        "assetPath": p.asset_path, "guid": p.guid, "issue": p.issue,
                        "destFolder": p.dest_folder, "newName": p.new_name, "reason": p.reason
                    })).collect::<Vec<_>>(),
                    "impact": r.impact.iter().map(|(k, n)| json!({ "issue": k, "count": n })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        _ => Err(err(Value::Null, -32602, &format!("未知工具:{name}"))),
    }
}

pub fn serve_stdio(proj: Arc<Mutex<ForgeProject>>) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = err(Value::Null, -32700, &format!("parse error: {e}"));
                let _ = writeln!(stdout, "{resp}");
                let _ = stdout.flush();
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let resp: Option<Value> = match method {
            "initialize" => id.map(|i| ok(i, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "asset-pipeline-mcp", "version": env!("CARGO_PKG_VERSION") }
            }))),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => id.map(|i| ok(i, json!({}))),
            "tools/list" => id.map(|i| ok(i, tool_list())),
            "tools/call" => id.map(|i| {
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                match call_tool(&proj, &params) {
                    Ok(result) => ok(i, tool_wrap(&result, false)),
                    Err(mut e) => { e["id"] = i.clone(); e }
                }
            }),
            "" => id.map(|i| err(i, -32600, "invalid request: 缺 method")),
            other => id.map(|i| err(i, -32601, &format!("method not found: {other}"))),
        };
        if let Some(r) = resp {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
    }
}
