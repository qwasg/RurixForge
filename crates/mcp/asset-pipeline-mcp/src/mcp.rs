//! MCP stdio 服务:initialize / tools/list / tools/call。复刻 engine-scene-mcp 骨架。

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use assetd::import::import_assets;
use assetd::meta::MetaDoc;
use assetd::project::ForgeProject;
use assetd::{meta_path_for, normalize_rel};
use forge_index::vector::Embedder;
use serde_json::{json, Value};

/// gend::embed::RemoteEmbedder → forge_index Embedder 适配(与 context-mcp 同)。
struct GendEmbedder(gend::embed::RemoteEmbedder);

impl Embedder for GendEmbedder {
    fn model(&self) -> &str {
        &self.0.model
    }
    fn embed(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, String> {
        self.0.embed_batch(texts)
    }
}

fn resolve_embedder() -> Option<GendEmbedder> {
    gend::embed::resolve_embedder().map(GendEmbedder)
}

/// F10-RAG:简介写后增量进语义索引(词法 + 已配 embedding 则向量)。
/// 返回并入响应的字段;索引未建/失败不阻断写结果,如实标注(indexed/indexError)。
fn index_after_description(project_root: &std::path::Path, asset_path: &str) -> Value {
    let rel = match normalize_rel(asset_path) {
        Ok(r) => r,
        Err(e) => {
            return json!({ "indexed": false, "indexError": format!("[{}] {}", e.code, e.message) })
        }
    };
    let embedder = resolve_embedder();
    match forge_index::build::upsert_asset_docs(
        project_root,
        &rel,
        embedder.as_ref().map(|e| e as &dyn Embedder),
    ) {
        Ok(o) => {
            let mut v = json!({
                "indexed": o.indexed,
                "indexedDocs": o.docs,
                "tier": o.tier,
            });
            if let Some(e) = o.embed_error {
                v["embedError"] = json!(e);
            }
            v
        }
        Err(e) => json!({ "indexed": false, "indexError": format!("[{}] {}", e.code, e.message) }),
    }
}

fn tool_list() -> Value {
    json!({
        "tools": [
            {"name":"shader_graph_get","description":"Read a Shader Graph document, stable IDs and sourceHash for optimistic concurrency","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"reference":{"type":"string"}}}},
            {"name":"shader_graph_save","description":"Save a Shader Graph draft atomically. Existing graphs require expectedHash; saving never publishes runtime changes.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"graph":{"type":"object"},"expectedHash":{"type":"string"}},"required":["path","graph"]}},
            {"name":"shader_graph_compile","description":"Compile a typed Shader Graph DAG to validated Rurix SPIR-V and Godot source with node diagnostics. Runtime preview verifies actual backend pipelines.","inputSchema":{"type":"object","properties":{"graph":{"type":"object"},"path":{"type":"string"},"reference":{"type":"string"}}}},
            {"name":"shader_graph_list_nodes","description":"Discover Shader Graph domains, typed nodes, ports and budgets progressively","inputSchema":{"type":"object","properties":{}}},
            {"name":"shader_material_create","description":"Create rxmat v2 binding a Shader Graph GUID to typed parameters and texture GUIDs","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"shaderGraph":{"type":"string"},"params":{"type":"object"},"textures":{"type":"object"}},"required":["path","shaderGraph"]}},
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
                "name": "font_list",
                "description": "D-045:列可用字体——项目已入库字体(含 GUID,Text 组件直接引用)+ 系统字体目录里的候选(未入库,需 font_import)。返回族名/子族/字形数/是否含中文字形",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "includeSystem": { "type": "boolean", "description": "是否列系统字体(缺省 true)" },
                        "query": { "type": "string", "description": "按族名或文件名过滤(不区分大小写)" },
                        "cjkOnly": { "type": "boolean", "description": "只列含中文字形的字体" },
                        "limit": { "type": "integer", "description": "系统字体条数上限(缺省 60,最大 300)" }
                    }
                }
            },
            {
                "name": "font_import",
                "description": "D-045:把字体文件(.ttf/.otf/.ttc,常为 font_list 列出的系统字体)复制进 Content/Fonts/ 入库,返回 GUID 供 Text.font 引用。provenance 记来源与授权提醒:系统字体仅供本地制作,发行前须确认授权",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sourcePath": { "type": "string", "description": "字体文件绝对路径或项目相对路径" },
                        "destFolder": { "type": "string", "description": "相对 Content/,缺省 \"Fonts\"" }
                    },
                    "required": ["sourcePath"]
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
                "name": "asset_set_description",
                "description": "写入 .meta semantic 段(description/tags/source);缺 .meta 时自动补建。写后自动增量进 RAG 语义索引(词法即时;索引为 hybrid 档时向量同步顶量。响应 indexed/tier 如实标注;索引未建则跳过,需 context_index_build 首建)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "description": { "type": "string", "description": "文字简介" },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "标签列表" },
                        "source": { "type": "string", "enum": ["human", "agent-vision", "agent-facts"], "description": "描述来源(I-7 溯源)" },
                        "model": { "type": "string", "description": "生成模型名(可选)" },
                        "contentHash": { "type": "string", "description": "内容摘要 hash(可选,用于 stale 判定)" }
                    },
                    "required": ["assetPath", "description", "source"]
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
            },
            {
                "name": "sprite_create",
                "description": "创建精灵图集资产(.rxsprite:单张贴图 + 帧 bbox + pivot 级联 + 动画 clip + 可选 animator 状态机);texture 须为已存在贴图 GUID;autoslice=true 时自动切帧(连通域检测,品红族/alpha 背景判定与视口色键同规则)填充 frames",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "精灵名(不含扩展名/路径分隔符)" },
                        "texture": { "type": "string", "description": "图集贴图资产 GUID" },
                        "destFolder": { "type": "string", "description": "目标文件夹(缺省 Sprites)" },
                        "pivot": { "type": "array", "items": { "type": "number" }, "description": "文档级锚点 [x,y](0..1,y 向下;缺省 [0.5,1] 脚底锚;VFX/飞行体用 [0.5,0.5])" },
                        "frames": { "type": "object", "description": "帧名 → { bbox: [x,y,w,h], pivot?: [x,y] }(与 autoslice 二选一)" },
                        "clips": { "type": "object", "description": "clip 名 → { frames: [帧名], fps?: 数, duration?: 总秒, loop?: 布尔, onFinish?: hold|first }" },
                        "animator": { "type": "object", "description": "可选状态机 { defaultState, parameters, states, transitions }" },
                        "autoslice": { "type": "boolean", "description": "自动切帧填充 frames(命名 frame_<i>,行序从上到下、行内从左到右)" },
                        "minArea": { "type": "integer", "description": "autoslice 连通域像素数下限(缺省 16)" }
                    },
                    "required": ["name", "texture"]
                }
            },
            {
                "name": "sprite_get",
                "description": "读 .rxsprite 文档(解析+校验后的规范形态)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPath": { "type": "string" } },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "sprite_set",
                "description": "整文档覆盖写 .rxsprite(先校验:clip 引用帧存在、animator 引用 clip/参数存在,坏文档拒绝;GUID 稳定不变)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "doc": { "type": "object", "description": "完整 .rxsprite JSON 文档(version/texture/pivot/frames/clips/animator)" }
                    },
                    "required": ["assetPath", "doc"]
                }
            },
            {
                "name": "sprite_autoslice",
                "description": "对贴图做自动切帧(不写文件):连通域检测出紧致 bbox 列表,行带分组排序(从上到下、行内从左到右);背景判定 = alpha 过低或品红族(g < 0.5*min(r,b),与视口色键同规则)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string", "description": "贴图资产路径(相对 Content/)" },
                        "minArea": { "type": "integer", "description": "连通域像素数下限(缺省 16;噪点多时调高)" },
                        "alphaThreshold": { "type": "integer", "description": "alpha 背景阈值 0-255(缺省 5)" }
                    },
                    "required": ["assetPath"]
                }
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
        "shader_graph_list_nodes"=>Ok(assetd::shader::catalog()),
        "shader_graph_get"|"shader_graph_save"|"shader_graph_compile"|"shader_material_create"=>{
            let p=lock(proj);let reference=args.get("reference").or_else(||args.get("path")).and_then(Value::as_str).unwrap_or("");
            let result=match name{
                "shader_graph_get"=>assetd::shader::load(&p,reference),
                "shader_graph_compile"=>if let Some(graph)=args.get("graph"){assetd::shader::compile_document(graph)}else{assetd::shader::compile(&p,reference)},
                "shader_graph_save"=>serde_json::from_value::<assetd::shader::GraphDoc>(args["graph"].clone()).map_err(|e|assetd::AssetError::new("SHADER_PARSE",e.to_string())).and_then(|g|assetd::shader::save(&p,reference,&g,args["expectedHash"].as_str())),
                _=>assetd::shader::create_material(&p,reference,args["shaderGraph"].as_str().unwrap_or(""),args.get("params").unwrap_or(&json!({})),args.get("textures").unwrap_or(&json!({}))),
            };Ok(match result{Ok(mut v)=>{if name=="shader_graph_save"{let indexed=index_after_description(&p.root,reference);v["index"]=indexed;}v},Err(e)=>json!({"ok":false,"error":e.code,"message":e.message})})
        },
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
        "font_list" => {
            let include_system = args.get("includeSystem").and_then(Value::as_bool).unwrap_or(true);
            let query = args.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let cjk_only = args.get("cjkOnly").and_then(Value::as_bool).unwrap_or(false);
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(60).clamp(1, 300) as usize;
            let keep = |path: &str, info: &assetd::font::FontInfo| {
                (!cjk_only || info.has_cjk)
                    && (query.is_empty()
                        || info.family.to_lowercase().contains(&query)
                        || path.to_lowercase().contains(&query))
            };
            let p = lock(proj);
            let mut project_fonts = Vec::new();
            for rel in p.scan_content().map_err(|e| err(Value::Null, -32000, &e.message))? {
                let abs = p.content_root().join(&rel);
                if !assetd::font::is_font_path(&abs) {
                    continue;
                }
                let meta_path = meta_path_for(&p.content_root(), &rel);
                let Ok(meta) = MetaDoc::load(&meta_path) else { continue };
                let Ok(info) = assetd::font::probe(&abs) else { continue };
                if keep(&rel, &info) {
                    project_fonts.push(json!({ "assetPath": rel, "guid": meta.guid, "font": info }));
                }
            }
            drop(p);
            let mut system = Vec::new();
            if include_system {
                // 先多取再过滤,过滤后再截到 limit。
                for (path, info) in assetd::font::list_fonts(&assetd::font::system_font_dirs(), 2000) {
                    let ps = path.to_string_lossy().to_string();
                    if keep(&ps, &info) {
                        system.push(json!({ "path": ps, "font": info }));
                        if system.len() >= limit {
                            break;
                        }
                    }
                }
            }
            Ok(json!({ "projectFonts": project_fonts, "systemFonts": system }))
        }
        "font_import" => {
            let source = args.get("sourcePath").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if source.is_empty() {
                return Ok(json!({ "error": "缺参数: sourcePath", "code": "INVALID_PARAMS" }));
            }
            let dest = args.get("destFolder").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or("Fonts");
            let p = lock(proj);
            let src_abs = if std::path::Path::new(&source).is_absolute() {
                std::path::PathBuf::from(&source)
            } else {
                p.root.join(&source)
            };
            if !assetd::font::is_font_path(&src_abs) {
                return Ok(json!({ "error": format!("不是字体文件(.ttf/.otf/.ttc): {source}"), "code": "FONT_INVALID" }));
            }
            let info = match assetd::font::probe(&src_abs) {
                Ok(i) => i,
                Err(e) => return Ok(json!({ "error": e.message, "code": e.code })),
            };
            match import_assets(&p, &[source.clone()], dest, None) {
                Ok(out) => {
                    if let Some(f) = out.failed.first() {
                        return Ok(json!({ "error": f.error, "code": "IMPORT_FAILED" }));
                    }
                    let Some(i) = out.imported.first() else {
                        return Ok(json!({ "error": "导入无结果", "code": "IMPORT_FAILED" }));
                    };
                    let rel = normalize_rel(&i.asset_path).unwrap_or_else(|_| i.asset_path.clone());
                    let meta_path = meta_path_for(&p.content_root(), &rel);
                    let system_dirs = assetd::font::system_font_dirs();
                    let from_system = system_dirs.iter().any(|d| src_abs.starts_with(d));
                    if let Ok(mut meta) = MetaDoc::load(&meta_path) {
                        meta.provenance = Some(assetd::meta::Provenance {
                            origin: "user-import".into(),
                            detail: Some(json!({
                                "source": source,
                                "kind": "font",
                                "family": info.family,
                                "licenseNote": if from_system {
                                    "系统字体:仅供本地制作,发行前须确认字体授权"
                                } else {
                                    "外部字体:发行前须确认字体授权"
                                },
                            })),
                        });
                        let _ = meta.save(&meta_path);
                    }
                    Ok(json!({ "assetPath": i.asset_path, "guid": i.guid, "font": info }))
                }
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
                    let (guid, atype, description, tags) = if meta_path.is_file() {
                        match MetaDoc::load(&meta_path) {
                            Ok(m) => {
                                let description = m.description().unwrap_or("").to_string();
                                let tags = m.tags().to_vec();
                                (m.guid, m.atype, description, tags)
                            }
                            Err(_) => (String::new(), "unknown".into(), String::new(), Vec::new()),
                        }
                    } else {
                        (String::new(), "unknown".into(), String::new(), Vec::new())
                    };
                    let size = p.content_root().join(&rel).metadata().ok().map(|m| m.len()).unwrap_or(0);
                    json!({
                        "path": rel,
                        "guid": guid,
                        "type": atype,
                        "size": size,
                        "description": description,
                        "tags": tags
                    })
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
        "asset_set_description" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let description = args.get("description").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 description"))?;
            let source = args.get("source").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 source"))?;
            let tags: Vec<String> = args.get("tags")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let model = args.get("model").and_then(Value::as_str);
            let content_hash = args.get("contentHash").and_then(Value::as_str);
            let p = lock(proj);
            match assetd::ops::set_description(
                &p, asset_path, description, &tags, source, model, content_hash,
            ) {
                Ok(()) => {
                    // F10-RAG:写后即增量进检索(索引未建/失败如实标注,不阻断写)。
                    let mut resp = json!({ "ok": true });
                    resp.as_object_mut().unwrap().extend(
                        index_after_description(&p.root, asset_path)
                            .as_object()
                            .cloned()
                            .unwrap_or_default(),
                    );
                    Ok(resp)
                }
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
        // ---- sprite_*(F-GAME-4:精灵图集资产面) ----
        "sprite_create" => {
            let name = args.get("name").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 name"))?;
            let texture = args.get("texture").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 texture"))?;
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let pivot: Option<[f32; 2]> = args.get("pivot")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let clips = args.get("clips").and_then(|v| v.as_object());
            let animator = args.get("animator");
            let autoslice = args.get("autoslice").and_then(Value::as_bool).unwrap_or(false);
            let p = lock(proj);
            // frames 来源:显式传入 > autoslice 检出 > 空(编辑器后续补)。
            let frames_owned: Option<serde_json::Map<String, Value>> = if autoslice {
                let mut opts = assetd::sprite::SliceOptions::default();
                if let Some(m) = args.get("minArea").and_then(Value::as_u64) {
                    opts.min_area = m as u32;
                }
                // GUID → 相对路径(autoslice 面接受路径;此处按 GUID 反查)。
                let rel = p.scan_content()
                    .ok()
                    .and_then(|rels| rels.into_iter().find(|r| {
                        let mp = meta_path_for(&p.content_root(), r);
                        mp.is_file() && MetaDoc::load(&mp).map(|m| m.guid == texture).unwrap_or(false)
                    }));
                match rel {
                    Some(rel) => match assetd::sprite::autoslice_texture(&p, &rel, opts) {
                        Ok(out) => Some(assetd::sprite::frames_from_boxes("frame", &out.boxes)),
                        Err(e) => return Ok(json!({ "error": e.code, "message": e.message })),
                    },
                    None => return Ok(json!({ "error": "UNKNOWN_GUID", "message": format!("texture GUID 不存在: {texture}") })),
                }
            } else {
                args.get("frames").and_then(|v| v.as_object()).cloned()
            };
            match assetd::sprite::create_sprite(
                &p, dest, name, texture, pivot, frames_owned.as_ref(), clips, animator,
            ) {
                Ok(c) => Ok(json!({
                    "assetPath": c.asset_path, "guid": c.guid,
                    "textureGuid": c.texture_guid,
                    "frameCount": c.frame_count, "clipCount": c.clip_count
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "sprite_get" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let p = lock(proj);
            let rel = normalize_rel(asset_path).map_err(|e| err(Value::Null, -32602, &e.message))?;
            let abs = p.content_root().join(&rel);
            match assetd::sprite::load_rxsprite(&abs) {
                Ok(doc) => {
                    let meta_path = meta_path_for(&p.content_root(), &rel);
                    let guid = MetaDoc::load(&meta_path).map(|m| m.guid).unwrap_or_default();
                    Ok(json!({ "guid": guid, "doc": serde_json::to_value(&doc).unwrap_or(Value::Null) }))
                }
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "sprite_set" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let doc = args.get("doc")
                .filter(|v| v.is_object())
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 doc 对象"))?;
            let p = lock(proj);
            match assetd::sprite::write_sprite_doc(&p, asset_path, doc) {
                Ok(parsed) => Ok(json!({
                    "ok": true,
                    "frameCount": parsed.frames.len(),
                    "clipCount": parsed.clips.len()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "sprite_autoslice" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let mut opts = assetd::sprite::SliceOptions::default();
            if let Some(m) = args.get("minArea").and_then(Value::as_u64) {
                opts.min_area = m as u32;
            }
            if let Some(a) = args.get("alphaThreshold").and_then(Value::as_u64) {
                opts.alpha_threshold = a.min(255) as u8;
            }
            let p = lock(proj);
            match assetd::sprite::autoslice_texture(&p, asset_path, opts) {
                Ok(out) => Ok(json!({
                    "width": out.width, "height": out.height,
                    "boxes": out.boxes.iter().map(|b| json!([b[0], b[1], b[2], b[3]])).collect::<Vec<_>>()
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
