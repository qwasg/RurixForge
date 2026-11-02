//! 细粒度编辑操作(rig_patch):对文档副本顺序执行全部操作,最后统一语义校验;
//! 任一失败整体不生效(原子)。删除类操作做安全级联(清理悬空引用),
//! 让 agent 不必手工维护引用一致性。

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::autogen::scaffold;
use crate::doc::{
    split_target, Keyform, KeyformGroup, RigAnimator, RigAuto, RigBone, RigClip, RigDeformer, RigDoc, RigParam,
    RigPart, PhysicsRig, TargetKind,
};
use crate::{validate_doc, RigError, Result};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatchOp {
    AddPart { part: Value },
    /// JSON merge-patch(RFC 7386,null 删除字段)。
    SetPart { id: String, fields: Value },
    RemovePart { id: String },
    AddBone { bone: Value },
    SetBone { id: String, fields: Value },
    RemoveBone { id: String },
    /// 二选一:weights(每顶点 [[骨骼, 权重]])或 bone(刚性);都不给 = 解绑。
    SetWeights {
        part: String,
        #[serde(default)]
        weights: Option<Vec<Vec<(String, f32)>>>,
        #[serde(default)]
        bone: Option<String>,
    },
    AddParameter { parameter: Value },
    SetParameter { id: String, fields: Value },
    RemoveParameter { id: String },
    AddDeformer { deformer: Value },
    SetDeformer { id: String, fields: Value },
    RemoveDeformer { id: String },
    /// 写 keyform 组中「参数值坐标 at」处的单个形态;组不存在时用 keys 新建(其余形态为空 = 无变化)。
    SetKeyform {
        target: String,
        params: Vec<String>,
        #[serde(default)]
        keys: Option<Vec<Vec<f32>>>,
        at: BTreeMap<String, f32>,
        form: Keyform,
    },
    /// 整组替换(target + params 相同视为同组)。
    SetKeyformGroup { group: KeyformGroup },
    RemoveKeyformGroup { target: String, params: Vec<String> },
    /// 按 id upsert。
    SetPhysics { physics: Value },
    RemovePhysics { id: String },
    SetAuto { auto: RigAuto },
    SetClip { name: String, clip: RigClip },
    RemoveClip { name: String },
    SetAnimator {
        #[serde(default)]
        animator: Option<RigAnimator>,
    },
    SetCanvas {
        #[serde(default)]
        size: Option<[f32; 2]>,
        #[serde(default)]
        origin: Option<[f32; 2]>,
        #[serde(default, rename = "pixelsPerUnit")]
        pixels_per_unit: Option<f32>,
    },
    /// 合并模板(bust | fullbody);已存在的同 id 项跳过,除非 overwrite。
    MergeScaffold {
        template: String,
        #[serde(default)]
        overwrite: bool,
    },
}

impl PatchOp {
    pub fn name(&self) -> &'static str {
        match self {
            PatchOp::AddPart { .. } => "add_part",
            PatchOp::SetPart { .. } => "set_part",
            PatchOp::RemovePart { .. } => "remove_part",
            PatchOp::AddBone { .. } => "add_bone",
            PatchOp::SetBone { .. } => "set_bone",
            PatchOp::RemoveBone { .. } => "remove_bone",
            PatchOp::SetWeights { .. } => "set_weights",
            PatchOp::AddParameter { .. } => "add_parameter",
            PatchOp::SetParameter { .. } => "set_parameter",
            PatchOp::RemoveParameter { .. } => "remove_parameter",
            PatchOp::AddDeformer { .. } => "add_deformer",
            PatchOp::SetDeformer { .. } => "set_deformer",
            PatchOp::RemoveDeformer { .. } => "remove_deformer",
            PatchOp::SetKeyform { .. } => "set_keyform",
            PatchOp::SetKeyformGroup { .. } => "set_keyform_group",
            PatchOp::RemoveKeyformGroup { .. } => "remove_keyform_group",
            PatchOp::SetPhysics { .. } => "set_physics",
            PatchOp::RemovePhysics { .. } => "remove_physics",
            PatchOp::SetAuto { .. } => "set_auto",
            PatchOp::SetClip { .. } => "set_clip",
            PatchOp::RemoveClip { .. } => "remove_clip",
            PatchOp::SetAnimator { .. } => "set_animator",
            PatchOp::SetCanvas { .. } => "set_canvas",
            PatchOp::MergeScaffold { .. } => "merge_scaffold",
        }
    }

    /// 是否删除类(调用方据此决定是否走 Proposal 门)。
    pub fn is_destructive(&self) -> bool {
        matches!(
            self,
            PatchOp::RemovePart { .. }
                | PatchOp::RemoveBone { .. }
                | PatchOp::RemoveParameter { .. }
                | PatchOp::RemoveDeformer { .. }
                | PatchOp::RemoveKeyformGroup { .. }
                | PatchOp::RemovePhysics { .. }
                | PatchOp::RemoveClip { .. }
        )
    }
}

/// 解析 ops 数组。
pub fn parse_ops(v: &Value) -> Result<Vec<PatchOp>> {
    let arr = v.as_array().ok_or_else(|| RigError::new("INVALID_OPS", "ops 须为数组"))?;
    arr.iter()
        .enumerate()
        .map(|(i, o)| {
            serde_json::from_value(o.clone())
                .map_err(|e| RigError::new("INVALID_OPS", format!("ops[{i}] 解析失败: {e}")))
        })
        .collect()
}

/// RFC 7386 JSON merge-patch。
pub fn merge_patch(target: &mut Value, patch: &Value) {
    if let Value::Object(p) = patch {
        if !target.is_object() {
            *target = Value::Object(Default::default());
        }
        let t = target.as_object_mut().expect("object");
        for (k, v) in p {
            if v.is_null() {
                t.remove(k);
            } else {
                merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    } else {
        *target = patch.clone();
    }
}

fn from_value<T: serde::de::DeserializeOwned>(what: &str, v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|e| RigError::invalid(format!("{what} 解析失败: {e}")))
}

fn to_value<T: serde::Serialize>(t: &T) -> Value {
    serde_json::to_value(t).expect("serializable")
}

fn set_by_id<T>(items: &mut [T], id: &str, what: &str, get_id: impl Fn(&T) -> &str, fields: &Value) -> Result<()>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let it = items
        .iter_mut()
        .find(|x| get_id(x) == id)
        .ok_or_else(|| RigError::invalid(format!("{what} 不存在: {id}")))?;
    let mut v = to_value(&*it);
    merge_patch(&mut v, fields);
    let new: T = from_value(what, v)?;
    if get_id(&new) != id {
        return Err(RigError::invalid(format!("{what} 不允许通过 set 改 id({id})")));
    }
    *it = new;
    Ok(())
}

fn add_unique<T>(items: &mut Vec<T>, v: Value, what: &str, get_id: impl Fn(&T) -> &str) -> Result<()>
where
    T: serde::de::DeserializeOwned,
{
    let new: T = from_value(what, v)?;
    if items.iter().any(|x| get_id(x) == get_id(&new)) {
        return Err(RigError::invalid(format!("{what} id 已存在: {}", get_id(&new))));
    }
    items.push(new);
    Ok(())
}

fn targets(t: &str, kind: TargetKind, id: &str) -> bool {
    split_target(t).is_some_and(|(k, i, _)| k == kind && i == id)
}

fn drop_target_refs(doc: &mut RigDoc, kind: TargetKind, id: &str) {
    doc.keyforms.retain(|g| !targets(&g.target, kind, id));
    for c in doc.clips.values_mut() {
        c.tracks.retain(|t| !targets(&t.target, kind, id));
    }
}

/// 应用单个操作(不校验)。
pub fn apply_op(doc: &mut RigDoc, op: PatchOp) -> Result<()> {
    match op {
        PatchOp::AddPart { part } => add_unique::<RigPart>(&mut doc.parts, part, "part", |p| &p.id),
        PatchOp::SetPart { id, fields } => set_by_id(&mut doc.parts, &id, "part", |p| &p.id, &fields),
        PatchOp::RemovePart { id } => {
            let n = doc.parts.len();
            doc.parts.retain(|p| p.id != id);
            if doc.parts.len() == n {
                return Err(RigError::invalid(format!("part 不存在: {id}")));
            }
            drop_target_refs(doc, TargetKind::Part, &id);
            Ok(())
        }
        PatchOp::AddBone { bone } => add_unique::<RigBone>(&mut doc.bones, bone, "bone", |b| &b.id),
        PatchOp::SetBone { id, fields } => set_by_id(&mut doc.bones, &id, "bone", |b| &b.id, &fields),
        PatchOp::RemoveBone { id } => {
            let Some(pos) = doc.bones.iter().position(|b| b.id == id) else {
                return Err(RigError::invalid(format!("bone 不存在: {id}")));
            };
            let removed = doc.bones.remove(pos);
            // 子骨骼挂到被删骨骼的父上(静止姿态为绝对坐标,无需换算)
            for b in doc.bones.iter_mut() {
                if b.parent.as_deref() == Some(id.as_str()) {
                    b.parent = removed.parent.clone();
                }
            }
            for p in doc.parts.iter_mut() {
                if p.bone.as_deref() == Some(id.as_str()) {
                    p.bone = removed.parent.clone();
                }
                if let Some(ws) = p.weights.as_mut() {
                    for inf in ws.iter_mut() {
                        inf.retain(|(b, _)| *b != id);
                    }
                }
            }
            drop_target_refs(doc, TargetKind::Bone, &id);
            Ok(())
        }
        PatchOp::SetWeights { part, weights, bone } => {
            let p = doc
                .parts
                .iter_mut()
                .find(|p| p.id == part)
                .ok_or_else(|| RigError::invalid(format!("part 不存在: {part}")))?;
            if weights.is_some() && bone.is_some() {
                return Err(RigError::invalid("set_weights:weights 与 bone 只能给一个".into()));
            }
            p.weights = weights;
            p.bone = bone;
            Ok(())
        }
        PatchOp::AddParameter { parameter } => {
            add_unique::<RigParam>(&mut doc.parameters, parameter, "parameter", |p| &p.id)
        }
        PatchOp::SetParameter { id, fields } => set_by_id(&mut doc.parameters, &id, "parameter", |p| &p.id, &fields),
        PatchOp::RemoveParameter { id } => {
            let n = doc.parameters.len();
            doc.parameters.retain(|p| p.id != id);
            if doc.parameters.len() == n {
                return Err(RigError::invalid(format!("parameter 不存在: {id}")));
            }
            doc.keyforms.retain(|g| !g.params.contains(&id));
            for c in doc.clips.values_mut() {
                c.tracks.retain(|t| !targets(&t.target, TargetKind::Param, &id));
            }
            for ph in doc.physics.iter_mut() {
                ph.inputs.retain(|i| i.param != id);
                ph.outputs.retain(|o| o.param != id);
            }
            doc.physics.retain(|ph| !ph.outputs.is_empty());
            if let Some(b) = doc.auto.blink.as_mut() {
                b.params.retain(|p| *p != id);
                if b.params.is_empty() {
                    doc.auto.blink = None;
                }
            }
            if doc.auto.breath.as_ref().is_some_and(|b| b.param == id) {
                doc.auto.breath = None;
            }
            Ok(())
        }
        PatchOp::AddDeformer { deformer } => {
            add_unique::<RigDeformer>(&mut doc.deformers, deformer, "deformer", |d| &d.id)
        }
        PatchOp::SetDeformer { id, fields } => set_by_id(&mut doc.deformers, &id, "deformer", |d| &d.id, &fields),
        PatchOp::RemoveDeformer { id } => {
            let Some(pos) = doc.deformers.iter().position(|d| d.id == id) else {
                return Err(RigError::invalid(format!("deformer 不存在: {id}")));
            };
            let removed = doc.deformers.remove(pos);
            for d in doc.deformers.iter_mut() {
                if d.parent.as_deref() == Some(id.as_str()) {
                    d.parent = removed.parent.clone();
                }
            }
            for p in doc.parts.iter_mut() {
                if p.deformer.as_deref() == Some(id.as_str()) {
                    p.deformer = removed.parent.clone();
                }
            }
            drop_target_refs(doc, TargetKind::Deformer, &id);
            Ok(())
        }
        PatchOp::SetKeyform { target, params, keys, at, form } => {
            let gi = match doc.keyforms.iter().position(|g| g.target == target && g.params == params) {
                Some(gi) => {
                    if let Some(k) = &keys {
                        if *k != doc.keyforms[gi].keys {
                            return Err(RigError::invalid(format!(
                                "set_keyform:keys 与现有组({target})不一致,改关键值请用 set_keyform_group"
                            )));
                        }
                    }
                    gi
                }
                None => {
                    let keys = keys.ok_or_else(|| {
                        RigError::invalid(format!("set_keyform:组 {target} × {params:?} 不存在,新建须提供 keys"))
                    })?;
                    if keys.len() != params.len() {
                        return Err(RigError::invalid("set_keyform:keys 轴数须与 params 一致".into()));
                    }
                    let n: usize = keys.iter().map(Vec::len).product();
                    doc.keyforms.push(KeyformGroup { target: target.clone(), params: params.clone(), keys, forms: vec![Keyform::default(); n] });
                    doc.keyforms.len() - 1
                }
            };
            let g = &mut doc.keyforms[gi];
            let mut idx = 0usize;
            for (ai, pid) in g.params.iter().enumerate() {
                let v = *at
                    .get(pid)
                    .ok_or_else(|| RigError::invalid(format!("set_keyform:at 缺参数 {pid}")))?;
                let ks = &g.keys[ai];
                let ki = ks.iter().position(|k| (k - v).abs() < 1e-3).ok_or_else(|| {
                    RigError::invalid(format!("set_keyform:at.{pid}={v} 不是关键值之一 {ks:?}"))
                })?;
                idx = idx * ks.len() + ki;
            }
            if idx >= g.forms.len() {
                return Err(RigError::invalid("set_keyform:形态下标越界(forms 数量与 keys 不符)".into()));
            }
            g.forms[idx] = form;
            Ok(())
        }
        PatchOp::SetKeyformGroup { group } => {
            match doc.keyforms.iter_mut().find(|g| g.target == group.target && g.params == group.params) {
                Some(g) => *g = group,
                None => doc.keyforms.push(group),
            }
            Ok(())
        }
        PatchOp::RemoveKeyformGroup { target, params } => {
            let n = doc.keyforms.len();
            doc.keyforms.retain(|g| !(g.target == target && g.params == params));
            if doc.keyforms.len() == n {
                return Err(RigError::invalid(format!("keyform 组不存在: {target} × {params:?}")));
            }
            Ok(())
        }
        PatchOp::SetPhysics { physics } => {
            let new: PhysicsRig = from_value("physics", physics)?;
            match doc.physics.iter_mut().find(|p| p.id == new.id) {
                Some(p) => *p = new,
                None => doc.physics.push(new),
            }
            Ok(())
        }
        PatchOp::RemovePhysics { id } => {
            let n = doc.physics.len();
            doc.physics.retain(|p| p.id != id);
            if doc.physics.len() == n {
                return Err(RigError::invalid(format!("physics 不存在: {id}")));
            }
            Ok(())
        }
        PatchOp::SetAuto { auto } => {
            doc.auto = auto;
            Ok(())
        }
        PatchOp::SetClip { name, clip } => {
            doc.clips.insert(name, clip);
            Ok(())
        }
        PatchOp::RemoveClip { name } => {
            if doc.clips.remove(&name).is_none() {
                return Err(RigError::invalid(format!("clip 不存在: {name}")));
            }
            Ok(())
        }
        PatchOp::SetAnimator { animator } => {
            doc.animator = animator;
            Ok(())
        }
        PatchOp::SetCanvas { size, origin, pixels_per_unit } => {
            if let Some(s) = size {
                doc.canvas.size = s;
            }
            if let Some(o) = origin {
                doc.canvas.origin = o;
            }
            if let Some(p) = pixels_per_unit {
                doc.pixels_per_unit = p;
            }
            Ok(())
        }
        PatchOp::MergeScaffold { template, overwrite } => {
            let s = scaffold(&template, doc.canvas.size)?;
            fn merge<T: Clone>(dst: &mut Vec<T>, src: Vec<T>, id: impl Fn(&T) -> String, overwrite: bool) {
                for it in src {
                    match dst.iter().position(|d| id(d) == id(&it)) {
                        Some(i) if overwrite => dst[i] = it,
                        Some(_) => {}
                        None => dst.push(it),
                    }
                }
            }
            merge(&mut doc.parameters, s.parameters, |p| p.id.clone(), overwrite);
            merge(&mut doc.bones, s.bones, |b| b.id.clone(), overwrite);
            merge(&mut doc.keyforms, s.keyforms, |g| format!("{}|{}", g.target, g.params.join(",")), overwrite);
            merge(&mut doc.physics, s.physics, |p| p.id.clone(), overwrite);
            if overwrite || doc.auto.blink.is_none() {
                doc.auto.blink = s.auto.blink.or(doc.auto.blink.take());
            }
            if overwrite || doc.auto.breath.is_none() {
                doc.auto.breath = s.auto.breath.or(doc.auto.breath.take());
            }
            Ok(())
        }
    }
}

/// 原子批量应用:返回新文档(已通过校验);失败时错误消息带 `ops[i](name)` 前缀。
pub fn apply_ops(doc: &RigDoc, ops: Vec<PatchOp>) -> Result<RigDoc> {
    let mut d = doc.clone();
    for (i, op) in ops.into_iter().enumerate() {
        let name = op.name();
        apply_op(&mut d, op).map_err(|e| RigError::new(e.code, format!("ops[{i}]({name}): {}", e.message)))?;
    }
    validate_doc(&d).map_err(|e| RigError::new(e.code, format!("应用后校验失败: {}", e.message)))?;
    Ok(d)
}
