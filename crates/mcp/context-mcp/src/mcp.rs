//! MCP stdio 服务:initialize / tools/list / tools/call。复刻 asset-pipeline-mcp 骨架。
//! 六工具:context_index_build(写缓存) / context_search / context_get /
//! context_index_status / asset_describe_batch / asset_set_description(写 .meta)。
//! embedding 经 gend::embed::resolve_embedder();未配置 = 词法档显式标注(I-5)。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use assetd::meta::MetaDoc;
use assetd::project::ForgeProject;
use assetd::meta_path_for;
use forge_index::build::build_index;
use forge_index::doc::{DocKind, IndexDoc};
use forge_index::extract::ExtractOptions;
use forge_index::search::{get_doc, search, SearchFilter};
use forge_index::store::{self, IndexPaths};
use forge_index::vector::Embedder;
use serde_json::{json, Value};

/// gend::embed::RemoteEmbedder → forge_index Embedder 适配。
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

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "context_index_build",
                "description": "重建语义索引(资产/场景实体/节点图/rx 符号/工作区文档 → BM25 词法索引;已配 embedding 渠道则增量向量化,仅重嵌变更项)。返回 {docs, embedded, reused, tier, embedError?, durationMs}",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "force": { "type": "boolean", "description": "true = 忽略增量,全量重嵌(缺省 false)" }
                    }
                }
            },
            {
                "name": "context_search",
                "description": "语义检索工作区素材(资产/实体/节点图/代码符号/文档)。返回 tier=lexical|hybrid(词法档如实标注,不假装语义检索)+ 命中列表(含描述/事实摘要)。改场景/找素材前先用本工具定位,不要盲目全量 asset_list",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "自然语言查询(中英混合皆可)" },
                        "topK": { "type": "integer", "description": "返回条数(缺省 8,上限 50)" },
                        "kinds": { "type": "array", "items": { "type": "string", "enum": ["asset", "entity", "graph", "symbol", "doc"] }, "description": "限定文档种类(缺省全部)" },
                        "types": { "type": "array", "items": { "type": "string" }, "description": "限定资产类型(mesh/texture/material/scene/script;仅对 kind=asset 生效)" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "context_get",
                "description": "按索引文档 id 或资产 GUID 取完整文档(全文 facts + refs)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "id": { "type": "string", "description": "IndexDoc id(如 asset:<guid> / entity:<scene>#<id>)或资产 GUID" } },
                    "required": ["id"]
                }
            },
            {
                "name": "context_index_status",
                "description": "索引健康度:{built, builtAt, docCount, tier, embedModel?, vectorCount, embeddingConfigured, assetsMissingDescription}",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "asset_describe_batch",
                "description": "列出待回填文字简介的资产(缺描述/事实已变 stale)+ 每项客观事实(尺寸/顶点数/引用)。本工具不调 LLM:由 agent 自己撰写描述后逐个调 asset_set_description 回写(贴图可先 asset_thumbnail 看图;contentHash 原样回传 factsHash)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "mode": { "type": "string", "enum": ["missing", "stale", "all"], "description": "missing=缺描述(缺省) / stale=事实已变 / all=全部" },
                        "limit": { "type": "integer", "description": "返回上限(缺省 20)" }
                    }
                }
            },
            {
                "name": "asset_set_description",
                "description": "写入资产 .meta semantic 段(description/tags/source;I-7 溯源:source=human|agent-vision|agent-facts;看过缩略图用 agent-vision,仅凭事实用 agent-facts)。写后自动增量进检索(词法即时;索引为 hybrid 档时向量同步顶量。响应 indexed/tier 如实标注;索引未建则跳过,需 context_index_build 首建)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "description": { "type": "string", "description": "文字简介(一两句,说清是什么/什么风格/适合什么场合)" },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "source": { "type": "string", "enum": ["human", "agent-vision", "agent-facts"] },
                        "model": { "type": "string", "description": "生成模型名(agent 写入时带上)" },
                        "contentHash": { "type": "string", "description": "asset_describe_batch 返回的 factsHash(stale 判定锚)" }
                    },
                    "required": ["assetPath", "description", "source"]
                }
            }
        ]
    })
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}
fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_wrap(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error {
        out["isError"] = json!(true);
    }
    out
}

/// 检索命中摘要长度(回给 LLM 的 snippet,控 token)。
const SNIPPET_MAX: usize = 240;

fn snippet(facts: &str) -> String {
    if facts.chars().count() > SNIPPET_MAX {
        let s: String = facts.chars().take(SNIPPET_MAX).collect();
        format!("{s}…")
    } else {
        facts.to_string()
    }
}

fn doc_json(d: &IndexDoc, full: bool) -> Value {
    json!({
        "id": d.id,
        "kind": d.kind.as_str(),
        "title": d.title,
        "path": d.path,
        "guid": d.guid,
        "type": d.atype,
        "description": d.description,
        "tags": d.tags,
        "facts": if full { d.facts.clone() } else { snippet(&d.facts) },
        "refs": d.refs,
    })
}

fn extract_opts(project_root: &Path, docs_root: &Path) -> ExtractOptions {
    ExtractOptions {
        project_root: project_root.to_path_buf(),
        docs_root: Some(docs_root.to_path_buf()),
    }
}

/// 文档腿根:--docs 显式指定优先,否则退回 workspace 根(单项目装机的原行为)。
fn resolve_docs_root(docs_root: Option<&Path>) -> PathBuf {
    docs_root
        .map(Path::to_path_buf)
        .unwrap_or_else(gend::config::workspace_root)
}

fn call_tool(project_root: &Path, docs_root: &Path, params: &Value) -> Result<Value, Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺工具名"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Err(err(Value::Null, -32602, "invalid params: arguments 须为对象"));
    }
    let paths = IndexPaths::for_project(project_root);

    match name {
        "context_index_build" => {
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let embedder = resolve_embedder();
            let opts = extract_opts(project_root, docs_root);
            match build_index(&opts, embedder.as_ref().map(|e| e as &dyn Embedder), force) {
                Ok(o) => Ok(json!({
                    "docs": o.docs,
                    "embedded": o.embedded,
                    "reused": o.reused,
                    "tier": o.tier,
                    "embedError": o.embed_error,
                    "durationMs": o.duration_ms,
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "context_search" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 query"))?;
            let top_k = args
                .get("topK")
                .and_then(Value::as_u64)
                .map(|k| (k as usize).clamp(1, 50))
                .unwrap_or(8);
            let kinds: Vec<DocKind> = args
                .get("kinds")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .filter_map(DocKind::parse)
                        .collect()
                })
                .unwrap_or_default();
            let atypes: Vec<String> = args
                .get("types")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let filter = SearchFilter { kinds, atypes };
            let embedder = resolve_embedder();
            match search(
                &paths,
                query,
                top_k,
                &filter,
                embedder.as_ref().map(|e| e as &dyn Embedder),
            ) {
                Ok(out) => Ok(json!({
                    "tier": out.tier,
                    "note": out.note,
                    "hits": out.hits.iter().map(|h| {
                        let mut v = doc_json(&h.doc, false);
                        v["score"] = json!(h.score);
                        v["lexicalRank"] = json!(h.lexical_rank);
                        v["vectorRank"] = json!(h.vector_rank);
                        v
                    }).collect::<Vec<_>>(),
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "context_get" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 id"))?;
            match get_doc(&paths, id) {
                Ok(Some(d)) => Ok(json!({ "doc": doc_json(&d, true) })),
                Ok(None) => Ok(json!({ "error": "DOC_NOT_FOUND", "message": format!("索引无此文档: {id}") })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "context_index_status" => Ok(index_status(project_root, &paths)),
        "asset_describe_batch" => {
            let mode = args.get("mode").and_then(Value::as_str).unwrap_or("missing");
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|l| (l as usize).clamp(1, 200))
                .unwrap_or(20);
            describe_batch(project_root, mode, limit)
        }
        "asset_set_description" => {
            let asset_path = args
                .get("assetPath")
                .and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let description = args
                .get("description")
                .and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 description"))?;
            let source = args
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 source"))?;
            let tags: Vec<String> = args
                .get("tags")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let model = args.get("model").and_then(Value::as_str);
            let content_hash = args.get("contentHash").and_then(Value::as_str);
            let project = load_project(project_root);
            match assetd::ops::set_description(
                &project, asset_path, description, &tags, source, model, content_hash,
            ) {
                Ok(()) => {
                    // F10-RAG:写后即增量进检索(词法+向量;未建索引/失败如实标注,不阻断写)。
                    let mut resp = json!({ "ok": true, "assetPath": asset_path });
                    match assetd::normalize_rel(asset_path) {
                        Ok(rel) => {
                            let embedder = resolve_embedder();
                            match forge_index::build::upsert_asset_docs(
                                project_root,
                                &rel,
                                embedder.as_ref().map(|e| e as &dyn Embedder),
                            ) {
                                Ok(o) => {
                                    resp["indexed"] = json!(o.indexed);
                                    resp["indexedDocs"] = json!(o.docs);
                                    resp["tier"] = json!(o.tier);
                                    if let Some(e) = o.embed_error {
                                        resp["embedError"] = json!(e);
                                    }
                                }
                                Err(e) => {
                                    resp["indexed"] = json!(false);
                                    resp["indexError"] =
                                        json!(format!("[{}] {}", e.code, e.message));
                                }
                            }
                        }
                        Err(e) => {
                            resp["indexed"] = json!(false);
                            resp["indexError"] = json!(format!("[{}] {}", e.code, e.message));
                        }
                    }
                    Ok(resp)
                }
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        _ => Err(err(Value::Null, -32602, &format!("未知工具:{name}"))),
    }
}

fn load_project(root: &Path) -> ForgeProject {
    ForgeProject::load(root).unwrap_or_else(|e| {
        eprintln!("[context-mcp] forge.toml 加载失败: {e};用缺省配置");
        ForgeProject::with_defaults(root.to_path_buf())
    })
}

/// 资产/文档比索引清单新 → stale(Studio 自动 rebuild,不算用户写审批)。
fn index_is_stale(project_root: &Path, paths: &IndexPaths) -> bool {
    let Ok(meta) = paths.manifest().metadata() else {
        return true;
    };
    let Ok(built) = meta.modified() else {
        return true;
    };
    let project = load_project(project_root);
    content_newer_than(&project.content_root(), built) || docs_newer_than(project_root, built)
}

const STALE_SKIP: &[&str] = &[
    ".git",
    ".forge",
    ".playwright-cli",
    "target",
    "node_modules",
    "dist",
    "out",
    "build",
    ".cache",
];

fn content_newer_than(root: &Path, built: std::time::SystemTime) -> bool {
    let Ok(rd) = std::fs::read_dir(root) else {
        return false;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if let Ok(m) = p.metadata() {
            if let Ok(t) = m.modified() {
                if t > built {
                    return true;
                }
            }
        }
        if p.is_dir() && content_newer_than(&p, built) {
            return true;
        }
    }
    false
}

fn docs_newer_than(root: &Path, built: std::time::SystemTime) -> bool {
    let Ok(rd) = std::fs::read_dir(root) else {
        return false;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        let name = ent.file_name();
        let name_s = name.to_string_lossy();
        if STALE_SKIP.iter().any(|s| *s == name_s) {
            continue;
        }
        if p.is_dir() {
            if docs_newer_than(&p, built) {
                return true;
            }
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "md" | "txt") {
            continue;
        }
        if let Ok(t) = p.metadata().and_then(|m| m.modified()) {
            if t > built {
                return true;
            }
        }
    }
    false
}

/// 索引健康度聚合。
fn index_status(project_root: &Path, paths: &IndexPaths) -> Value {
    let manifest = store::load_manifest(paths).ok().flatten();
    let vector_count = store::load_vectors(paths).map(|v| v.len()).unwrap_or(0);
    let embedding = gend::embed::embedding_status();
    // 缺描述资产计数(直接扫 .meta,不依赖索引新旧)。
    let project = load_project(project_root);
    let mut missing_desc = 0usize;
    if let Ok(rels) = project.scan_content() {
        for rel in rels {
            let mp = meta_path_for(&project.content_root(), &rel);
            let has_desc = mp.is_file()
                && MetaDoc::load(&mp)
                    .ok()
                    .and_then(|m| m.description().map(|d| !d.is_empty()))
                    .unwrap_or(false);
            if !has_desc {
                missing_desc += 1;
            }
        }
    }
    match manifest {
        Some(m) => json!({
            "built": true,
            "builtAt": m.built_at,
            "docCount": m.doc_count,
            "tier": m.tier,
            "embedModel": m.embed_model,
            "vectorCount": vector_count,
            "embeddingConfigured": embedding.configured,
            "assetsMissingDescription": missing_desc,
            "stale": index_is_stale(project_root, paths),
        }),
        None => json!({
            "built": false,
            "vectorCount": 0,
            "embeddingConfigured": embedding.configured,
            "assetsMissingDescription": missing_desc,
        }),
    }
}

/// 待回填清单:抽取资产事实 + 对照 .meta semantic 判定 missing/stale。
fn describe_batch(project_root: &Path, mode: &str, limit: usize) -> Result<Value, Value> {
    let opts = ExtractOptions {
        project_root: project_root.to_path_buf(),
        docs_root: None,
    };
    let docs = forge_index::extract::extract_all(&opts)
        .map_err(|e| err(Value::Null, -32000, &e.message))?;
    let project = load_project(project_root);
    let content_root = project.content_root();

    let mut items = Vec::new();
    let mut total_missing = 0usize;
    let mut total_stale = 0usize;
    for d in docs.iter().filter(|d| d.kind == DocKind::Asset) {
        // .meta 排除项:meta 文件自身不是资产(extract 已滤);描述与锚 hash 取自 meta。
        let mp = meta_path_for(&content_root, &d.path);
        let semantic = if mp.is_file() {
            MetaDoc::load(&mp).ok().and_then(|m| m.semantic)
        } else {
            None
        };
        let facts_hash = forge_index::sha256_hex(d.facts.as_bytes());
        let missing = d.description.is_empty();
        let stale = !missing
            && semantic
                .as_ref()
                .and_then(|s| s.content_hash.as_deref())
                .map(|h| h != facts_hash)
                .unwrap_or(false);
        if missing {
            total_missing += 1;
        }
        if stale {
            total_stale += 1;
        }
        let selected = match mode {
            "missing" => missing,
            "stale" => stale,
            "all" => true,
            _ => missing,
        };
        if selected && items.len() < limit {
            items.push(json!({
                "assetPath": d.path,
                "guid": d.guid,
                "type": d.atype,
                "description": d.description,
                "tags": d.tags,
                "facts": d.facts,
                "factsHash": facts_hash,
                "missing": missing,
                "stale": stale,
            }));
        }
    }
    Ok(json!({
        "items": items,
        "totalMissing": total_missing,
        "totalStale": total_stale,
        "hint": "对 texture 可先调 mcp__asset-pipeline__asset_thumbnail 看图(source=agent-vision);\
仅凭事实撰写用 source=agent-facts。写回调 asset_set_description,contentHash 原样回传 factsHash;\
写入即自动增量进检索,无需再调 context_index_build。",
    }))
}

pub fn serve_stdio(project_root: PathBuf, docs_root: Option<PathBuf>) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
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
            "initialize" => id.map(|i| {
                ok(
                    i,
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "context-mcp", "version": env!("CARGO_PKG_VERSION") }
                    }),
                )
            }),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => id.map(|i| ok(i, json!({}))),
            "tools/list" => id.map(|i| ok(i, tool_list())),
            "tools/call" => id.map(|i| {
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                let docs = resolve_docs_root(docs_root.as_deref());
                match call_tool(&project_root, &docs, &params) {
                    Ok(result) => {
                        let is_error = result.get("error").is_some();
                        ok(i, tool_wrap(&result, is_error))
                    }
                    Err(mut e) => {
                        e["id"] = i.clone();
                        e
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "context-mcp-{tag}-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        let scripts = dir.join("Content").join("Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("door.rx"),
            "// 开门逻辑\n#[export(c)]\npub fn door_open_angle() -> f32 { 90.0 }\n",
        )
        .unwrap();
        dir
    }

    fn call(root: &Path, name: &str, args: Value) -> Value {
        let params = json!({ "name": name, "arguments": args });
        let docs = resolve_docs_root(None);
        call_tool(root, &docs, &params).expect("工具调用不应协议级失败")
    }

    #[test]
    fn full_tool_cycle_lexical_tier() {
        let dir = setup_project("cycle");
        // 未构建 → search 显式 INDEX_NOT_BUILT。
        let r = call(&dir, "context_search", json!({ "query": "开门" }));
        assert_eq!(r["error"], "INDEX_NOT_BUILT", "{r}");
        // 构建(无 embedding 配置 → lexical;注:若宿主 data/ 配置了 embedding 渠道,
        // 本测试环境不设 FORGE_GEN_DATA_DIR 时可能拿到真实配置——用隔离目录规避)。
        std::env::set_var("FORGE_GEN_DATA_DIR", dir.join("data-isolated").to_string_lossy().to_string());
        let r = call(&dir, "context_index_build", json!({}));
        assert_eq!(r["tier"], "lexical", "{r}");
        assert!(r["docs"].as_u64().unwrap() > 0, "{r}");
        // 检索命中 + 档位标注。
        let r = call(&dir, "context_search", json!({ "query": "开门角度" }));
        assert_eq!(r["tier"], "lexical", "{r}");
        assert!(r["note"].as_str().is_some(), "词法档须带 note:{r}");
        let hits = r["hits"].as_array().unwrap();
        assert!(!hits.is_empty(), "{r}");
        assert!(hits[0]["path"].as_str().unwrap().contains("door"), "{r}");
        // context_get 按 id。
        let id = hits[0]["id"].as_str().unwrap();
        let r = call(&dir, "context_get", json!({ "id": id }));
        assert_eq!(r["doc"]["id"], id, "{r}");
        // status。
        let r = call(&dir, "context_index_status", json!({}));
        assert_eq!(r["built"], true, "{r}");
        assert_eq!(r["tier"], "lexical", "{r}");
        // describe_batch:door.rx 缺描述 → missing 命中。
        let r = call(&dir, "asset_describe_batch", json!({}));
        let items = r["items"].as_array().unwrap();
        assert!(!items.is_empty(), "{r}");
        let item = &items[0];
        assert_eq!(item["missing"], true);
        let facts_hash = item["factsHash"].as_str().unwrap().to_string();
        let asset_path = item["assetPath"].as_str().unwrap().to_string();
        // 写描述(agent-facts 档)。
        let r = call(
            &dir,
            "asset_set_description",
            json!({
                "assetPath": asset_path,
                "description": "开门逻辑脚本,输出门的开启角度",
                "tags": ["逻辑", "门"],
                "source": "agent-facts",
                "model": "test-model",
                "contentHash": facts_hash,
            }),
        );
        assert_eq!(r["ok"], true, "{r}");
        // F10-RAG:写后自动增量进检索(indexed=true),无需重建即命中描述文本。
        assert_eq!(r["indexed"], true, "{r}");
        let r = call(&dir, "context_search", json!({ "query": "开启角度脚本" }));
        let hits = r["hits"].as_array().unwrap();
        assert!(!hits.is_empty(), "写后未重建须已可检索:{r}");
        // 重建后检索仍命中描述文本(全量路径与增量一致)。
        let r = call(&dir, "context_index_build", json!({}));
        assert!(r["docs"].as_u64().unwrap() > 0);
        let r = call(&dir, "context_search", json!({ "query": "开启角度脚本" }));
        let hits = r["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        // describe_batch:已写描述 → missing 清零。
        let r = call(&dir, "asset_describe_batch", json!({}));
        assert_eq!(r["totalMissing"].as_u64().unwrap(), 0, "{r}");
        // 新文档比清单新 → stale。
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("NEW_DOC.md"), "# 新文档\n后写的策划。\n").unwrap();
        let r = call(&dir, "context_index_status", json!({}));
        assert_eq!(r["stale"], true, "{r}");
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&dir).ok();
    }
}
