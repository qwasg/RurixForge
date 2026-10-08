//! Stage 4 step 3(01 §5.1):MeshRenderer 真网格。
//! - 单三角(真 .rxmesh,经 asset-pipeline-mcp 导入):正面 / 背面都与 rurix 同色,确认 (a, c, b) 绕序与正面判定;
//! - 内置 cube 六个面逐面与 rurix 对照(±X 面在 forge 里几何绕序与存的法线相反,见 mesh.rs);
//! - 缺失的网格引用回退 cube,meshFallbacks 与 rurix 相同(三角上限同在共享的 meshres 加载器里判);
//! - RID 用 RAII:反复 asset.reload,存活句柄数不增长。
mod common;
mod g4util;

use serde_json::json;

use common::{serial, sha256, Rpc};
use g4util::{at, godot_at, import_asset, temp_project, RurixAt, CONFIGS};

/// f5w2_chair.gltf 同款单三角:(0,0,0)、(1,0,0)、(0,1,0),从 +Z 看逆时针(forge 正面朝 +Z)。
const TRI_GLTF: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"mode":4}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","max":[1,1,0],"min":[0,0,0]}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}]}"#;

const W: u32 = 128;
const H: u32 = 128;
const BG: [u8; 3] = [23, 24, 29];

fn frame(r: &mut Rpc) -> (serde_json::Value, Vec<u8>) {
    r.frame(W, H)
}

fn tri_scene(r: &mut Rpc, guid: &str) {
    r.call("scene.new", json!({ "name": "g4-tri" }));
    r.call("entity.create", json!({ "name": "tri", "translation": [-0.3, -0.3, 0.0],
        "components": [{ "type": "MeshRenderer", "enabled": true, "props": { "mesh": guid, "material": "" } }] }));
}

fn close(a: [u8; 3], b: [u8; 3], tol: u8) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= tol)
}

#[test]
fn single_triangle_front_and_back_match_rurix() {
    let _g = serial();
    let root = temp_project("g4-tri", None);
    let src = root.join("tri-src.gltf");
    std::fs::write(&src, TRI_GLTF).unwrap();
    let guid = import_asset(&root, &src, "Meshes");
    // 三角内部一点(世界 (0,0,0) 附近,平移 -0.3 后落在三角内)与三角外一点。
    let (inside, outside) = ((W / 2 - 4, H / 2 + 4), (W / 2 + 30, H / 2 - 30));
    let views = [("front", 0.0f64), ("back", 180.0)];
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    tri_scene(&mut rr, &guid);
    let mut refs = Vec::new();
    for (name, yaw) in views {
        rr.call("viewport.setCamera", json!({ "target": [0.0, 0.0, 0.0], "yaw": yaw, "pitch": 0.0, "dist": 2.5 }));
        let (f, px) = frame(&mut rr);
        assert_eq!((f["draws"].clone(), f["meshFallbacks"].clone()), (json!(1), json!(0)), "rurix {name}: {f}");
        let c = at(&px, W, inside.0, inside.1);
        assert_ne!(c, BG, "rurix {name} 三角应覆盖内部点");
        refs.push(c);
    }
    assert_eq!(refs[0], refs[1], "rurix 不剔除、不翻法线:正反两面同色");
    drop(rurix);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        tri_scene(&mut r, &guid);
        for (k, (name, yaw)) in views.iter().enumerate() {
            r.call("viewport.setCamera", json!({ "target": [0.0, 0.0, 0.0], "yaw": yaw, "pitch": 0.0, "dist": 2.5 }));
            let (f, px) = frame(&mut r);
            let (_, px2) = frame(&mut r);
            assert_eq!(sha256(&px), sha256(&px2), "{method}/{driver} {name} 两帧相同");
            assert_eq!((f["draws"].clone(), f["meshFallbacks"].clone(), f["triangles"].clone()), (json!(1), json!(0), json!(1)), "{method}/{driver} {name}: {f}");
            let c = at(&px, W, inside.0, inside.1);
            let o = at(&px, W, outside.0, outside.1);
            eprintln!("{method}/{driver} {name}: godot={c:?} rurix={:?} outside={o:?}", refs[k]);
            assert!(close(c, refs[k], 3), "{method}/{driver} {name}: godot {c:?} vs rurix {:?}(正面判定 / 绕序)", refs[k]);
            assert!(close(o, BG, 1), "{method}/{driver} {name}: 三角外应是清屏色 {o:?}");
        }
    }
}

/// 内置 cube:六个面各从正对的方向看,中心像素与 rurix 同色(±3,缺省灯拟合误差见 01 §5.3)。
#[test]
fn builtin_cube_six_faces_match_rurix() {
    let _g = serial();
    let root = temp_project("g4-cube", None);
    let views = [("+Z", 0.0, 0.0), ("+X", 90.0, 0.0), ("-Z", 180.0, 0.0), ("-X", 270.0, 0.0), ("+Y", 0.0, 80.0), ("-Y", 0.0, -80.0)];
    let build = |r: &mut Rpc| {
        r.call("scene.new", json!({ "name": "g4-cube" }));
        r.call("entity.create", json!({ "name": "c", "components": [{ "type": "MeshRenderer", "enabled": true, "props": { "mesh": "cube", "material": "" } }] }));
    };
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    build(&mut rr);
    let mut refs = Vec::new();
    for (_, yaw, pitch) in views {
        rr.call("viewport.setCamera", json!({ "target": [0.0, 0.0, 0.0], "yaw": yaw, "pitch": pitch, "dist": 3.0 }));
        refs.push(at(&frame(&mut rr).1, W, W / 2, H / 2));
    }
    drop(rurix);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        build(&mut r);
        for (k, (face, yaw, pitch)) in views.iter().enumerate() {
            r.call("viewport.setCamera", json!({ "target": [0.0, 0.0, 0.0], "yaw": yaw, "pitch": pitch, "dist": 3.0 }));
            let c = at(&frame(&mut r).1, W, W / 2, H / 2);
            eprintln!("{method}/{driver} face {face}: godot={c:?} rurix={:?}", refs[k]);
            // 背光面只有环境项(rurix 0.28·c):拟合在 c = 0.7 处精确,调色板两端 ±2;Mobile 的 3D 缓冲是 RGB10A2(线性),
            // 暗部再量化 1-2 LSB,所以 Mobile 放宽到 5(01 §5.3 记录)。
            let tol = if method == "mobile" { 5 } else { 3 };
            assert!(close(c, refs[k], tol), "{method}/{driver} face {face}: godot {c:?} vs rurix {:?}", refs[k]);
        }
    }
}

/// 解析不了的网格引用:两边都回退 cube,meshFallbacks = 1、triangles = 12。
#[test]
fn missing_mesh_falls_back_to_cube_like_rurix() {
    let _g = serial();
    let root = temp_project("g4-fallback", None);
    let build = |r: &mut Rpc| {
        r.call("scene.new", json!({ "name": "g4-fb" }));
        r.call("entity.create", json!({ "name": "m", "components": [{ "type": "MeshRenderer", "enabled": true, "props": { "mesh": "Meshes/__missing__.gltf", "material": "" } }] }));
    };
    let rurix = RurixAt::start(&root);
    let mut rr = rurix.rpc();
    build(&mut rr);
    let (rf, _) = frame(&mut rr);
    drop(rurix);
    let g = godot_at("forward_plus", "d3d12", &root, &[]);
    let mut r = g.rpc();
    build(&mut r);
    let (gf, _) = frame(&mut r);
    for k in ["draws", "meshFallbacks", "triangles"] {
        assert_eq!(gf[k], rf[k], "{k}: godot {gf} vs rurix {rf}");
    }
    assert_eq!(gf["meshFallbacks"], json!(1));
}

/// RAII:同一场景反复 asset.reload + 取帧,每次 reload 后打印的存活句柄数相同(不随轮数增长)。
#[test]
fn rid_handles_do_not_leak_across_reloads() {
    let _g = serial();
    let g = godot_at("forward_plus", "d3d12", &common::repo_root().join("projects").join("demo"), &[]);
    let mut r = g.rpc();
    r.call("scene.load", json!({ "path": "Content/Scenes/ab_level1.rxscene" }));
    for _ in 0..4 {
        frame(&mut r);
        r.call("asset.reload", json!({}));
        frame(&mut r);
    }
    let counts: Vec<i64> = g
        .log()
        .iter()
        .filter_map(|l| l.split("live RID handles = ").nth(1).and_then(|v| v.trim().parse().ok()))
        .collect();
    eprintln!("live handles after each reload: {counts:?}");
    assert!(counts.len() >= 4, "应读到每次 reload 的计数:{counts:?}");
    assert!(counts.windows(2).all(|w| w[0] == w[1]), "存活句柄数不应增长:{counts:?}");
}
