//! Stage 4 step 5(01 §5.3):缺省灯标定(灰卡)与 Light 实体映射。
//! - 灰卡:6 块白 Lambert 面,法线与 rurix 写死光向的夹角使 NdotL = 1 / 0.8 / 0.6 / 0.4 / 0.2 / 0;正交相机、卡心像素已知。
//!   模型腿(ModelRenderer 白 PBR,l = normalize(0.45, 0.8, 0.35))与 sprite_mesh 腿(MeshRenderer cube 调色板灰,
//!   L = normalize(0.45, 0.75, 0.35))各一套;逐卡与 rurix 比,并从数据反推最优 energy(k),结果写 evidence。
//! - Light 实体:rurix 不读 Light(同一场景加灯前后逐字节相同);Godot 按表映射 directional / point / spot、颜色、强度、阴影。
mod common;
mod g4util;

use serde_json::{json, Value};

use common::{serial, Rpc};
use g4util::*;

const W: u32 = 640;
const H: u32 = 360;
const NDL: [f32; 6] = [1.0, 0.8, 0.6, 0.4, 0.2, 0.0];

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|x| x / l)
}

/// n = cosθ·l + sinθ·p,p = ẑ 去掉 l 分量后归一(n 的 z 分量恒为正,正对 +Z 的相机都看得见)。
fn card_normal(l: [f32; 3], ndl: f32) -> [f32; 3] {
    let z = [0.0, 0.0, 1.0];
    let d = l[2];
    let p = norm([z[0] - l[0] * d, z[1] - l[1] * d, z[2] - l[2] * d]);
    let (c, s) = (ndl, (1.0 - ndl * ndl).max(0.0).sqrt());
    norm([c * l[0] + s * p[0], c * l[1] + s * p[1], c * l[2] + s * p[2]])
}

/// 把 +Z 转到 n 的最短弧四元数 [x, y, z, w]。
fn quat_from_z(n: [f32; 3]) -> [f32; 4] {
    let (ax, ay, az) = (-n[1], n[0], 0.0f32); // ẑ × n
    let c = n[2].clamp(-1.0, 1.0);
    let s = (ax * ax + ay * ay + az * az).sqrt();
    if s < 1e-6 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let half = c.acos() * 0.5;
    let k = half.sin() / s;
    [ax * k, ay * k, az * k, half.cos()]
}

fn card_x(i: usize) -> f32 {
    -2.25 + 0.9 * i as f32
}

fn probe_px(i: usize) -> (u32, u32) {
    ((320.0 + 120.0 * card_x(i)).round() as u32, 180)
}

fn ortho_cam() -> Value {
    json!({ "target": [0.0, 0.0, 0.0], "yaw": 0.0, "pitch": 0.0, "dist": 10.0, "ortho": true, "orthoSize": 1.5 })
}

fn srgb_to_lin(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// 8 bit 输出 → tonemap 前的线性值。rurix 模型腿:(c/(1+c))^(1/2.2);Godot Reinhard 腿:sRGB(c/(1+c))。
fn pre_tonemap(v: f64, rurix: bool) -> f64 {
    let y = if rurix { (v / 255.0).powf(2.2) } else { srgb_to_lin(v / 255.0) };
    y / (1.0 - y).max(1e-6)
}

fn gray(px: &[u8], i: usize) -> f64 {
    let (x, y) = probe_px(i);
    let m = mean_box(px, W, x, y, 4);
    (m[0] + m[1] + m[2]) / 3.0
}

fn model_cards(project: &std::path::Path) -> Vec<Value> {
    let plane = model("00000000-0000-4000-8000-0000000000b1", vec![node("card", &[0], [0.0; 3], [0.0, 0.0, 0.0, 1.0])],
                      vec![quad_prim("q", 0, 0.7)], vec![material("white", [1.0; 4])], vec![]);
    let mref = write_model(project, "plane", &plane);
    let l = norm([0.45, 0.8, 0.35]);
    (0..6).map(|i| {
        let q = quat_from_z(card_normal(l, NDL[i]));
        json!({ "name": format!("card{i}"), "translation": [card_x(i), 0.0, 0.0], "rotation": q,
                "components": [{ "type": "ModelRenderer", "enabled": true, "props": { "model": mref } }] })
    }).collect()
}

/// sprite_mesh 腿灰卡:导入的平面 quad(真 .rxmesh)转到 nᵢ(cube 会让别的面挡住卡心,不能当灰卡);
/// 只把 MeshRenderer 挂在 id % 8 == 6 的实体上(调色板 [0.60, 0.60, 0.66])。
fn mesh_cards(r: &mut Rpc, quad: &str) {
    r.call("scene.new", json!({ "name": "g4-graycard-mesh" }));
    let l = norm([0.45, 0.75, 0.35]);
    let mut i = 0;
    while i < 6 {
        let e = r.call("entity.create", json!({ "name": format!("e{i}") }));
        let id = e["id"].as_u64().unwrap();
        if id % 8 != 6 {
            continue;
        }
        let q = quat_from_z(card_normal(l, NDL[i]));
        r.call("transform.set", json!({ "id": id, "translation": [card_x(i), 0.0, 0.0], "rotation": q, "scale": [0.7, 0.7, 0.7] }));
        r.call("component.add", json!({ "id": id, "type": "MeshRenderer", "enabled": true, "props": { "mesh": quad, "material": "" } }));
        i += 1;
    }
    r.call("viewport.setCamera", ortho_cam());
}


fn write_evidence(name: &str, v: &Value) {
    let dir = common::repo_root().join("evidence").join("godot-backend").join("stage4").join("calibration");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.json")), serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

/// 模型腿灰卡:逐卡 rurix / godot 8 bit 值、tonemap 前线性值,反推 energy。
#[test]
fn model_leg_gray_card_calibration() {
    let _g = serial();
    let root = temp_project("g4-gray-model", None);
    let cards = model_cards(&root);
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    build(&mut rr, &cards, &ortho_cam());
    let (_, rp) = rr.frame(W, H);
    drop(rurix);
    let rv: Vec<f64> = (0..6).map(|i| gray(&rp, i)).collect();
    let mut report = json!({ "leg": "model", "ndl": NDL, "rurix": rv, "configs": {} });
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        build(&mut r, &cards, &ortho_cam());
        let (_, gp) = r.frame(W, H);
        let gv: Vec<f64> = (0..6).map(|i| gray(&gp, i)).collect();
        // tonemap 前:c_r = rurix,c_g = godot;直射部分 = c − c(NdotL=0);最优 energy = 现值 × Σ ndl·Δr / Σ ndl·Δg。
        let cr: Vec<f64> = rv.iter().map(|v| pre_tonemap(*v, true)).collect();
        let cg: Vec<f64> = gv.iter().map(|v| pre_tonemap(*v, false)).collect();
        let (mut num, mut den) = (0.0, 0.0);
        for i in 0..5 {
            num += NDL[i] as f64 * (cr[i] - cr[5]);
            den += NDL[i] as f64 * (cg[i] - cg[5]);
        }
        let e_fit = 0.9167 * num / den;
        let diffs: Vec<f64> = (0..6).map(|i| gv[i] - rv[i]).collect();
        eprintln!("{method}/{driver} model gray card: e_fit={e_fit:.4} ambient r/g={:.4}/{:.4}", cr[5], cg[5]);
        for i in 0..6 {
            eprintln!("  NdotL={:.1} rurix={:.1} godot={:.1} d={:+.1} c_r={:.4} c_g={:.4}", NDL[i], rv[i], gv[i], diffs[i], cr[i], cg[i]);
        }
        report["configs"][format!("{method}/{driver}")] = json!({ "godot": gv, "diff": diffs, "c_rurix": cr, "c_godot": cg, "energyFit": e_fit });
        let tol = if method == "mobile" { 5.0 } else { 3.0 };
        assert!(diffs.iter().all(|d| d.abs() <= tol), "{method}/{driver} 模型腿灰卡逐卡差应 ≤ {tol}:{diffs:?}");
    }
    write_evidence("gray-card-model", &report);
}

/// sprite_mesh 腿灰卡(调色板灰 cube 的 +Z 面):轴向面由拟合保证(g4_mesh);这里量任意朝向的误差曲线,
/// NdotL = 0(只有环境项)必须贴合,其余如实记录(01 §5.3 的误差表)。
#[test]
fn sprite_mesh_gray_card_error_profile() {
    let _g = serial();
    let root = temp_project("g4-gray-mesh", None);
    let src = root.join("quad-src.gltf");
    std::fs::write(&src, quad_gltf()).unwrap();
    let quad = import_asset(&root, &src, "Meshes");
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    mesh_cards(&mut rr, &quad);
    let (_, rp) = rr.frame(W, H);
    drop(rurix);
    let rv: Vec<f64> = (0..6).map(|i| gray(&rp, i)).collect();
    let mut report = json!({ "leg": "sprite_mesh", "ndl": NDL, "rurix": rv, "configs": {} });
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        mesh_cards(&mut r, &quad);
        let (_, gp) = r.frame(W, H);
        let gv: Vec<f64> = (0..6).map(|i| gray(&gp, i)).collect();
        let diffs: Vec<f64> = (0..6).map(|i| gv[i] - rv[i]).collect();
        eprintln!("{method}/{driver} sprite_mesh gray card:");
        for i in 0..6 {
            eprintln!("  NdotL={:.1} rurix={:.1} godot={:.1} d={:+.1}", NDL[i], rv[i], gv[i], diffs[i]);
        }
        report["configs"][format!("{method}/{driver}")] = json!({ "godot": gv, "diff": diffs });
        let tol0 = if method == "mobile" { 5.0 } else { 3.0 };
        assert!(diffs[5].abs() <= tol0, "{method}/{driver} NdotL=0(环境项)应贴合:{diffs:?}");
        assert!(diffs.iter().all(|d| d.abs() <= 20.0), "{method}/{driver} 任意朝向误差应有界:{diffs:?}");
        assert!(gv.windows(2).all(|w| w[0] >= w[1] - 0.5), "{method}/{driver} 亮度随 NdotL 单调:{gv:?}");
    }
    write_evidence("gray-card-sprite-mesh", &report);
}


fn light(kind: &str, color: [f32; 3], intensity: f32, shadow: bool, t: [f32; 3]) -> Value {
    json!({ "name": format!("light-{kind}"), "translation": t,
            "components": [{ "type": "Light", "enabled": true,
                             "props": { "kind": kind, "color": color, "intensity": intensity, "castShadow": shadow } }] })
}

fn tilted(mut v: Value) -> Value {
    v["rotation"] = json!([-0.258_819, 0.0, 0.0, 0.965_926]);
    v
}

/// Light 实体映射(01 §5.3 表):rurix 不读 Light;Godot 的 directional / point / spot / 未知 kind、颜色、强度、阴影。
#[test]
fn light_entities_map_to_godot_lights() {
    let _g = serial();
    let root = temp_project("g4-lights", None);
    let plane = model("00000000-0000-4000-8000-0000000000b2", vec![node("p", &[0], [0.0; 3], [0.0, 0.0, 0.0, 1.0])],
                      vec![quad_prim("q", 0, 1.0)], vec![material("white", [1.0; 4])], vec![]);
    let mref = write_model(&root, "plane", &plane);
    let wall = json!({ "name": "wall", "scale": [4.0, 4.0, 1.0],
                       "components": [{ "type": "ModelRenderer", "enabled": true, "props": { "model": mref } }] });
    let occluder = json!({ "name": "occluder", "translation": [0.0, 0.0, 0.5], "scale": [0.6, 0.6, 1.0],
                           "components": [{ "type": "ModelRenderer", "enabled": true, "props": { "model": mref } }] });
    let cam = json!({ "target": [0.0, 0.0, 0.0], "yaw": 0.0, "pitch": 0.0, "dist": 3.0 });
    let white = [1.0f32, 1.0, 1.0];
    let cases: Vec<(&str, Vec<Value>)> = vec![
        ("none", vec![]),
        ("dir1", vec![light("directional", white, 1.0, false, [0.0; 3])]),
        ("dir2", vec![light("directional", white, 2.0, false, [0.0; 3])]),
        ("red", vec![light("directional", [1.0, 0.0, 0.0], 1.0, false, [0.0; 3])]),
        ("area", vec![light("area", white, 1.0, false, [0.0; 3])]),
        ("point", vec![light("point", white, 1.0, false, [0.0, 0.0, 1.0])]),
        ("spot", vec![light("spot", white, 3.0, false, [0.0, 0.0, 1.0])]),
        // 方向光绕 X 转 −30°(从上方斜照):遮挡物(z = 0.5)的影子落在它下方 0.29 个单位,相机能看见。
        ("tilt", vec![tilted(light("directional", white, 1.0, true, [0.0; 3]))]),
        ("shadow", vec![tilted(light("directional", white, 1.0, true, [0.0; 3])), occluder.clone()]),
    ];
    let scene = |extra: &[Value]| -> Vec<Value> {
        let mut v = vec![wall.clone()];
        v.extend_from_slice(extra);
        v
    };
    // 中心、离中心 1.5 个单位、遮挡物影子区(墙上 y = −0.48)(透视 dist 3、fov 50:每单位 128.7 px)。
    let (c, e, s) = ((320u32, 180u32), (513u32, 180u32), (320u32, 242u32));
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    build(&mut rr, &scene(&cases[0].1), &cam);
    let r_none = rr.frame(W, H).1;
    build(&mut rr, &scene(&cases[1].1), &cam);
    let r_dir = rr.frame(W, H).1;
    drop(rurix);
    assert_eq!(common::sha256(&r_none), common::sha256(&r_dir), "rurix 不读 Light 组件(02 §3.7):加灯前后逐字节相同");
    // 只有环境项时白面的输出:c = 0.14 → sRGB(Reinhard);directional 强度 1 正对:c ≈ 0.14 + k。
    let ambient_only = 255.0 * (1.055 * (0.14f64 / 1.14).powf(1.0 / 2.4) - 0.055);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let mut m = std::collections::HashMap::new();
        let mut shade = std::collections::HashMap::new();
        for (name, extra) in &cases {
            build(&mut r, &scene(extra), &cam);
            let px = r.frame(W, H).1;
            m.insert(*name, (mean_box(&px, W, c.0, c.1, 3), mean_box(&px, W, e.0, e.1, 3)));
            shade.insert(*name, mean_box(&px, W, s.0, s.1, 3));
        }
        let l = |k: &str, center: bool| -> [f64; 3] { if center { m[k].0 } else { m[k].1 } };
        for (name, _) in &cases {
            eprintln!("{method}/{driver} {name:7} center={:?} edge={:?}", l(name, true).map(f64::round), l(name, false).map(f64::round));
        }
        let expect_dir1 = 255.0 * (1.055 * ((0.14 + 0.9167f64) / (1.0 + 0.14 + 0.9167)).powf(1.0 / 2.4) - 0.055);
        assert!((l("dir1", true)[0] - expect_dir1).abs() <= 8.0, "{method}/{driver} 强度 1 正对:≈ 缺省灯 NdotL=1({expect_dir1:.0})");
        assert!(l("dir2", true)[0] > l("dir1", true)[0] + 10.0, "{method}/{driver} 强度 2 更亮");
        let red = l("red", true);
        assert!((red[0] - l("dir1", true)[0]).abs() <= 3.0 && (red[1] - ambient_only).abs() <= 6.0, "{method}/{driver} 红光:R 同 dir1、G 只剩环境项 {red:?}");
        assert!((l("area", true)[0] - l("dir1", true)[0]).abs() <= 2.0, "{method}/{driver} 未知 kind 按 directional");
        assert!(l("point", true)[0] > l("point", false)[0] + 10.0, "{method}/{driver} 点光源随距离衰减");
        assert!(l("spot", true)[0] > l("spot", false)[0] + 10.0 && (l("spot", false)[0] - ambient_only).abs() <= 6.0, "{method}/{driver} 聚光锥外只剩环境项");
        let (tilt, shadow) = (shade["tilt"], shade["shadow"]);
        eprintln!("{method}/{driver} shadow probe: tilt={:?} shadow={:?}", tilt.map(f64::round), shadow.map(f64::round));
        let expect_tilt = 255.0 * (1.055 * ((0.14 + 0.9167 * 0.866f64) / (1.0 + 0.14 + 0.9167 * 0.866)).powf(1.0 / 2.4) - 0.055);
        assert!((tilt[0] - expect_tilt).abs() <= 6.0, "{method}/{driver} 带 castShadow 的斜照方向光亮度 ≈ {expect_tilt:.0}(不过曝){tilt:?}");
        if method == "gl_compatibility" {
            // GLES3 附加 pass 与 Reinhard 冲突 → Compatibility 下 castShadow 不生效(light.rs、capabilities.coverage)。
            assert!(max_d3(shadow, tilt) <= 2.0, "{method}/{driver} 阴影关闭:{shadow:?} vs {tilt:?}");
        } else {
            assert!(shadow[0] < tilt[0] - 20.0, "{method}/{driver} castShadow:遮挡物的影子区应变暗 {shadow:?} vs {tilt:?}");
        }
        assert!((l("shadow", false)[0] - l("tilt", false)[0]).abs() <= 4.0, "{method}/{driver} 影子外不受影响");
    }
}

fn max_d3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max)
}
