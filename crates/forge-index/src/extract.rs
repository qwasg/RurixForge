//! 五路抽取器:资产 / 场景实体 / 节点图 / rx 符号 / md 文档 → IndexDoc。
//! 全部输出按 id 排序(scan_content 依赖文件系统序,此处收敛为确定性)。
//! 诚实降级:mesh 未构建(MESH_NOT_BUILT)→ facts 如实标注,不静默触发重建。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use assetd::meta::MetaDoc;
use assetd::project::ForgeProject;
use assetd::{meta_path_for, AssetType};
use forge_logic::graph::{GraphDoc, ValueSource};
use forge_logic::registry as node_registry;
use forge_logic::rxexport::scan_export_c_fns;
use forge_scene::{classify, Scene};

use crate::doc::{DocKind, IndexDoc};
use crate::Result;

/// 抽取配置。docs_root = 文档腿根(递归 *.md / *.txt);None = 跳过文档腿。
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    pub project_root: PathBuf,
    pub docs_root: Option<PathBuf>,
}

/// 单文档 facts 上限(控 token;超出如实截断标注)。
const FACTS_MAX_CHARS: usize = 1500;
/// md 单文件分块上限(防爆炸)。
const DOC_CHUNKS_PER_FILE: usize = 40;

/// 全量抽取:五路合并,按 id 排序。
pub fn extract_all(opts: &ExtractOptions) -> Result<Vec<IndexDoc>> {
    let project = ForgeProject::load(&opts.project_root)
        .unwrap_or_else(|_| ForgeProject::with_defaults(opts.project_root.clone()));
    let mut rels = project.scan_content().unwrap_or_default();
    rels.sort();

    // guid → 路径映射(场景组件 props 里的 GUID 解析为人类可读路径)。
    let mut guid_to_path: HashMap<String, String> = HashMap::new();
    let mut metas: HashMap<String, MetaDoc> = HashMap::new();
    for rel in &rels {
        let mp = meta_path_for(&project.content_root(), rel);
        if mp.is_file() {
            if let Ok(m) = MetaDoc::load(&mp) {
                guid_to_path.insert(m.guid.clone(), rel.clone());
                metas.insert(rel.clone(), m);
            }
        }
    }

    let mut docs: Vec<IndexDoc> = Vec::new();
    docs.extend(extract_assets(&project, &rels, &metas, &guid_to_path));
    docs.extend(extract_scene_entities(&project, &rels, &guid_to_path));
    docs.extend(extract_graphs(&project, &rels, &metas));
    docs.extend(extract_rx_symbols(&project, &rels, &metas));
    if let Some(root) = &opts.docs_root {
        docs.extend(extract_markdown_docs(root));
    }

    for d in &mut docs {
        d.finalize_hash();
    }
    docs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(docs)
}

fn truncate_facts(mut s: String) -> String {
    if s.chars().count() > FACTS_MAX_CHARS {
        s = s.chars().take(FACTS_MAX_CHARS).collect();
        s.push_str("…(截断)");
    }
    s
}

fn file_name_of(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}

// ---------- 1. 资产 ----------

fn extract_assets(
    project: &ForgeProject,
    rels: &[String],
    metas: &HashMap<String, MetaDoc>,
    guid_to_path: &HashMap<String, String>,
) -> Vec<IndexDoc> {
    // 引用图出边(读缓存 refgraph.json;缺失 = 空图,不在抽取期重建 O(n²))。
    let refgraph = assetd::refs::RefGraph::load(project).unwrap_or_default();

    rels.iter()
        .map(|rel| asset_doc(project, rel, metas, guid_to_path, &refgraph))
        .collect()
}

/// 单资产文档抽取(extract_assets 循环体;upsert 单资产增量时复用,保 hash 与全量一致)。
fn asset_doc(
    project: &ForgeProject,
    rel: &str,
    metas: &HashMap<String, MetaDoc>,
    guid_to_path: &HashMap<String, String>,
    refgraph: &assetd::refs::RefGraph,
) -> IndexDoc {
    {
        let meta = metas.get(rel);
        let ext = Path::new(rel)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let atype = meta
            .map(|m| m.atype.clone())
            .or_else(|| AssetType::from_extension(&ext).map(|(t, _)| t.as_str().to_string()))
            .unwrap_or_else(|| "unknown".into());

        let abs = project.content_root().join(rel);
        let size = abs.metadata().ok().map(|m| m.len()).unwrap_or(0);
        let mut facts = format!("资产类型:{atype};大小:{size}B");

        match (atype.as_str(), meta) {
            ("texture", _) => {
                match assetd::texture::decode_size(&abs) {
                    Ok((w, h)) => facts.push_str(&format!(";尺寸:{w}x{h}")),
                    Err(_) => facts.push_str(";尺寸:解码失败"),
                }
            }
            ("mesh", _) => match assetd::inspect::inspect_mesh(project, rel) {
                Ok(r) => {
                    facts.push_str(&format!(
                        ";顶点:{};三角形:{};LOD:{}",
                        r.vertices, r.triangles, r.lods
                    ));
                    if let Some((mn, mx)) = r.bounds {
                        facts.push_str(&format!(
                            ";包围盒:[{:.2},{:.2},{:.2}]~[{:.2},{:.2},{:.2}]",
                            mn[0], mn[1], mn[2], mx[0], mx[1], mx[2]
                        ));
                    }
                    if !r.materials.is_empty() {
                        facts.push_str(&format!(";材质槽:{}", r.materials.join(",")));
                    }
                }
                Err(e) => facts.push_str(&format!(";网格统计不可用({})", e.code)),
            },
            ("material", _) => {
                if let Ok(text) = std::fs::read_to_string(&abs) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(shader) = v.get("shader").and_then(|s| s.as_str()) {
                            facts.push_str(&format!(";shader:{shader}"));
                        }
                        if let Some(params) = v.get("params").and_then(|p| p.as_object()) {
                            let keys: Vec<&str> = params.keys().map(String::as_str).collect();
                            if !keys.is_empty() {
                                facts.push_str(&format!(";参数:{}", keys.join(",")));
                            }
                        }
                        if let Some(tex) = v.get("textures").and_then(|t| t.as_object()) {
                            for (slot, g) in tex {
                                let gp = g
                                    .as_str()
                                    .and_then(|g| guid_to_path.get(g))
                                    .map(String::as_str)
                                    .unwrap_or_else(|| g.as_str().unwrap_or("?"));
                                facts.push_str(&format!(";贴图槽 {slot}={gp}"));
                            }
                        }
                    }
                }
            }
            ("scene", _) => {
                if let Ok(scene) = Scene::load(&abs) {
                    let names: Vec<&str> = scene
                        .entities
                        .iter()
                        .take(20)
                        .map(|e| e.name.as_str())
                        .collect();
                    facts.push_str(&format!(
                        ";场景名:{};实体数:{};实体:{}",
                        scene.name,
                        scene.entities.len(),
                        names.join(",")
                    ));
                }
            }
            ("script", _) if ext == "rx" => {
                if let Ok(text) = std::fs::read_to_string(&abs) {
                    let fns = scan_export_c_fns(&text);
                    if !fns.is_empty() {
                        let names: Vec<&str> = fns.iter().map(|f| f.name.as_str()).collect();
                        facts.push_str(&format!(";导出函数:{}", names.join(",")));
                    }
                }
            }
            ("script", _) if ext == "rxgraph" => {
                if let Ok(text) = std::fs::read_to_string(&abs) {
                    if let Ok(g) = GraphDoc::from_json(&text) {
                        facts.push_str(&format!(";节点图:{};节点数:{}", g.name, g.nodes.len()));
                    }
                }
            }
            // F-GAME-4:.rxsprite 帧/clip 统计 + 贴图引用进检索面。
            ("sprite", _) => {
                if let Ok(doc) = assetd::sprite::load_rxsprite(&abs) {
                    let clips: Vec<&str> = doc.clips.keys().map(String::as_str).collect();
                    let tex = guid_to_path
                        .get(&doc.texture)
                        .map(String::as_str)
                        .unwrap_or(doc.texture.as_str());
                    facts.push_str(&format!(
                        ";帧数:{};clip:{};贴图:{}{}",
                        doc.frames.len(),
                        if clips.is_empty() { "无".to_string() } else { clips.join(",") },
                        tex,
                        if doc.animator.is_some() { ";含animator" } else { "" },
                    ));
                }
            }
            _ => {}
        }

        let (guid, description, tags) = match meta {
            Some(m) => (
                Some(m.guid.clone()),
                m.description().unwrap_or("").to_string(),
                m.tags().to_vec(),
            ),
            None => (None, String::new(), Vec::new()),
        };
        let refs: Vec<String> = guid
            .as_deref()
            .map(|g| {
                refgraph
                    .refs(g)
                    .iter()
                    .map(|e| e.to_guid.clone())
                    .collect()
            })
            .unwrap_or_default();

        let id = match &guid {
            Some(g) => format!("asset:{g}"),
            None => format!("asset:path:{rel}"),
        };
        IndexDoc {
            id,
            kind: DocKind::Asset,
            title: file_name_of(rel),
            path: rel.to_string(),
            guid,
            atype,
            description,
            facts: truncate_facts(facts),
            tags,
            refs,
            content_hash: String::new(),
        }
    }
}

// ---------- 2. 场景实体 ----------

fn extract_scene_entities(
    project: &ForgeProject,
    rels: &[String],
    guid_to_path: &HashMap<String, String>,
) -> Vec<IndexDoc> {
    let mut out = Vec::new();
    for rel in rels.iter().filter(|r| r.ends_with(".rxscene")) {
        let abs = project.content_root().join(rel);
        let Ok(scene) = Scene::load(&abs) else { continue };
        for e in &scene.entities {
            let cat = classify(e);
            let t = &e.transform.translation;
            let mut facts = format!(
                "场景:{};分类:{cat};位置:[{:.1},{:.1},{:.1}]",
                scene.name, t[0], t[1], t[2]
            );
            let mut refs = Vec::new();
            for c in &e.components {
                if !c.enabled {
                    continue;
                }
                let detail = component_brief(&c.ctype, &c.props, guid_to_path, &mut refs);
                facts.push_str(&format!(";{}{}", c.ctype, detail));
            }
            out.push(IndexDoc {
                id: format!("entity:{rel}#{}", e.id),
                kind: DocKind::Entity,
                title: e.name.clone(),
                path: rel.clone(),
                guid: None,
                atype: String::new(),
                description: String::new(),
                facts: truncate_facts(facts),
                tags: vec![cat.to_string()],
                refs,
                content_hash: String::new(),
            });
        }
    }
    out
}

/// 组件要点(引用 GUID 解析为路径并记入 refs)。
fn component_brief(
    ctype: &str,
    props: &serde_json::Value,
    guid_to_path: &HashMap<String, String>,
    refs: &mut Vec<String>,
) -> String {
    let get = |k: &str| props.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let resolve = |v: &str, refs: &mut Vec<String>| -> String {
        if let Some(p) = guid_to_path.get(v) {
            refs.push(v.to_string());
            p.clone()
        } else {
            v.to_string()
        }
    };
    match ctype {
        "MeshRenderer" => {
            let mesh = resolve(get("mesh"), refs);
            let mat = get("material");
            if mat.is_empty() {
                format!("(mesh={mesh})")
            } else {
                format!("(mesh={mesh},material={})", resolve(mat, refs))
            }
        }
        "Tag" => format!("(tag={})", get("tag")),
        "Script" => {
            let module = get("module");
            let graph = get("graphRef");
            match (module.is_empty(), graph.is_empty()) {
                (false, true) => format!("(module={module})"),
                (true, false) => format!("(graph={graph})"),
                (false, false) => format!("(module={module},graph={graph})"),
                (true, true) => String::new(),
            }
        }
        "RigidBody" => format!("(kind={})", get("kind")),
        // F-GAME-3:Sprite 贴图 GUID 解析入 refs(asset_delete 引用阻断覆盖 2D 精灵);
        // F-GAME-4:sprite(.rxsprite GUID)引用亦入 refs,clip 名入 facts。
        "Sprite" => {
            let order = props
                .get("sortingOrder")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let sprite_ref = get("sprite");
            if !sprite_ref.is_empty() {
                let sp = resolve(sprite_ref, refs);
                let clip = get("clip");
                if clip.is_empty() {
                    format!("(sprite={sp},order={order})")
                } else {
                    format!("(sprite={sp},clip={clip},order={order})")
                }
            } else {
                let tex = resolve(get("texture"), refs);
                format!("(texture={tex},order={order})")
            }
        }
        "Light" => {
            let intensity = props
                .get("intensity")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            format!("(kind={},intensity={intensity})", get("kind"))
        }
        "Camera" => {
            let fov = props.get("fov").and_then(|v| v.as_f64()).unwrap_or(0.0);
            // F-GAME-3:投影方式进索引(正交相机可被 context_search 检索)。
            let proj = props.get("projection").and_then(|v| v.as_str()).unwrap_or("perspective");
            format!("(fov={fov},projection={proj})")
        }
        "Trigger" => {
            let ext = props
                .get("extents")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .map(|x| format!("{:.1}", x.as_f64().unwrap_or(0.0)))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            format!("(extents=[{ext}])")
        }
        "Category" => format!("(category={})", get("category")),
        _ => String::new(),
    }
}

// ---------- 3. 节点图 ----------

/// 文件的 .meta semantic(description, tags);图级/符号级文档继承所属文件描述,
/// 使 kinds 过滤检索(kind=graph/symbol)也能命中人写/agent 写的中文简介。
fn semantic_of<'a>(
    metas: &'a HashMap<String, MetaDoc>,
    rel: &str,
) -> (String, Vec<String>) {
    metas
        .get(rel)
        .map(|m| {
            (
                m.description().unwrap_or("").to_string(),
                m.tags().to_vec(),
            )
        })
        .unwrap_or_default()
}

fn extract_graphs(
    project: &ForgeProject,
    rels: &[String],
    metas: &HashMap<String, MetaDoc>,
) -> Vec<IndexDoc> {
    rels.iter()
        .filter(|r| r.ends_with(".rxgraph"))
        .filter_map(|rel| graph_doc(project, rel, metas))
        .collect()
}

/// 单图文档(extract_graphs 循环体;upsert 复用)。文件不可读/解析失败 → None。
fn graph_doc(
    project: &ForgeProject,
    rel: &str,
    metas: &HashMap<String, MetaDoc>,
) -> Option<IndexDoc> {
    let abs = project.content_root().join(rel);
    let text = std::fs::read_to_string(&abs).ok()?;
    let g = GraphDoc::from_json(&text).ok()?;
    let (description, tags) = semantic_of(metas, rel);

    let mut facts = format!("节点图 {}({} 节点,{} 执行边)", g.name, g.nodes.len(), g.edges.len());
    if !g.exposed_props.is_empty() {
        let props: Vec<String> = g
            .exposed_props
            .iter()
            .map(|p| format!("{}({:?})", p.name, p.kind))
            .collect();
        facts.push_str(&format!(";暴露属性:{}", props.join(",")));
    }
    for n in &g.nodes {
        let kind = node_registry::find_spec(&n.ntype)
            .map(|s| format!("{:?}", s.kind))
            .unwrap_or_else(|| "?".into());
        facts.push_str(&format!(";节点 {}:{}[{kind}]", n.id, n.ntype));
        // 常量输入是最有语义价值的部分(tag="player"、消息名等)。
        for (pin, src) in &n.inputs {
            if let ValueSource::Const { konst } = src {
                facts.push_str(&format!(" {pin}={}", compact_value(konst)));
            }
        }
    }
    if !g.edges.is_empty() {
        let chain: Vec<String> = g
            .edges
            .iter()
            .map(|e| format!("{}.{}→{}", e.from[0], e.from[1], e.to[0]))
            .collect();
        facts.push_str(&format!(";执行链:{}", chain.join(" ")));
    }

    Some(IndexDoc {
        id: format!("graph:{rel}"),
        kind: DocKind::Graph,
        title: g.name.clone(),
        path: rel.to_string(),
        guid: None,
        atype: String::new(),
        description,
        facts: truncate_facts(facts),
        tags,
        refs: Vec::new(),
        content_hash: String::new(),
    })
}

fn compact_value(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 40 {
        let t: String = s.chars().take(40).collect();
        format!("{t}…")
    } else {
        s
    }
}

// ---------- 4. rx 符号 ----------

fn extract_rx_symbols(
    project: &ForgeProject,
    rels: &[String],
    metas: &HashMap<String, MetaDoc>,
) -> Vec<IndexDoc> {
    let mut out = Vec::new();
    for rel in rels.iter().filter(|r| r.ends_with(".rx")) {
        let abs = project.content_root().join(rel);
        let Ok(text) = std::fs::read_to_string(&abs) else { continue };
        if let Some(d) = symbol_file_doc(rel, &text, metas) {
            out.push(d);
        }

        let fns = scan_export_c_fns(&text);
        for f in &fns {
            out.push(IndexDoc {
                id: format!("symbol:{rel}#{}", f.name),
                kind: DocKind::Symbol,
                title: f.name.clone(),
                path: rel.clone(),
                guid: None,
                atype: String::new(),
                description: String::new(),
                facts: truncate_facts(format!("{};所在脚本:{rel}", fn_signature(f))),
                tags: Vec::new(),
                refs: Vec::new(),
                content_hash: String::new(),
            });
        }
    }
    out
}

/// rx 文件级符号文档(头注释 + 导出签名 + .meta 简介;upsert 复用)。
fn symbol_file_doc(
    rel: &str,
    text: &str,
    metas: &HashMap<String, MetaDoc>,
) -> Option<IndexDoc> {
    let (description, tags) = semantic_of(metas, rel);

    // 文件头注释块(往往是最有 RAG 价值的自然语言,如 maze.rx 的迷宫图)。
    let header: String = text
        .lines()
        .take_while(|l| l.trim_start().starts_with("//") || l.trim().is_empty())
        .map(|l| l.trim_start().trim_start_matches("//").trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let fns = scan_export_c_fns(text);

    let stem = file_name_of(rel);
    let mut file_facts = String::new();
    if !header.is_empty() {
        file_facts.push_str(&format!("头注释:{header}"));
    }
    if !fns.is_empty() {
        let sigs: Vec<String> = fns.iter().map(fn_signature).collect();
        if !file_facts.is_empty() {
            file_facts.push(';');
        }
        file_facts.push_str(&format!("导出函数:{}", sigs.join(" | ")));
    }
    Some(IndexDoc {
        id: format!("symbol:{rel}"),
        kind: DocKind::Symbol,
        title: stem,
        path: rel.to_string(),
        guid: None,
        atype: String::new(),
        description,
        facts: truncate_facts(file_facts),
        tags,
        refs: Vec::new(),
        content_hash: String::new(),
    })
}

/// 单文件全量文档(asset + graph + symbol 文件级;资产简介 upsert 用——
/// .meta semantic 同时喂这三类文档,单点写入后须同步增量)。
/// 返回 None = rel 不在 Content 扫描内。
pub fn extract_file_docs(project_root: &Path, rel: &str) -> Result<Option<Vec<IndexDoc>>> {
    let project = ForgeProject::load(project_root)
        .unwrap_or_else(|_| ForgeProject::with_defaults(project_root.to_path_buf()));
    let rels = project.scan_content().unwrap_or_default();
    if !rels.iter().any(|r| r == rel) {
        return Ok(None);
    }
    // 与 extract_all 同一 meta 扫描/guid 映射,保 content_hash 与全量构建一致。
    let mut guid_to_path: HashMap<String, String> = HashMap::new();
    let mut metas: HashMap<String, MetaDoc> = HashMap::new();
    for r in &rels {
        let mp = meta_path_for(&project.content_root(), r);
        if mp.is_file() {
            if let Ok(m) = MetaDoc::load(&mp) {
                guid_to_path.insert(m.guid.clone(), r.clone());
                metas.insert(r.clone(), m);
            }
        }
    }
    let refgraph = assetd::refs::RefGraph::load(&project).unwrap_or_default();

    let mut out = vec![asset_doc(&project, rel, &metas, &guid_to_path, &refgraph)];
    if rel.ends_with(".rxgraph") {
        if let Some(d) = graph_doc(&project, rel, &metas) {
            out.push(d);
        }
    }
    if rel.ends_with(".rx") {
        let abs = project.content_root().join(rel);
        if let Ok(text) = std::fs::read_to_string(&abs) {
            if let Some(d) = symbol_file_doc(rel, &text, &metas) {
                out.push(d);
            }
        }
    }
    for d in &mut out {
        d.finalize_hash();
    }
    Ok(Some(out))
}

fn fn_signature(f: &forge_logic::rxexport::ExportedFn) -> String {
    let params: Vec<String> = f.params.iter().map(|(n, t)| format!("{n}:{t}")).collect();
    format!("fn {}({}) -> {}", f.name, params.join(", "), f.ret)
}

// ---------- 5. md / txt 文档(递归) ----------

/// 跳过的目录名(构建产物 / VCS / 依赖 / 引擎缓存)。
const SKIP_DIRS: &[&str] = &[
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

fn is_doc_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()),
        Some(ref e) if e == "md" || e == "txt"
    )
}

fn collect_doc_files(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let abs = root.join(rel);
    let Ok(entries) = std::fs::read_dir(&abs) else { return };
    let mut ents: Vec<_> = entries.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for ent in ents {
        let name = ent.file_name();
        let name_s = name.to_string_lossy();
        if SKIP_DIRS.iter().any(|s| *s == name_s) {
            continue;
        }
        let child_rel = rel.join(&name);
        let p = ent.path();
        if p.is_dir() {
            collect_doc_files(root, &child_rel, out);
        } else if is_doc_file(&p) {
            out.push(p);
        }
    }
}

fn extract_markdown_docs(docs_root: &Path) -> Vec<IndexDoc> {
    let mut out = Vec::new();
    let mut files = Vec::new();
    collect_doc_files(docs_root, Path::new(""), &mut files);
    files.sort();

    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let rel = path
            .strip_prefix(docs_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.md")
            .to_string();
        let file_title = text
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l.trim_start_matches("# ").trim().to_string())
            .unwrap_or_else(|| fname.clone());

        // 按 "## " 分块并记行号(resource_get 用 Lstart-Lend 读源文件片段)。
        let mut chunks: Vec<(String, String, usize, usize)> = Vec::new();
        let mut cur_head = file_title.clone();
        let mut cur_body = String::new();
        let mut cur_start = 1usize;
        for (i, line) in text.lines().enumerate() {
            let line_no = i + 1;
            if let Some(h) = line.strip_prefix("## ") {
                if !cur_body.trim().is_empty() {
                    chunks.push((cur_head.clone(), cur_body.clone(), cur_start, line_no.saturating_sub(1)));
                }
                cur_head = h.trim().to_string();
                cur_body = String::new();
                cur_start = line_no;
            } else {
                cur_body.push_str(line);
                cur_body.push('\n');
            }
            if chunks.len() >= DOC_CHUNKS_PER_FILE {
                break;
            }
        }
        if !cur_body.trim().is_empty() && chunks.len() < DOC_CHUNKS_PER_FILE {
            let end = text.lines().count().max(cur_start);
            chunks.push((cur_head, cur_body, cur_start, end));
        }

        for (n, (heading, body, start, end)) in chunks.into_iter().enumerate() {
            out.push(IndexDoc {
                id: format!("doc:{rel}#{n}"),
                kind: DocKind::Doc,
                title: if heading == file_title {
                    file_title.clone()
                } else {
                    format!("{file_title} § {heading}")
                },
                path: rel.clone(),
                guid: None,
                atype: String::new(),
                description: String::new(),
                facts: truncate_facts(format!("L{start}-L{end}\n{}", body.trim())),
                tags: vec![format!("L{start}-L{end}")],
                refs: Vec::new(),
                content_hash: String::new(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_project() -> (std::path::PathBuf, ForgeProject) {
        let dir = std::env::temp_dir().join(format!(
            "forge-index-extract-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        let content = dir.join("Content");
        std::fs::create_dir_all(content.join("Scenes")).unwrap();
        std::fs::create_dir_all(content.join("Scripts")).unwrap();
        std::fs::create_dir_all(content.join("Graphs")).unwrap();

        // 场景:两实体(player 带 Tag;门带 Script graphRef)。
        let scene = r#"{
  "name": "TestScene",
  "next_id": 3,
  "entities": [
    { "id": 1, "name": "Player", "transform": { "translation": [1.0, 0.5, 2.0], "rotation": [0,0,0,1], "scale": [1,1,1] }, "components": [
      { "type": "Tag", "enabled": true, "props": { "tag": "player" } }
    ] },
    { "id": 2, "name": "Door", "transform": { "translation": [0,0,0], "rotation": [0,0,0,1], "scale": [1,1,1] }, "components": [
      { "type": "Script", "enabled": true, "props": { "module": "", "graphRef": "Content/Graphs/door.rxgraph", "props": {} } }
    ] }
  ]
}"#;
        std::fs::write(content.join("Scenes/test.rxscene"), scene).unwrap();

        // 节点图。
        let graph = r#"{
  "version": 1, "id": "g_door", "name": "DoorOpener", "exposedProps": [],
  "nodes": [
    { "id": "t", "type": "event.on_trigger_enter", "pos": [0,0] },
    { "id": "h", "type": "entity.has_tag", "pos": [0,1], "inputs": { "entity": { "node": "t", "pin": "otherEntity" }, "tag": { "const": "player" } } }
  ],
  "edges": [ { "from": ["t", "exec"], "to": ["h", "exec"] } ]
}"#;
        std::fs::write(content.join("Graphs/door.rxgraph"), graph).unwrap();

        // rx 脚本。
        let rx = "// 迷宫规则脚本\n#[export(c)]\npub fn open_at_cell(cell: i32) -> bool { true }\n";
        std::fs::write(content.join("Scripts/maze.rx"), rx).unwrap();

        let project = ForgeProject::with_defaults(dir.clone());
        (dir, project)
    }

    #[test]
    fn extract_is_deterministic_and_covers_all_kinds() {
        let (dir, _project) = fixture_project();
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        let a = extract_all(&opts).unwrap();
        let b = extract_all(&opts).unwrap();
        assert_eq!(a, b, "两次抽取须逐字节确定");

        let kinds: Vec<&str> = a.iter().map(|d| d.kind.as_str()).collect();
        assert!(kinds.contains(&"asset"), "{kinds:?}");
        assert!(kinds.contains(&"entity"), "{kinds:?}");
        assert!(kinds.contains(&"graph"), "{kinds:?}");
        assert!(kinds.contains(&"symbol"), "{kinds:?}");

        // 实体分类:Player 带 Tag=player → role;Door 带 Script → interaction。
        let player = a.iter().find(|d| d.title == "Player").unwrap();
        assert!(player.tags.contains(&"role".to_string()), "{player:?}");
        let door = a.iter().find(|d| d.title == "Door").unwrap();
        assert!(door.tags.contains(&"interaction".to_string()), "{door:?}");

        // 图 facts 含常量输入(tag="player")。
        let g = a.iter().find(|d| d.kind == DocKind::Graph).unwrap();
        assert!(g.facts.contains("player"), "{}", g.facts);

        // 符号:文件级 + 函数级。
        assert!(a.iter().any(|d| d.id.ends_with("maze.rx")), "文件级符号缺失");
        assert!(a.iter().any(|d| d.id.ends_with("#open_at_cell")), "函数级符号缺失");

        // hash 已回填。
        assert!(a.iter().all(|d| !d.content_hash.is_empty()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn markdown_docs_split_by_h2() {
        let dir = std::env::temp_dir().join(format!(
            "forge-index-md-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("01_TEST.md"),
            "# 测试文档\n\n概览段。\n\n## 第一节\n\n第一节内容。\n\n## 第二节\n\n第二节内容。\n",
        )
        .unwrap();
        let docs = extract_markdown_docs(&dir);
        assert_eq!(docs.len(), 3, "{docs:?}");
        assert!(docs[0].facts.contains("概览段"));
        assert!(docs[1].title.contains("第一节"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn markdown_docs_recurse_and_skip_build_dirs() {
        let dir = std::env::temp_dir().join(format!(
            "forge-index-md-rec-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(dir.join("docs").join("design")).unwrap();
        std::fs::create_dir_all(dir.join("target").join("debug")).unwrap();
        std::fs::write(dir.join("docs/design/plan.md"), "# 策划\n\n飞机大战关卡设计。\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "纯文本备忘: 敌人波次。\n").unwrap();
        std::fs::write(dir.join("target/debug/noise.md"), "# 不该进索引\n").unwrap();
        let docs = extract_markdown_docs(&dir);
        assert!(docs.iter().any(|d| d.path == "docs/design/plan.md"), "{docs:?}");
        assert!(docs.iter().any(|d| d.path == "notes.txt"), "{docs:?}");
        assert!(docs.iter().all(|d| !d.path.contains("target")), "{docs:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
