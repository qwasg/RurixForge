//! forge-rig2d 核心契约测试:校验 / keyform 插值 / FK + LBS / 形变器 / clip /
//! 物理确定性 / 眨眼 / 自动网格 / 自动权重 / patch 原子性与级联 / 预览光栅。

use std::collections::BTreeMap;

use forge_rig2d::autogen::{mesh_auto, rect_mesh, scaffold, weights_auto, MeshAutoOpts, WeightOpts};
use forge_rig2d::eval::{corner_weights, sample_track, warp_map};
use forge_rig2d::patch::{apply_ops, parse_ops};
use forge_rig2d::raster::{render_preview, silhouette_iou, Overlay, PreviewOpts};
use forge_rig2d::sim::SimOptions;
use forge_rig2d::{evaluate, parse_rxrig, pose_from, CompiledRig, Pose, RigCommand, RigSim};
use serde_json::{json, Value};

fn close(a: [f32; 2], b: [f32; 2], eps: f32) -> bool {
    (a[0] - b[0]).abs() < eps && (a[1] - b[1]).abs() < eps
}

/// 两部件:臂(两骨加权)+ 头(刚性 + warp 形变器 + keyform)。
fn fixture() -> Value {
    json!({
        "version": 1,
        "texture": "tex-guid",
        "canvas": { "size": [200, 200], "origin": [100, 200] },
        "parameters": [
            { "id": "ParamAngleX", "min": -30, "max": 30, "default": 0 },
            { "id": "ParamEyeOpen", "min": 0, "max": 1, "default": 1 },
            { "id": "ParamBend", "min": -1, "max": 1, "default": 0 },
            { "id": "ParamHair", "min": -1, "max": 1, "default": 0 }
        ],
        "bones": [
            { "id": "upper", "position": [0, 100], "rotation": 0, "length": 50 },
            { "id": "lower", "parent": "upper", "position": [50, 100], "rotation": 0, "length": 50 }
        ],
        "deformers": [
            { "id": "faceWarp", "type": "warp", "rect": [100, 0, 100, 100], "grid": [1, 1] },
            { "id": "headRot", "type": "rotation", "origin": [150, 100] }
        ],
        "parts": [
            { "id": "arm", "drawOrder": 10,
              "mesh": { "vertices": [[0,95],[100,95],[100,105],[0,105]],
                        "uvs": [[0,0],[100,0],[100,10],[0,10]],
                        "triangles": [[0,1,2],[0,2,3]] },
              "weights": [[["upper",1]],[["lower",1]],[["lower",1]],[["upper",1]]] },
            { "id": "face", "drawOrder": 5, "deformer": "faceWarp",
              "mesh": { "vertices": [[100,0],[200,0],[200,100],[100,100]],
                        "uvs": [[0,20],[100,20],[100,120],[0,120]],
                        "triangles": [[0,1,2],[0,2,3]] } }
        ],
        "keyforms": [
            { "target": "part:face", "params": ["ParamAngleX"], "keys": [[-30, 0, 30]],
              "forms": [ { "offsets": [[-10,0],[-10,0],[-10,0],[-10,0]] }, {}, { "offsets": [[10,0],[10,0],[10,0],[10,0]] } ] },
            { "target": "bone:lower", "params": ["ParamBend"], "keys": [[-1, 1]],
              "forms": [ { "rotation": -90 }, { "rotation": 90 } ] },
            { "target": "deformer:faceWarp", "params": ["ParamEyeOpen"], "keys": [[0, 1]],
              "forms": [ { "offsets": [[0,0],[0,0],[0,-20],[0,-20]] }, {} ] }
        ],
        "physics": [
            { "id": "hair", "inputs": [ { "param": "ParamAngleX", "weight": 1, "kind": "x" } ],
              "segments": [ { "length": 40 } ],
              "outputs": [ { "param": "ParamHair", "segment": 0, "scale": 0.05 } ] }
        ],
        "auto": { "blink": { "params": ["ParamEyeOpen"], "intervalSec": [1, 1] } },
        "clips": {
            "wave": { "duration": 2, "loop": true, "tracks": [
                { "target": "param:ParamBend", "keys": [[0, -1], [1, 1], [2, -1]] },
                { "target": "bone:upper.rotation", "keys": [[0, 0], [2, 20]] },
                { "target": "part:face.opacity", "keys": [[0, 1], [2, 0.5]] }
            ] },
            "once": { "duration": 0.5, "loop": false, "tracks": [] }
        },
        "animator": {
            "defaultState": "idle",
            "parameters": { "go": "trigger" },
            "states": { "idle": { "clip": "wave" }, "jump": { "clip": "once" } },
            "transitions": [
                { "from": "idle", "to": "jump", "when": [ { "trigger": "go" } ] },
                { "from": "jump", "to": "idle", "hasExitTime": true }
            ]
        }
    })
}

fn rig() -> CompiledRig {
    CompiledRig::compile(parse_rxrig(&fixture()).expect("fixture valid")).expect("compile")
}

fn expect_err(mut v: Value, f: impl FnOnce(&mut Value), needle: &str) {
    f(&mut v);
    let e = parse_rxrig(&v).expect_err("should fail");
    assert!(e.message.contains(needle) || e.code.contains(needle), "{needle} not in {e}");
}

#[test]
fn validation_rejects_each_class_of_error() {
    let base = fixture();
    expect_err(base.clone(), |v| v["version"] = json!(2), "version");
    expect_err(base.clone(), |v| v["parts"][0]["mesh"]["triangles"][0] = json!([0, 1, 9]), "越界");
    expect_err(base.clone(), |v| v["parts"][0]["mesh"]["uvs"] = json!([[0, 0]]), "uvs");
    expect_err(base.clone(), |v| v["bones"][0]["parent"] = json!("lower"), "成环");
    expect_err(base.clone(), |v| v["parts"][1]["deformer"] = json!("nope"), "deformer");
    expect_err(base.clone(), |v| v["keyforms"][0]["forms"] = json!([{}, {}]), "forms 数量");
    expect_err(base.clone(), |v| v["keyforms"][0]["keys"] = json!([[0, -30, 30]]), "升序");
    expect_err(base.clone(), |v| v["keyforms"][1]["forms"][0] = json!({ "offsets": [] }), "不适用");
    expect_err(base.clone(), |v| v["parts"][0]["bone"] = json!("upper"), "互斥");
    expect_err(base.clone(), |v| v["parts"][0]["mask"] = json!(["face"]), "RIG_FEATURE_UNSUPPORTED");
    expect_err(base.clone(), |v| v["clips"]["wave"]["tracks"][0]["target"] = json!("bone:upper.zz"), "target");
    expect_err(base.clone(), |v| v["animator"]["defaultState"] = json!("x"), "defaultState");
    expect_err(base, |v| v["physics"][0]["outputs"][0]["segment"] = json!(3), "越界");
}

#[test]
fn corner_weights_multilinear() {
    let k = vec![vec![-30.0, 0.0, 30.0], vec![0.0, 1.0]];
    let w = corner_weights(&k, &[15.0, 0.25]);
    let total: f32 = w.iter().map(|x| x.1).sum();
    assert!((total - 1.0).abs() < 1e-6);
    // axis0 between idx1,2 t=.5; axis1 between 0,1 t=.25 → forms 2,3,4,5
    let mut m: BTreeMap<usize, f32> = BTreeMap::new();
    for (i, x) in w {
        *m.entry(i).or_default() += x;
    }
    assert!((m[&2] - 0.375).abs() < 1e-6 && (m[&3] - 0.125).abs() < 1e-6);
    assert!((m[&4] - 0.375).abs() < 1e-6 && (m[&5] - 0.125).abs() < 1e-6);
    // 夹取
    assert_eq!(corner_weights(&k, &[99.0, -5.0]), vec![(4, 1.0)]);
}

#[test]
fn rest_pose_is_identity() {
    let r = rig();
    let d = evaluate(&r, &Pose::rest(&r));
    for dp in &d.parts {
        assert_eq!(dp.positions, r.doc.parts[dp.part].mesh.vertices, "part {}", dp.part);
    }
    // drawOrder 排序:face(5) 先于 arm(10)
    assert_eq!(d.parts[0].part, 1);
}

#[test]
fn part_keyform_offsets_interpolate() {
    let r = rig();
    let mut pose = Pose::rest(&r);
    pose.set_param(&r, "ParamAngleX", 15.0);
    let d = evaluate(&r, &pose);
    let face = d.parts.iter().find(|p| p.part == 1).unwrap();
    assert!(close(face.positions[0], [105.0, 0.0], 1e-4), "{:?}", face.positions[0]);
}

#[test]
fn bone_fk_and_lbs() {
    let r = rig();
    let mut pose = Pose::rest(&r);
    pose.set_param(&r, "ParamBend", 1.0); // lower +90°
    let d = evaluate(&r, &pose);
    let arm = d.parts.iter().find(|p| p.part == 0).unwrap();
    // 顶点 (100,95) 全权重 lower:绕 (50,100) 转 90° → (55, 150)
    assert!(close(arm.positions[1], [55.0, 150.0], 1e-3), "{:?}", arm.positions[1]);
    // upper 顶点不动
    assert!(close(arm.positions[0], [0.0, 95.0], 1e-4));
    // 骨骼世界矩阵:lower 尾端
    let tail = d.bone_world[1].apply([50.0, 0.0]);
    assert!(close(tail, [50.0, 150.0], 1e-3), "{tail:?}");
}

#[test]
fn warp_deformer_maps_bilinear() {
    let pts = vec![[0.0, 0.0], [10.0, 0.0], [0.0, 10.0], [10.0, 10.0]];
    assert!(close(warp_map([0.0, 0.0, 10.0, 10.0], [1, 1], &pts, [3.0, 7.0]), [3.0, 7.0], 1e-5));
    let r = rig();
    let mut pose = Pose::rest(&r);
    pose.set_param(&r, "ParamEyeOpen", 0.0);
    let d = evaluate(&r, &pose);
    let face = d.parts.iter().find(|p| p.part == 1).unwrap();
    // 底边控制点上移 20 → 底边顶点 y = 80
    assert!(close(face.positions[2], [200.0, 80.0], 1e-3), "{:?}", face.positions[2]);
    assert!(close(face.positions[0], [100.0, 0.0], 1e-3));
}

#[test]
fn rotation_deformer_rotates_about_origin() {
    let mut v = fixture();
    v["parts"][1]["deformer"] = json!("headRot");
    v["keyforms"].as_array_mut().unwrap().push(json!({
        "target": "deformer:headRot", "params": ["ParamHair"], "keys": [[0, 1]],
        "forms": [ {}, { "rotation": 90 } ]
    }));
    let r = CompiledRig::compile(parse_rxrig(&v).unwrap()).unwrap();
    let mut pose = Pose::rest(&r);
    pose.set_param(&r, "ParamHair", 1.0);
    let d = evaluate(&r, &pose);
    let face = d.parts.iter().find(|p| p.part == 1).unwrap();
    // (200,100) 绕 (150,100) 转 90° → (150,150)
    assert!(close(face.positions[2], [150.0, 150.0], 1e-3), "{:?}", face.positions[2]);
}

#[test]
fn clip_tracks_sample_and_apply() {
    let r = rig();
    let wave = &r.doc.clips["wave"];
    assert!((sample_track(&wave.tracks[0], 0.5) - 0.0).abs() < 1e-6);
    let (pose, unknown) = pose_from(&r, &BTreeMap::new(), Some("wave"), 3.0); // 循环 → t=1
    assert!(unknown.is_empty());
    assert!((pose.params[r.param_index["ParamBend"]] - 1.0).abs() < 1e-6);
    assert!((pose.bones[0].rotation - 10.0).abs() < 1e-5);
    assert!((pose.part_opacity[1] - 0.75).abs() < 1e-5);
    let mut ov = BTreeMap::new();
    ov.insert("ParamBend".to_string(), -0.5);
    ov.insert("Nope".to_string(), 1.0);
    let (pose, unknown) = pose_from(&r, &ov, Some("ghost"), 0.0);
    assert!((pose.params[r.param_index["ParamBend"]] + 0.5).abs() < 1e-6);
    assert_eq!(unknown, vec!["clip:ghost".to_string(), "param:Nope".to_string()]);
}

fn run_sim(seed: u64, steps: usize, angle: f32) -> Vec<Vec<f32>> {
    let r = rig();
    let mut sim = RigSim::new(&r, "wave", seed, SimOptions::default());
    let mut out = vec![];
    for i in 0..steps {
        if i == 30 {
            // 链在静止处初始化后再改输入,才会因惯性摆动
            sim.apply(&r, RigCommand::SetParam { param: "ParamAngleX".into(), value: angle, blend: None }).unwrap();
        }
        out.push(sim.step(&r, 1.0 / 60.0).params);
    }
    out
}

#[test]
fn physics_is_deterministic_and_reacts() {
    let a = run_sim(7, 600, 30.0);
    let b = run_sim(7, 600, 30.0);
    assert_eq!(a, b, "同 seed 逐位一致");
    let hair = rig().param_index["ParamHair"];
    let max_abs = a.iter().map(|p| p[hair].abs()).fold(0.0f32, f32::max);
    assert!(max_abs > 0.05, "头发应因根点位移摆动,max={max_abs}");
    // 收敛:末段波动很小
    let tail: Vec<f32> = a[540..].iter().map(|p| p[hair]).collect();
    let spread = tail.iter().cloned().fold(f32::MIN, f32::max) - tail.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread < 0.05, "应阻尼收敛,spread={spread}");
}

#[test]
fn auto_blink_closes_eyes_periodically() {
    let ps = run_sim(1, 180, 0.0);
    let eye = rig().param_index["ParamEyeOpen"];
    let min = ps.iter().map(|p| p[eye]).fold(1.0f32, f32::min);
    assert!(min < 0.05, "3 秒内至少完整眨眼一次,min={min}");
    assert!((ps[10][eye] - 1.0).abs() < 1e-6);
}

#[test]
fn fsm_trigger_and_exit_time() {
    let r = rig();
    let mut sim = RigSim::new(&r, "", 0, SimOptions::default());
    assert_eq!(sim.fsm_state.as_deref(), Some("idle"));
    assert!(sim.apply(&r, RigCommand::Play { clip: "once".into(), restart: false }).is_err());
    sim.apply(&r, RigCommand::SetTrigger { param: "go".into() }).unwrap();
    sim.step(&r, 1.0 / 60.0);
    assert_eq!(sim.fsm_state.as_deref(), Some("jump"));
    for _ in 0..40 {
        sim.step(&r, 1.0 / 60.0);
    }
    assert_eq!(sim.fsm_state.as_deref(), Some("idle"), "hasExitTime 播完回 idle");
    assert!(sim.apply(&r, RigCommand::SetTrigger { param: "nope".into() }).is_err());
}

#[test]
fn manual_mode_play_stop_and_unknowns() {
    let r = rig();
    let mut sim = RigSim::new(&r, "wave", 0, SimOptions::default());
    assert!(!sim.is_fsm());
    assert!(sim.apply(&r, RigCommand::Play { clip: "ghost".into(), restart: false }).is_err());
    assert!(sim.apply(&r, RigCommand::SetBool { param: "x".into(), value: true }).is_err());
    sim.apply(&r, RigCommand::Play { clip: "once".into(), restart: true }).unwrap();
    for _ in 0..60 {
        sim.step(&r, 1.0 / 60.0);
    }
    assert!(sim.finished && !sim.playing);
}

fn disc_atlas(w: u32, h: u32) -> Vec<u8> {
    let mut px = vec![0u8; (w * h * 4) as usize];
    let (cx, cy, r) = (w as f32 / 2.0, h as f32 / 2.0, w.min(h) as f32 * 0.4);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d <= r {
                px[i..i + 4].copy_from_slice(&[200, 120, 80, 255]);
            } else {
                px[i..i + 4].copy_from_slice(&[255, 0, 255, 255]); // 品红底
            }
        }
    }
    px
}

#[test]
fn mesh_auto_covers_disc_and_stays_inside() {
    let (w, h) = (128u32, 128u32);
    let atlas = disc_atlas(w, h);
    let mesh = mesh_auto(&atlas, w, h, [0, 0, w, h], [10.0, 20.0], &MeshAutoOpts::default()).unwrap();
    assert!(mesh.vertices.len() >= 8 && mesh.triangles.len() >= 6);
    assert_eq!(mesh.vertices.len(), mesh.uvs.len());
    // 画布 = 图集 + offset
    assert!(close(mesh.vertices[0], [mesh.uvs[0][0] + 10.0, mesh.uvs[0][1] + 20.0], 1e-4));
    let area: f32 = mesh
        .triangles
        .iter()
        .map(|t| {
            let (a, b, c) = (mesh.uvs[t[0] as usize], mesh.uvs[t[1] as usize], mesh.uvs[t[2] as usize]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() * 0.5
        })
        .sum();
    let disc = std::f32::consts::PI * (128.0 * 0.4f32).powi(2);
    assert!(area >= disc * 0.97 && area <= disc * 1.15, "area={area} disc={disc}");
    // 全透明区域如实报错
    let empty = vec![0u8; (w * h * 4) as usize];
    assert_eq!(mesh_auto(&empty, w, h, [0, 0, w, h], [0.0, 0.0], &MeshAutoOpts::default()).unwrap_err().code, "RIG_MESH_FAILED");
}

#[test]
fn weights_auto_normalized_and_nearest_dominant() {
    let r = rig();
    let ws = weights_auto(&r.doc, "arm", &WeightOpts { smooth: false, ..Default::default() }).unwrap();
    assert_eq!(ws.len(), 4);
    for inf in &ws {
        let s: f32 = inf.iter().map(|x| x.1).sum();
        assert!((s - 1.0).abs() < 0.01, "sum={s}");
    }
    assert_eq!(ws[0][0].0, "upper");
    assert_eq!(ws[1][0].0, "lower");
}

#[test]
fn scaffold_merges_and_validates() {
    let mut doc = forge_rig2d::RigDoc::empty("tex", [400.0, 600.0]);
    doc.parts.push(forge_rig2d::doc::RigPart {
        id: "body".into(),
        draw_order: 0,
        mesh: rect_mesh([0.0, 0.0, 100.0, 100.0], [150.0, 300.0]),
        weights: None,
        bone: None,
        deformer: None,
        opacity: 1.0,
        blend: Default::default(),
        visible: true,
        mask: None,
    });
    for t in ["bust", "fullbody"] {
        assert!(scaffold(t, [400.0, 600.0]).is_ok());
    }
    let ops = parse_ops(&json!([
        { "op": "merge_scaffold", "template": "bust" },
        { "op": "set_weights", "part": "body", "bone": "body" }
    ]))
    .unwrap();
    let d2 = apply_ops(&doc, ops).unwrap();
    let r = CompiledRig::compile(d2).unwrap();
    // 物理驱动头发参数 → 头发骨骼 keyform 生效
    let mut sim = RigSim::new(&r, "", 3, SimOptions::default());
    let mut moved = false;
    for i in 0..90 {
        if i == 10 {
            sim.apply(&r, RigCommand::SetParam { param: "ParamAngleX".into(), value: 30.0, blend: None }).unwrap();
        }
        let pose = sim.step(&r, 1.0 / 60.0);
        if pose.params[r.param_index["ParamHairFront"]].abs() > 0.02 {
            moved = true;
        }
    }
    assert!(moved);
}

#[test]
fn patch_is_atomic_and_cascades() {
    let doc = parse_rxrig(&fixture()).unwrap();
    // 第二个 op 失败 → 整体不生效(返回 Err,原文档不变)
    let bad = parse_ops(&json!([
        { "op": "set_part", "id": "arm", "fields": { "drawOrder": 99 } },
        { "op": "set_part", "id": "ghost", "fields": {} }
    ]))
    .unwrap();
    let e = apply_ops(&doc, bad).unwrap_err();
    assert!(e.message.starts_with("ops[1](set_part)"), "{e}");
    assert_eq!(doc.part("arm").unwrap().draw_order, 10);

    // remove_bone:子骨骼重挂、权重剥离、keyform/轨道清理
    let ops = parse_ops(&json!([{ "op": "remove_bone", "id": "upper" }])).unwrap();
    let d = apply_ops(&doc, ops).unwrap();
    assert_eq!(d.bone("lower").unwrap().parent, None);
    assert!(d.clips["wave"].tracks.iter().all(|t| !t.target.starts_with("bone:upper")));

    // remove_parameter:keyform 组、物理输出、自动眨眼级联
    let ops = parse_ops(&json!([{ "op": "remove_parameter", "id": "ParamEyeOpen" }])).unwrap();
    let d = apply_ops(&doc, ops).unwrap();
    assert!(d.auto.blink.is_none());
    assert!(d.keyforms.iter().all(|g| !g.params.contains(&"ParamEyeOpen".to_string())));

    // set_keyform:新建组 + 坐标写入
    let ops = parse_ops(&json!([
        { "op": "set_keyform", "target": "part:arm", "params": ["ParamHair"], "keys": [[-1, 0, 1]],
          "at": { "ParamHair": 1 }, "form": { "opacity": 0.5 } },
        { "op": "set_keyform", "target": "part:arm", "params": ["ParamHair"],
          "at": { "ParamHair": 0.3 }, "form": {} }
    ]))
    .unwrap();
    let e = apply_ops(&doc, ops).unwrap_err();
    assert!(e.message.contains("不是关键值之一"), "{e}");
    let ops = parse_ops(&json!([
        { "op": "set_keyform", "target": "part:arm", "params": ["ParamHair"], "keys": [[-1, 0, 1]],
          "at": { "ParamHair": 1 }, "form": { "opacity": 0.5 } }
    ]))
    .unwrap();
    let d = apply_ops(&doc, ops).unwrap();
    let g = d.keyforms.iter().find(|g| g.target == "part:arm").unwrap();
    assert_eq!(g.forms[2].opacity, Some(0.5));
}

#[test]
fn json_roundtrip_is_deterministic() {
    let doc = parse_rxrig(&fixture()).unwrap();
    let s1 = doc.to_json_string();
    let s2 = forge_rig2d::parse_rxrig_str(&s1).unwrap().to_json_string();
    assert_eq!(s1, s2);
    assert!(s1.ends_with('\n'));
}

#[test]
fn preview_renders_and_iou() {
    let r = rig();
    let (aw, ah) = (100u32, 120u32);
    let atlas = vec![255u8; (aw * ah * 4) as usize];
    let d = evaluate(&r, &Pose::rest(&r));
    let opts = PreviewOpts { width: 100, height: 100, ..Default::default() };
    let img = render_preview(&r, &d, &atlas, aw, ah, &opts);
    assert_eq!(img.len(), 100 * 100 * 4);
    // 画布 200 → 输出 100(scale .5):face 覆盖右上 50×50
    let at = |x: usize, y: usize| img[(y * 100 + x) * 4 + 3];
    assert_eq!(at(75, 25), 255);
    assert_eq!(at(25, 25), 0);
    assert!((silhouette_iou(&img, &img, true) - 1.0).abs() < 1e-6);
    let mut pose = Pose::rest(&r);
    pose.set_param(&r, "ParamAngleX", 30.0);
    let img2 = render_preview(&r, &evaluate(&r, &pose), &atlas, aw, ah, &opts);
    let iou = silhouette_iou(&img, &img2, true);
    assert!(iou < 0.99 && iou > 0.5, "iou={iou}");
    for ov in [Overlay::Mesh, Overlay::Bones, Overlay::Weights(1)] {
        let o = render_preview(&r, &d, &atlas, aw, ah, &PreviewOpts { overlay: ov, ..opts });
        assert_ne!(o, img, "{ov:?} 应有可见叠加");
    }
}
