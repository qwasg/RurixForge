//! RS 网格构建(01 §5.1 / §5.2),只在 [gmain] 调用。
//! 绕序:forge 以逆时针为正面(面法线 = (b−a)×(c−a),meshres.rs),Godot 以顺时针为正面(ArrayMesh.xml),
//! 所以每个三角形按 (a, c, b) 写入,法线 / 切线不变(01 C1;g4_mesh 的单三角测试锁定)。

use godot::classes::rendering_server::PrimitiveType;
use godot::classes::RenderingServer;
use godot::prelude::*;

use engine_host::{MeshData, ModelPrimitive};

use crate::rid::Owned;

// Godot 4 Mesh::ArrayType(rendering_server_enums.h:126-140)。
const ARRAY_VERTEX: usize = 0;
const ARRAY_NORMAL: usize = 1;
const ARRAY_TANGENT: usize = 2;
const ARRAY_TEX_UV: usize = 4;
const ARRAY_BONES: usize = 10;
const ARRAY_WEIGHTS: usize = 11;
const ARRAY_INDEX: usize = 12;
const ARRAY_MAX: usize = 13;

fn surface(rs: &mut Gd<RenderingServer>, mut slots: Vec<(usize, Variant)>) -> Owned {
    let mut arrays = VarArray::new();
    for i in 0..ARRAY_MAX {
        match slots.iter().position(|(k, _)| *k == i) {
            Some(p) => arrays.push(&slots.swap_remove(p).1),
            None => arrays.push(&Variant::nil()),
        }
    }
    let mesh = rs.mesh_create();
    rs.mesh_add_surface_from_arrays(mesh, PrimitiveType::TRIANGLES, &arrays);
    Owned::new(mesh)
}

/// 炸开的交错 pos3 + normal3(stride 24,MeshRenderer)→ 平直着色网格;不放索引、不做平滑法线(01 §5.1)。
/// 内置 cube 的 ±X 面在 forge 里是"从外面看顺时针"(几何法线 (b−a)×(c−a) 与存的面法线相反,assets.rs cube_mesh_bytes),
/// rurix 按法线属性着色、不看绕序,所以看不出来;Godot 在 CULL_DISABLED 下会把背面的法线取反(DO_SIDE_CHECK),
/// 那两个面就成了背光面。这里按存的法线定向每个三角形:几何法线与它同向 → (a, c, b),反向 → (a, b, c),
/// 保证 Godot 的正面恒为法线那一侧(g4_mesh 的单三角 / cube 六面测试锁定)。
/// `two_sided`:rurix 的光栅管线 CULL_MODE_NONE 且不翻背面法线(rurix-rt render_exec.rs:8190),从背面看开放网格
/// 与正面着色相同;Godot 的 CULL_DISABLED 会翻法线。于是 sprite_mesh 腿给每个三角形再补一个反绕序的副本(法线不变),
/// 材质用 CULL_BACK:两面各由"正面朝向相机"的那一份画出,效果 = 不剔除且不翻法线。
pub fn flat_mesh(rs: &mut Gd<RenderingServer>, m: &MeshData, two_sided: bool) -> Owned {
    let f = |i: usize| f32::from_le_bytes([m.vertices[i], m.vertices[i + 1], m.vertices[i + 2], m.vertices[i + 3]]);
    let n = m.vertex_count as usize / 3 * 3;
    let mut pos = Vec::with_capacity(if two_sided { 2 * n } else { n });
    let mut nrm = Vec::with_capacity(pos.capacity());
    for tri in 0..n / 3 {
        let p = |k: usize| {
            let b = (tri * 3 + k) * 24;
            Vector3::new(f(b), f(b + 4), f(b + 8))
        };
        let q = |k: usize| {
            let b = (tri * 3 + k) * 24;
            Vector3::new(f(b + 12), f(b + 16), f(b + 20))
        };
        let g = (p(1) - p(0)).cross(p(2) - p(0));
        let front = if g.dot(q(0)) >= 0.0 { [0usize, 2, 1] } else { [0usize, 1, 2] };
        let back = [front[0], front[2], front[1]];
        let copies: &[[usize; 3]] = if two_sided { &[front, back] } else { &[front] };
        for order in copies {
            for &k in order {
                pos.push(p(k));
                nrm.push(q(k));
            }
        }
    }
    surface(rs, vec![
        (ARRAY_VERTEX, PackedVector3Array::from(pos.as_slice()).to_variant()),
        (ARRAY_NORMAL, PackedVector3Array::from(nrm.as_slice()).to_variant()),
    ])
}

/// 朝 +z 的单位 quad(TexQuad / LegacyQuad):与 viewport/rurix.rs `quad_mesh_bytes`、legacy_draw 的角点和 UV 相同
/// (左下 (0,1)、右下 (1,1)、右上 (1,0)、左上 (0,0);贴图首行在上)。
pub fn quad_mesh(rs: &mut Gd<RenderingServer>) -> Owned {
    let corners = [[-0.5f32, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]];
    let uvs = [[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    // 两个三角形 (0,1,2) / (0,2,3),各按 (a, c, b) 写入。
    for i in [0usize, 2, 1, 0, 3, 2] {
        pos.push(Vector3::new(corners[i][0], corners[i][1], 0.0));
        nrm.push(Vector3::new(0.0, 0.0, 1.0));
        uv.push(Vector2::new(uvs[i][0], uvs[i][1]));
    }
    surface(rs, vec![
        (ARRAY_VERTEX, PackedVector3Array::from(pos.as_slice()).to_variant()),
        (ARRAY_NORMAL, PackedVector3Array::from(nrm.as_slice()).to_variant()),
        (ARRAY_TEX_UV, PackedVector2Array::from(uv.as_slice()).to_variant()),
    ])
}

/// 模型 primitive(01 §5.2):模型空间的 VERTEX / NORMAL / TANGENT(w = 副切线符号)/ TEX_UV / INDEX,
/// 蒙皮时加 BONES / WEIGHTS(权重按和归一,与 modelrt::vertices 的 `/ sum` 同义)。缺省值同 modelrt::vertices。
pub fn model_mesh(rs: &mut Gd<RenderingServer>, p: &ModelPrimitive, skinned: bool) -> Result<Owned, String> {
    let n = p.positions.len();
    if n == 0 || p.indices.len() < 3 {
        return Err("primitive 没有三角形".into());
    }
    let pos: Vec<Vector3> = p.positions.iter().map(|v| Vector3::new(v[0], v[1], v[2])).collect();
    let nrm: Vec<Vector3> =
        (0..n).map(|i| p.normals.get(i).copied().unwrap_or([0., 1., 0.])).map(|v| Vector3::new(v[0], v[1], v[2])).collect();
    let tan: Vec<f32> = (0..n).flat_map(|i| p.tangents.get(i).copied().unwrap_or([1., 0., 0., 1.])).collect();
    let uv: Vec<Vector2> = (0..n).map(|i| p.uv0.get(i).copied().unwrap_or([0., 0.])).map(|v| Vector2::new(v[0], v[1])).collect();
    let mut idx = Vec::with_capacity(p.indices.len());
    for t in p.indices.chunks_exact(3) {
        if t.iter().any(|&i| i as usize >= n) {
            return Err("mesh index out of range".into());
        }
        idx.extend([t[0] as i32, t[2] as i32, t[1] as i32]);
    }
    let mut slots = vec![
        (ARRAY_VERTEX, PackedVector3Array::from(pos.as_slice()).to_variant()),
        (ARRAY_NORMAL, PackedVector3Array::from(nrm.as_slice()).to_variant()),
        (ARRAY_TANGENT, PackedFloat32Array::from(tan.as_slice()).to_variant()),
        (ARRAY_TEX_UV, PackedVector2Array::from(uv.as_slice()).to_variant()),
        (ARRAY_INDEX, PackedInt32Array::from(idx.as_slice()).to_variant()),
    ];
    if skinned {
        let mut bones = Vec::with_capacity(n * 4);
        let mut weights = Vec::with_capacity(n * 4);
        for i in 0..n {
            let j = p.joints.get(i).copied().unwrap_or([0; 4]);
            let w = p.weights.get(i).copied().unwrap_or([1., 0., 0., 0.]);
            let sum: f32 = w.iter().sum();
            let w = if sum > 0. { w.map(|x| x / sum) } else { [1., 0., 0., 0.] };
            bones.extend(j.map(i32::from));
            weights.extend(w);
        }
        slots.push((ARRAY_BONES, PackedInt32Array::from(bones.as_slice()).to_variant()));
        slots.push((ARRAY_WEIGHTS, PackedFloat32Array::from(weights.as_slice()).to_variant()));
    }
    Ok(surface(rs, slots))
}
