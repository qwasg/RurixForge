//! Model animation and CPU skinning. All evaluation uses explicit scene time.
use crate::viewport::{m4_mul, trs_model, M4};
use assetd::model::{ModelAnimationChannel, ModelBundle};
use forge_scene::{Entity, Scene, Transform};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub const IDENTITY: M4 = [
    [1., 0., 0., 0.],
    [0., 1., 0., 0.],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];
static MODELS: OnceLock<Mutex<HashMap<String, Arc<ModelBundle>>>> = OnceLock::new();
pub fn invalidate() {
    if let Some(c) = MODELS.get() {
        c.lock().unwrap().clear();
    }
}
#[cfg(test)]
static TEST_MODELS: OnceLock<Mutex<HashMap<String, Arc<ModelBundle>>>> = OnceLock::new();
#[cfg(test)]
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))] // 调用方(modelrender / golden 测试)只在 backend-rurix 下编译
pub fn prime(model: ModelBundle) {
    TEST_MODELS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(model.guid.clone(), Arc::new(model));
}
pub fn load(reference: &str) -> Result<Arc<ModelBundle>, String> {
    load_revision(reference, None)
}
pub fn load_revision(reference: &str, revision: Option<u64>) -> Result<Arc<ModelBundle>, String> {
    #[cfg(test)]
    if let Some(model) = TEST_MODELS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .get(reference)
        .cloned()
    {
        return Ok(model);
    }
    let root = crate::rpc::project_root();
    let key = format!("{}\0{reference}\0{revision:?}", root.display());
    let mut cache = MODELS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if let Some(m) = cache.get(&key) {
        return Ok(m.clone());
    }
    let project = assetd::project::ForgeProject::load(&root).map_err(|e| e.to_string())?;
    let model = Arc::new(
        match revision {
            Some(rev) => assetd::model::load_model_revision(&project, reference, rev),
            None => assetd::model::load_model(&project, reference),
        }
        .map_err(|e| e.to_string())?,
    );
    cache.insert(key, model.clone());
    Ok(model)
}
pub fn from_cols(a: [f32; 16]) -> M4 {
    std::array::from_fn(|r| std::array::from_fn(|c| a[c * 4 + r]))
}
pub fn point(m: M4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}
pub fn vector(m: M4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2])
}
pub fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if l > 1e-10 {
        v.map(|x| x / l)
    } else {
        [0., 1., 0.]
    }
}
pub fn inverse(m: M4) -> Result<M4, String> {
    let mut a = [[0f32; 8]; 4];
    for r in 0..4 {
        for c in 0..4 {
            a[r][c] = m[r][c];
            a[r][c + 4] = IDENTITY[r][c];
        }
    }
    for c in 0..4 {
        let p = (c..4)
            .max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs()))
            .unwrap();
        if a[p][c].abs() < 1e-10 {
            return Err("MODEL_INVALID: singular transform".into());
        }
        a.swap(c, p);
        let d = a[c][c];
        for j in 0..8 {
            a[c][j] /= d;
        }
        for r in 0..4 {
            if r != c {
                let d = a[r][c];
                for j in 0..8 {
                    a[r][j] -= d * a[c][j];
                }
            }
        }
    }
    Ok(std::array::from_fn(|r| {
        std::array::from_fn(|c| a[r][c + 4])
    }))
}
pub fn normal_matrix(m: M4) -> Result<M4, String> {
    let i = inverse(m)?;
    Ok(std::array::from_fn(|r| std::array::from_fn(|c| i[c][r])))
}
pub fn entity_world(scene: &Scene, e: &Entity) -> Result<M4, String> {
    let mut chain = vec![e];
    let mut current = e;
    while let Some(p) = current
        .component("Parent")
        .filter(|p| p.enabled)
        .and_then(|p| p.props.get("entity"))
        .and_then(|p| p.as_u64())
    {
        if chain.iter().any(|e| e.id == p) {
            return Err("MODEL_INVALID: entity parent cycle".into());
        }
        current = scene
            .entity(p)
            .ok_or_else(|| format!("missing parent {p}"))?;
        chain.push(current);
    }
    Ok(chain
        .iter()
        .rev()
        .fold(IDENTITY, |m, e| m4_mul(m, trs_model(&e.transform))))
}
pub fn ancestor_component<'a>(
    scene: &'a Scene,
    e: &'a Entity,
    kind: &str,
) -> Option<&'a forge_scene::Component> {
    let mut current = e;
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(current.id) {
            return None;
        }
        if let Some(c) = current.component(kind).filter(|c| c.enabled) {
            return Some(c);
        }
        let p = current.component("Parent")?.props["entity"].as_u64()?;
        current = scene.entity(p)?;
    }
}
fn quat_norm(q: [f32; 4]) -> [f32; 4] {
    let l = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if l < 1e-10 {
        [0., 0., 0., 1.]
    } else {
        q.map(|v| v / l)
    }
}
fn slerp(a: [f32; 4], mut b: [f32; 4], t: f32) -> [f32; 4] {
    let a = quat_norm(a);
    b = quat_norm(b);
    let mut dot = (0..4).map(|i| a[i] * b[i]).sum::<f32>();
    if dot < 0. {
        b = b.map(|v| -v);
        dot = -dot;
    }
    if dot > 0.9995 {
        return quat_norm(std::array::from_fn(|i| a[i] + t * (b[i] - a[i])));
    }
    let theta = dot.clamp(-1., 1.).acos();
    let den = theta.sin();
    quat_norm(std::array::from_fn(|i| {
        (a[i] * ((1. - t) * theta).sin() + b[i] * (t * theta).sin()) / den
    }))
}
pub fn sample(ch: &ModelAnimationChannel, time: f32) -> Result<[f32; 4], String> {
    if ch.times.is_empty() {
        return Err("animation channel has no keys".into());
    }
    let cubic = ch.interpolation == "CUBICSPLINE";
    let stride = if cubic { 3 } else { 1 };
    if ch.values.len() != ch.times.len() * stride {
        return Err("animation key/value count mismatch".into());
    }
    let value = |i: usize| ch.values[i * stride + if cubic { 1 } else { 0 }];
    let upper = ch.times.partition_point(|t| *t <= time);
    if upper == 0 {
        return Ok(value(0));
    }
    if upper >= ch.times.len() {
        return Ok(value(ch.times.len() - 1));
    }
    let a = upper - 1;
    let b = upper;
    let dt = ch.times[b] - ch.times[a];
    let t = ((time - ch.times[a]) / dt).clamp(0., 1.);
    if ch.interpolation == "STEP" {
        return Ok(value(a));
    }
    let out = if cubic {
        let p = value(a);
        let q = value(b);
        let m = ch.values[a * 3 + 2];
        let n = ch.values[b * 3];
        let t2 = t * t;
        let t3 = t2 * t;
        std::array::from_fn(|i| {
            (2. * t3 - 3. * t2 + 1.) * p[i]
                + (t3 - 2. * t2 + t) * dt * m[i]
                + (-2. * t3 + 3. * t2) * q[i]
                + (t3 - t2) * dt * n[i]
        })
    } else if ch.path == "rotation" {
        slerp(value(a), value(b), t)
    } else {
        std::array::from_fn(|i| value(a)[i] * (1. - t) + value(b)[i] * t)
    };
    Ok(if ch.path == "rotation" {
        quat_norm(out)
    } else {
        out
    })
}
pub fn node_worlds(
    model: &ModelBundle,
    clip: &str,
    time: f32,
    looped: bool,
) -> Result<Vec<M4>, String> {
    let mut local: Vec<_> = model
        .nodes
        .iter()
        .map(|n| {
            n.matrix.map(from_cols).unwrap_or_else(|| {
                trs_model(&Transform {
                    translation: n.translation,
                    rotation: n.rotation,
                    scale: n.scale,
                })
            })
        })
        .collect();
    if !clip.is_empty() {
        let a = model
            .animations
            .iter()
            .find(|a| a.name == clip)
            .ok_or_else(|| format!("ANIMATION_NOT_FOUND: {clip}"))?;
        let t = if looped && a.duration > 0. {
            time.max(0.).rem_euclid(a.duration)
        } else {
            time.clamp(0., a.duration.max(0.))
        };
        let mut trs: Vec<_> = model
            .nodes
            .iter()
            .map(|n| Transform {
                translation: n.translation,
                rotation: n.rotation,
                scale: n.scale,
            })
            .collect();
        for ch in &a.channels {
            let v = sample(ch, t)?;
            let tr = trs.get_mut(ch.node).ok_or("animation node out of range")?;
            match ch.path.as_str() {
                "translation" => tr.translation = [v[0], v[1], v[2]],
                "rotation" => tr.rotation = v,
                "scale" => tr.scale = [v[0], v[1], v[2]],
                _ => return Err("unsupported animation channel".into()),
            }
            local[ch.node] = trs_model(tr);
        }
    }
    let mut parent = vec![None; model.nodes.len()];
    for (i, n) in model.nodes.iter().enumerate() {
        for &c in &n.children {
            if c >= parent.len() || parent[c].is_some() {
                return Err("invalid model hierarchy".into());
            }
            parent[c] = Some(i);
        }
    }
    fn walk(
        i: usize,
        p: &[Option<usize>],
        local: &[M4],
        out: &mut [M4],
        seen: &mut [u8],
    ) -> Result<(), String> {
        if seen[i] == 2 {
            return Ok(());
        }
        if seen[i] == 1 {
            return Err("model parent cycle".into());
        }
        seen[i] = 1;
        out[i] = if let Some(j) = p[i] {
            walk(j, p, local, out, seen)?;
            m4_mul(out[j], local[i])
        } else {
            local[i]
        };
        seen[i] = 2;
        Ok(())
    }
    let mut out = vec![IDENTITY; local.len()];
    let mut seen = vec![0; local.len()];
    for i in 0..local.len() {
        walk(i, &parent, &local, &mut out, &mut seen)?;
    }
    Ok(out)
}
/// Evaluated primitives in model space. Skin matrix is jointWorld * inverseBind.
pub fn vertices(
    model: &ModelBundle,
    node: usize,
    primitive: usize,
    worlds: &[M4],
    entity: M4,
) -> Result<Vec<u8>, String> {
    let n = &model.nodes[node];
    let p = &model.primitives[primitive];
    let node_m = m4_mul(entity, worlds[node]);
    let palette = if let Some(si) = n.skin {
        let s = model.skins.get(si).ok_or("skin index out of range")?;
        if s.joints.len() != s.inverse_bind_matrices.len() {
            return Err("skin joint/bind count mismatch".into());
        }
        Some(
            s.joints
                .iter()
                .zip(&s.inverse_bind_matrices)
                .map(|(&j, &ib)| {
                    worlds
                        .get(j)
                        .map(|&w| m4_mul(entity, m4_mul(w, from_cols(ib))))
                        .ok_or("joint node out of range")
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        None
    };
    let mut out = Vec::with_capacity(p.indices.len() * 48);
    for &idx in &p.indices {
        let i = idx as usize;
        let pos = *p.positions.get(i).ok_or("mesh index out of range")?;
        let matrix = if let Some(pal) = &palette {
            let joints = p.joints.get(i).ok_or("skin weights missing")?;
            let weights = p.weights.get(i).ok_or("skin weights missing")?;
            let sum: f32 = weights.iter().sum();
            if sum <= 0. {
                return Err("zero skin weights".into());
            }
            let mut m = [[0.; 4]; 4];
            for k in 0..4 {
                if weights[k] > 0. {
                    let jm = pal
                        .get(joints[k] as usize)
                        .ok_or("vertex joint out of range")?;
                    for r in 0..4 {
                        for c in 0..4 {
                            m[r][c] += jm[r][c] * weights[k] / sum;
                        }
                    }
                }
            }
            m
        } else {
            node_m
        };
        let nm = normal_matrix(matrix)?;
        let v = point(matrix, pos);
        let normal = norm(vector(nm, *p.normals.get(i).unwrap_or(&[0., 1., 0.])));
        let tan = *p.tangents.get(i).unwrap_or(&[1., 0., 0., 1.]);
        let tangent = norm(vector(matrix, [tan[0], tan[1], tan[2]]));
        let uv = *p.uv0.get(i).unwrap_or(&[0., 0.]);
        for f in [
            v[0], v[1], v[2], normal[0], normal[1], normal[2], uv[0], uv[1], tangent[0],
            tangent[1], tangent[2], tan[3],
        ] {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    Ok(out)
}
pub fn advance(scene: &mut Scene, dt: f32) {
    for e in &mut scene.entities {
        if let Some(a) = e.component_mut("Animator").filter(|a| a.enabled) {
            if a.props
                .get("playing")
                .and_then(|v| v.as_bool())
                .unwrap_or(true)
            {
                let t = a.props.get("time").and_then(|v| v.as_f64()).unwrap_or(0.);
                let speed = a.props.get("speed").and_then(|v| v.as_f64()).unwrap_or(1.);
                a.props["time"] = serde_json::json!(t + f64::from(dt) * speed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interpolation_and_quaternion_short_path() {
        let mut c = ModelAnimationChannel {
            node: 0,
            path: "translation".into(),
            times: vec![0., 2.],
            values: vec![[0., 0., 0., 0.], [4., 2., 0., 0.]],
            interpolation: "LINEAR".into(),
        };
        assert_eq!(sample(&c, 1.).unwrap(), [2., 1., 0., 0.]);
        c.path = "rotation".into();
        c.values = vec![[0., 0., 0., 1.], [0., 0., 0., -1.]];
        assert_eq!(sample(&c, 1.).unwrap(), [0., 0., 0., 1.]);
    }
    #[test]
    fn inverse_trs_and_normal() {
        let m = trs_model(&Transform {
            translation: [2., 3., 4.],
            rotation: [0., 0., 0., 1.],
            scale: [2., 3., 4.],
        });
        assert_eq!(
            point(inverse(m).unwrap(), point(m, [1., 2., 3.])),
            [1., 2., 3.]
        );
    }
    #[test]
    fn hierarchy_cycle_is_error() {
        let mut s = Scene::new("cycle");
        s.entities.push(Entity { entity_guid: None,
            id: 1,
            name: "x".into(),
            transform: Transform::default(),
            components: vec![forge_scene::Component::new(
                "Parent",
                serde_json::json!({"entity":1}),
            )],
        });
        assert!(entity_world(&s, &s.entities[0]).is_err());
    }
}
