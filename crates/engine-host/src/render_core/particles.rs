//! ParticleEmitter 事件解码(后端中立)。`gpu_particles` 整个模块受 backend-rurix 门控(lib.rs),godot-host 拿不到;
//! 为了不动 rurix 模块,这里按 gpu_particles.rs:16-58 同一判据另写一份,单测逐字节锁定与 `emitter_bytes` 相同。
//! Transform 编码:translation = 中心;scale = [事件年龄 s, 寿命 s, 样式 1..=4];x 或 y ≤ −50 = 池化闲置。

use forge_scene::Scene;

use super::list::ParticleItem;

pub(crate) const MAX_EMITTERS: usize = 64;

/// 与 gpu_particles::enabled() 同一判据(环境变量 FORGE_GPU_PARTICLES = on | 1)。
pub fn enabled() -> bool {
    std::env::var("FORGE_GPU_PARTICLES").is_ok_and(|v| v == "on" || v == "1")
}

/// 启用的发射器按场景序取前 64 个(无效的也占槽),只返回有效事件。
pub(crate) fn decode(scene: &Scene) -> Vec<ParticleItem> {
    let mut out = Vec::new();
    for (slot, entity) in scene
        .entities
        .iter()
        .filter(|e| e.component("ParticleEmitter").is_some_and(|c| c.enabled))
        .take(MAX_EMITTERS)
        .enumerate()
    {
        let t = entity.transform;
        let valid = t.translation.iter().chain(t.scale.iter()).all(|v| v.is_finite())
            && t.translation[0] > -50.
            && t.translation[1] > -50.
            && t.scale[0] >= 0.
            && t.scale[1] > 0.
            && t.scale[0] < t.scale[1]
            && (1. ..=4.).contains(&t.scale[2]);
        if !valid {
            continue;
        }
        out.push(ParticleItem {
            slot: slot as u32,
            entity: entity.id,
            center: t.translation,
            age: t.scale[0],
            lifetime: t.scale[1],
            kind: t.scale[2].round(),
            seed: (entity.id % 1_000_000) as f32,
        });
    }
    out
}

/// extract 用:开关关着时为空(rurix 同样不画)。
pub(crate) fn items(scene: &Scene) -> Vec<ParticleItem> {
    if enabled() {
        decode(scene)
    } else {
        Vec::new()
    }
}

#[cfg(all(test, feature = "backend-rurix"))]
mod tests {
    use super::*;
    use forge_scene::{Component, Entity, Transform};

    /// 把解码结果按 gpu_particles 的 32 字节记录格式重新打包,必须与 emitter_bytes 逐字节相同。
    fn pack(items: &[ParticleItem]) -> (Vec<u8>, usize) {
        let mut bytes = vec![0u8; MAX_EMITTERS * 32];
        for it in items {
            let rec = [it.center[0], it.center[1], it.center[2], it.age, it.lifetime, it.kind, it.seed, 1.0];
            for (i, v) in rec.iter().enumerate() {
                let o = it.slot as usize * 32 + i * 4;
                bytes[o..o + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        (bytes, items.len())
    }

    #[test]
    fn decode_matches_gpu_particles_emitter_bytes() {
        let mut s = Scene::new("p");
        let mk = |id: u64, t: [f32; 3], sc: [f32; 3], enabled: bool| {
            let mut c = Component::new("ParticleEmitter", serde_json::json!({}));
            c.enabled = enabled;
            Entity { entity_guid: None, id, name: format!("p{id}"), transform: Transform { translation: t, scale: sc, ..Transform::default() }, components: vec![c] }
        };
        s.entities.push(mk(3, [1.0, 2.0, 0.5], [0.3, 1.2, 2.4], true));
        s.entities.push(mk(9, [0.0, -60.0, 0.0], [0.1, 1.0, 1.0], true)); // 池化闲置:占槽不出记录
        s.entities.push(mk(12, [4.0, 1.0, 0.0], [0.5, 0.4, 3.0], true)); // age ≥ life:无效
        s.entities.push(mk(1_000_007, [2.0, 2.0, 0.0], [0.0, 2.0, 4.0], true)); // 种子取模
        s.entities.push(mk(5, [2.0, 2.0, 0.0], [0.2, 2.0, 1.0], false)); // 未启用:不占槽
        for i in 0..70u64 {
            s.entities.push(mk(100 + i, [i as f32 * 0.1, 0.0, 0.0], [0.1, 1.0, 1.0], true)); // 超过 64 个的截断
        }
        let d = decode(&s);
        assert_eq!(pack(&d), crate::gpu_particles::emitter_bytes(&s));
        assert_eq!(d.iter().map(|p| p.slot).take(3).collect::<Vec<_>>(), [0, 3, 4]);
    }
}
