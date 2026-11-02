//! 2D 绑骨资产(F-GAME-5 / D-050):`.rxrig` 的磁盘 IO 面。
//!
//! 纯计算(格式 / 校验 / 求值 / 自动网格与权重 / 预览光栅)全部在 `forge-rig2d`;
//! 本模块只负责与项目磁盘交互,风格对齐 [`crate::sprite`]:
//! - 读写:[`load_rxrig`] / [`create_rig`] / [`write_rig_doc`],确定性 JSON
//!   (`RigDoc::to_json_string`),已有 `.meta` 复用 GUID(重导入语义);
//! - 编辑:[`patch_rig`](原子批量操作,dryRun 不落盘)、[`mesh_auto_parts`]、
//!   [`weights_auto_parts`];
//! - 只读:[`evaluate_json`]、[`preview_rig`](解码图集 → CPU 光栅 → PNG 写
//!   `.forge/tmp/`,可与参考图比较剪影 IoU)、[`doc_view`](rig_get 省略大数组);
//! - 部件来源:[`import_psd`](8-bit RGB PSD → shelf 图集 + 矩形部件)、
//!   [`parts_from_sheet`](生图部件表 bbox + 画布位置组装)。
//!
//! 资产路径一律相对 `Content/`(与 sprite_* / asset_* 工具同);为兼容契约示例,
//! 也容忍带 `Content/` 前缀的项目相对路径([`content_rel`])。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use forge_rig2d::autogen;
use forge_rig2d::doc::{split_target, PartBlend, RigPart, TargetKind};
use forge_rig2d::patch::{self, PatchOp};
use forge_rig2d::raster::{self, Overlay, PreviewOpts};
use forge_rig2d::{CompiledRig, RigError};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::meta::{MetaDoc, Provenance};
use crate::project::ForgeProject;
use crate::{meta_path_for, new_guid, normalize_rel, AssetError, Result};

pub use forge_rig2d::autogen::{MeshAutoOpts, WeightOpts};
pub use forge_rig2d::{RigDoc, RigStats};

/// 缺省目标目录(相对 Content/)。
pub const DEFAULT_FOLDER: &str = "Rigs";
/// rig_get `sections` 允许值。
pub const SECTIONS: &[&str] = &[
    "parameters", "bones", "deformers", "parts", "keyforms", "physics", "auto", "clips", "animator", "canvas",
];
/// PSD 图集边长上限(像素)。
pub const ATLAS_MAX: u32 = 4096;
/// PSD 图集部件间距(像素)。
pub const ATLAS_PAD: u32 = 2;

fn rig_err(e: RigError) -> AssetError {
    AssetError::new(e.code, e.message)
}

fn psd_unsupported(msg: impl Into<String>) -> AssetError {
    AssetError::new("RIG_PSD_UNSUPPORTED", msg)
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('.') {
        return Err(AssetError::new("INVALID_OPS", format!("rig 名非法(禁含 / \\ .): {name}")));
    }
    Ok(())
}

fn folder_or_default(dest_folder: &str) -> Result<String> {
    if dest_folder.is_empty() {
        Ok(DEFAULT_FOLDER.to_string())
    } else {
        normalize_rel(dest_folder)
    }
}

// ───────────────────────────── 路径 / GUID 解析 ─────────────────────────────

/// 资产路径归一为相对 Content/ 的正斜杠路径(容忍 `Content/` 前缀与 `./`)。
pub fn content_rel(project: &ForgeProject, p: &str) -> Result<String> {
    let s = p.replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(&s);
    let prefix = format!("{}/", project.content_dir.trim_end_matches('/'));
    let s = s.strip_prefix(prefix.as_str()).unwrap_or(s);
    normalize_rel(s)
}

/// 项目相对(或项目内绝对)文件路径 → 根内绝对路径(compareTo / psdRef 用;逃逸拒绝)。
pub fn resolve_project_file(project: &ForgeProject, user: &str) -> Result<PathBuf> {
    let p = forge_util::pathutil::confine_under(&[project.root.as_path()], user)
        .map_err(|e| AssetError::new("PROJECT_OUT_OF_ROOT", format!("{e}: {user}")))?;
    if !p.is_file() {
        return Err(AssetError::new("NOT_FOUND", format!("文件不存在: {user}")));
    }
    Ok(p)
}

/// 全项目 GUID → (相对路径, 资产类型)。
fn guid_index(project: &ForgeProject) -> Result<HashMap<String, (String, String)>> {
    let mut map = HashMap::new();
    for rel in project.scan_content()? {
        let mp = meta_path_for(&project.content_root(), &rel);
        if mp.is_file() {
            if let Ok(m) = MetaDoc::load(&mp) {
                map.insert(m.guid, (rel, m.atype));
            }
        }
    }
    Ok(map)
}

/// 贴图 GUID → 相对路径(须存在且为 texture)。
pub fn texture_rel(project: &ForgeProject, guid: &str) -> Result<String> {
    match guid_index(project)?.remove(guid) {
        Some((rel, t)) if t == "texture" => Ok(rel),
        Some((_, t)) => Err(AssetError::new(
            "WRONG_TYPE",
            format!("texture 引用的 GUID 不是贴图(实际 {t}): {guid}"),
        )),
        None => Err(AssetError::new("UNKNOWN_GUID", format!("texture 引用的 GUID 不存在: {guid}"))),
    }
}

/// 解析 rig 引用(assetPath 优先,否则 guid)→ (相对路径, GUID)。
pub fn resolve_rig(project: &ForgeProject, asset_path: Option<&str>, guid: Option<&str>) -> Result<(String, String)> {
    if let Some(p) = asset_path.filter(|s| !s.is_empty()) {
        let rel = content_rel(project, p)?;
        if !rel.ends_with(".rxrig") {
            return Err(AssetError::new("WRONG_TYPE", format!("仅接受 .rxrig: {rel}")));
        }
        let abs = project.content_root().join(&rel);
        if !abs.is_file() {
            return Err(AssetError::new("NOT_FOUND", format!("rig 不存在: {rel}")));
        }
        let mp = meta_path_for(&project.content_root(), &rel);
        let g = if mp.is_file() { MetaDoc::load(&mp).map(|m| m.guid).unwrap_or_default() } else { String::new() };
        return Ok((rel, g));
    }
    if let Some(g) = guid.filter(|s| !s.is_empty()) {
        return match guid_index(project)?.remove(g) {
            Some((rel, t)) if t == "rig" => Ok((rel, g.to_string())),
            Some((_, t)) => Err(AssetError::new("WRONG_TYPE", format!("GUID 不是 rig(实际 {t}): {g}"))),
            None => Err(AssetError::new("UNKNOWN_GUID", format!("rig GUID 不存在: {g}"))),
        };
    }
    Err(AssetError::new("INVALID_PARAMS", "须提供 assetPath 或 guid"))
}

// ───────────────────────────── 读写 ─────────────────────────────

/// 从磁盘读 .rxrig(解析 + 语义校验)。
pub fn load_rxrig(path: &Path) -> Result<RigDoc> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| AssetError::new("IO", format!("读 .rxrig 失败 {}: {e}", path.display())))?;
    forge_rig2d::parse_rxrig_str(&text).map_err(rig_err)
}

/// 按相对路径读。
pub fn load_rel(project: &ForgeProject, rel: &str) -> Result<RigDoc> {
    load_rxrig(&project.content_root().join(rel))
}

/// 校验 + 确定性写出 + .meta(已存在复用 GUID;provenance 仅缺省时写入)。返回 GUID。
fn write_rig_file(project: &ForgeProject, rel: &str, doc: &RigDoc, origin: Option<&str>) -> Result<String> {
    forge_rig2d::validate_doc(doc).map_err(rig_err)?;
    let abs = project.content_root().join(rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&abs, doc.to_json_string())?;
    let meta_path = meta_path_for(&project.content_root(), rel);
    let mut meta = if meta_path.is_file() { MetaDoc::load(&meta_path)? } else { MetaDoc::new(rel, new_guid())? };
    if meta.provenance.is_none() {
        meta.provenance = Some(Provenance {
            origin: origin.filter(|o| !o.is_empty()).unwrap_or("user-import").to_string(),
            detail: None,
        });
    }
    meta.build_state = Some("current".into());
    meta.save(&meta_path)?;
    Ok(meta.guid)
}

/// rig_create / rig_parts_from_sheet 返回。
#[derive(Debug, Clone)]
pub struct RigCreated {
    pub asset_path: String,
    pub guid: String,
    pub texture_guid: String,
    pub stats: RigStats,
}

/// 创建 rig:空白文档(可合并 bust/fullbody 模板)。canvas 缺省 = 贴图像素尺寸。
pub fn create_rig(
    project: &ForgeProject,
    dest_folder: &str,
    name: &str,
    texture_guid: &str,
    canvas_size: Option<[f32; 2]>,
    template: Option<&str>,
    origin: Option<&str>,
) -> Result<RigCreated> {
    check_name(name)?;
    let tex = texture_rel(project, texture_guid)?;
    let canvas = match canvas_size {
        Some(c) => c,
        None => {
            let (w, h) = crate::texture::decode_size(&project.content_root().join(&tex))?;
            [w as f32, h as f32]
        }
    };
    let mut doc = RigDoc::empty(texture_guid, canvas);
    if let Some(t) = template.filter(|t| !t.is_empty()) {
        doc = patch::apply_ops(&doc, vec![PatchOp::MergeScaffold { template: t.to_string(), overwrite: false }])
            .map_err(rig_err)?;
    }
    let rel = format!("{}/{name}.rxrig", folder_or_default(dest_folder)?);
    let guid = write_rig_file(project, &rel, &doc, origin)?;
    Ok(RigCreated { asset_path: rel, guid, texture_guid: texture_guid.to_string(), stats: doc.stats() })
}

/// 整文档覆盖写(rig_set):先校验(含贴图 GUID 存在且为 texture);.meta 不动,GUID 稳定。
pub fn write_rig_doc(project: &ForgeProject, rel_path: &str, doc: &Value) -> Result<RigDoc> {
    let rel = content_rel(project, rel_path)?;
    if !rel.ends_with(".rxrig") {
        return Err(AssetError::new("WRONG_TYPE", format!("rig_set 仅接受 .rxrig: {rel}")));
    }
    let parsed = forge_rig2d::parse_rxrig(doc).map_err(rig_err)?;
    if !meta_path_for(&project.content_root(), &rel).is_file() {
        return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
    }
    texture_rel(project, &parsed.texture)?;
    write_rig_file(project, &rel, &parsed, None)?;
    Ok(parsed)
}

/// rig_validate:文档语义校验 + 贴图 GUID 解析。
pub fn validate_value(project: &ForgeProject, doc: &Value) -> Result<RigDoc> {
    let parsed = forge_rig2d::parse_rxrig(doc).map_err(rig_err)?;
    texture_rel(project, &parsed.texture)?;
    Ok(parsed)
}

/// rig_patch 返回。
#[derive(Debug, Clone)]
pub struct PatchOutcome {
    pub applied: usize,
    pub dry_run: bool,
    /// 删除类操作在 ops 中的下标(调用方据此走 Proposal 门)。
    pub destructive: Vec<usize>,
    pub stats: RigStats,
    pub doc: RigDoc,
}

/// 原子批量编辑:全部操作在副本上执行并整体校验,任一失败整体不生效;dry_run 不落盘。
pub fn patch_rig(project: &ForgeProject, rel: &str, ops: &Value, dry_run: bool) -> Result<PatchOutcome> {
    let doc = load_rel(project, rel)?;
    let ops = patch::parse_ops(ops).map_err(rig_err)?;
    let destructive: Vec<usize> = ops.iter().enumerate().filter(|(_, o)| o.is_destructive()).map(|(i, _)| i).collect();
    let applied = ops.len();
    let new = patch::apply_ops(&doc, ops).map_err(rig_err)?;
    if !dry_run {
        write_rig_file(project, rel, &new, None)?;
    }
    Ok(PatchOutcome { applied, dry_run, destructive, stats: new.stats(), doc: new })
}

fn select_parts(doc: &RigDoc, parts: Option<&[String]>) -> Result<Vec<String>> {
    match parts {
        Some(ids) => {
            for id in ids {
                if doc.part(id).is_none() {
                    return Err(AssetError::new("RIG_INVALID", format!("部件不存在: {id}")));
                }
            }
            Ok(ids.to_vec())
        }
        None => Ok(doc.parts.iter().map(|p| p.id.clone()).collect()),
    }
}

/// 解码 rig 图集贴图 → (宽, 高, RGBA8)。
pub fn decode_atlas(project: &ForgeProject, texture_guid: &str) -> Result<(u32, u32, Vec<u8>)> {
    let rel = texture_rel(project, texture_guid)?;
    crate::texture::decode_rgba(&project.content_root().join(rel))
}

/// rig_mesh_auto 单部件结果。
#[derive(Debug, Clone)]
pub struct MeshAutoPart {
    pub id: String,
    pub vertices: usize,
    pub triangles: usize,
}

/// rig_mesh_auto 返回。
#[derive(Debug, Clone)]
pub struct MeshAutoOutcome {
    pub parts: Vec<MeshAutoPart>,
    /// 原有 weights 被丢弃的部件(顶点变了)。
    pub weights_cleared: Vec<String>,
    /// 指向该部件的 keyform 顶点偏移被清空的部件(opacity/drawOrder 形态保留)。
    pub keyforms_cleared: Vec<String>,
    pub stats: RigStats,
}

/// 按图集轮廓重建部件网格(保持图集→画布摆放平移)。原 weights 与 keyform 顶点偏移随顶点失效被清空。
pub fn mesh_auto_parts(
    project: &ForgeProject,
    rel: &str,
    parts: Option<&[String]>,
    opts: &MeshAutoOpts,
) -> Result<MeshAutoOutcome> {
    let mut doc = load_rel(project, rel)?;
    let ids = select_parts(&doc, parts)?;
    let (aw, ah, atlas) = decode_atlas(project, &doc.texture)?;
    let mut out = MeshAutoOutcome { parts: vec![], weights_cleared: vec![], keyforms_cleared: vec![], stats: RigStats::default() };
    for id in &ids {
        {
            let part = doc.parts.iter_mut().find(|p| &p.id == id).expect("selected part exists");
            let region = autogen::mesh_uv_region(&part.mesh);
            let offset = autogen::mesh_canvas_offset(&part.mesh);
            let mesh = autogen::mesh_auto(&atlas, aw, ah, region, offset, opts)
                .map_err(|e| AssetError::new(e.code, format!("部件 {id}: {}", e.message)))?;
            out.parts.push(MeshAutoPart { id: id.clone(), vertices: mesh.vertices.len(), triangles: mesh.triangles.len() });
            part.mesh = mesh;
            if part.weights.take().is_some() {
                out.weights_cleared.push(id.clone());
            }
        }
        let mut hit = false;
        for g in doc.keyforms.iter_mut() {
            if split_target(&g.target).is_some_and(|(k, t, _)| k == TargetKind::Part && t == id) {
                for f in g.forms.iter_mut() {
                    hit |= f.offsets.take().is_some();
                }
            }
        }
        if hit {
            out.keyforms_cleared.push(id.clone());
        }
    }
    write_rig_file(project, rel, &doc, None)?;
    out.stats = doc.stats();
    Ok(out)
}

/// rig_weights_auto 单部件结果。
#[derive(Debug, Clone)]
pub struct WeightsPart {
    pub id: String,
    /// 骨骼 → 以其为最大权重的顶点数。
    pub dominant_bones: BTreeMap<String, usize>,
}

/// rig_weights_auto 返回。
#[derive(Debug, Clone)]
pub struct WeightsOutcome {
    pub parts: Vec<WeightsPart>,
    /// 未显式指定 parts 时跳过的刚性绑定部件(`bone` 已设)。
    pub skipped_rigid: Vec<String>,
    pub stats: RigStats,
}

/// 自动蒙皮权重(骨段距离反比,≤4 影响)。写 weights 并清除刚性 `bone`(二者互斥)。
/// 未指定 parts 时跳过已刚性绑定的部件(瞳孔 / 饰品等),显式指定则照写。
pub fn weights_auto_parts(
    project: &ForgeProject,
    rel: &str,
    parts: Option<&[String]>,
    opts: &WeightOpts,
) -> Result<WeightsOutcome> {
    let mut doc = load_rel(project, rel)?;
    let explicit = parts.is_some();
    let ids = select_parts(&doc, parts)?;
    let mut out = WeightsOutcome { parts: vec![], skipped_rigid: vec![], stats: RigStats::default() };
    for id in &ids {
        let part = doc.part(id).expect("selected part exists");
        if !explicit && part.bone.is_some() {
            out.skipped_rigid.push(id.clone());
            continue;
        }
        let ws = autogen::weights_auto(&doc, id, opts).map_err(rig_err)?;
        let mut dominant: BTreeMap<String, usize> = BTreeMap::new();
        for inf in &ws {
            if let Some((b, _)) = inf.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
                *dominant.entry(b.clone()).or_default() += 1;
            }
        }
        let part = doc.parts.iter_mut().find(|p| &p.id == id).expect("selected part exists");
        part.weights = Some(ws);
        part.bone = None;
        out.parts.push(WeightsPart { id: id.clone(), dominant_bones: dominant });
    }
    write_rig_file(project, rel, &doc, None)?;
    out.stats = doc.stats();
    Ok(out)
}

// ───────────────────────────── 只读:视图 / 求值 / 预览 ─────────────────────────────

/// rig_get 视图:`include_mesh=false` 时 parts[].mesh → {vertexCount, triangleCount, uvBounds},
/// parts[].weights → {count},keyforms[].forms[].offsets → {count};`sections` 只保留指定段
/// (始终带 version / texture / pixelsPerUnit)。
pub fn doc_view(doc: &RigDoc, sections: Option<&[String]>, include_mesh: bool) -> Result<Value> {
    let mut v = serde_json::to_value(doc).map_err(|e| AssetError::new("META_SERIALIZE", e.to_string()))?;
    if !include_mesh {
        if let Some(parts) = v.get_mut("parts").and_then(Value::as_array_mut) {
            for (p, src) in parts.iter_mut().zip(&doc.parts) {
                let m = &src.mesh;
                p["mesh"] = json!({
                    "vertexCount": m.vertices.len(),
                    "triangleCount": m.triangles.len(),
                    "uvBounds": autogen::mesh_uv_region(m),
                });
                if let Some(ws) = &src.weights {
                    p["weights"] = json!({ "count": ws.len() });
                }
            }
        }
        if let Some(groups) = v.get_mut("keyforms").and_then(Value::as_array_mut) {
            for g in groups {
                if let Some(forms) = g.get_mut("forms").and_then(Value::as_array_mut) {
                    for f in forms {
                        if let Some(o) = f.get_mut("offsets") {
                            let n = o.as_array().map(Vec::len).unwrap_or(0);
                            *o = json!({ "count": n });
                        }
                    }
                }
            }
        }
    }
    if let Some(secs) = sections {
        let obj = v.as_object().cloned().unwrap_or_default();
        let mut out = Map::new();
        for k in ["version", "texture", "pixelsPerUnit"] {
            if let Some(x) = obj.get(k) {
                out.insert(k.into(), x.clone());
            }
        }
        for s in secs {
            if !SECTIONS.contains(&s.as_str()) {
                return Err(AssetError::new(
                    "INVALID_OPS",
                    format!("未知 section: {s}(可选 {})", SECTIONS.join("/")),
                ));
            }
            out.insert(s.clone(), obj.get(s).cloned().unwrap_or(Value::Null));
        }
        v = Value::Object(out);
    }
    Ok(v)
}

fn r2(v: f32) -> f64 {
    (v as f64 * 100.0).round() / 100.0
}

/// rig_evaluate:给定参数 / clip / 时间求一帧形变(画布像素,Y 向下)。
pub fn evaluate_json(
    doc: &RigDoc,
    params: &BTreeMap<String, f32>,
    clip: Option<&str>,
    time: f32,
    parts: Option<&[String]>,
) -> Result<Value> {
    let rig = CompiledRig::compile(doc.clone()).map_err(rig_err)?;
    let (pose, mut unknown) = forge_rig2d::pose_from(&rig, params, clip, time);
    let d = forge_rig2d::evaluate(&rig, &pose);
    if let Some(ids) = parts {
        for id in ids {
            if doc.part(id).is_none() {
                unknown.push(format!("part:{id}"));
            }
        }
    }
    let out_parts: Vec<Value> = d
        .parts
        .iter()
        .filter(|dp| parts.is_none_or(|ids| ids.iter().any(|i| *i == doc.parts[dp.part].id)))
        .map(|dp| {
            json!({
                "id": doc.parts[dp.part].id,
                "drawOrder": dp.draw_order,
                "opacity": r2(dp.opacity),
                "positions": dp.positions.iter().map(|p| [r2(p[0]), r2(p[1])]).collect::<Vec<_>>(),
            })
        })
        .collect();
    let bones: Vec<Value> = doc
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let m = d.bone_world.get(i).copied().unwrap_or_default();
            let (h, t) = (m.apply([0.0, 0.0]), m.apply([b.length, 0.0]));
            json!({ "id": b.id, "head": [r2(h[0]), r2(h[1])], "tail": [r2(t[0]), r2(t[1])] })
        })
        .collect();
    Ok(json!({
        "bbox": d.bbox.iter().map(|v| r2(*v)).collect::<Vec<_>>(),
        "parts": out_parts,
        "bones": bones,
        "unknown": unknown,
    }))
}

/// rig_preview 请求(MCP 参数直接反序列化;未知字段忽略)。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewRequest {
    pub params: BTreeMap<String, f32>,
    pub clip: Option<String>,
    pub time: f32,
    /// 方形边长(64..2048,缺省 512)。
    pub size: Option<u32>,
    /// none | mesh | bones | weights:<boneId>
    pub overlay: Option<String>,
    /// 项目相对参考图路径(算剪影 IoU)。
    pub compare_to: Option<String>,
    /// 1–2 个参数的取值网格(每轴 ≤ 5 值),出拼图。
    pub grid: Option<BTreeMap<String, Vec<f32>>>,
}

/// rig_preview 返回。
#[derive(Debug, Clone)]
pub struct PreviewOutcome {
    /// 项目相对 PNG 路径(`.forge/tmp/rig_preview_<guid>_<hash>.png`)。
    pub file_ref: String,
    pub width: u32,
    pub height: u32,
    pub silhouette_iou: Option<f32>,
    pub unknown: Vec<String>,
}

fn parse_overlay(rig: &CompiledRig, s: Option<&str>) -> Result<Overlay> {
    match s.unwrap_or("none") {
        "" | "none" => Ok(Overlay::None),
        "mesh" => Ok(Overlay::Mesh),
        "bones" => Ok(Overlay::Bones),
        o => match o.strip_prefix("weights:") {
            Some(b) => rig
                .bone_index
                .get(b)
                .copied()
                .map(Overlay::Weights)
                .ok_or_else(|| AssetError::new("RIG_INVALID", format!("overlay 引用的骨骼不存在: {b}"))),
            None => Err(AssetError::new(
                "INVALID_OPS",
                format!("overlay 非法: {o}(none | mesh | bones | weights:<boneId>)"),
            )),
        },
    }
}

struct Frame<'a> {
    rig: &'a CompiledRig,
    atlas: &'a [u8],
    aw: u32,
    ah: u32,
}

impl Frame<'_> {
    fn render(&self, params: &BTreeMap<String, f32>, clip: Option<&str>, time: f32, w: u32, h: u32, overlay: Overlay) -> (Vec<u8>, Vec<String>) {
        let (pose, unknown) = forge_rig2d::pose_from(self.rig, params, clip, time);
        let d = forge_rig2d::evaluate(self.rig, &pose);
        let opts = PreviewOpts { width: w, height: h, background: [0, 0, 0, 0], overlay, chroma_key: true };
        (raster::render_preview(self.rig, &d, self.atlas, self.aw, self.ah, &opts), unknown)
    }
}

/// 参考图按 rig canvas 适配到输出画布(参考图尺寸视为 canvas 尺寸;最近邻;画布外透明)。
fn fit_reference(canvas: [f32; 2], refw: u32, refh: u32, rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (s, ox, oy) = raster::fit_transform(canvas, w, h);
    let mut out = vec![0u8; (w * h * 4) as usize];
    if refw == 0 || refh == 0 || s <= 0.0 {
        return out;
    }
    for y in 0..h {
        let cy = (y as f32 + 0.5 - oy) / s;
        if cy < 0.0 || cy >= canvas[1] {
            continue;
        }
        let ry = ((cy / canvas[1] * refh as f32) as u32).min(refh - 1);
        for x in 0..w {
            let cx = (x as f32 + 0.5 - ox) / s;
            if cx < 0.0 || cx >= canvas[0] {
                continue;
            }
            let rx = ((cx / canvas[0] * refw as f32) as u32).min(refw - 1);
            let si = ((ry * refw + rx) * 4) as usize;
            let di = ((y * w + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    out
}

fn short_hash(parts: &[&[u8]]) -> String {
    let mut h = rurix_pkg::sha256::Sha256::new();
    for p in parts {
        h.update(p);
        h.update(b"|");
    }
    rurix_pkg::sha256::hex(&h.finalize())[..12].to_string()
}

/// RGBA8 → PNG 文件。
pub fn write_png(path: &Path, w: u32, h: u32, rgba: Vec<u8>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let img = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| AssetError::new("IO", format!("像素缓冲与尺寸不符 {w}x{h}")))?;
    img.save(path).map_err(|e| AssetError::new("IO", format!("写 PNG 失败 {}: {e}", path.display())))
}

/// rig_preview:CPU 光栅一帧(或参数网格拼图)写 `.forge/tmp/`;compareTo 时算剪影 IoU
/// (IoU 总在无 overlay 的渲染上计算,拼图模式不算)。
pub fn preview_rig(project: &ForgeProject, rel: &str, guid: &str, req: &PreviewRequest) -> Result<PreviewOutcome> {
    let doc = load_rel(project, rel)?;
    let size = req.size.unwrap_or(512).clamp(64, 2048);
    let (aw, ah, atlas) = decode_atlas(project, &doc.texture)?;
    let rig = CompiledRig::compile(doc.clone()).map_err(rig_err)?;
    let overlay = parse_overlay(&rig, req.overlay.as_deref())?;
    let frame = Frame { rig: &rig, atlas: &atlas, aw, ah };
    let clip = req.clip.as_deref();
    let mut unknown: Vec<String> = vec![];
    let mut push_unknown = |u: Vec<String>| {
        for x in u {
            if !unknown.contains(&x) {
                unknown.push(x);
            }
        }
    };

    let (w, h, px, iou) = match req.grid.as_ref().filter(|g| !g.is_empty()) {
        Some(grid) => {
            if grid.len() > 2 {
                return Err(AssetError::new("INVALID_OPS", "grid 最多 2 个参数"));
            }
            let axes: Vec<(&String, &Vec<f32>)> = grid.iter().collect();
            for (k, vs) in &axes {
                if vs.is_empty() || vs.len() > 5 {
                    return Err(AssetError::new("INVALID_OPS", format!("grid.{k} 取值数须 1..=5")));
                }
            }
            let cols = axes[0].1.len() as u32;
            let rows = axes.get(1).map(|a| a.1.len() as u32).unwrap_or(1);
            let cell = size.min(ATLAS_MAX / cols.max(rows)).max(16);
            let (w, h) = (cell * cols, cell * rows);
            let mut px = vec![0u8; (w * h * 4) as usize];
            for r in 0..rows {
                for c in 0..cols {
                    let mut params = req.params.clone();
                    params.insert(axes[0].0.clone(), axes[0].1[c as usize]);
                    if let Some((k, vs)) = axes.get(1) {
                        params.insert((*k).clone(), vs[r as usize]);
                    }
                    let (cpx, u) = frame.render(&params, clip, req.time, cell, cell, overlay);
                    push_unknown(u);
                    for y in 0..cell {
                        let src = (y * cell * 4) as usize;
                        let dst = (((r * cell + y) * w + c * cell) * 4) as usize;
                        px[dst..dst + (cell * 4) as usize].copy_from_slice(&cpx[src..src + (cell * 4) as usize]);
                    }
                }
            }
            (w, h, px, None)
        }
        None => {
            let (px, u) = frame.render(&req.params, clip, req.time, size, size, overlay);
            push_unknown(u);
            let iou = match req.compare_to.as_deref().filter(|s| !s.is_empty()) {
                Some(cmp) => {
                    let abs = resolve_project_file(project, cmp)?;
                    let (rw, rh, rgba) = crate::texture::decode_rgba(&abs)?;
                    let reference = fit_reference(doc.canvas.size, rw, rh, &rgba, size, size);
                    let plain = if overlay == Overlay::None {
                        px.clone()
                    } else {
                        frame.render(&req.params, clip, req.time, size, size, Overlay::None).0
                    };
                    Some(raster::silhouette_iou(&plain, &reference, true))
                }
                None => None,
            };
            (size, size, px, iou)
        }
    };

    let req_sig = format!(
        "{:?}|{:?}|{}|{}|{:?}|{:?}|{:?}",
        req.params, req.clip, req.time, size, req.overlay, req.compare_to, req.grid
    );
    let hash = short_hash(&[doc.to_json_string().as_bytes(), req_sig.as_bytes()]);
    let tag = if guid.is_empty() { "noguid" } else { guid };
    let file_name = format!("rig_preview_{tag}_{hash}.png");
    write_png(&project.tmp_root().join(&file_name), w, h, px)?;
    Ok(PreviewOutcome {
        file_ref: format!(".forge/tmp/{file_name}"),
        width: w,
        height: h,
        silhouette_iou: iou.map(|v| (v * 10000.0).round() / 10000.0),
        unknown,
    })
}

// ───────────────────────────── 部件表组装 ─────────────────────────────

/// rig_parts_from_sheet 的单部件输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetPart {
    pub name: String,
    /// 图集像素 [x, y, w, h](通常来自 sprite_autoslice)。
    pub bbox: [u32; 4],
    /// 部件左上角在画布中的位置(像素)。
    pub position: [f32; 2],
    #[serde(default)]
    pub draw_order: Option<i32>,
}

fn rect_part(id: String, draw_order: i32, atlas_rect: [f32; 4], canvas_pos: [f32; 2]) -> RigPart {
    RigPart {
        id,
        draw_order,
        mesh: autogen::rect_mesh(atlas_rect, canvas_pos),
        weights: None,
        bone: None,
        deformer: None,
        opacity: 1.0,
        blend: PartBlend::Alpha,
        visible: true,
        mask: None,
    }
}

/// 用部件表 bbox + 画布位置组装 rig(每部件矩形网格;drawOrder 缺省 = 序号 × 10)。
#[allow(clippy::too_many_arguments)]
pub fn parts_from_sheet(
    project: &ForgeProject,
    texture_guid: &str,
    name: &str,
    canvas_size: [f32; 2],
    parts: &[SheetPart],
    dest_folder: &str,
    origin: Option<&str>,
) -> Result<RigCreated> {
    check_name(name)?;
    let tex = texture_rel(project, texture_guid)?;
    let (tw, th) = crate::texture::decode_size(&project.content_root().join(&tex))?;
    if parts.is_empty() {
        return Err(AssetError::new("INVALID_OPS", "parts 不得为空"));
    }
    let mut doc = RigDoc::empty(texture_guid, canvas_size);
    for (i, p) in parts.iter().enumerate() {
        let [x, y, w, h] = p.bbox;
        if w == 0 || h == 0 || x + w > tw || y + h > th {
            return Err(AssetError::new(
                "RIG_INVALID",
                format!("parts[{i}]({}) bbox {:?} 超出贴图 {tw}x{th} 或宽高为 0", p.name, p.bbox),
            ));
        }
        let order = p.draw_order.unwrap_or(i as i32 * 10);
        doc.parts.push(rect_part(p.name.clone(), order, [x as f32, y as f32, w as f32, h as f32], p.position));
    }
    let rel = format!("{}/{name}.rxrig", folder_or_default(dest_folder)?);
    let guid = write_rig_file(project, &rel, &doc, origin)?;
    Ok(RigCreated { asset_path: rel, guid, texture_guid: texture_guid.to_string(), stats: doc.stats() })
}

// ───────────────────────────── PSD 导入 ─────────────────────────────

/// rig_import_psd 单部件结果。
#[derive(Debug, Clone)]
pub struct PsdPart {
    pub id: String,
    pub draw_order: i32,
    /// 画布像素 [x, y, w, h](紧致 alpha 包围盒)。
    pub canvas_rect: [u32; 4],
}

/// rig_import_psd 返回。
#[derive(Debug, Clone)]
pub struct PsdImported {
    pub asset_path: String,
    pub guid: String,
    pub atlas_guid: String,
    pub atlas_path: String,
    pub parts: Vec<PsdPart>,
    /// 如实告知的降级项(剪贴蒙版图层按普通部件导入、图层解码失败被跳过等)。
    pub warnings: Vec<String>,
}

/// 头部预检(在 psd crate 之前给出明确的不支持原因):返回 (宽, 高)。
fn check_psd_header(b: &[u8]) -> Result<(u32, u32)> {
    if b.len() < 26 || &b[0..4] != b"8BPS" {
        return Err(psd_unsupported("不是 PSD 文件(缺 8BPS 签名)"));
    }
    let be16 = |i: usize| u16::from_be_bytes([b[i], b[i + 1]]);
    let be32 = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    match be16(4) {
        1 => {}
        2 => return Err(psd_unsupported("PSB(大文档格式)不支持,请另存为 PSD")),
        v => return Err(psd_unsupported(format!("未知 PSD 版本 {v}"))),
    }
    let (h, w, depth, mode) = (be32(14), be32(18), be16(22), be16(24));
    if depth != 8 {
        return Err(psd_unsupported(format!("仅支持 8-bit 通道,实际 {depth}-bit;请导出 8-bit RGB PSD")));
    }
    if mode != 3 {
        let name = match mode {
            0 => "Bitmap",
            1 => "Grayscale",
            2 => "Indexed",
            4 => "CMYK",
            7 => "Multichannel",
            8 => "Duotone",
            9 => "Lab",
            _ => "未知",
        };
        return Err(psd_unsupported(format!("仅支持 RGB 颜色模式,实际 {name}({mode})")));
    }
    if w == 0 || h == 0 {
        return Err(psd_unsupported("PSD 宽高为 0"));
    }
    Ok((w, h))
}

/// 图层 / 组名 → 部件 id 片段(字母数字含 CJK、`_`、`-` 保留,其余替换为 `_`)。
fn sanitize_segment(s: &str) -> String {
    let t: String = s
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    if t.is_empty() {
        "layer".into()
    } else {
        t
    }
}

/// psd 0.3 未公开导出 `BlendMode` 类型路径,按其 Debug 名映射(其余模式退化为普通 alpha)。
fn psd_blend(mode_debug: &str) -> PartBlend {
    match mode_debug {
        "Multiply" => PartBlend::Multiply,
        "LinearDodge" => PartBlend::Additive,
        _ => PartBlend::Alpha,
    }
}

/// 按给定宽度做 shelf 排布 → (各矩形左上角, 所需高度)。
fn shelf_layout(sizes: &[(u32, u32)], pad: u32, width: u32) -> (Vec<(u32, u32)>, u32) {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|&a, &b| sizes[b].1.cmp(&sizes[a].1).then(a.cmp(&b)));
    let mut pos = vec![(0, 0); sizes.len()];
    let (mut x, mut y, mut shelf_h) = (pad, pad, 0u32);
    for i in order {
        let (w, h) = sizes[i];
        if x > pad && x + w + pad > width {
            y += shelf_h + pad;
            x = pad;
            shelf_h = 0;
        }
        pos[i] = (x, y);
        x += w + pad;
        shelf_h = shelf_h.max(h);
    }
    (pos, y + shelf_h + pad)
}

/// shelf 打包(≤ max²,间距 pad):返回 (位置, 图集宽, 图集高);放不下 → RIG_TOO_LARGE。
fn shelf_pack(sizes: &[(u32, u32)], pad: u32, max: u32) -> Result<(Vec<(u32, u32)>, u32, u32)> {
    let too_large = |m: String| AssetError::new("RIG_TOO_LARGE", m);
    let widest = sizes.iter().map(|s| s.0).max().unwrap_or(1) + 2 * pad;
    let tallest = sizes.iter().map(|s| s.1).max().unwrap_or(1) + 2 * pad;
    if widest > max || tallest > max {
        return Err(too_large(format!("单个图层 {widest}x{tallest}(含间距)超出图集上限 {max}²")));
    }
    let area: f64 = sizes.iter().map(|(w, h)| ((w + pad) as f64) * ((h + pad) as f64)).sum();
    let mut width = ((area * 1.15).sqrt().ceil() as u32).max(widest).min(max);
    loop {
        let (pos, h) = shelf_layout(sizes, pad, width);
        if h <= max {
            return Ok((pos, width, h));
        }
        if width >= max {
            return Err(too_large(format!("图层总面积超出图集上限 {max}²(请拆分 PSD 或缩小画布)")));
        }
        width = (width * 2).min(max);
    }
}

struct PsdLayerPix {
    id: String,
    rect: [u32; 4],
    rgba: Vec<u8>,
    opacity: f32,
    blend: PartBlend,
}

/// PSD → 图集 `Textures/<name>_atlas.png`(+ .meta,经 import 流程登记)+ `.rxrig`
/// (每个可见像素图层一个矩形部件;drawOrder = 自底向上序 × 10;canvas = PSD 尺寸,
/// origin = 底边中点)。
pub fn import_psd(project: &ForgeProject, psd_path: &Path, name: &str, dest_folder: &str) -> Result<PsdImported> {
    check_name(name)?;
    let bytes = std::fs::read(psd_path)
        .map_err(|e| AssetError::new("IO", format!("读 PSD 失败 {}: {e}", psd_path.display())))?;
    let (cw, ch) = check_psd_header(&bytes)?;
    // psd crate 对畸形输入可能 panic(越界切片 / unimplemented ZIP 压缩):一律转为不支持。
    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| psd::Psd::from_bytes(&bytes)))
        .map_err(|_| psd_unsupported("PSD 解析崩溃(可能含 ZIP 压缩通道或结构损坏)"))?;
    let doc_psd = parsed.map_err(|e| psd_unsupported(format!("PSD 解析失败: {e}")))?;

    // 注意:psd 0.3 的 `visible()` 直接取 flags 第 1 位,而 Photoshop 实际语义是「第 1 位 = 隐藏」
    // (常见可见图层 flags = 0x08)。因此可见 ⇔ `!visible()`。
    let groups = doc_psd.groups();
    let group_chain = |mut gid: Option<u32>| -> (Vec<String>, bool) {
        let mut names = vec![];
        let mut hidden = false;
        let mut guard = 0;
        while let Some(id) = gid {
            let Some(g) = groups.get(&id) else { break };
            names.push(sanitize_segment(g.name()));
            hidden |= g.visible();
            gid = g.parent_id();
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        names.reverse();
        (names, hidden)
    };

    let mut warnings = vec![];
    let mut layers: Vec<PsdLayerPix> = vec![];
    let mut used: HashSet<String> = HashSet::new();
    // layers() 为自顶向下序(crate 已把文件中的自底向上序反转);逆序遍历 = 自底向上。
    for layer in doc_psd.layers().iter().rev() {
        let lname = layer.name().to_string();
        let (mut segs, group_hidden) = group_chain(layer.parent_id());
        if layer.visible() || group_hidden || layer.opacity() == 0 {
            continue;
        }
        let rgba = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| layer.rgba())) {
            Ok(px) if px.len() >= (cw as usize) * (ch as usize) * 4 => px,
            _ => {
                warnings.push(format!("图层 {lname} 像素解码失败,已跳过"));
                continue;
            }
        };
        let x0 = layer.layer_left().max(0) as u32;
        let y0 = layer.layer_top().max(0) as u32;
        let x1 = ((layer.layer_right() + 1).max(0) as u32).min(cw);
        let y1 = ((layer.layer_bottom() + 1).max(0) as u32).min(ch);
        // 紧致 alpha 包围盒
        let (mut bx0, mut by0, mut bx1, mut by1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in y0..y1 {
            for x in x0..x1 {
                if rgba[((y * cw + x) * 4 + 3) as usize] > 0 {
                    bx0 = bx0.min(x);
                    by0 = by0.min(y);
                    bx1 = bx1.max(x + 1);
                    by1 = by1.max(y + 1);
                }
            }
        }
        if bx0 == u32::MAX {
            continue; // 无可见像素
        }
        let (w, h) = (bx1 - bx0, by1 - by0);
        let mut crop = Vec::with_capacity((w * h * 4) as usize);
        for y in by0..by1 {
            let s = ((y * cw + bx0) * 4) as usize;
            crop.extend_from_slice(&rgba[s..s + (w * 4) as usize]);
        }
        segs.push(sanitize_segment(&lname));
        let base = segs.join("/");
        let mut id = base.clone();
        let mut k = 2;
        while used.contains(&id) {
            id = format!("{base}_{k}");
            k += 1;
        }
        used.insert(id.clone());
        if !layer.is_clipping_mask() {
            // psd 0.3:is_clipping_mask() = 「剪贴基底」;false 表示该层被剪贴到下层。
            warnings.push(format!("图层 {lname} 为剪贴蒙版图层,v1 不支持蒙版,已按普通部件导入"));
        }
        layers.push(PsdLayerPix {
            id,
            rect: [bx0, by0, w, h],
            rgba: crop,
            opacity: (layer.opacity() as f32 / 255.0 * 1000.0).round() / 1000.0,
            blend: psd_blend(&format!("{:?}", layer.blend_mode())),
        });
    }
    if layers.is_empty() {
        return Err(psd_unsupported("PSD 中没有可见的像素图层(仅含合成图或全部隐藏)"));
    }

    let sizes: Vec<(u32, u32)> = layers.iter().map(|l| (l.rect[2], l.rect[3])).collect();
    let (pos, aw, ah) = shelf_pack(&sizes, ATLAS_PAD, ATLAS_MAX)?;
    let mut atlas = vec![0u8; (aw * ah * 4) as usize];
    for (l, &(ax, ay)) in layers.iter().zip(&pos) {
        let (w, h) = (l.rect[2], l.rect[3]);
        for y in 0..h {
            let s = (y * w * 4) as usize;
            let d = (((ay + y) * aw + ax) * 4) as usize;
            atlas[d..d + (w * 4) as usize].copy_from_slice(&l.rgba[s..s + (w * 4) as usize]);
        }
    }
    let atlas_rel = format!("Textures/{name}_atlas.png");
    let atlas_abs = project.content_root().join(&atlas_rel);
    write_png(&atlas_abs, aw, ah, atlas)?;
    let imported = crate::import::import_assets(project, &[atlas_abs.to_string_lossy().into_owned()], "Textures", None)?;
    let atlas_guid = match (imported.imported.first(), imported.failed.first()) {
        (Some(one), _) => one.guid.clone(),
        (None, Some(f)) => return Err(AssetError::new("IO", format!("图集登记失败: {}", f.error))),
        (None, None) => return Err(AssetError::new("IO", "图集登记失败")),
    };

    let mut doc = RigDoc::empty(&atlas_guid, [cw as f32, ch as f32]);
    let mut parts = vec![];
    for (k, (l, &(ax, ay))) in layers.iter().zip(&pos).enumerate() {
        let [x, y, w, h] = l.rect;
        let order = k as i32 * 10;
        let mut p = rect_part(l.id.clone(), order, [ax as f32, ay as f32, w as f32, h as f32], [x as f32, y as f32]);
        p.opacity = l.opacity;
        p.blend = l.blend;
        doc.parts.push(p);
        parts.push(PsdPart { id: l.id.clone(), draw_order: order, canvas_rect: l.rect });
    }
    let rel = format!("{}/{name}.rxrig", folder_or_default(dest_folder)?);
    let guid = write_rig_file(project, &rel, &doc, Some("user-import"))?;
    Ok(PsdImported { asset_path: rel, guid, atlas_guid, atlas_path: atlas_rel, parts, warnings })
}

/// 测试 / 工具用:构造最小 8-bit RGB 分层 PSD 字节(无压缩)。
///
/// `layers` 按**自底向上**(文件序)给出:(名字, [left, top, right, bottom) 画布像素, RGB 颜色, 可见)。
/// 用于在测试中生成 fixture,避免向仓库提交二进制 PSD。
pub fn build_test_psd(width: u32, height: u32, layers: &[(&str, [i32; 4], [u8; 3], bool)]) -> Vec<u8> {
    fn p16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    fn p32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    let mut out = vec![];
    out.extend_from_slice(b"8BPS");
    p16(&mut out, 1);
    out.extend_from_slice(&[0u8; 6]);
    p16(&mut out, 3); // 合成图通道数
    p32(&mut out, height);
    p32(&mut out, width);
    p16(&mut out, 8);
    p16(&mut out, 3); // RGB
    p32(&mut out, 0); // color mode data
    p32(&mut out, 0); // image resources

    let mut info = vec![];
    p16(&mut info, layers.len() as u16);
    let mut channel_data = vec![];
    for (name, [l, t, r, b], rgb, visible) in layers {
        let (w, h) = ((r - l).max(0) as u32, (b - t).max(0) as u32);
        let n = (w * h) as usize;
        for v in [*t, *l, *b, *r] {
            info.extend_from_slice(&v.to_be_bytes());
        }
        p16(&mut info, 4);
        for id in [0i16, 1, 2, -1] {
            info.extend_from_slice(&id.to_be_bytes());
            p32(&mut info, 2 + n as u32);
        }
        info.extend_from_slice(b"8BIMnorm");
        info.push(255); // opacity
        info.push(0); // clipping base
        info.push(if *visible { 0x08 } else { 0x0A }); // bit1 = 隐藏(Photoshop 实际语义)
        info.push(0);
        let nb = name.as_bytes();
        let mut pascal = vec![nb.len() as u8];
        pascal.extend_from_slice(nb);
        while pascal.len() % 4 != 0 {
            pascal.push(0);
        }
        p32(&mut info, (8 + pascal.len()) as u32);
        p32(&mut info, 0); // layer mask
        p32(&mut info, 0); // blending ranges
        info.extend_from_slice(&pascal);
        for c in [rgb[0], rgb[1], rgb[2], 255u8] {
            p16(&mut channel_data, 0);
            channel_data.extend(std::iter::repeat_n(c, n));
        }
    }
    info.extend_from_slice(&channel_data);
    if info.len() % 2 == 1 {
        info.push(0);
    }
    let mut lm = vec![];
    p32(&mut lm, info.len() as u32);
    lm.extend_from_slice(&info);
    p32(&mut lm, 0); // global layer mask info
    p32(&mut out, lm.len() as u32);
    out.extend_from_slice(&lm);

    p16(&mut out, 0); // 合成图:raw
    out.extend(std::iter::repeat_n(255u8, (width * height * 3) as usize));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(tag: &str) -> ForgeProject {
        let dir = std::env::temp_dir().join(format!(
            "assetd-rig-{tag}-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = ForgeProject::with_defaults(dir);
        p.ensure_dirs().unwrap();
        p
    }

    /// 64x32 品红底图集:左块 24x24 绿色圆角、右块 20x16 蓝色。
    fn put_atlas(p: &ForgeProject, name: &str) -> String {
        let mut img = image::RgbaImage::from_pixel(64, 32, image::Rgba([255, 0, 255, 255]));
        for y in 4..28 {
            for x in 4..28 {
                let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
                if dx * dx + dy * dy <= 12.0 * 12.0 {
                    img.put_pixel(x, y, image::Rgba([30, 200, 40, 255]));
                }
            }
        }
        for y in 8..24 {
            for x in 36..56 {
                img.put_pixel(x, y, image::Rgba([20, 40, 220, 255]));
            }
        }
        let abs = p.content_root().join("Textures").join(name);
        img.save(&abs).unwrap();
        let (_, meta) = crate::meta::ensure_meta(&p.content_root(), &format!("Textures/{name}")).unwrap();
        meta.guid
    }

    fn sheet(p: &ForgeProject, tex: &str) -> RigCreated {
        let parts = vec![
            SheetPart { name: "face".into(), bbox: [4, 4, 24, 24], position: [20.0, 10.0], draw_order: None },
            SheetPart { name: "body".into(), bbox: [36, 8, 20, 16], position: [22.0, 40.0], draw_order: Some(-5) },
        ];
        parts_from_sheet(p, tex, "hero", [64.0, 64.0], &parts, "", None).unwrap()
    }

    #[test]
    fn create_rig_defaults_canvas_and_merges_template() {
        let p = temp_project("create");
        let tex = put_atlas(&p, "atlas.png");
        let c = create_rig(&p, "", "Hero", &tex, None, Some("bust"), None).unwrap();
        assert_eq!(c.asset_path, "Rigs/Hero.rxrig");
        let doc = load_rel(&p, &c.asset_path).unwrap();
        assert_eq!(doc.canvas.size, [64.0, 32.0], "canvas 缺省 = 贴图尺寸");
        assert_eq!(doc.canvas.origin, [32.0, 32.0], "origin = 底边中点");
        assert!(doc.bone("head").is_some() && doc.param("ParamAngleX").is_some());
        let meta = MetaDoc::load(&meta_path_for(&p.content_root(), &c.asset_path)).unwrap();
        assert_eq!(meta.atype, "rig");
        assert_eq!(meta.guid, c.guid);
        assert_eq!(meta.provenance.unwrap().origin, "user-import");

        // 重建同名:GUID 稳定
        let again = create_rig(&p, "", "Hero", &tex, Some([128.0, 128.0]), None, None).unwrap();
        assert_eq!(again.guid, c.guid);
        // 名字 / 贴图校验
        assert_eq!(create_rig(&p, "", "a.b", &tex, None, None, None).unwrap_err().code, "INVALID_OPS");
        assert_eq!(create_rig(&p, "", "X", "nope", None, None, None).unwrap_err().code, "UNKNOWN_GUID");
        assert_eq!(create_rig(&p, "", "X", &c.guid, None, None, None).unwrap_err().code, "WRONG_TYPE");
        // 解析:guid / 带 Content 前缀的路径
        assert_eq!(resolve_rig(&p, None, Some(&c.guid)).unwrap().0, "Rigs/Hero.rxrig");
        assert_eq!(resolve_rig(&p, Some("Content/Rigs/Hero.rxrig"), None).unwrap().1, c.guid);
    }

    #[test]
    fn sheet_mesh_weights_patch_evaluate_preview() {
        let p = temp_project("flow");
        let tex = put_atlas(&p, "atlas.png");
        let c = sheet(&p, &tex);
        assert_eq!(c.stats.parts, 2);
        let rel = c.asset_path.clone();

        // 自动网格:顶点贴合圆形轮廓,保持摆放平移
        let m = mesh_auto_parts(&p, &rel, Some(&["face".to_string()]), &MeshAutoOpts { interior_spacing: 8.0, ..Default::default() }).unwrap();
        assert_eq!(m.parts.len(), 1);
        assert!(m.parts[0].vertices > 4, "{m:?}");
        let doc = load_rel(&p, &rel).unwrap();
        let face = doc.part("face").unwrap();
        for (v, u) in face.mesh.vertices.iter().zip(&face.mesh.uvs) {
            assert!(((v[0] - u[0]) - 16.0).abs() < 1e-3 && ((v[1] - u[1]) - 6.0).abs() < 1e-3);
        }

        // 骨架 + 权重
        let ops = json!([{ "op": "merge_scaffold", "template": "bust" }]);
        let dry = patch_rig(&p, &rel, &ops, true).unwrap();
        assert!(dry.dry_run && dry.stats.bones > 0);
        assert_eq!(load_rel(&p, &rel).unwrap().bones.len(), 0, "dryRun 不落盘");
        patch_rig(&p, &rel, &ops, false).unwrap();
        let w = weights_auto_parts(&p, &rel, None, &WeightOpts::default()).unwrap();
        assert_eq!(w.parts.len(), 2);
        let total: usize = w.parts[0].dominant_bones.values().sum();
        assert_eq!(total, load_rel(&p, &rel).unwrap().part("face").unwrap().mesh.vertices.len());

        // 重跑网格 → 权重被清
        let m2 = mesh_auto_parts(&p, &rel, Some(&["face".to_string()]), &MeshAutoOpts::default()).unwrap();
        assert_eq!(m2.weights_cleared, vec!["face".to_string()]);

        // keyform:MouthOpen=1 时 body 整体右移 5px
        let n = load_rel(&p, &rel).unwrap().part("body").unwrap().mesh.vertices.len();
        let offs: Vec<[f32; 2]> = vec![[5.0, 0.0]; n];
        let ops = json!([
            { "op": "set_weights", "part": "body", "bone": "body" },
            { "op": "set_keyform", "target": "part:body", "params": ["ParamMouthOpenY"], "keys": [[0.0, 1.0]],
              "at": { "ParamMouthOpenY": 1.0 }, "form": { "offsets": offs } },
            { "op": "remove_clip", "name": "nosuch" }
        ]);
        assert!(patch_rig(&p, &rel, &ops, false).is_err(), "任一失败整体回滚");
        let ops = json!([
            { "op": "set_weights", "part": "body", "bone": "body" },
            { "op": "set_keyform", "target": "part:body", "params": ["ParamMouthOpenY"], "keys": [[0.0, 1.0]],
              "at": { "ParamMouthOpenY": 1.0 }, "form": { "offsets": offs } }
        ]);
        let out = patch_rig(&p, &rel, &ops, false).unwrap();
        assert_eq!(out.applied, 2);
        assert!(out.destructive.is_empty());
        let doc = load_rel(&p, &rel).unwrap();
        let rest = evaluate_json(&doc, &BTreeMap::new(), None, 0.0, Some(&["body".to_string()])).unwrap();
        let mut params = BTreeMap::new();
        params.insert("ParamMouthOpenY".to_string(), 1.0);
        params.insert("NoSuch".to_string(), 1.0);
        let open = evaluate_json(&doc, &params, None, 0.0, Some(&["body".to_string()])).unwrap();
        let x0 = rest["parts"][0]["positions"][0][0].as_f64().unwrap();
        let x1 = open["parts"][0]["positions"][0][0].as_f64().unwrap();
        assert!((x1 - x0 - 5.0).abs() < 0.01, "{x0} → {x1}");
        assert_eq!(open["unknown"], json!(["param:NoSuch"]));
        assert_eq!(open["parts"].as_array().unwrap().len(), 1);

        // 视图省略
        let view = doc_view(&doc, Some(&["parts".into(), "keyforms".into()]), false).unwrap();
        assert!(view["parts"][0]["mesh"]["vertexCount"].as_u64().unwrap() >= 3);
        assert_eq!(view["keyforms"].as_array().unwrap().last().unwrap()["forms"][1]["offsets"]["count"], n);
        assert!(view.get("bones").is_none());
        assert!(doc_view(&doc, Some(&["nope".into()]), false).is_err());

        // 预览:PNG 落盘;与自身渲染比较 IoU = 1
        let req = PreviewRequest { size: Some(128), ..Default::default() };
        let pv = preview_rig(&p, &rel, &c.guid, &req).unwrap();
        let abs = p.root.join(&pv.file_ref);
        assert!(abs.is_file() && pv.width == 128);
        let reference = p.root.join("ref.png");
        let rgba = image::open(&abs).unwrap().to_rgba8();
        // 参考图视为 canvas 尺寸:把 128² 渲染图(canvas 64² 等比填满)直接当参考
        rgba.save(&reference).unwrap();
        let req = PreviewRequest { size: Some(128), compare_to: Some("ref.png".into()), overlay: Some("mesh".into()), ..Default::default() };
        let pv2 = preview_rig(&p, &rel, &c.guid, &req).unwrap();
        assert!(pv2.silhouette_iou.unwrap() > 0.99, "{pv2:?}");
        // 网格拼图
        let mut grid = BTreeMap::new();
        grid.insert("ParamMouthOpenY".to_string(), vec![0.0, 1.0]);
        let req = PreviewRequest { size: Some(64), grid: Some(grid), ..Default::default() };
        let pv3 = preview_rig(&p, &rel, &c.guid, &req).unwrap();
        assert_eq!((pv3.width, pv3.height), (128, 64));
        // overlay 未知骨骼
        let req = PreviewRequest { overlay: Some("weights:nope".into()), ..Default::default() };
        assert_eq!(preview_rig(&p, &rel, &c.guid, &req).unwrap_err().code, "RIG_INVALID");
    }

    #[test]
    fn write_doc_and_validate() {
        let p = temp_project("set");
        let tex = put_atlas(&p, "atlas.png");
        let c = sheet(&p, &tex);
        let doc = load_rel(&p, &c.asset_path).unwrap();
        let mut v = serde_json::to_value(&doc).unwrap();
        v["parts"][0]["drawOrder"] = json!(99);
        let parsed = write_rig_doc(&p, &c.asset_path, &v).unwrap();
        assert_eq!(parsed.part("face").unwrap().draw_order, 99);
        let mut bad = v.clone();
        bad["parts"][0]["mesh"]["triangles"] = json!([[0, 1, 9]]);
        assert_eq!(write_rig_doc(&p, &c.asset_path, &bad).unwrap_err().code, "RIG_INVALID");
        let mut bad_tex = v.clone();
        bad_tex["texture"] = json!("nope");
        assert_eq!(validate_value(&p, &bad_tex).unwrap_err().code, "UNKNOWN_GUID");
        // 导入路径:坏 .rxrig 导入失败
        let src = p.root.join("broken.rxrig");
        std::fs::write(&src, "{\"version\": 2}").unwrap();
        let out = crate::import::import_assets(&p, &[src.to_string_lossy().into_owned()], "Rigs", None).unwrap();
        assert!(out.imported.is_empty() && out.failed.len() == 1, "{out:?}");
    }

    #[test]
    fn psd_import_three_layers() {
        let p = temp_project("psd");
        // 自底向上:body(红)/ face(绿)/ hidden(隐藏)/ eye(蓝,同名组路径非法字符)
        let bytes = build_test_psd(
            40,
            30,
            &[
                ("body", [5, 15, 35, 30], [200, 20, 20], true),
                ("face", [10, 2, 30, 18], [20, 200, 20], true),
                ("hidden", [0, 0, 4, 4], [9, 9, 9], false),
                ("eye L.1", [14, 6, 18, 9], [20, 20, 220], true),
            ],
        );
        let psd_abs = p.root.join("char.psd");
        std::fs::write(&psd_abs, &bytes).unwrap();
        let out = import_psd(&p, &psd_abs, "Char", "").unwrap();
        assert_eq!(out.asset_path, "Rigs/Char.rxrig");
        assert_eq!(out.atlas_path, "Textures/Char_atlas.png");
        let ids: Vec<&str> = out.parts.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["body", "face", "eye_L_1"], "隐藏图层跳过、自底向上");
        assert_eq!(out.parts[2].draw_order, 20);
        assert_eq!(out.parts[0].canvas_rect, [5, 15, 30, 15]);

        let doc = load_rel(&p, &out.asset_path).unwrap();
        assert_eq!(doc.texture, out.atlas_guid);
        assert_eq!(doc.canvas.size, [40.0, 30.0]);
        assert_eq!(doc.canvas.origin, [20.0, 30.0]);
        let atlas_meta = MetaDoc::load(&meta_path_for(&p.content_root(), &out.atlas_path)).unwrap();
        assert_eq!(atlas_meta.atype, "texture");
        assert_eq!(atlas_meta.guid, out.atlas_guid);

        // 图集像素 = 图层颜色(uv 处取样)
        let (aw, _, atlas) = decode_atlas(&p, &doc.texture).unwrap();
        let face = doc.part("face").unwrap();
        let [u, v] = face.mesh.uvs[0];
        let i = ((v as u32 * aw + u as u32) * 4) as usize;
        assert_eq!(&atlas[i..i + 4], &[20, 200, 20, 255]);
        assert_eq!(face.mesh.vertices[0], [10.0, 2.0]);

        // 不支持的格式
        let mut psb = bytes.clone();
        psb[5] = 2;
        std::fs::write(&psd_abs, &psb).unwrap();
        assert_eq!(import_psd(&p, &psd_abs, "Char", "").unwrap_err().code, "RIG_PSD_UNSUPPORTED");
        let mut cmyk = bytes.clone();
        cmyk[25] = 4;
        std::fs::write(&psd_abs, &cmyk).unwrap();
        assert_eq!(import_psd(&p, &psd_abs, "Char", "").unwrap_err().code, "RIG_PSD_UNSUPPORTED");
        let mut d16 = bytes.clone();
        d16[23] = 16;
        std::fs::write(&psd_abs, &d16).unwrap();
        assert_eq!(import_psd(&p, &psd_abs, "Char", "").unwrap_err().code, "RIG_PSD_UNSUPPORTED");
        std::fs::write(&psd_abs, &bytes[..60]).unwrap();
        assert_eq!(import_psd(&p, &psd_abs, "Char", "").unwrap_err().code, "RIG_PSD_UNSUPPORTED");
    }

    #[test]
    fn shelf_pack_limits() {
        let (pos, w, h) = shelf_pack(&[(10, 10), (20, 5), (5, 30)], 2, 4096).unwrap();
        assert_eq!(pos.len(), 3);
        assert!(w <= 4096 && h <= 4096);
        // 不重叠
        let sizes = [(10u32, 10u32), (20, 5), (5, 30)];
        for i in 0..3 {
            for j in i + 1..3 {
                let (a, b) = (pos[i], pos[j]);
                let overlap = a.0 < b.0 + sizes[j].0 && b.0 < a.0 + sizes[i].0 && a.1 < b.1 + sizes[j].1 && b.1 < a.1 + sizes[i].1;
                assert!(!overlap);
            }
        }
        assert_eq!(shelf_pack(&[(5000, 10)], 2, 4096).unwrap_err().code, "RIG_TOO_LARGE");
        assert_eq!(shelf_pack(&vec![(4000, 1500); 3], 2, 4096).unwrap_err().code, "RIG_TOO_LARGE");
    }
}
