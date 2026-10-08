//! Stage 4 step 4(01 §5.2):`.mat` / 模型材质 → BaseMaterial3D,逐行与 rurix 对照。
//! 一张 2×4 的 quad 阵列(模型腿、缺省灯):棋盘 albedo(nearest)、法线贴图 ±Y 倾斜(判定法线 Y 方向)、
//! AO(左 0 右 255、strength 0.5,判定 AO 与 ao_light_affect 的对应)、emissive、unlit、MASK、materialOverrides;
//! 另测 BLEND / doubleSided,以及 sprite_mesh 腿 MeshRenderer + 贴图材质(贴图 quad)。
mod common;
mod g4util;

use serde_json::{json, Value};

use common::{serial, sha256, Rpc};
use g4util::*;

const W: u32 = 256;
const H: u32 = 128;
const ID: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// 法线贴图:切线空间 normalize(0, ty, 1) 编码成 RGB8。
fn normal_tex(id: &str, ty: f32) -> Value {
    let l = (ty * ty + 1.0f32).sqrt();
    let enc = |c: f32| ((c / l * 0.5 + 0.5) * 255.0).round() as u8;
    texture(id, 4, 4, rgba(4, 4, |_, _| [enc(0.0), enc(ty), enc(1.0), 255]), true)
}

/// 8 个节点、各一个 quad、各一个材质(见模块注释);返回模型 JSON。
fn rows_model() -> Value {
    let mut mats = Vec::new();
    let mut checker = material("checker", [1.0, 1.0, 1.0, 1.0]);
    checker["baseColorTexture"] = json!(0);
    checker["roughness"] = json!(0.8);
    mats.push(checker);
    let mut ny_up = material("ny_up", [1.0; 4]);
    ny_up["normalTexture"] = json!(1);
    mats.push(ny_up);
    let mut ny_dn = material("ny_dn", [1.0; 4]);
    ny_dn["normalTexture"] = json!(2);
    mats.push(ny_dn);
    let mut ao = material("ao", [1.0; 4]);
    ao["occlusionTexture"] = json!(3);
    ao["occlusionStrength"] = json!(0.5);
    mats.push(ao);
    let mut em = material("emissive", [0.2, 0.2, 0.2, 1.0]);
    em["emissive"] = json!([1.0, 0.5, 0.0]);
    mats.push(em);
    let mut unlit = material("unlit", [0.0, 1.0, 0.0, 1.0]);
    unlit["unlit"] = json!(true);
    mats.push(unlit);
    let mut mask = material("mask", [1.0; 4]);
    mask["baseColorTexture"] = json!(4);
    mask["alphaMode"] = json!("MASK");
    mats.push(mask);
    mats.push(material("override", [1.0; 4]));
    let texs = vec![
        texture("checker", 8, 8, rgba(8, 8, |x, y| match (x < 4, y < 4) {
            (true, true) => [255, 0, 0, 255],
            (false, true) => [0, 0, 255, 255],
            (true, false) => [0, 255, 0, 255],
            (false, false) => [255, 255, 255, 255],
        }), true),
        normal_tex("ny_up", 0.5),
        normal_tex("ny_dn", -0.5),
        texture("ao", 4, 4, rgba(4, 4, |x, _| if x < 2 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }), true),
        texture("mask", 4, 4, rgba(4, 4, |x, _| if x < 2 { [255, 255, 255, 0] } else { [255, 255, 255, 255] }), true),
    ];
    let pos = |i: usize| -> [f32; 3] { [-1.65 + 1.1 * (i % 4) as f32, if i < 4 { 0.55 } else { -0.55 }, 0.0] };
    let nodes = (0..8).map(|i| node(&format!("n{i}"), &[i], pos(i), ID)).collect();
    let prims = (0..8).map(|i| quad_prim(&format!("q{i}"), i, 0.9)).collect();
    model("00000000-0000-4000-8000-0000000000a1", nodes, prims, mats, texs)
}

/// 8 块 quad 中心的像素(透视、dist 2.8、fov 50、256×128:每世界单位 49 px)。
fn center(i: usize) -> (u32, u32) {
    let x = (128.0 + (-1.65 + 1.1 * (i % 4) as f32) * 49.0).round() as u32;
    let y = if i < 4 { 37 } else { 91 };
    (x, y)
}

fn cam() -> Value {
    json!({ "target": [0.0, 0.0, 0.0], "yaw": 0.0, "pitch": 0.0, "dist": 2.8 })
}

fn rows_entities(model_ref: &str) -> Vec<Value> {
    // 节点 7("override")经 materialOverrides 改成蓝色(slot 7 = 该 primitive 的材质下标)。
    let ovr = json!({ "7": { "baseColor": [0.0, 0.0, 1.0, 1.0] } });
    vec![json!({ "name": "rows", "components": [{ "type": "ModelRenderer", "enabled": true,
                 "props": { "model": model_ref, "materialOverrides": ovr } }] })]
}

/// 每个探针点:(名字, 块号, 相对中心的像素偏移)。
const PROBES: [(&str, usize, i32, i32); 14] = [
    ("checker-red", 0, -11, -11), ("checker-blue", 0, 11, -11), ("checker-green", 0, -11, 11), ("checker-white", 0, 11, 11),
    ("normal+Y", 1, 0, 0), ("normal-Y", 2, 0, 0), ("ao-left", 3, -11, 0), ("ao-right", 3, 11, 0),
    ("emissive", 4, 0, 0), ("unlit", 5, 0, 0), ("mask-left", 6, -11, 0), ("mask-right", 6, 11, 0),
    ("override", 7, 0, 0), ("bg", 99, 0, 0),
];

fn probe(px: &[u8], i: usize, dx: i32, dy: i32) -> [f64; 3] {
    let (x, y) = if i == 99 { (128, 64) } else { center(i) };
    mean_box(px, W, (x as i32 + dx) as u32, (y as i32 + dy) as u32, 2)
}

fn lum(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn max_d(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max)
}

fn frame_of(r: &mut Rpc) -> Vec<u8> {
    let (f, px) = r.frame(W, H);
    let (_, px2) = r.frame(W, H);
    assert_eq!(sha256(&px), sha256(&px2), "两帧相同:{f}");
    px
}


/// 与 rurix 的逐探针容差(模型腿缺省灯是标定出来的近似,不是逐像素一致;数值见 01 §5.2 / §5.3)。
const T_PBR: f64 = 12.0;

#[test]
fn pbr_material_rows_follow_rurix() {
    let _g = serial();
    let root = temp_project("g4-mat", None);
    let mref = write_model(&root, "rows", &rows_model());
    let ents = rows_entities(&mref);
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    build(&mut rr, &ents, &cam());
    let rp = frame_of(&mut rr);
    drop(rurix);
    let rv: Vec<[f64; 3]> = PROBES.iter().map(|&(_, i, dx, dy)| probe(&rp, i, dx, dy)).collect();
    let (bg, nyu, nyd) = (rv[13], rv[4], rv[5]);
    assert!(max_d(bg, [9.0, 11.0, 15.0]) <= 1.0, "rurix 模型腿清屏色 {bg:?}");
    assert!(max_d(rv[10], bg) <= 1.0 && max_d(rv[11], bg) > 20.0, "rurix MASK:左半丢弃、右半可见");
    assert!(lum(nyu) > lum(nyd) + 5.0, "rurix:法线朝 +Y 倾斜的块更亮(光从上方来) {nyu:?} vs {nyd:?}");
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        build(&mut r, &ents, &cam());
        let gp = frame_of(&mut r);
        let gv: Vec<[f64; 3]> = PROBES.iter().map(|&(_, i, dx, dy)| probe(&gp, i, dx, dy)).collect();
        for (k, (name, ..)) in PROBES.iter().enumerate() {
            eprintln!("{method}/{driver} {name:14} rurix={:?} godot={:?} d={:.1}",
                rv[k].map(|v| v.round()), gv[k].map(|v| v.round()), max_d(rv[k], gv[k]));
        }
        let gbg = gv[13];
        let bg_tol = if method == "mobile" { 4.0 } else { 2.0 };
        assert!(max_d(gbg, bg) <= bg_tol, "{method}/{driver} 清屏色(Reinhard 逆变换预补偿){gbg:?}");
        assert!(max_d(gv[10], gbg) <= 2.0 && max_d(gv[11], gbg) > 20.0, "{method}/{driver} MASK 左丢弃右可见");
        for (k, ch) in [(0usize, 0usize), (1, 2), (2, 1)] {
            let c = gv[k];
            assert!((0..3).all(|j| j == ch || c[ch] > c[j] + 30.0), "{method}/{driver} {} 主通道应是 {ch}:{c:?}", PROBES[k].0);
        }
        assert!(lum(gv[4]) > lum(gv[5]) + 5.0, "{method}/{driver} 法线 Y 方向:+Y 倾斜应更亮(与 rurix 同),{:?} vs {:?}", gv[4], gv[5]);
        assert!(lum(gv[6]) < lum(gv[7]), "{method}/{driver} AO 左半更暗");
        assert!(gv[12][2] > gv[12][0] + 30.0 && gv[12][2] > gv[12][1] + 30.0, "{method}/{driver} materialOverrides 改成蓝色");
        for k in [0usize, 1, 2, 3, 4, 5, 6, 7, 8, 9, 12] {
            assert!(max_d(rv[k], gv[k]) <= T_PBR, "{method}/{driver} {}: godot {:?} vs rurix {:?}", PROBES[k].0, gv[k], rv[k]);
        }
    }
}

/// BLEND(半透明红叠在不透明白上)与 doubleSided(背对相机:单面被剔除、双面可见)。
#[test]
fn alpha_blend_and_double_sided_follow_rurix() {
    let _g = serial();
    let root = temp_project("g4-alpha", None);
    let mut blend = material("blend", [1.0, 0.0, 0.0, 0.5]);
    blend["alphaMode"] = json!("BLEND");
    let mut two = material("two", [0.2, 0.8, 0.2, 1.0]);
    two["doubleSided"] = json!(true);
    let back = [0.0, 1.0, 0.0, 0.0]; // 绕 Y 转 180°:quad 背对相机
    let m = model(
        "00000000-0000-4000-8000-0000000000a2",
        vec![
            node("white", &[0], [-1.1, 0.0, 0.0], ID),
            node("blend", &[1], [-0.8, 0.0, 0.3], ID),
            node("single", &[2], [0.55, 0.0, 0.0], back),
            node("double", &[3], [1.65, 0.0, 0.0], back),
        ],
        vec![quad_prim("a", 0, 0.9), quad_prim("b", 1, 0.9), quad_prim("c", 2, 0.9), quad_prim("d", 3, 0.9)],
        vec![material("white", [1.0; 4]), blend, material("single", [0.2, 0.8, 0.2, 1.0]), two],
        vec![],
    );
    let mref = write_model(&root, "alpha", &m);
    let ents = vec![model_entity("alpha", &mref, [0.0, 0.0, 0.0], vec![])];
    // 白块中心 px ≈ 74(52..96);半透明块离相机近 0.3,投影放大 1.12 倍,覆盖 59..108。
    let pts = [("white-only", 55u32, 64u32), ("blend-over-white", 84, 64), ("single-back", 155, 64), ("double-back", 209, 64)];
    let sample = |px: &[u8]| -> Vec<[f64; 3]> { pts.iter().map(|&(_, x, y)| mean_box(px, W, x, y, 2)).collect() };
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    build(&mut rr, &ents, &cam());
    let rv = sample(&frame_of(&mut rr));
    drop(rurix);
    assert!(max_d(rv[2], [9.0, 11.0, 15.0]) <= 1.0, "rurix 单面背面被丢弃 {:?}", rv[2]);
    assert!(rv[3][1] > 40.0, "rurix 双面背面可见 {:?}", rv[3]);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        build(&mut r, &ents, &cam());
        let gv = sample(&frame_of(&mut r));
        for (k, (name, ..)) in pts.iter().enumerate() {
            eprintln!("{method}/{driver} {name:16} rurix={:?} godot={:?}", rv[k].map(|v| v.round()), gv[k].map(|v| v.round()));
        }
        // Mobile 的 3D 缓冲是 RGB10A2(线性、量程 2):清屏这种很暗的值步长约 0.002,sRGB 下差 3-4 LSB。
        let bg_tol = if method == "mobile" { 4.0 } else { 2.0 };
        assert!(max_d(gv[2], [9.0, 11.0, 15.0]) <= bg_tol, "{method}/{driver} 单面背面应被剔除 {:?}", gv[2]);
        assert!(gv[3][1] > 40.0, "{method}/{driver} 双面背面可见 {:?}", gv[3]);
        assert!(gv[1][0] > gv[1][1] + 20.0 && gv[1][0] < gv[0][0] + 1.0, "{method}/{driver} 半透明红叠在白上 {:?}", gv[1]);
        for k in [0usize, 1, 3] {
            // GLES3(Compatibility)在场景着色器里逐物体 tonemap + sRGB 编码,再在 sRGB 帧缓冲里混合;
            // rurix 与 RD 渲染器在(近似)线性空间混合。半透明像素在 Compatibility 下偏暗约 30,属渲染方式差异(01 §7)。
            let tol = if k == 1 && method == "gl_compatibility" { 32.0 } else { T_PBR };
            assert!(max_d(rv[k], gv[k]) <= tol, "{method}/{driver} {}: godot {:?} vs rurix {:?}", pts[k].0, gv[k], rv[k]);
        }
    }
}

/// sprite_mesh 腿:MeshRenderer + 带 albedo 贴图的材质 → 贴图 quad(不受光、最近邻、洋红色键),与 rurix 几乎逐像素一致。
#[test]
fn textured_mesh_renderer_quads_match_rurix() {
    let _g = serial();
    let demo = common::repo_root().join("projects").join("demo");
    let (w, h) = (320u32, 180u32);
    let scene = json!({ "path": "Content/Scenes/ab_level1.rxscene" });
    let rurix = RurixAt::start(&demo);
    let mut rr = rurix.rpc();
    rr.call("scene.load", scene.clone());
    let (rf, rp) = rr.frame(w, h);
    drop(rurix);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &demo, &[]);
        let mut r = g.rpc();
        r.call("scene.load", scene.clone());
        let (gf, gp) = r.frame(w, h);
        let (max, mean, gt2) = stats(&rp, &gp);
        eprintln!("{method}/{driver} ab_level1: draws {} vs {} max={max} mean={mean:.3} >2={gt2:.4}", gf["draws"], rf["draws"]);
        assert_eq!(gf["draws"], rf["draws"], "{method}/{driver} draws");
        // 最大差来自最近邻采样恰落在 texel 边界的个别像素;GLES3 的 sRGB 往返精度低,约 1/10 像素差 3 以内。
        let gt2_max = if method == "gl_compatibility" { 0.15 } else { 0.02 };
        assert!(mean <= 0.5 && gt2 <= gt2_max, "{method}/{driver} 贴图 quad 应几乎逐像素一致:mean={mean} >2={gt2}");
    }
}
