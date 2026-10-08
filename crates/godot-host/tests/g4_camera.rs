//! Stage 4 step 6(01 §5.4 / §2.3 C1):相机。只含几何的场景,比较 rurix 与 Godot 的前景轮廓(IoU / 异或像素数)。
//! 编辑器相机(透视 fov 50、fovY 90、正交 orthoSize)、PIE 场景相机(透视带滚转 → 滚转被丢掉、正交、near / far 裁剪)。
mod common;
mod g4util;

use serde_json::{json, Value};

use common::{serial, Rpc};
use g4util::*;

const W: u32 = 320;
const H: u32 = 180;

/// 前景 = 与本帧清屏色任一通道差 > 8。
fn mask(px: &[u8], bg: [u8; 3]) -> Vec<bool> {
    px.chunks_exact(4).map(|p| (0..3).any(|i| p[i].abs_diff(bg[i]) > 8)).collect()
}

fn iou(a: &[bool], b: &[bool]) -> (f64, usize, usize) {
    let (mut i, mut u, mut x) = (0usize, 0usize, 0usize);
    for (p, q) in a.iter().zip(b) {
        i += usize::from(*p && *q);
        u += usize::from(*p || *q);
        x += usize::from(*p != *q);
    }
    (if u == 0 { 1.0 } else { i as f64 / u as f64 }, x, u)
}

fn cubes() -> Vec<Value> {
    let mr = json!([{ "type": "MeshRenderer", "enabled": true, "props": { "mesh": "cube", "material": "" } }]);
    [([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]), ([2.0, 0.5, -1.0], [1.0, 2.0, 1.0]), ([-2.2, 0.3, 0.8], [0.6, 0.6, 0.6]),
     ([0.5, 0.2, 3.0], [0.4, 0.4, 0.4]), ([-1.0, 1.5, -4.0], [1.5, 0.5, 1.5])]
        .iter()
        .enumerate()
        .map(|(i, (t, s))| json!({ "name": format!("c{i}"), "translation": t, "scale": s, "components": mr }))
        .collect()
}

fn camera_entity(props: Value, t: [f32; 3], q: [f32; 4]) -> Value {
    json!({ "name": "cam", "translation": t, "rotation": q, "components": [{ "type": "Camera", "enabled": true, "props": props }] })
}

/// 场景相机:位于 (0, 1, 8),先绕 X 俯 10°,再绕自身 Z 滚 `roll`°(四元数 q = qx · qz)。
fn cam_rot(roll_deg: f32) -> [f32; 4] {
    let (hx, hz) = ((-10.0f32).to_radians() / 2.0, roll_deg.to_radians() / 2.0);
    let (sx, cx, sz, cz) = (hx.sin(), hx.cos(), hz.sin(), hz.cos());
    // qx = (sx, 0, 0, cx),qz = (0, 0, sz, cz);qx · qz
    [sx * cz, -sx * sz, cx * sz, cx * cz]
}

struct Case {
    name: &'static str,
    cam: Value,
    scene_cam: Option<Value>,
}

fn cases() -> Vec<Case> {
    let persp = json!({ "projection": "perspective", "fov": 40.0, "near": 0.1, "far": 100.0 });
    let ortho = json!({ "projection": "orthographic", "orthoSize": 2.5, "near": 0.1, "far": 100.0 });
    let far = json!({ "projection": "perspective", "fov": 40.0, "near": 0.1, "far": 9.0 });
    let near = json!({ "projection": "perspective", "fov": 40.0, "near": 5.5, "far": 100.0 });
    let ed = json!({ "target": [0.0, 0.5, 0.0], "yaw": 35.0, "pitch": 28.0, "dist": 9.0, "fovY": 50.0, "ortho": false });
    vec![
        Case { name: "editor-persp-50", cam: ed.clone(), scene_cam: None },
        Case { name: "editor-persp-90", cam: json!({ "target": [0.0, 0.5, 0.0], "yaw": -20.0, "pitch": 15.0, "dist": 6.0, "fovY": 90.0, "ortho": false }), scene_cam: None },
        Case { name: "editor-ortho", cam: json!({ "target": [0.0, 0.5, 0.0], "yaw": 35.0, "pitch": 28.0, "dist": 9.0, "ortho": true, "orthoSize": 3.0 }), scene_cam: None },
        Case { name: "pie-persp-roll30", cam: ed.clone(), scene_cam: Some(camera_entity(persp.clone(), [0.0, 1.0, 8.0], cam_rot(30.0))) },
        Case { name: "pie-persp-noroll", cam: ed.clone(), scene_cam: Some(camera_entity(persp, [0.0, 1.0, 8.0], cam_rot(0.0))) },
        Case { name: "pie-ortho", cam: ed.clone(), scene_cam: Some(camera_entity(ortho, [0.0, 1.0, 8.0], cam_rot(0.0))) },
        Case { name: "pie-far-9", cam: ed.clone(), scene_cam: Some(camera_entity(far, [0.0, 1.0, 8.0], cam_rot(0.0))) },
        Case { name: "pie-near-5.5", cam: ed, scene_cam: Some(camera_entity(near, [0.0, 1.0, 8.0], cam_rot(0.0))) },
    ]
}

fn shoot(r: &mut Rpc, c: &Case) -> Vec<u8> {
    let mut ents = cubes();
    if let Some(sc) = &c.scene_cam {
        ents.push(sc.clone());
    }
    build(r, &ents, &c.cam);
    if c.scene_cam.is_some() {
        r.call("play.enter", json!({}));
    }
    let px = r.frame(W, H).1;
    if c.scene_cam.is_some() {
        r.call("play.exit", json!({}));
    }
    px
}

#[test]
fn silhouettes_match_rurix_for_editor_and_scene_cameras() {
    let _g = serial();
    let root = temp_project("g4-camera", None);
    let bg = [23u8, 24, 29];
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    let refs: Vec<Vec<bool>> = cases().iter().map(|c| mask(&shoot(&mut rr, c), bg)).collect();
    drop(rurix);
    let idx = |n: &str| cases().iter().position(|c| c.name == n).unwrap();
    assert_eq!(refs[idx("pie-persp-roll30")], refs[idx("pie-persp-noroll")], "rurix 丢掉场景相机的滚转");
    let (full, _, _) = iou(&refs[idx("pie-persp-noroll")], &refs[idx("pie-far-9")]);
    assert!(full < 0.99, "far = 9 应裁掉远处的方块(rurix)");
    let mut report = json!({});
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let bgg = if method == "mobile" { [22u8, 25, 28] } else { bg };
        let ms: Vec<Vec<bool>> = cases().iter().map(|c| mask(&shoot(&mut r, c), bgg)).collect();
        for (k, c) in cases().iter().enumerate() {
            let (v, xor, union) = iou(&refs[k], &ms[k]);
            eprintln!("{method}/{driver} {:18} IoU={v:.5} xor={xor} union={union}", c.name);
            report[format!("{method}/{driver}")][c.name] = json!({ "iou": v, "xor": xor, "union": union });
            assert!(union > 500, "{} 应有前景", c.name);
            assert!(v >= 0.995, "{method}/{driver} {}: 轮廓 IoU {v} < 0.995(xor {xor})", c.name);
        }
        assert_eq!(ms[idx("pie-persp-roll30")], ms[idx("pie-persp-noroll")], "{method}/{driver} 同样丢掉滚转");
    }
    let dir = common::repo_root().join("evidence").join("godot-backend").join("stage4").join("calibration");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("camera-iou.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
