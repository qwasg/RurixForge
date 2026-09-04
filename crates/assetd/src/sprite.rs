//! 精灵资产(F-GAME-4):`.rxsprite` JSON 图集定义——单张贴图 + 逐帧紧致 bbox +
//! pivot 级联(帧级 > 文档级 > 缺省脚底锚)+ 动画 clip + 可选 animator 状态机。
//!
//! 设计对齐 VibeGame manifest/animation 语义(D-031):
//! - 帧 = 图集内任意矩形(非等格网格,适配 AI 生成的不规则表);
//! - clip 时长:`duration`(总秒数,帧数可变时手感不漂移)优先于 `fps`(缺省 10);
//! - animator:参数驱动状态机,转换表有序首匹配,trigger 触发即消费,
//!   `hasExitTime` 等当前 clip 播完;
//! - 自动切帧:连通域检测,背景判定与视口色键**同规则**(alpha<0.02 或品红族
//!   g < 0.5*min(r,b),见 engine-host viewport.rs FS_TEX_WGSL)。
//!
//! .rxsprite 格式(JSON,确定性键序写出):
//! ```json
//! { "version": 1, "texture": "<texture guid>", "pivot": [0.5, 1.0],
//!   "frames": { "walk_0": { "bbox": [2, 11, 86, 131] } },
//!   "clips": { "walk": { "frames": ["walk_0"], "fps": 8, "loop": true } },
//!   "animator": { "defaultState": "idle", "parameters": {}, "states": {}, "transitions": [] } }
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::meta::{MetaDoc, Provenance};
use crate::project::ForgeProject;
use crate::{meta_path_for, new_guid, normalize_rel, AssetError, Result};

/// 文档级缺省 pivot:底心(脚底锚,地面角色正确值;VFX/飞行体显式写 [0.5,0.5])。
pub fn default_pivot() -> [f32; 2] {
    [0.5, 1.0]
}

fn default_fps() -> f32 {
    10.0
}

fn default_true() -> bool {
    true
}

fn default_on_finish() -> String {
    "hold".to_string()
}

/// 帧定义:图集内紧致 bbox(像素 [x, y, w, h])+ 可选帧级 pivot 覆盖。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteFrame {
    pub bbox: [u32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot: Option<[f32; 2]>,
}

/// 动画 clip:帧名序列 + 时长(duration 优先于 fps)+ 循环/收尾行为。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteClip {
    pub frames: Vec<String>,
    #[serde(default = "default_fps")]
    pub fps: f32,
    /// 总时长秒(设置时逐帧时长 = duration / 帧数,优先于 fps)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(rename = "loop", default = "default_true")]
    pub looped: bool,
    /// 非循环收尾:"hold"(停末帧,缺省)| "first"(回首帧)。
    #[serde(rename = "onFinish", default = "default_on_finish")]
    pub on_finish: String,
}

impl SpriteClip {
    /// 单帧时长秒(duration 优先;fps 兜底,非法值钳制防除零)。
    pub fn frame_duration(&self) -> f32 {
        if let Some(d) = self.duration {
            if d > 0.0 && !self.frames.is_empty() {
                return d / self.frames.len() as f32;
            }
        }
        1.0 / self.fps.max(0.0001)
    }
}

/// animator 状态(引用 clip 名)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimatorState {
    pub clip: String,
}

/// 转换来源:单状态名 / 状态名数组;"Any" 通配。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FromSpec {
    One(String),
    Many(Vec<String>),
}

impl FromSpec {
    /// 是否匹配当前状态("Any" 恒匹配)。
    pub fn matches(&self, state: &str) -> bool {
        match self {
            FromSpec::One(s) => s == "Any" || s == state,
            FromSpec::Many(v) => v.iter().any(|s| s == "Any" || s == state),
        }
    }

    fn names(&self) -> Vec<&str> {
        match self {
            FromSpec::One(s) => vec![s.as_str()],
            FromSpec::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

/// 转换条件:bool 参数比较 / trigger 消费(untagged:字段形状区分)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnimatorCond {
    Param { param: String, eq: bool },
    Trigger { trigger: String },
}

/// 状态转换:from(有序表首匹配)→ to;when 全部满足;hasExitTime 等 clip 播完。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimatorTransition {
    pub from: FromSpec,
    pub to: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<AnimatorCond>,
    #[serde(rename = "hasExitTime", default)]
    pub has_exit_time: bool,
}

/// animator 状态机定义(JSON 数据驱动;宿主运行时逐实体持独立状态)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteAnimator {
    #[serde(rename = "defaultState")]
    pub default_state: String,
    /// 参数名 → "bool" | "trigger"。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    pub states: BTreeMap<String, AnimatorState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<AnimatorTransition>,
}

/// .rxsprite 文档(单一事实源:贴图引用 + 帧 + clip + 可选状态机)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteDoc {
    pub version: u32,
    /// 图集贴图 GUID(单张;多动作一律进同一张表,identity 一致性纪律)。
    pub texture: String,
    #[serde(default = "default_pivot")]
    pub pivot: [f32; 2],
    #[serde(default)]
    pub frames: BTreeMap<String, SpriteFrame>,
    #[serde(default)]
    pub clips: BTreeMap<String, SpriteClip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animator: Option<SpriteAnimator>,
}

impl SpriteDoc {
    /// 解析 pivot 级联:帧级 > 文档级(文档级带缺省 [0.5,1])。
    pub fn resolve_pivot(&self, frame_name: &str) -> [f32; 2] {
        self.frames
            .get(frame_name)
            .and_then(|f| f.pivot)
            .unwrap_or(self.pivot)
    }

    /// clip 第 idx 帧的帧定义(越界如实 None)。
    pub fn clip_frame(&self, clip_name: &str, idx: usize) -> Option<(&str, &SpriteFrame)> {
        let clip = self.clips.get(clip_name)?;
        let name = clip.frames.get(idx)?;
        self.frames.get(name).map(|f| (name.as_str(), f))
    }

    /// 首个可用帧名(编辑态无 clip 指定时的显示帧:frames 键字典序首个)。
    pub fn first_frame_name(&self) -> Option<&str> {
        self.frames.keys().next().map(String::as_str)
    }
}

fn pivot_ok(p: &[f32; 2]) -> bool {
    (0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1])
}

/// 语义校验(serde 解析后的第二道门)。
fn validate_doc(doc: &SpriteDoc) -> Result<()> {
    let inv = |msg: String| AssetError::new("SPRITE_INVALID", msg);
    if doc.version != 1 {
        return Err(inv(format!(".rxsprite version 须为 1,实际 {}", doc.version)));
    }
    if doc.texture.is_empty() {
        return Err(inv(".rxsprite texture 须为非空 GUID".into()));
    }
    if !pivot_ok(&doc.pivot) {
        return Err(inv(format!(".rxsprite pivot 须在 [0,1]²,实际 {:?}", doc.pivot)));
    }
    for (name, f) in &doc.frames {
        if name.is_empty() {
            return Err(inv("帧名不得为空".into()));
        }
        if f.bbox[2] == 0 || f.bbox[3] == 0 {
            return Err(inv(format!("帧 {name} bbox 宽高须 > 0,实际 {:?}", f.bbox)));
        }
        if let Some(p) = &f.pivot {
            if !pivot_ok(p) {
                return Err(inv(format!("帧 {name} pivot 须在 [0,1]²,实际 {p:?}")));
            }
        }
    }
    for (cname, clip) in &doc.clips {
        if clip.frames.is_empty() {
            return Err(inv(format!("clip {cname} frames 不得为空")));
        }
        for fname in &clip.frames {
            if !doc.frames.contains_key(fname) {
                return Err(inv(format!("clip {cname} 引用不存在的帧: {fname}")));
            }
        }
        if !(clip.fps > 0.0) {
            return Err(inv(format!("clip {cname} fps 须 > 0,实际 {}", clip.fps)));
        }
        if let Some(d) = clip.duration {
            if !(d > 0.0) {
                return Err(inv(format!("clip {cname} duration 须 > 0,实际 {d}")));
            }
        }
        if clip.on_finish != "hold" && clip.on_finish != "first" {
            return Err(inv(format!(
                "clip {cname} onFinish 须为 hold|first,实际 {:?}",
                clip.on_finish
            )));
        }
    }
    if let Some(a) = &doc.animator {
        if a.states.is_empty() {
            return Err(inv("animator states 不得为空".into()));
        }
        for (sname, st) in &a.states {
            if !doc.clips.contains_key(&st.clip) {
                return Err(inv(format!(
                    "animator 状态 {sname} 引用不存在的 clip: {}",
                    st.clip
                )));
            }
        }
        if !a.states.contains_key(&a.default_state) {
            return Err(inv(format!(
                "animator defaultState 不存在: {}",
                a.default_state
            )));
        }
        for (pname, kind) in &a.parameters {
            if kind != "bool" && kind != "trigger" {
                return Err(inv(format!(
                    "animator 参数 {pname} 类型须为 bool|trigger,实际 {kind:?}"
                )));
            }
        }
        for (i, t) in a.transitions.iter().enumerate() {
            if !a.states.contains_key(&t.to) {
                return Err(inv(format!("transition[{i}] to 状态不存在: {}", t.to)));
            }
            for name in t.from.names() {
                if name != "Any" && !a.states.contains_key(name) {
                    return Err(inv(format!("transition[{i}] from 状态不存在: {name}")));
                }
            }
            for c in &t.when {
                match c {
                    AnimatorCond::Param { param, .. } => {
                        if a.parameters.get(param).map(String::as_str) != Some("bool") {
                            return Err(inv(format!(
                                "transition[{i}] 条件引用的 bool 参数不存在: {param}"
                            )));
                        }
                    }
                    AnimatorCond::Trigger { trigger } => {
                        if a.parameters.get(trigger).map(String::as_str) != Some("trigger") {
                            return Err(inv(format!(
                                "transition[{i}] 条件引用的 trigger 参数不存在: {trigger}"
                            )));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// 解析 + 校验 .rxsprite JSON 文档。
pub fn parse_rxsprite(v: &Value) -> Result<SpriteDoc> {
    let doc: SpriteDoc = serde_json::from_value(v.clone())
        .map_err(|e| AssetError::new("SPRITE_INVALID", format!(".rxsprite 解析失败: {e}")))?;
    validate_doc(&doc)?;
    Ok(doc)
}

/// 校验 .rxsprite JSON(不需要文档时)。
pub fn validate_rxsprite(v: &Value) -> Result<()> {
    parse_rxsprite(v).map(|_| ())
}

/// 从磁盘读 .rxsprite(解析 + 校验)。
pub fn load_rxsprite(path: &Path) -> Result<SpriteDoc> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| AssetError::new("IO", format!("读 .rxsprite 失败 {}: {e}", path.display())))?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| AssetError::new("SPRITE_INVALID", format!(".rxsprite 非合法 JSON: {e}")))?;
    parse_rxsprite(&v)
}

/// sprite_create 返回。
#[derive(Debug, Clone)]
pub struct SpriteCreated {
    pub asset_path: String,
    pub guid: String,
    pub texture_guid: String,
    pub frame_count: usize,
    pub clip_count: usize,
}

/// 收集项目内全部已知 GUID → 资产类型(纹理存在性/类型校验用)。
fn known_guid_types(project: &ForgeProject) -> Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
    for rel in project.scan_content()? {
        let mp = meta_path_for(&project.content_root(), &rel);
        if mp.is_file() {
            if let Ok(m) = MetaDoc::load(&mp) {
                map.insert(m.guid, m.atype);
            }
        }
    }
    Ok(map)
}

/// 创建精灵资产:写 .rxsprite(确定性 JSON)+ .meta(已存在则复用 GUID,reimport 语义)。
/// texture GUID 须存在且为 texture 类型(引用图重建自动成边 sprite→texture)。
#[allow(clippy::too_many_arguments)]
pub fn create_sprite(
    project: &ForgeProject,
    dest_folder: &str,
    name: &str,
    texture_guid: &str,
    pivot: Option<[f32; 2]>,
    frames: Option<&Map<String, Value>>,
    clips: Option<&Map<String, Value>>,
    animator: Option<&Value>,
) -> Result<SpriteCreated> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('.') {
        return Err(AssetError::new(
            "INVALID_OPS",
            format!("精灵名非法(禁含 / \\ .): {name}"),
        ));
    }
    let known = known_guid_types(project)?;
    match known.get(texture_guid) {
        Some(t) if t == "texture" => {}
        Some(t) => {
            return Err(AssetError::new(
                "WRONG_TYPE",
                format!("texture 引用的 GUID 不是贴图(实际 {t}): {texture_guid}"),
            ))
        }
        None => {
            return Err(AssetError::new(
                "UNKNOWN_GUID",
                format!("texture 引用的 GUID 不存在: {texture_guid}"),
            ))
        }
    }

    let folder = if dest_folder.is_empty() {
        "Sprites".to_string()
    } else {
        normalize_rel(dest_folder)?
    };
    let rel = format!("{folder}/{name}.rxsprite");

    let mut doc = serde_json::Map::new();
    doc.insert("version".into(), Value::from(1u32));
    doc.insert("texture".into(), Value::from(texture_guid));
    let piv = pivot.unwrap_or_else(default_pivot);
    doc.insert("pivot".into(), serde_json::json!([piv[0], piv[1]]));
    doc.insert(
        "frames".into(),
        Value::Object(frames.cloned().unwrap_or_default()),
    );
    doc.insert(
        "clips".into(),
        Value::Object(clips.cloned().unwrap_or_default()),
    );
    if let Some(a) = animator {
        if !a.is_null() {
            doc.insert("animator".into(), a.clone());
        }
    }
    let doc = Value::Object(doc);
    let parsed = parse_rxsprite(&doc)?;

    // 确定性写出:serde_json Map 默认 BTreeMap(键字典序)+ 两空格缩进 + 末尾换行。
    let text = serde_json::to_string_pretty(&doc)
        .map_err(|e| AssetError::new("META_SERIALIZE", format!(".rxsprite 序列化失败: {e}")))?;
    let abs = project.content_root().join(&rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&abs, format!("{text}\n"))?;

    let meta_path = meta_path_for(&project.content_root(), &rel);
    let mut meta = if meta_path.is_file() {
        MetaDoc::load(&meta_path)?
    } else {
        MetaDoc::new(&rel, new_guid())?
    };
    if meta.provenance.is_none() {
        meta.provenance = Some(Provenance {
            origin: "user-import".into(),
            detail: None,
        });
    }
    meta.build_state = Some("current".into());
    meta.save(&meta_path)?;

    Ok(SpriteCreated {
        asset_path: rel,
        guid: meta.guid,
        texture_guid: texture_guid.to_string(),
        frame_count: parsed.frames.len(),
        clip_count: parsed.clips.len(),
    })
}

/// 覆盖写 .rxsprite 文档(sprite_set:校验后整文档写入;.meta 不动,GUID 稳定)。
pub fn write_sprite_doc(project: &ForgeProject, rel_path: &str, doc: &Value) -> Result<SpriteDoc> {
    let rel = normalize_rel(rel_path)?;
    if !rel.ends_with(".rxsprite") {
        return Err(AssetError::new(
            "WRONG_TYPE",
            format!("sprite_set 仅接受 .rxsprite: {rel}"),
        ));
    }
    let parsed = parse_rxsprite(doc)?;
    let meta_path = meta_path_for(&project.content_root(), &rel);
    if !meta_path.is_file() {
        return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
    }
    let text = serde_json::to_string_pretty(doc)
        .map_err(|e| AssetError::new("META_SERIALIZE", format!(".rxsprite 序列化失败: {e}")))?;
    let abs = project.content_root().join(&rel);
    std::fs::write(&abs, format!("{text}\n"))?;
    Ok(parsed)
}

// ---------- 自动切帧(连通域检测) ----------

/// 切帧选项。
#[derive(Debug, Clone, Copy)]
pub struct SliceOptions {
    /// alpha 低于此值视为背景(缺省 5 ≈ shader 的 0.02×255)。
    pub alpha_threshold: u8,
    /// 连通域像素数下限(过滤噪点,缺省 16)。
    pub min_area: u32,
    /// 帧数上限保护(超限如实报错,提示调高 min_area;缺省 256)。
    pub max_frames: usize,
}

impl Default for SliceOptions {
    fn default() -> Self {
        SliceOptions {
            alpha_threshold: 5,
            min_area: 16,
            max_frames: 256,
        }
    }
}

/// 背景判定:与视口 FS_TEX_WGSL 色键**同规则**——alpha 过低,或品红族
/// (G 显著低于 R/B 两者:g < 0.5 * min(r, b))。
fn is_background(px: &[u8], alpha_threshold: u8) -> bool {
    let (r, g, b, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3]);
    if a < alpha_threshold {
        return true;
    }
    2 * g < r.min(b)
}

/// RGBA8 图像连通域检测(4 连通,迭代栈防深递归)→ 按行带分组、行内 x 排序的
/// 紧致 bbox 列表([x, y, w, h])。确定性:同图同参恒同序。
pub fn autoslice_image(w: u32, h: u32, rgba: &[u8], opts: SliceOptions) -> Result<Vec<[u32; 4]>> {
    let (wi, hi) = (w as usize, h as usize);
    if rgba.len() < wi * hi * 4 {
        return Err(AssetError::new(
            "SPRITE_SLICE",
            format!("像素缓冲不足: {}x{} 需 {} 字节,实际 {}", w, h, wi * hi * 4, rgba.len()),
        ));
    }
    let mut visited = vec![false; wi * hi];
    let mut boxes: Vec<([u32; 4], u32)> = Vec::new(); // (bbox, 像素数)

    for start in 0..wi * hi {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let px = &rgba[start * 4..start * 4 + 4];
        if is_background(px, opts.alpha_threshold) {
            continue;
        }
        // flood fill(4 连通)。
        let (mut min_x, mut min_y, mut max_x, mut max_y) =
            (start % wi, start / wi, start % wi, start / wi);
        let mut count = 0u32;
        let mut stack = vec![start];
        while let Some(idx) = stack.pop() {
            count += 1;
            let (x, y) = (idx % wi, idx / wi);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            let mut try_push = |nx: usize, ny: usize| {
                let nidx = ny * wi + nx;
                if !visited[nidx] {
                    visited[nidx] = true;
                    let npx = &rgba[nidx * 4..nidx * 4 + 4];
                    if !is_background(npx, opts.alpha_threshold) {
                        stack.push(nidx);
                    }
                }
            };
            if x > 0 {
                try_push(x - 1, y);
            }
            if x + 1 < wi {
                try_push(x + 1, y);
            }
            if y > 0 {
                try_push(x, y - 1);
            }
            if y + 1 < hi {
                try_push(x, y + 1);
            }
        }
        if count >= opts.min_area {
            boxes.push((
                [
                    min_x as u32,
                    min_y as u32,
                    (max_x - min_x + 1) as u32,
                    (max_y - min_y + 1) as u32,
                ],
                count,
            ));
        }
    }

    if boxes.len() > opts.max_frames {
        return Err(AssetError::new(
            "SPRITE_SLICE",
            format!(
                "检出 {} 个连通域超上限 {}(疑为噪点,建议调高 minArea,当前 {})",
                boxes.len(),
                opts.max_frames,
                opts.min_area
            ),
        ));
    }

    // 行带分组:按中心 y 排序,中心 y 落入当前行带(带高 = 行内最大帧高)则同行;
    // 行间按 y、行内按 x,得到自然的"从左到右、从上到下"帧序。
    let mut items: Vec<[u32; 4]> = boxes.into_iter().map(|(b, _)| b).collect();
    items.sort_by_key(|b| (b[1] + b[3] / 2, b[0]));
    let mut rows: Vec<(u32, u32, Vec<[u32; 4]>)> = Vec::new(); // (band_top, band_bottom, boxes)
    for b in items {
        let cy = b[1] + b[3] / 2;
        match rows.last_mut() {
            Some((top, bottom, row)) if cy >= *top && cy <= *bottom => {
                *top = (*top).min(b[1]);
                *bottom = (*bottom).max(b[1] + b[3]);
                row.push(b);
            }
            _ => rows.push((b[1], b[1] + b[3], vec![b])),
        }
    }
    let mut out = Vec::new();
    for (_, _, mut row) in rows {
        row.sort_by_key(|b| (b[0], b[1]));
        out.extend(row);
    }
    Ok(out)
}

/// autoslice 返回(贴图尺寸 + bbox 列表)。
#[derive(Debug, Clone)]
pub struct SliceOutcome {
    pub width: u32,
    pub height: u32,
    pub boxes: Vec<[u32; 4]>,
}

/// 对项目内贴图资产做自动切帧(解码 RGBA → 连通域)。
pub fn autoslice_texture(
    project: &ForgeProject,
    rel_path: &str,
    opts: SliceOptions,
) -> Result<SliceOutcome> {
    let rel = normalize_rel(rel_path)?;
    let meta_path = meta_path_for(&project.content_root(), &rel);
    if !meta_path.is_file() {
        return Err(AssetError::new("NO_META", format!("缺 .meta: {rel}")));
    }
    let meta = MetaDoc::load(&meta_path)?;
    if meta.atype != "texture" {
        return Err(AssetError::new(
            "WRONG_TYPE",
            format!("sprite_autoslice 仅接受 texture,当前: {}", meta.atype),
        ));
    }
    let abs = project.content_root().join(&rel);
    let (w, h, rgba) = crate::texture::decode_rgba(&abs)?;
    let boxes = autoslice_image(w, h, &rgba, opts)?;
    Ok(SliceOutcome {
        width: w,
        height: h,
        boxes,
    })
}

/// 由 bbox 列表构造 frames JSON map(命名 `<prefix>_<i>`;autoslice → create 组合用)。
pub fn frames_from_boxes(prefix: &str, boxes: &[[u32; 4]]) -> Map<String, Value> {
    let mut map = Map::new();
    let pad = if boxes.len() > 10 { 2 } else { 1 };
    for (i, b) in boxes.iter().enumerate() {
        map.insert(
            format!("{prefix}_{i:0pad$}", pad = pad),
            serde_json::json!({ "bbox": [b[0], b[1], b[2], b[3]] }),
        );
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_doc() -> Value {
        json!({
            "version": 1,
            "texture": "guid-tex-1",
            "pivot": [0.5, 1.0],
            "frames": {
                "walk_0": { "bbox": [0, 0, 32, 48] },
                "walk_1": { "bbox": [32, 0, 32, 48], "pivot": [0.48, 1.0] },
                "idle_0": { "bbox": [64, 0, 32, 48] }
            },
            "clips": {
                "walk": { "frames": ["walk_0", "walk_1"], "fps": 8, "loop": true },
                "idle": { "frames": ["idle_0"], "duration": 0.5, "loop": false, "onFinish": "first" }
            },
            "animator": {
                "defaultState": "idle",
                "parameters": { "isMoving": "bool", "attack": "trigger" },
                "states": { "idle": { "clip": "idle" }, "walk": { "clip": "walk" } },
                "transitions": [
                    { "from": "idle", "to": "walk", "when": [{ "param": "isMoving", "eq": true }] },
                    { "from": ["walk"], "to": "idle", "when": [{ "param": "isMoving", "eq": false }] },
                    { "from": "Any", "to": "walk", "when": [{ "trigger": "attack" }], "hasExitTime": true }
                ]
            }
        })
    }

    #[test]
    fn parse_valid_doc_and_helpers() {
        let doc = parse_rxsprite(&valid_doc()).unwrap();
        assert_eq!(doc.frames.len(), 3);
        assert_eq!(doc.clips.len(), 2);
        // pivot 级联:帧级覆盖 > 文档级。
        assert_eq!(doc.resolve_pivot("walk_1"), [0.48, 1.0]);
        assert_eq!(doc.resolve_pivot("walk_0"), [0.5, 1.0]);
        // duration 优先于 fps;fps 兜底。
        let idle = &doc.clips["idle"];
        assert!((idle.frame_duration() - 0.5).abs() < 1e-6);
        let walk = &doc.clips["walk"];
        assert!((walk.frame_duration() - 0.125).abs() < 1e-6);
        // clip_frame 越界如实 None。
        assert_eq!(doc.clip_frame("walk", 1).unwrap().0, "walk_1");
        assert!(doc.clip_frame("walk", 2).is_none());
        assert!(doc.clip_frame("nosuch", 0).is_none());
        // animator FromSpec 匹配。
        let a = doc.animator.as_ref().unwrap();
        assert!(a.transitions[2].from.matches("idle"));
        assert!(a.transitions[0].from.matches("idle"));
        assert!(!a.transitions[0].from.matches("walk"));
    }

    #[test]
    fn validate_rejects_bad_docs() {
        let mut v = valid_doc();
        v["version"] = json!(2);
        assert!(validate_rxsprite(&v).is_err(), "version≠1 须拒");

        let mut v = valid_doc();
        v["texture"] = json!("");
        assert!(validate_rxsprite(&v).is_err(), "texture 空须拒");

        let mut v = valid_doc();
        v["frames"]["bad"] = json!({ "bbox": [0, 0, 0, 10] });
        assert!(validate_rxsprite(&v).is_err(), "零宽 bbox 须拒");

        let mut v = valid_doc();
        v["clips"]["walk"]["frames"] = json!(["nosuch"]);
        assert!(validate_rxsprite(&v).is_err(), "clip 引用不存在帧须拒");

        let mut v = valid_doc();
        v["clips"]["walk"]["onFinish"] = json!("stop");
        assert!(validate_rxsprite(&v).is_err(), "onFinish 越界须拒");

        let mut v = valid_doc();
        v["animator"]["defaultState"] = json!("nosuch");
        assert!(validate_rxsprite(&v).is_err(), "defaultState 不存在须拒");

        let mut v = valid_doc();
        v["animator"]["states"]["idle"]["clip"] = json!("nosuch");
        assert!(validate_rxsprite(&v).is_err(), "状态引用不存在 clip 须拒");

        let mut v = valid_doc();
        v["animator"]["transitions"][0]["to"] = json!("nosuch");
        assert!(validate_rxsprite(&v).is_err(), "transition.to 不存在须拒");

        let mut v = valid_doc();
        v["animator"]["transitions"][0]["when"] = json!([{ "param": "nosuch", "eq": true }]);
        assert!(validate_rxsprite(&v).is_err(), "条件引用不存在参数须拒");

        let mut v = valid_doc();
        v["pivot"] = json!([1.5, 0.5]);
        assert!(validate_rxsprite(&v).is_err(), "pivot 越界须拒");

        // trigger 条件引用 bool 参数 → 拒(类型不匹配)。
        let mut v = valid_doc();
        v["animator"]["transitions"][2]["when"] = json!([{ "trigger": "isMoving" }]);
        assert!(validate_rxsprite(&v).is_err(), "trigger 引用 bool 参数须拒");
    }

    #[test]
    fn clips_optional_animator_optional() {
        // 最小文档:仅 version + texture(帧/clip 后续编辑器补)。
        let v = json!({ "version": 1, "texture": "g" });
        let doc = parse_rxsprite(&v).unwrap();
        assert!(doc.frames.is_empty());
        assert!(doc.clips.is_empty());
        assert!(doc.animator.is_none());
        assert_eq!(doc.pivot, [0.5, 1.0], "缺省脚底锚");
    }

    fn temp_project(tag: &str) -> ForgeProject {
        let dir = std::env::temp_dir().join(format!(
            "assetd-sprite-{tag}-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = ForgeProject::with_defaults(dir);
        p.ensure_dirs().unwrap();
        p
    }

    fn put_texture(p: &ForgeProject, name: &str) -> String {
        // 4x4 品红底 png(真实编码,decode_rgba 可读)。
        let mut img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 255, 255]));
        img.put_pixel(1, 1, image::Rgba([20, 200, 20, 255]));
        let abs = p.content_root().join("Textures").join(name);
        img.save(&abs).unwrap();
        let rel = format!("Textures/{name}");
        let (_, meta) = crate::meta::ensure_meta(&p.content_root(), &rel).unwrap();
        meta.guid
    }

    #[test]
    fn create_sprite_writes_doc_and_meta() {
        let p = temp_project("create");
        let tex_guid = put_texture(&p, "hero.png");

        let mut frames = Map::new();
        frames.insert("f_0".into(), json!({ "bbox": [0, 0, 2, 2] }));
        let mut clips = Map::new();
        clips.insert("idle".into(), json!({ "frames": ["f_0"], "fps": 4 }));

        let created = create_sprite(
            &p, "", "Hero", &tex_guid, None, Some(&frames), Some(&clips), None,
        )
        .unwrap();
        assert_eq!(created.asset_path, "Sprites/Hero.rxsprite");
        assert_eq!(created.frame_count, 1);
        assert_eq!(created.clip_count, 1);

        // 落盘可重读且校验通过;.meta 类型正确。
        let abs = p.content_root().join(&created.asset_path);
        let doc = load_rxsprite(&abs).unwrap();
        assert_eq!(doc.texture, tex_guid);
        let meta = MetaDoc::load(&meta_path_for(&p.content_root(), &created.asset_path)).unwrap();
        assert_eq!(meta.atype, "sprite");
        assert_eq!(meta.importer, "sprite");

        // 再建同名 → GUID 复用(reimport 语义)。
        let again = create_sprite(
            &p, "", "Hero", &tex_guid, None, Some(&frames), Some(&clips), None,
        )
        .unwrap();
        assert_eq!(again.guid, created.guid);

        // 未知纹理 GUID / 非贴图 GUID 拒绝。
        assert!(create_sprite(&p, "", "Bad", "no-such-guid", None, None, None, None).is_err());
        assert!(create_sprite(&p, "", "Bad", &created.guid, None, None, None, None).is_err());

        // write_sprite_doc:合法覆盖 ok,坏文档拒。
        let v = json!({ "version": 1, "texture": tex_guid, "frames": {}, "clips": {} });
        assert!(write_sprite_doc(&p, &created.asset_path, &v).is_ok());
        let bad = json!({ "version": 9, "texture": tex_guid });
        assert!(write_sprite_doc(&p, &created.asset_path, &bad).is_err());

        std::fs::remove_dir_all(&p.root).ok();
    }

    /// 合成图集:品红底,两行三帧(不规则大小),外加 1 像素噪点被 minArea 滤除。
    #[test]
    fn autoslice_detects_frames_in_row_order() {
        let bg = [255u8, 0, 255, 255];
        let fg = [40u8, 180, 60, 255];
        let (w, h) = (64u32, 32u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&bg);
        }
        let mut fill = |x0: u32, y0: u32, bw: u32, bh: u32| {
            for y in y0..y0 + bh {
                for x in x0..x0 + bw {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&fg);
                }
            }
        };
        // 第一行两帧(顶部略有错位仍应归同行),第二行一帧。
        fill(2, 3, 10, 12);
        fill(20, 5, 8, 10);
        fill(4, 20, 12, 8);
        // 噪点(1px)。
        let i = ((1 * w + 60) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&fg);

        let boxes = autoslice_image(w, h, &rgba, SliceOptions::default()).unwrap();
        assert_eq!(boxes.len(), 3, "噪点须被 minArea 滤除: {boxes:?}");
        assert_eq!(boxes[0], [2, 3, 10, 12]);
        assert_eq!(boxes[1], [20, 5, 8, 10]);
        assert_eq!(boxes[2], [4, 20, 12, 8]);

        // 透明背景同样可切(alpha 判定腿)。
        let bg_t = [0u8, 0, 0, 0];
        let mut rgba2 = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba2.extend_from_slice(&bg_t);
        }
        for y in 3..15 {
            for x in 2..12 {
                let i = ((y * w + x) * 4) as usize;
                rgba2[i..i + 4].copy_from_slice(&fg);
            }
        }
        let boxes2 = autoslice_image(w, h, &rgba2, SliceOptions::default()).unwrap();
        assert_eq!(boxes2, vec![[2, 3, 10, 12]]);

        // frames_from_boxes 命名。
        let m = frames_from_boxes("frame", &boxes);
        assert!(m.contains_key("frame_0") && m.contains_key("frame_2"));
    }

    #[test]
    fn autoslice_background_rule_matches_shader() {
        // 品红族(g 显著低于 r/b)= 背景;绿/蓝/红/黄不受误伤(与 FS_TEX_WGSL 注释一致)。
        assert!(is_background(&[255, 0, 255, 255], 5), "纯品红");
        assert!(is_background(&[248, 13, 228, 255], 5), "色漂品红");
        assert!(is_background(&[10, 10, 10, 0], 5), "全透明");
        assert!(!is_background(&[20, 200, 20, 255], 5), "绿身");
        assert!(!is_background(&[30, 100, 220, 255], 5), "蓝天");
        assert!(!is_background(&[220, 60, 40, 255], 5), "红屋");
        assert!(!is_background(&[240, 220, 40, 255], 5), "黄瓣");
    }
}
