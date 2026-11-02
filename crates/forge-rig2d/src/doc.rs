//! `.rxrig` v1 文档模型与语义校验(F-GAME-5 / D-050)。
//!
//! 坐标约定:
//! - 画布像素坐标,Y 向下;`canvas.origin` 为映射到实体世界原点的画布点(缺省脚底中心)。
//! - 部件 `mesh.vertices` 为画布像素;`mesh.uvs` 为图集像素(渲染端再除以图集宽高)。
//! - 骨骼 `position` / `rotation` 为**绝对**静止姿态(画布像素 / 度);角度按画布坐标的
//!   标准旋转矩阵计(Y 向下 → 视觉上正角度为顺时针)。
//! - keyform / clip / 逻辑对骨骼的修改均为相对静止姿态的**局部增量**。
//!
//! keyform 组 `forms` 行主序:最后一个参数轴变化最快,
//! `idx = ((i0 * n1) + i1) * n2 + i2`。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{RigError, Result};

/// 单个 rig 顶点总数上限(性能预算,超出报 RIG_TOO_LARGE)。
pub const MAX_TOTAL_VERTICES: usize = 8192;
/// keyform 组参数轴上限。
pub const MAX_KEYFORM_AXES: usize = 3;
/// 单顶点骨骼影响上限。
pub const MAX_INFLUENCES: usize = 4;

fn d_ppu() -> f32 {
    100.0
}
fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_one(v: &f32) -> bool {
    (*v - 1.0).abs() < f32::EPSILON
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigCanvas {
    /// 画布宽高(像素)。
    pub size: [f32; 2],
    /// 映射到世界原点的画布点(像素)。
    pub origin: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigParam {
    pub id: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// 循环参数(如 360° 旋转):越界取模而非夹取。
    #[serde(default, skip_serializing_if = "is_false")]
    pub repeat: bool,
}

impl RigParam {
    pub fn clamp(&self, v: f32) -> f32 {
        if !v.is_finite() {
            return self.default;
        }
        if self.repeat {
            let span = self.max - self.min;
            if span <= 0.0 {
                return self.min;
            }
            self.min + (v - self.min).rem_euclid(span)
        } else {
            v.clamp(self.min, self.max)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigBone {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// 绝对静止关节位置(画布像素)。
    pub position: [f32; 2],
    /// 绝对静止朝向(度)。
    #[serde(default)]
    pub rotation: f32,
    /// 骨长(像素;0 = 点骨,仅用于自动权重与显示)。
    #[serde(default)]
    pub length: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", deny_unknown_fields)]
pub enum RigDeformerKind {
    /// 弯曲形变器:rect 内 (cols+1)×(rows+1) 控制点网格,双线性映射。
    Warp { rect: [f32; 4], grid: [u32; 2] },
    /// 旋转形变器:绕 origin 旋转/缩放。
    Rotation { origin: [f32; 2] },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigDeformer {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(flatten)]
    pub kind: RigDeformerKind,
}

impl RigDeformer {
    /// 控制点数(warp = (c+1)(r+1);rotation = 0)。
    pub fn control_points(&self) -> usize {
        match &self.kind {
            RigDeformerKind::Warp { grid, .. } => ((grid[0] + 1) * (grid[1] + 1)) as usize,
            RigDeformerKind::Rotation { .. } => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigMesh {
    /// 画布像素顶点。
    pub vertices: Vec<[f32; 2]>,
    /// 图集像素 UV(与 vertices 等长)。
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PartBlend {
    #[default]
    Alpha,
    Additive,
    Multiply,
}

fn is_alpha(b: &PartBlend) -> bool {
    *b == PartBlend::Alpha
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigPart {
    pub id: String,
    #[serde(default)]
    pub draw_order: i32,
    pub mesh: RigMesh,
    /// 每顶点 ≤4 个 [骨骼 id, 权重];与 `bone` 互斥。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<Vec<(String, f32)>>>,
    /// 刚性绑定骨骼。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone: Option<String>,
    /// 父形变器。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deformer: Option<String>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "is_alpha")]
    pub blend: PartBlend,
    #[serde(default = "yes", skip_serializing_if = "Clone::clone")]
    pub visible: bool,
    /// 剪贴蒙版(v1 不支持:非空即 RIG_FEATURE_UNSUPPORTED)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Vec<String>>,
}

/// keyform 单个形态。字段按目标类型取用(校验期拒绝不适用字段):
/// - part:`offsets`(每顶点)/ `opacity`(乘子)/ `drawOrder`(覆盖)
/// - warp 形变器:`offsets`(每控制点)
/// - rotation 形变器:`rotation`(度)/ `position`(原点偏移)/ `scale`
/// - bone:`rotation` / `position` / `scale`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Keyform {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offsets: Option<Vec<[f32; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_order: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyformGroup {
    /// "part:<id>" | "deformer:<id>" | "bone:<id>"
    pub target: String,
    pub params: Vec<String>,
    pub keys: Vec<Vec<f32>>,
    pub forms: Vec<Keyform>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PhysicsInputKind {
    /// 根点水平位移。
    X,
    /// 根点角度(重力方向偏转)。
    Angle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PhysicsInput {
    pub param: String,
    #[serde(default = "one")]
    pub weight: f32,
    pub kind: PhysicsInputKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PhysicsSegment {
    /// 段长(像素)。
    pub length: f32,
    /// 速度阻尼 0..1:每 1/60 秒移除的速度比例(与步长无关)。
    #[serde(default = "d_damping")]
    pub damping: f32,
    /// 回正刚度(把段拉回静止方向,1/s²)。
    #[serde(default = "d_stiffness")]
    pub stiffness: f32,
}
fn d_damping() -> f32 {
    0.1
}
fn d_stiffness() -> f32 {
    20.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PhysicsOutput {
    pub param: String,
    /// 取第几段的摆角。
    #[serde(default)]
    pub segment: usize,
    /// 参数单位 / 度。
    #[serde(default = "one")]
    pub scale: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PhysicsRig {
    pub id: String,
    pub inputs: Vec<PhysicsInput>,
    pub segments: Vec<PhysicsSegment>,
    pub outputs: Vec<PhysicsOutput>,
    /// 重力加速度(像素/秒²)。
    #[serde(default = "d_gravity")]
    pub gravity: f32,
    /// 输入参数满量程(归一化 ±1)对应的根点位移像素 / 角度度数。
    #[serde(default = "d_input_scale")]
    pub input_scale: f32,
}
fn d_gravity() -> f32 {
    980.0
}
fn d_input_scale() -> f32 {
    30.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoBlink {
    pub params: Vec<String>,
    #[serde(default = "d_interval")]
    pub interval_sec: [f32; 2],
    #[serde(default = "d_close")]
    pub close_sec: f32,
    #[serde(default = "d_hold")]
    pub hold_sec: f32,
    #[serde(default = "d_open")]
    pub open_sec: f32,
}
fn d_interval() -> [f32; 2] {
    [2.5, 6.0]
}
fn d_close() -> f32 {
    0.08
}
fn d_hold() -> f32 {
    0.04
}
fn d_open() -> f32 {
    0.12
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoBreath {
    pub param: String,
    #[serde(default = "d_period")]
    pub period_sec: f32,
    #[serde(default = "one")]
    pub amplitude: f32,
}
fn d_period() -> f32 {
    3.2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigAuto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blink: Option<AutoBlink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breath: Option<AutoBreath>,
}

impl RigAuto {
    pub fn is_empty(&self) -> bool {
        self.blink.is_none() && self.breath.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Interp {
    #[default]
    Linear,
    Step,
    Smooth,
}

fn is_linear(i: &Interp) -> bool {
    *i == Interp::Linear
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigTrack {
    /// "param:<id>" | "bone:<id>.rotation|x|y|sx|sy" | "part:<id>.opacity"
    pub target: String,
    /// [[时间秒, 值], ...] 按时间升序。
    pub keys: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "is_linear")]
    pub interp: Interp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigClip {
    pub duration: f32,
    #[serde(rename = "loop", default = "yes")]
    pub looped: bool,
    #[serde(default)]
    pub tracks: Vec<RigTrack>,
}

// ── animator(与 .rxsprite 同 JSON 语义,assetd::sprite::SpriteAnimator 镜像)──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimatorState {
    pub clip: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FromSpec {
    One(String),
    Many(Vec<String>),
}

impl FromSpec {
    pub fn matches(&self, state: &str) -> bool {
        match self {
            FromSpec::One(s) => s == "Any" || s == state,
            FromSpec::Many(v) => v.iter().any(|s| s == "Any" || s == state),
        }
    }
    pub fn names(&self) -> Vec<&str> {
        match self {
            FromSpec::One(s) => vec![s.as_str()],
            FromSpec::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnimatorCond {
    Param { param: String, eq: bool },
    Trigger { trigger: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimatorTransition {
    pub from: FromSpec,
    pub to: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<AnimatorCond>,
    #[serde(rename = "hasExitTime", default)]
    pub has_exit_time: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RigAnimator {
    #[serde(rename = "defaultState")]
    pub default_state: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    pub states: BTreeMap<String, AnimatorState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<AnimatorTransition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RigDoc {
    pub version: u32,
    /// 图集贴图 GUID(全部部件共用一张)。
    pub texture: String,
    #[serde(default = "d_ppu")]
    pub pixels_per_unit: f32,
    pub canvas: RigCanvas,
    #[serde(default)]
    pub parameters: Vec<RigParam>,
    #[serde(default)]
    pub bones: Vec<RigBone>,
    #[serde(default)]
    pub deformers: Vec<RigDeformer>,
    #[serde(default)]
    pub parts: Vec<RigPart>,
    #[serde(default)]
    pub keyforms: Vec<KeyformGroup>,
    #[serde(default)]
    pub physics: Vec<PhysicsRig>,
    #[serde(default, skip_serializing_if = "RigAuto::is_empty")]
    pub auto: RigAuto,
    #[serde(default)]
    pub clips: BTreeMap<String, RigClip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animator: Option<RigAnimator>,
}

/// keyform / track 目标解析结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Part,
    Deformer,
    Bone,
    Param,
}

/// 解析 "kind:id[.channel]" 目标串。
pub fn split_target(t: &str) -> Option<(TargetKind, &str, Option<&str>)> {
    let (k, rest) = t.split_once(':')?;
    let kind = match k {
        "part" => TargetKind::Part,
        "deformer" => TargetKind::Deformer,
        "bone" => TargetKind::Bone,
        "param" => TargetKind::Param,
        _ => return None,
    };
    if rest.is_empty() {
        return None;
    }
    // 参数 id 允许含 '.';只有 bone/part 带通道。
    match kind {
        TargetKind::Bone | TargetKind::Part => match rest.rsplit_once('.') {
            Some((id, ch)) if !id.is_empty() && !ch.is_empty() => Some((kind, id, Some(ch))),
            _ => Some((kind, rest, None)),
        },
        _ => Some((kind, rest, None)),
    }
}

/// 统计摘要(MCP 工具返回用)。
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RigStats {
    pub parts: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub bones: usize,
    pub deformers: usize,
    pub parameters: usize,
    pub keyform_groups: usize,
    pub physics: usize,
    pub clips: usize,
    pub has_animator: bool,
}

impl RigDoc {
    /// 空白 rig(无部件),供 rig_create 起步。
    pub fn empty(texture: &str, canvas_size: [f32; 2]) -> RigDoc {
        RigDoc {
            version: 1,
            texture: texture.to_string(),
            pixels_per_unit: d_ppu(),
            canvas: RigCanvas { size: canvas_size, origin: [canvas_size[0] * 0.5, canvas_size[1]] },
            parameters: vec![],
            bones: vec![],
            deformers: vec![],
            parts: vec![],
            keyforms: vec![],
            physics: vec![],
            auto: RigAuto::default(),
            clips: BTreeMap::new(),
            animator: None,
        }
    }

    pub fn stats(&self) -> RigStats {
        RigStats {
            parts: self.parts.len(),
            vertices: self.parts.iter().map(|p| p.mesh.vertices.len()).sum(),
            triangles: self.parts.iter().map(|p| p.mesh.triangles.len()).sum(),
            bones: self.bones.len(),
            deformers: self.deformers.len(),
            parameters: self.parameters.len(),
            keyform_groups: self.keyforms.len(),
            physics: self.physics.len(),
            clips: self.clips.len(),
            has_animator: self.animator.is_some(),
        }
    }

    pub fn param(&self, id: &str) -> Option<&RigParam> {
        self.parameters.iter().find(|p| p.id == id)
    }
    pub fn part(&self, id: &str) -> Option<&RigPart> {
        self.parts.iter().find(|p| p.id == id)
    }
    pub fn bone(&self, id: &str) -> Option<&RigBone> {
        self.bones.iter().find(|b| b.id == id)
    }
    pub fn deformer(&self, id: &str) -> Option<&RigDeformer> {
        self.deformers.iter().find(|d| d.id == id)
    }

    /// 目标的「点数」:part = 顶点数,warp = 控制点数,其余 0。不存在 → None。
    pub fn target_points(&self, kind: TargetKind, id: &str) -> Option<usize> {
        match kind {
            TargetKind::Part => self.part(id).map(|p| p.mesh.vertices.len()),
            TargetKind::Deformer => self.deformer(id).map(|d| d.control_points()),
            TargetKind::Bone => self.bone(id).map(|_| 0),
            TargetKind::Param => self.param(id).map(|_| 0),
        }
    }

    /// 确定性 JSON 写出(BTreeMap 键序 + 尾换行)。
    pub fn to_json_string(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("RigDoc serializes");
        s.push('\n');
        s
    }
}

/// 解析 + 校验 `.rxrig` JSON。
pub fn parse_rxrig(v: &Value) -> Result<RigDoc> {
    let doc: RigDoc = serde_json::from_value(v.clone())
        .map_err(|e| RigError::new("RIG_INVALID", format!(".rxrig 解析失败: {e}")))?;
    validate_doc(&doc)?;
    Ok(doc)
}

/// 从字符串解析 + 校验。
pub fn parse_rxrig_str(s: &str) -> Result<RigDoc> {
    let v: Value = serde_json::from_str(s)
        .map_err(|e| RigError::new("RIG_INVALID", format!(".rxrig 不是合法 JSON: {e}")))?;
    parse_rxrig(&v)
}

fn finite2(p: &[f32; 2]) -> bool {
    p[0].is_finite() && p[1].is_finite()
}

fn check_ids<'a>(what: &str, ids: impl Iterator<Item = &'a str>) -> Result<HashMap<&'a str, usize>> {
    let mut m = HashMap::new();
    for (i, id) in ids.enumerate() {
        if id.is_empty() {
            return Err(RigError::invalid(format!("{what}[{i}].id 不得为空")));
        }
        if id.contains(':') {
            return Err(RigError::invalid(format!("{what}[{i}].id 不得含 ':'({id})")));
        }
        if m.insert(id, i).is_some() {
            return Err(RigError::invalid(format!("{what} id 重复: {id}")));
        }
    }
    Ok(m)
}

fn check_acyclic(what: &str, parents: &[(String, Option<String>)]) -> Result<()> {
    let map: HashMap<&str, Option<&str>> =
        parents.iter().map(|(id, p)| (id.as_str(), p.as_deref())).collect();
    for (id, _) in parents {
        let mut seen = BTreeSet::new();
        let mut cur = Some(id.as_str());
        while let Some(c) = cur {
            if !seen.insert(c) {
                return Err(RigError::invalid(format!("{what} 父链成环: {id}")));
            }
            cur = map.get(c).copied().flatten();
        }
    }
    Ok(())
}

/// 语义校验(serde 之后的第二道门);错误统一 RIG_INVALID(另有 RIG_TOO_LARGE /
/// RIG_FEATURE_UNSUPPORTED),消息携带 JSON 路径。
pub fn validate_doc(doc: &RigDoc) -> Result<()> {
    let inv = RigError::invalid;
    if doc.version != 1 {
        return Err(inv(format!("version 须为 1,实际 {}", doc.version)));
    }
    if doc.texture.is_empty() {
        return Err(inv("texture 须为非空 GUID".into()));
    }
    if !(doc.pixels_per_unit > 0.0) {
        return Err(inv(format!("pixelsPerUnit 须 > 0,实际 {}", doc.pixels_per_unit)));
    }
    if !(doc.canvas.size[0] > 0.0 && doc.canvas.size[1] > 0.0) || !finite2(&doc.canvas.origin) {
        return Err(inv(format!("canvas 非法: {:?}", doc.canvas)));
    }

    let params = check_ids("parameters", doc.parameters.iter().map(|p| p.id.as_str()))?;
    for (i, p) in doc.parameters.iter().enumerate() {
        if !(p.min < p.max) || !(p.min <= p.default && p.default <= p.max) {
            return Err(inv(format!(
                "parameters[{i}]({}) 须满足 min < max 且 default ∈ [min,max],实际 {}/{}/{}",
                p.id, p.min, p.default, p.max
            )));
        }
    }

    let bones = check_ids("bones", doc.bones.iter().map(|b| b.id.as_str()))?;
    for (i, b) in doc.bones.iter().enumerate() {
        if let Some(p) = &b.parent {
            if !bones.contains_key(p.as_str()) {
                return Err(inv(format!("bones[{i}]({}).parent 不存在: {p}", b.id)));
            }
        }
        if !finite2(&b.position) || !b.rotation.is_finite() || !(b.length >= 0.0) {
            return Err(inv(format!("bones[{i}]({}) 数值非法", b.id)));
        }
    }
    check_acyclic(
        "bones",
        &doc.bones.iter().map(|b| (b.id.clone(), b.parent.clone())).collect::<Vec<_>>(),
    )?;

    let deformers = check_ids("deformers", doc.deformers.iter().map(|d| d.id.as_str()))?;
    for (i, d) in doc.deformers.iter().enumerate() {
        if let Some(p) = &d.parent {
            if !deformers.contains_key(p.as_str()) {
                return Err(inv(format!("deformers[{i}]({}).parent 不存在: {p}", d.id)));
            }
        }
        if let RigDeformerKind::Warp { rect, grid } = &d.kind {
            if !(rect[2] > 0.0 && rect[3] > 0.0) || grid[0] == 0 || grid[1] == 0 || grid[0] > 16 || grid[1] > 16 {
                return Err(inv(format!(
                    "deformers[{i}]({}) warp 须 rect 宽高 > 0 且 grid ∈ [1,16]²",
                    d.id
                )));
            }
        }
    }
    check_acyclic(
        "deformers",
        &doc.deformers.iter().map(|d| (d.id.clone(), d.parent.clone())).collect::<Vec<_>>(),
    )?;

    let parts = check_ids("parts", doc.parts.iter().map(|p| p.id.as_str()))?;
    let mut total = 0usize;
    for (i, p) in doc.parts.iter().enumerate() {
        let path = format!("parts[{i}]({})", p.id);
        let n = p.mesh.vertices.len();
        total += n;
        if n < 3 || p.mesh.triangles.is_empty() {
            return Err(inv(format!("{path}.mesh 至少 3 顶点 1 三角形")));
        }
        if p.mesh.uvs.len() != n {
            return Err(inv(format!("{path}.mesh.uvs 长度 {} ≠ 顶点数 {n}", p.mesh.uvs.len())));
        }
        if n > u16::MAX as usize {
            return Err(RigError::new("RIG_TOO_LARGE", format!("{path} 顶点数 {n} 超过 65535")));
        }
        if p.mesh.vertices.iter().chain(p.mesh.uvs.iter()).any(|v| !finite2(v)) {
            return Err(inv(format!("{path}.mesh 含非有限数值")));
        }
        for (t, tri) in p.mesh.triangles.iter().enumerate() {
            if tri.iter().any(|&k| k as usize >= n) {
                return Err(inv(format!("{path}.mesh.triangles[{t}] 下标越界 {tri:?}")));
            }
        }
        if !(0.0..=1.0).contains(&p.opacity) {
            return Err(inv(format!("{path}.opacity 须 ∈ [0,1]")));
        }
        if p.weights.is_some() && p.bone.is_some() {
            return Err(inv(format!("{path} weights 与 bone 互斥")));
        }
        if let Some(b) = &p.bone {
            if !bones.contains_key(b.as_str()) {
                return Err(inv(format!("{path}.bone 不存在: {b}")));
            }
        }
        if let Some(w) = &p.weights {
            if w.len() != n {
                return Err(inv(format!("{path}.weights 长度 {} ≠ 顶点数 {n}", w.len())));
            }
            for (vi, inf) in w.iter().enumerate() {
                if inf.len() > MAX_INFLUENCES {
                    return Err(inv(format!("{path}.weights[{vi}] 影响数 > {MAX_INFLUENCES}")));
                }
                for (b, wt) in inf {
                    if !bones.contains_key(b.as_str()) {
                        return Err(inv(format!("{path}.weights[{vi}] 引用不存在的骨骼: {b}")));
                    }
                    if !(wt.is_finite() && *wt >= 0.0) {
                        return Err(inv(format!("{path}.weights[{vi}] 权重须 ≥ 0")));
                    }
                }
            }
        }
        if let Some(d) = &p.deformer {
            if !deformers.contains_key(d.as_str()) {
                return Err(inv(format!("{path}.deformer 不存在: {d}")));
            }
        }
        if p.mask.as_ref().is_some_and(|m| !m.is_empty()) {
            return Err(RigError::new(
                "RIG_FEATURE_UNSUPPORTED",
                format!("{path}.mask:剪贴蒙版 v1 不支持(D-050),请移除该字段"),
            ));
        }
    }
    if total > MAX_TOTAL_VERTICES {
        return Err(RigError::new(
            "RIG_TOO_LARGE",
            format!("顶点总数 {total} 超过上限 {MAX_TOTAL_VERTICES};请调大 rig_mesh_auto 的 interiorSpacing"),
        ));
    }

    for (gi, g) in doc.keyforms.iter().enumerate() {
        let path = format!("keyforms[{gi}]({})", g.target);
        let (kind, id, ch) = split_target(&g.target)
            .ok_or_else(|| inv(format!("{path}.target 须为 part:|deformer:|bone:<id>")))?;
        if ch.is_some() || kind == TargetKind::Param {
            return Err(inv(format!("{path}.target 须为 part:|deformer:|bone:<id>(不带通道)")));
        }
        let points = match kind {
            TargetKind::Part => parts.get(id).map(|&i| doc.parts[i].mesh.vertices.len()),
            TargetKind::Deformer => deformers.get(id).map(|&i| doc.deformers[i].control_points()),
            TargetKind::Bone => bones.get(id).map(|_| 0),
            TargetKind::Param => None,
        }
        .ok_or_else(|| inv(format!("{path}.target 不存在")))?;
        let is_rot = kind == TargetKind::Deformer
            && matches!(doc.deformers[deformers[id]].kind, RigDeformerKind::Rotation { .. });
        if g.params.is_empty() || g.params.len() > MAX_KEYFORM_AXES {
            return Err(inv(format!("{path}.params 轴数须 ∈ [1,{MAX_KEYFORM_AXES}]")));
        }
        if g.keys.len() != g.params.len() {
            return Err(inv(format!("{path}.keys 轴数须与 params 一致")));
        }
        let mut expect = 1usize;
        for (ai, pid) in g.params.iter().enumerate() {
            let p = params
                .get(pid.as_str())
                .map(|&i| &doc.parameters[i])
                .ok_or_else(|| inv(format!("{path}.params[{ai}] 参数不存在: {pid}")))?;
            let ks = &g.keys[ai];
            if ks.is_empty() {
                return Err(inv(format!("{path}.keys[{ai}] 不得为空")));
            }
            for w in ks.windows(2) {
                if !(w[0] < w[1]) {
                    return Err(inv(format!("{path}.keys[{ai}] 须严格升序")));
                }
            }
            if ks.iter().any(|k| *k < p.min - 1e-4 || *k > p.max + 1e-4) {
                return Err(inv(format!("{path}.keys[{ai}] 须落在参数 {pid} 的 [min,max] 内")));
            }
            expect *= ks.len();
        }
        if g.forms.len() != expect {
            return Err(inv(format!("{path}.forms 数量 {} ≠ 关键值组合数 {expect}", g.forms.len())));
        }
        for (fi, f) in g.forms.iter().enumerate() {
            let fpath = format!("{path}.forms[{fi}]");
            let bad = |field: &str| inv(format!("{fpath}.{field} 不适用于该目标类型"));
            match kind {
                TargetKind::Part => {
                    if f.position.is_some() {
                        return Err(bad("position"));
                    }
                    if f.rotation.is_some() {
                        return Err(bad("rotation"));
                    }
                    if f.scale.is_some() {
                        return Err(bad("scale"));
                    }
                }
                TargetKind::Deformer if !is_rot => {
                    if f.position.is_some() || f.rotation.is_some() || f.scale.is_some() || f.opacity.is_some() || f.draw_order.is_some() {
                        return Err(bad("position/rotation/scale/opacity/drawOrder"));
                    }
                }
                _ => {
                    if f.offsets.is_some() {
                        return Err(bad("offsets"));
                    }
                    if f.opacity.is_some() || f.draw_order.is_some() {
                        return Err(bad("opacity/drawOrder"));
                    }
                }
            }
            if let Some(o) = &f.offsets {
                if o.len() != points {
                    return Err(inv(format!("{fpath}.offsets 长度 {} ≠ 目标点数 {points}", o.len())));
                }
            }
        }
    }

    let phys = check_ids("physics", doc.physics.iter().map(|p| p.id.as_str()))?;
    let _ = phys;
    for (i, p) in doc.physics.iter().enumerate() {
        let path = format!("physics[{i}]({})", p.id);
        if p.segments.is_empty() || p.segments.len() > 16 {
            return Err(inv(format!("{path}.segments 段数须 ∈ [1,16]")));
        }
        if p.segments.iter().any(|s| !(s.length > 0.0) || !(0.0..=1.0).contains(&s.damping) || !(s.stiffness >= 0.0)) {
            return Err(inv(format!("{path}.segments 须 length > 0、damping ∈ [0,1]、stiffness ≥ 0")));
        }
        for inp in &p.inputs {
            if !params.contains_key(inp.param.as_str()) {
                return Err(inv(format!("{path}.inputs 参数不存在: {}", inp.param)));
            }
        }
        if p.outputs.is_empty() {
            return Err(inv(format!("{path}.outputs 不得为空")));
        }
        for o in &p.outputs {
            if !params.contains_key(o.param.as_str()) {
                return Err(inv(format!("{path}.outputs 参数不存在: {}", o.param)));
            }
            if o.segment >= p.segments.len() {
                return Err(inv(format!("{path}.outputs.segment {} 越界", o.segment)));
            }
        }
    }

    if let Some(b) = &doc.auto.blink {
        if b.params.is_empty() {
            return Err(inv("auto.blink.params 不得为空".into()));
        }
        for pid in &b.params {
            if !params.contains_key(pid.as_str()) {
                return Err(inv(format!("auto.blink.params 参数不存在: {pid}")));
            }
        }
        if !(b.interval_sec[0] > 0.0 && b.interval_sec[0] <= b.interval_sec[1]) {
            return Err(inv("auto.blink.intervalSec 须 0 < min ≤ max".into()));
        }
    }
    if let Some(b) = &doc.auto.breath {
        if !params.contains_key(b.param.as_str()) {
            return Err(inv(format!("auto.breath.param 参数不存在: {}", b.param)));
        }
        if !(b.period_sec > 0.0) {
            return Err(inv("auto.breath.periodSec 须 > 0".into()));
        }
    }

    for (cname, c) in &doc.clips {
        let path = format!("clips.{cname}");
        if cname.is_empty() {
            return Err(inv("clip 名不得为空".into()));
        }
        if !(c.duration > 0.0) {
            return Err(inv(format!("{path}.duration 须 > 0")));
        }
        for (ti, t) in c.tracks.iter().enumerate() {
            let tpath = format!("{path}.tracks[{ti}]({})", t.target);
            let (kind, id, ch) =
                split_target(&t.target).ok_or_else(|| inv(format!("{tpath}.target 格式非法")))?;
            let ok = match kind {
                TargetKind::Param => params.contains_key(id),
                TargetKind::Bone => {
                    bones.contains_key(id) && matches!(ch, Some("rotation" | "x" | "y" | "sx" | "sy"))
                }
                TargetKind::Part => parts.contains_key(id) && ch == Some("opacity"),
                TargetKind::Deformer => false,
            };
            if !ok {
                return Err(inv(format!(
                    "{tpath}.target 须为 param:<id> | bone:<id>.rotation|x|y|sx|sy | part:<id>.opacity 且目标存在"
                )));
            }
            if t.keys.is_empty() {
                return Err(inv(format!("{tpath}.keys 不得为空")));
            }
            for w in t.keys.windows(2) {
                if !(w[0][0] <= w[1][0]) {
                    return Err(inv(format!("{tpath}.keys 时间须升序")));
                }
            }
            if t.keys.iter().any(|k| !k[0].is_finite() || !k[1].is_finite()) {
                return Err(inv(format!("{tpath}.keys 含非有限数值")));
            }
        }
    }

    if let Some(a) = &doc.animator {
        if a.states.is_empty() {
            return Err(inv("animator.states 不得为空".into()));
        }
        for (sname, st) in &a.states {
            if !doc.clips.contains_key(&st.clip) {
                return Err(inv(format!("animator 状态 {sname} 引用不存在的 clip: {}", st.clip)));
            }
        }
        if !a.states.contains_key(&a.default_state) {
            return Err(inv(format!("animator.defaultState 不存在: {}", a.default_state)));
        }
        for (pname, kind) in &a.parameters {
            if kind != "bool" && kind != "trigger" {
                return Err(inv(format!("animator 参数 {pname} 类型须为 bool|trigger")));
            }
        }
        for (i, t) in a.transitions.iter().enumerate() {
            if !a.states.contains_key(&t.to) {
                return Err(inv(format!("animator.transitions[{i}].to 状态不存在: {}", t.to)));
            }
            for name in t.from.names() {
                if name != "Any" && !a.states.contains_key(name) {
                    return Err(inv(format!("animator.transitions[{i}].from 状态不存在: {name}")));
                }
            }
            for c in &t.when {
                let ok = match c {
                    AnimatorCond::Param { param, .. } => {
                        a.parameters.get(param).map(String::as_str) == Some("bool")
                    }
                    AnimatorCond::Trigger { trigger } => {
                        a.parameters.get(trigger).map(String::as_str) == Some("trigger")
                    }
                };
                if !ok {
                    return Err(inv(format!("animator.transitions[{i}] 条件引用的参数不存在或类型不符")));
                }
            }
        }
    }
    Ok(())
}
