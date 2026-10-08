//! Stage 4 step 7(01 §5.5):Animator。姿态在 forge 侧 CPU 求值(extract 给骨骼矩阵),Godot 用 skeleton_* 做 GPU 蒙皮。
//! - 编辑态三个动画时刻:与 rurix(CPU 蒙皮)的前景轮廓 IoU 与平均差;
//! - PIE 确定性:两个独立的 Godot 进程,同样的 play.enter → pause → step×N,按 steps 对齐后逐帧哈希相同;
//! - 蒙皮开销:150×150 顶点、8 骨骼的网格,静止 vs 每帧换姿态的取帧耗时(profile 的 Godot 侧,forge 侧见 extract3d::skinning_profile)。
mod common;
mod g4util;

use serde_json::{json, Value};

use common::{serial, sha256, Rpc};
use g4util::*;

const W: u32 = 256;
const H: u32 = 144;
const IDQ: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// render_core::extract3d 测试同款:蒙皮 quad(关节 = "bone")+ 挂在 bone 下的 "prop" quad;动画 walk:bone x 0.25 → 1.25。
fn skinned_model() -> Value {
    let mut quad = quad_prim("a", 0, 1.0);
    quad["joints"] = json!([[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]]);
    quad["weights"] = json!([[1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
    let mut skinned = node("skinned", &[0], [0.0; 3], IDQ);
    skinned["skin"] = json!(0);
    let mut bone = node("bone", &[], [0.25, 0.0, 0.0], IDQ);
    bone["children"] = json!([2]);
    let prop = node("prop", &[1], [0.0, 1.1, 0.0], IDQ);
    let mut m = model("00000000-0000-4000-8000-0000000000c1", vec![skinned, bone, prop],
                      vec![quad, quad_prim("b", 1, 0.5)],
                      vec![material("skin", [0.8, 0.3, 0.2, 1.0]), material("prop", [0.2, 0.5, 0.9, 1.0])], vec![]);
    m["roots"] = json!([0, 1]);
    m["skins"] = json!([{ "name": "s", "joints": [1],
        "inverseBindMatrices": [[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.25, 0.0, 0.0, 1.0]], "skeleton": 1 }]);
    m["animations"] = json!([{ "name": "walk", "duration": 2.0, "channels": [{ "node": 1, "path": "translation",
        "times": [0.0, 2.0], "values": [[0.25, 0.0, 0.0, 0.0], [1.25, 0.0, 0.0, 0.0]], "interpolation": "LINEAR" }] }]);
    m
}

fn anim_entity(mref: &str, time: f32, playing: bool) -> Value {
    json!({ "name": "anim", "translation": [-0.5, -0.4, 0.0], "components": [
        { "type": "ModelRenderer", "enabled": true, "props": { "model": mref } },
        { "type": "Animator", "enabled": true, "props": { "clip": "walk", "time": time, "loop": true, "playing": playing } } ] })
}

fn cam() -> Value {
    json!({ "target": [0.0, 0.0, 0.0], "yaw": 0.0, "pitch": 0.0, "dist": 3.2 })
}

fn fg(px: &[u8]) -> Vec<bool> {
    px.chunks_exact(4).map(|p| p[0] > 20 || p[1] > 22 || p[2] > 26).collect()
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let i = a.iter().zip(b).filter(|(p, q)| **p && **q).count();
    let u = a.iter().zip(b).filter(|(p, q)| **p || **q).count();
    i as f64 / u.max(1) as f64
}

#[test]
fn skinned_model_matches_rurix_in_edit_mode() {
    let _g = serial();
    let root = temp_project("g4-anim", None);
    let mref = write_model(&root, "skinned", &skinned_model());
    let times = [0.0f32, 0.5, 1.5];
    let shoot = |r: &mut Rpc, t: f32| -> Vec<u8> {
        build(r, &[anim_entity(&mref, t, false)], &cam());
        r.frame(W, H).1
    };
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    let refs: Vec<Vec<u8>> = times.iter().map(|t| shoot(&mut rr, *t)).collect();
    drop(rurix);
    assert_ne!(sha256(&refs[0]), sha256(&refs[2]), "rurix:动画时刻不同,画面不同");
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        for (k, t) in times.iter().enumerate() {
            let px = shoot(&mut r, *t);
            let (max, mean, gt2) = stats(&refs[k], &px);
            let v = iou(&fg(&refs[k]), &fg(&px));
            eprintln!("{method}/{driver} t={t}: IoU={v:.5} max={max} mean={mean:.3} >2={gt2:.4}");
            assert!(v >= 0.99, "{method}/{driver} t={t}: 蒙皮后轮廓 IoU {v}");
            // Mobile 的模型腿背景(9,11,15)被 RGB10A2 量化成 (6,13,13),全图平均差因此约 2.2(g4_material 同一现象)。
            let mean_tol = if method == "mobile" { 3.0 } else { 1.5 };
            assert!(mean <= mean_tol, "{method}/{driver} t={t}: 平均差 {mean}");
        }
    }
}

/// 一次 PIE:enter → pause → 记下 steps,再 step×N,每步取帧 → [(steps, 帧哈希)]。
fn pie_run(root: &std::path::Path, mref: &str, n: usize) -> Vec<(u64, String)> {
    let g = godot_at("forward_plus", "d3d12", root, &[]);
    let mut r = g.rpc();
    build(&mut r, &[anim_entity(mref, 0.0, true)], &cam());
    r.call("play.enter", json!({}));
    r.call("play.pause", json!({}));
    let mut out = Vec::new();
    for _ in 0..n {
        let s = r.call("play.step", json!({}))["steps"].as_u64().unwrap();
        out.push((s, sha256(&r.frame(W, H).1)));
    }
    r.call("play.exit", json!({}));
    out
}

#[test]
fn pie_animation_is_deterministic_across_processes() {
    let _g = serial();
    let root = temp_project("g4-anim-pie", None);
    let mref = write_model(&root, "skinned", &skinned_model());
    let a = pie_run(&root, &mref, 10);
    let b = pie_run(&root, &mref, 10);
    let common: Vec<_> = a.iter().filter_map(|(s, h)| b.iter().find(|(t, _)| t == s).map(|(_, g)| (*s, h.clone(), g.clone()))).collect();
    eprintln!("run A steps {:?} / run B steps {:?} / 共同 {}", a.iter().map(|x| x.0).collect::<Vec<_>>(), b.iter().map(|x| x.0).collect::<Vec<_>>(), common.len());
    assert!(common.len() >= 5, "两次运行应有足够多的共同 step");
    for (s, h, g) in &common {
        assert_eq!(h, g, "step {s}:两次 PIE 同一步的帧应逐字节相同");
    }
    let distinct: std::collections::HashSet<_> = a.iter().map(|x| x.1.clone()).collect();
    assert!(distinct.len() >= 5, "动画在播放:逐步画面应在变化({} 种)", distinct.len());
}


/// 150×150 顶点、8 骨骼链的网格(与 extract3d::skinning_profile 同形)。
fn grid_model(n: usize, bones: usize) -> Value {
    let (mut pos, mut nrm, mut tan, mut uv, mut jnt, mut wts, mut idx) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f32 / (n - 1) as f32, y as f32 / (n - 1) as f32);
            pos.push(json!([u * 4.0 - 2.0, v - 0.5, 0.0]));
            nrm.push(json!([0.0, 0.0, 1.0]));
            tan.push(json!([1.0, 0.0, 0.0, 1.0]));
            uv.push(json!([u, v]));
            let b = ((u * (bones - 1) as f32).floor() as usize).min(bones - 2);
            let t = u * (bones - 1) as f32 - b as f32;
            jnt.push(json!([b, b + 1, 0, 0]));
            wts.push(json!([1.0 - t, t, 0.0, 0.0]));
        }
    }
    for y in 0..n - 1 {
        for x in 0..n - 1 {
            let i = y * n + x;
            idx.extend([i, i + 1, i + n + 1, i, i + n + 1, i + n]);
        }
    }
    let prim = json!({ "id": "grid", "positions": pos, "normals": nrm, "tangents": tan, "uv0": uv, "indices": idx,
                       "joints": jnt, "weights": wts, "material": 0 });
    let mut nodes = vec![{ let mut s = node("skinned", &[0], [0.0; 3], IDQ); s["skin"] = json!(0); s }];
    for b in 0..bones {
        let mut nb = node(&format!("b{b}"), &[], [if b == 0 { -2.0 } else { 4.0 / (bones - 1) as f32 }, 0.0, 0.0], IDQ);
        if b + 1 < bones {
            nb["children"] = json!([b + 2]);
        }
        nodes.push(nb);
    }
    let mut m = model("00000000-0000-4000-8000-0000000000c2", nodes, vec![prim], vec![material("grid", [0.7, 0.7, 0.75, 1.0])], vec![]);
    m["roots"] = json!([0, 1]);
    let ib: Vec<Value> = (0..bones)
        .map(|b| json!([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0 - b as f32 * 4.0 / (bones - 1) as f32, 0.0, 0.0, 1.0]))
        .collect();
    m["skins"] = json!([{ "name": "s", "joints": (1..=bones).collect::<Vec<_>>(), "inverseBindMatrices": ib, "skeleton": 1 }]);
    let ch: Vec<Value> = (1..=bones).map(|j| json!({ "node": j, "path": "rotation", "times": [0.0, 1.0, 2.0],
        "values": [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.1305, 0.9914], [0.0, 0.0, 0.0, 1.0]], "interpolation": "LINEAR" })).collect();
    m["animations"] = json!([{ "name": "walk", "duration": 2.0, "channels": ch }]);
    m
}

/// profile 的 Godot 侧:静止取帧 vs 每帧推进一步动画后取帧(GPU 蒙皮每帧只上传 8 个骨骼矩阵)。
#[test]
fn gpu_skinning_frame_cost() {
    let _g = serial();
    let root = temp_project("g4-anim-cost", None);
    let mref = write_model(&root, "grid", &grid_model(150, 8));
    let ms = |r: &mut Rpc, animate: bool, n: u32| -> f64 {
        r.frame(W, H);
        let t0 = std::time::Instant::now();
        for _ in 0..n {
            if animate {
                r.call("play.step", json!({}));
            }
            r.frame(W, H);
        }
        t0.elapsed().as_secs_f64() * 1000.0 / n as f64
    };
    let mut report = json!({});
    for (label, godot) in [("rurix", false), ("godot-forward_plus-d3d12", true)] {
        let (_keep_g, _keep_r, mut r);
        if godot {
            let g = godot_at("forward_plus", "d3d12", &root, &[]);
            r = g.rpc();
            _keep_g = Some(g);
            _keep_r = None;
        } else {
            let x = RurixAt::start(&root);
            r = x.rpc();
            _keep_r = Some(x);
            _keep_g = None;
        }
        build(&mut r, &[anim_entity(&mref, 0.3, true)], &cam());
        let still = ms(&mut r, false, 20);
        r.call("play.enter", json!({}));
        r.call("play.pause", json!({}));
        let moving = ms(&mut r, true, 20);
        r.call("play.exit", json!({}));
        eprintln!("SKINNING_FRAME_COST {label}: still={still:.2}ms/frame animated={moving:.2}ms/frame (22500 vertices, 8 bones)");
        report[label] = json!({ "stillMs": still, "animatedMs": moving });
    }
    let dir = common::repo_root().join("evidence").join("godot-backend").join("stage4").join("calibration");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("skinning-frame-cost.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
