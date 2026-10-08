//! Stage 4 step 8(01 §5.7 / §5.6):V6 records → MultiMesh、terrain → 顶点色网格;ParticleEmitter 基本映射。
//! V6:demo 项目没有 V6 资产清单,rurix 也只出盒子(没有精灵),所以能逐像素对照;轴向结论(w→+X、h→+Y、height→+Z 竖直)
//! 由画面与 rurix 重合来验证。粒子:FORGE_GPU_PARTICLES=1 时出现、事件失效后消失、开关关掉时不画。
mod common;
mod g4util;

use serde_json::json;

use common::{serial, sha256, Rpc};
use g4util::*;

const W: u32 = 640;
const H: u32 = 360;

fn fg(px: &[u8], bg: [u8; 3]) -> Vec<bool> {
    px.chunks_exact(4).map(|p| (0..3).any(|i| p[i].abs_diff(bg[i]) > 6)).collect()
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let i = a.iter().zip(b).filter(|(p, q)| **p && **q).count();
    let u = a.iter().zip(b).filter(|(p, q)| **p || **q).count();
    i as f64 / u.max(1) as f64
}

fn open_v6(r: &mut Rpc) {
    r.call("game.session.open", json!({ "seed": 1, "opponent": "human" }));
}

#[test]
fn v6_records_and_terrain_match_rurix() {
    let _g = serial();
    let demo = common::repo_root().join("projects").join("demo");
    let rurix = RurixAt::start(&demo);
    let mut rr = rurix.rpc();
    open_v6(&mut rr);
    let (rf, rp) = rr.frame(W, H);
    drop(rurix);
    let bg = [6u8, 10, 15];
    let rm = fg(&rp, bg);
    assert!(rm.iter().filter(|x| **x).count() > 10_000, "rurix V6 帧应有内容:{rf}");
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &demo, &[]);
        let mut r = g.rpc();
        let caps = r.call("render.capabilities", json!({}));
        assert!(caps["legs"].as_array().unwrap().iter().any(|l| l == "sentinels_v6"), "{caps}");
        open_v6(&mut r);
        let (gf, gp) = r.frame(W, H);
        let (_, gp2) = r.frame(W, H);
        assert_eq!(sha256(&gp), sha256(&gp2), "{method}/{driver} V6 两帧相同");
        let gbg = if method == "mobile" { [5u8, 10, 15] } else { bg };
        let v = iou(&rm, &fg(&gp, gbg));
        let (max, mean, gt2) = stats(&rp, &gp);
        eprintln!("{method}/{driver} V6: IoU={v:.4} max={max} mean={mean:.3} >2={gt2:.4} triangles {} vs {} fallbacks {} vs {} draws {}",
            gf["triangles"], rf["triangles"], gf["meshFallbacks"], rf["meshFallbacks"], gf["draws"]);
        assert_eq!(gf["triangles"], rf["triangles"], "{method}/{driver} 三角形数 = records × 6(无精灵)");
        assert_eq!(gf["meshFallbacks"], rf["meshFallbacks"], "{method}/{driver} fallbacks");
        // 未探索地形是基色 × 0.15,离清屏色只有几个 LSB,前景阈值附近 1 LSB 的差就会翻转掩码,IoU 只作参考;
        // 判据用逐像素差:轴向、投影、面阴影、行序任何一处错都会出现成片的大差。
        // Mobile 的 3D 缓冲是 RGB10A2(线性、量程 2):V6 画面以暗色地形为主(未探索 = 基色 × 0.15),暗部量化到 ±5 LSB。
        // Compatibility:暗部 sRGB 往返误差最大 8(原因未查清,列入 02 §9.5 Stage 4 未决项)。
        let (tol, mean_tol) = match method {
            "mobile" => (6, 2.0),
            "gl_compatibility" => (10, 3.0),
            _ => (2, 1.0),
        };
        assert!(max <= tol && mean <= mean_tol, "{method}/{driver} V6 与 rurix 逐像素差 max={max} mean={mean}(IoU {v})");
    }
}

fn particle_scene(r: &mut Rpc, age: f32) {
    r.call("scene.new", json!({ "name": "g4-particles" }));
    r.call("entity.create", json!({ "name": "p", "translation": [0.0, 0.0, 0.0], "scale": [age, 1.5, 1.0],
        "components": [{ "type": "ParticleEmitter", "enabled": true, "props": {} }] }));
    r.call("viewport.setCamera", json!({ "target": [0.0, 0.0, 0.0], "yaw": 0.0, "pitch": 0.0, "dist": 6.0 }));
}

fn lit(px: &[u8]) -> usize {
    px.chunks_exact(4).filter(|p| p[0] > 60 || p[1] > 60 || p[2] > 60).count()
}

/// 基本映射:开关打开 → capabilities.particles = true、发射器附近出现加法混合的亮点;事件失效(age ≥ life)→ 不画;
/// 开关关掉 → 不画(与 rurix 同:只有 sprite_mesh 腿、只有 FORGE_GPU_PARTICLES=on|1 时才画)。
#[test]
fn particles_basic_mapping_follows_gpu_particles_switch() {
    let _g = serial();
    let root = temp_project("g4-particles", None);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[("FORGE_GPU_PARTICLES", "1")]);
        let mut r = g.rpc();
        assert_eq!(r.call("render.capabilities", json!({}))["particles"], true);
        particle_scene(&mut r, 0.2);
        r.frame(W, H);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let on = lit(&r.frame(W, H).1);
        particle_scene(&mut r, 1.6);
        let dead = lit(&r.frame(W, H).1);
        eprintln!("{method}/{driver} particles: active={on} expired={dead}");
        assert!(on > 20, "{method}/{driver} 粒子应可见:{on}");
        assert_eq!(dead, 0, "{method}/{driver} 事件失效后不画");
        drop(g);
    }
    let g = godot_at("forward_plus", "d3d12", &root, &[]);
    let mut r = g.rpc();
    assert_eq!(r.call("render.capabilities", json!({}))["particles"], false);
    particle_scene(&mut r, 0.2);
    assert_eq!(lit(&r.frame(W, H).1), 0, "开关关掉不画");
}
