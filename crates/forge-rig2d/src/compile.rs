//! `RigDoc` → `CompiledRig`:id 解析为下标、骨骼拓扑序与静止矩阵、权重归一化。
//! 每帧求值只用下标访问,不做字符串查找。

use std::collections::HashMap;

use crate::doc::{split_target, RigDeformerKind, RigDoc, TargetKind};
use crate::math::Affine;
use crate::{validate_doc, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Skin {
    None,
    Rigid(usize),
    /// 每顶点 (骨骼下标, 归一化权重);空 = 不受骨骼影响。
    Weighted(Vec<Vec<(usize, f32)>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledPart {
    pub skin: Skin,
    pub deformer: Option<usize>,
    pub tris: Vec<[u16; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Part(usize),
    Deformer(usize),
    Bone(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledGroup {
    /// doc.keyforms 下标。
    pub src: usize,
    pub target: Target,
    pub params: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRig {
    pub doc: RigDoc,
    pub param_index: HashMap<String, usize>,
    pub bone_index: HashMap<String, usize>,
    pub part_index: HashMap<String, usize>,
    pub deformer_index: HashMap<String, usize>,
    /// 父先子后。
    pub bone_order: Vec<usize>,
    pub bone_parent: Vec<Option<usize>>,
    pub bone_rest_world: Vec<Affine>,
    pub bone_rest_inv: Vec<Affine>,
    pub bone_local_rest: Vec<Affine>,
    pub deformer_parent: Vec<Option<usize>>,
    /// warp 静止控制点(rotation 为空)。
    pub deformer_rest_points: Vec<Vec<[f32; 2]>>,
    pub parts: Vec<CompiledPart>,
    pub groups: Vec<CompiledGroup>,
}

fn topo(parents: &[Option<usize>]) -> Vec<usize> {
    let n = parents.len();
    let mut depth = vec![0usize; n];
    for (i, d) in depth.iter_mut().enumerate() {
        let mut cur = parents[i];
        let mut k = 0;
        while let Some(p) = cur {
            k += 1;
            cur = parents[p];
            if k > n {
                break; // 校验已拒绝成环,这里只是防御
            }
        }
        *d = k;
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (depth[i], i));
    order
}

impl CompiledRig {
    pub fn compile(doc: RigDoc) -> Result<CompiledRig> {
        validate_doc(&doc)?;
        let idx = |it: Vec<&String>| -> HashMap<String, usize> {
            it.into_iter().enumerate().map(|(i, s)| (s.clone(), i)).collect()
        };
        let param_index = idx(doc.parameters.iter().map(|p| &p.id).collect());
        let bone_index = idx(doc.bones.iter().map(|b| &b.id).collect());
        let part_index = idx(doc.parts.iter().map(|p| &p.id).collect());
        let deformer_index = idx(doc.deformers.iter().map(|d| &d.id).collect());

        let bone_parent: Vec<Option<usize>> =
            doc.bones.iter().map(|b| b.parent.as_ref().map(|p| bone_index[p])).collect();
        let bone_order = topo(&bone_parent);
        let bone_rest_world: Vec<Affine> =
            doc.bones.iter().map(|b| Affine::trs(b.position, b.rotation, [1.0, 1.0])).collect();
        let bone_rest_inv: Vec<Affine> = bone_rest_world.iter().map(Affine::inverse).collect();
        let bone_local_rest: Vec<Affine> = (0..doc.bones.len())
            .map(|i| match bone_parent[i] {
                Some(p) => bone_rest_inv[p].mul(&bone_rest_world[i]),
                None => bone_rest_world[i],
            })
            .collect();

        let deformer_parent: Vec<Option<usize>> =
            doc.deformers.iter().map(|d| d.parent.as_ref().map(|p| deformer_index[p])).collect();
        let deformer_rest_points = doc
            .deformers
            .iter()
            .map(|d| match &d.kind {
                RigDeformerKind::Warp { rect, grid } => {
                    let (c, r) = (grid[0] as usize, grid[1] as usize);
                    let mut pts = Vec::with_capacity((c + 1) * (r + 1));
                    for j in 0..=r {
                        for i in 0..=c {
                            pts.push([
                                rect[0] + rect[2] * i as f32 / c as f32,
                                rect[1] + rect[3] * j as f32 / r as f32,
                            ]);
                        }
                    }
                    pts
                }
                RigDeformerKind::Rotation { .. } => vec![],
            })
            .collect();

        let parts = doc
            .parts
            .iter()
            .map(|p| {
                let skin = if let Some(b) = &p.bone {
                    Skin::Rigid(bone_index[b])
                } else if let Some(ws) = &p.weights {
                    Skin::Weighted(
                        ws.iter()
                            .map(|inf| {
                                let sum: f32 = inf.iter().map(|(_, w)| *w).sum();
                                if sum <= 1e-8 {
                                    return vec![];
                                }
                                inf.iter()
                                    .filter(|(_, w)| *w > 0.0)
                                    .map(|(b, w)| (bone_index[b], w / sum))
                                    .collect()
                            })
                            .collect(),
                    )
                } else {
                    Skin::None
                };
                CompiledPart {
                    skin,
                    deformer: p.deformer.as_ref().map(|d| deformer_index[d]),
                    tris: p
                        .mesh
                        .triangles
                        .iter()
                        .map(|t| [t[0] as u16, t[1] as u16, t[2] as u16])
                        .collect(),
                }
            })
            .collect();

        let groups = doc
            .keyforms
            .iter()
            .enumerate()
            .map(|(src, g)| {
                let (kind, id, _) = split_target(&g.target).expect("validated");
                let target = match kind {
                    TargetKind::Part => Target::Part(part_index[id]),
                    TargetKind::Deformer => Target::Deformer(deformer_index[id]),
                    TargetKind::Bone => Target::Bone(bone_index[id]),
                    TargetKind::Param => unreachable!("validated"),
                };
                CompiledGroup { src, target, params: g.params.iter().map(|p| param_index[p]).collect() }
            })
            .collect();

        Ok(CompiledRig {
            doc,
            param_index,
            bone_index,
            part_index,
            deformer_index,
            bone_order,
            bone_parent,
            bone_rest_world,
            bone_rest_inv,
            bone_local_rest,
            deformer_parent,
            deformer_rest_points,
            parts,
            groups,
        })
    }

    /// 参数缺省值向量(按 doc.parameters 序)。
    pub fn default_params(&self) -> Vec<f32> {
        self.doc.parameters.iter().map(|p| p.default).collect()
    }

    /// 画布像素 → 实体本地世界单位(Y 翻转、除以 ppu、以 canvas.origin 为原点)。
    pub fn to_local_units(&self, p: [f32; 2]) -> [f32; 2] {
        let o = self.doc.canvas.origin;
        let ppu = self.doc.pixels_per_unit;
        [(p[0] - o[0]) / ppu, (o[1] - p[1]) / ppu]
    }
}
