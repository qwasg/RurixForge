//! Stage 5 step 4 的两项有意修正(02 §9.5 Stage 5):
//! 1. 缺省环境下仅全金属面补上 rurix 的环境项 base·0.14·ao(EMISSION,tonemap 之前);
//!    全金属和半金属探针均与 rurix 差 ≤ 8,避免旧补丁放大半金属偏差。
//!    场景里有 Environment 组件时不补(纯 Godot 语义)。
//! 2. Mobile 的 3D 缓冲改成 RGBA16F(内层视口 use_hdr_2d + 外层 8 bit 转换):清屏色不再经 RGB10A2 量化,与 rurix ±1。
mod common;
mod g4util;
mod g5util;

use serde_json::json;

use common::serial;
use g4util::{godot_at, material, stats, temp_project, RurixAt, CONFIGS};
use g5util::*;

#[test]
fn metal_faces_get_rurix_ambient_in_default_environment() {
    let _g = serial();
    let root = temp_project("g5-metal", None);
    let mut m = material("metal", [0.9, 0.9, 0.9, 1.0]);
    m["metallic"] = json!(1.0);
    m["roughness"] = json!(0.3);
    let metal = quad_model(&root, "metal", m, vec![]);
    let half = {
        let mut m = material("half", [0.8, 0.4, 0.2, 1.0]);
        m["metallic"] = json!(0.5);
        m["roughness"] = json!(0.6);
        quad_model(&root, "half", m, vec![])
    };
    let ents = vec![
        ent("metal", Some(&metal), [-0.8, 0.0, 0.0], ID, [1.2, 1.2, 1.0], vec![]),
        ent("half", Some(&half), [0.8, 0.0, 0.0], ID, [1.2, 1.2, 1.0], vec![]),
    ];
    let probes = [px_of(-0.8, 0.0), px_of(0.8, 0.0)];
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    scene(&mut rr, "g5-metal", &ents, &ortho_cam());
    let rp = rr.frame(W, H).1;
    drop(rurix);
    let at = |px: &[u8], (x, y): (u32, u32)| mean_rect(px, x - 5, y - 5, x + 5, y + 5);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let ids = scene(&mut r, "g5-metal", &ents, &ortho_cam());
        let gp = r.frame(W, H).1;
        // 有 Environment 组件(纯 Godot 语义、参数与缺省灯的环境相同):不补金属环境项 = 修正前的画面。
        r.call("component.add", json!({ "id": ids[0], "type": "Environment",
            "props": { "ambientSource": "color", "ambientColor": [0.4135, 0.4135, 0.4135, 1.0], "reflectionSource": "disabled", "tonemap": "reinhard", "white": 1000.0 } }));
        let ep = r.frame(W, H).1;
        let maxd = |a: [f64; 3], b: [f64; 3]| (0..3).map(|c| (a[c] - b[c]).abs()).fold(0.0, f64::max);
        for (k, p) in probes.iter().enumerate() {
            let (a, b, n) = (at(&rp, *p), at(&gp, *p), at(&ep, *p));
            let (d, dn) = (maxd(a, b), maxd(a, n));
            eprintln!("G5 {method}/{driver} metal probe{k}: rurix {a:?} godot {b:?} (maxdiff {d:.1}) without-fix {n:?} (maxdiff {dn:.1})");
            if k == 0 {
                assert!(d <= 8.0, "{method}/{driver} 全金属探针:rurix {a:?} vs godot {b:?}");
                assert!(lum(b) - lum(n) > 20.0, "{method}/{driver}:有 Environment 时金属不补环境项");
            }
            if k == 1 {
                assert!(d <= 8.0, "{method}/{driver} 半金属探针:rurix {a:?} vs godot {b:?}");
                assert!((0..3).all(|c| (b[c] - n[c]).abs() <= 1.0), "半金属不应套用全金属环境补偿");
            }
        }
        drop(g);
    }
}

#[test]
fn background_matches_rurix_in_every_config() {
    let _g = serial();
    let root = temp_project("g5-bg", None);
    let w = white(&root);
    let ents = vec![ent("card", Some(&w), [0.0; 3], ID, [1.0, 1.0, 1.0], vec![])];
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    scene(&mut rr, "g5-bg", &ents, &ortho_cam());
    let rp = rr.frame(W, H).1;
    drop(rurix);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        scene(&mut r, "g5-bg", &ents, &ortho_cam());
        let gp = r.frame(W, H).1;
        let (a, b) = (mean_rect(&rp, 2, 2, 40, 40), mean_rect(&gp, 2, 2, 40, 40));
        let (max, mean, _) = stats(&rp, &gp);
        eprintln!("G5 {method}/{driver} bg rurix {a:?} godot {b:?} frame max={max} mean={mean:.3}");
        assert!((0..3).all(|c| (a[c] - b[c]).abs() <= 1.0), "{method}/{driver} 背景 {a:?} vs {b:?}");
        assert!(mean <= 1.0, "{method}/{driver} 全帧平均差 {mean:.3}");
        drop(g);
    }
}
