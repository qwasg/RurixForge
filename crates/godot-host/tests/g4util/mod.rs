//! Stage 4 集成测试的公共部分(g4_*.rs 共用;Stage 3 的 common/mod.rs 不改,照常 `mod common;` 复用)。
//! - 临时项目:target/g4-fixtures/<名字>-<pid>-<序号>(宿主会往项目里写 .forge,不能直接用仓库里的夹具目录);
//! - rurix 参照宿主可指定项目根(common::Rurix 固定 demo);
//! - 网格导入:经 asset-pipeline-mcp.exe(stdio NDJSON,与编辑器导入同一条 assetd::import 管线)构建真 .rxmesh;
//! - 像素统计:区域均值、与 rurix 的最大 / 平均差。
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};

use crate::common::{repo_root, Godot, Rpc};

pub const CONFIGS: [(&str, &str); 4] = [
    ("forward_plus", "d3d12"),
    ("forward_plus", "vulkan"),
    ("mobile", "d3d12"),
    ("gl_compatibility", "opengl3"),
];

pub fn fixtures() -> PathBuf {
    repo_root()
        .join("crates")
        .join("godot-host")
        .join("tests")
        .join("fixtures")
        .join("stage4")
}

pub fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let p = e.path();
        let to = dst.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &to);
        } else {
            std::fs::copy(&p, &to).unwrap();
        }
    }
}

/// 新建临时项目;`from` = 夹具项目目录(None = 只有 forge.toml 与空 Content)。
pub fn temp_project(name: &str, from: Option<&Path>) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = crate::common::build_target()
        .join("g4-fixtures")
        .join(format!(
            "{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
    let _ = std::fs::remove_dir_all(&dir);
    match from {
        Some(src) => copy_dir(src, &dir),
        None => {
            std::fs::create_dir_all(dir.join("Content").join("Scenes")).unwrap();
            std::fs::write(
                dir.join("forge.toml"),
                format!("[project]\nname = \"{name}\"\nengine-version = \"0.1.0\"\nmode = \"3d\"\n\n[dirs]\ncontent = \"Content\"\n"),
            )
            .unwrap();
        }
    }
    dir
}

/// rurix 参照宿主(target/debug/engine-host.exe)在指定项目根上。
pub struct RurixAt {
    pub child: Child,
    pub port: u16,
}

impl RurixAt {
    pub fn start(root: &Path) -> RurixAt {
        let exe = crate::common::build_target()
            .join("debug")
            .join("engine-host.exe");
        assert!(
            exe.exists(),
            "缺 {}:先 cargo build -p engine-host",
            exe.display()
        );
        // A running Windows binary locks its path. Use a per-fixture copy so other
        // review/integration workers can rebuild the shared target concurrently.
        let bin = root.join(".test-bin");
        std::fs::create_dir_all(&bin).unwrap();
        let isolated = bin.join("engine-host.exe");
        std::fs::copy(&exe, &isolated).unwrap();
        let mut child = Command::new(isolated)
            .args(["--port", "0"])
            .env("FORGE_PROJECT_ROOT", root)
            .env_remove("FORGE_GPU_PARTICLES")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line
            .trim()
            .strip_prefix("FORGE_HOST_LISTENING port=")
            .expect("rurix 就绪行")
            .parse()
            .unwrap();
        RurixAt { child, port }
    }

    pub fn rpc(&self) -> Rpc {
        Rpc::connect(self.port)
    }
}

impl Drop for RurixAt {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn godot_at(method: &str, driver: &str, root: &Path, env: &[(&str, &str)]) -> Godot {
    let r = root.to_string_lossy().to_string();
    let mut e: Vec<(&str, &str)> = vec![("FORGE_PROJECT_ROOT", r.as_str())];
    e.extend_from_slice(env);
    Godot::start(method, driver, &[], &e)
}

/// 经 asset-pipeline-mcp.exe 导入源文件(glTF 等)到 `<project>/Content/<dest>`,返回 guid(同时建好 .rxmesh)。
pub fn import_asset(project: &Path, source: &Path, dest: &str) -> String {
    let exe = crate::common::build_target()
        .join("debug")
        .join("asset-pipeline-mcp.exe");
    assert!(
        exe.exists(),
        "缺 {}:先 cargo build -p asset-pipeline-mcp",
        exe.display()
    );
    let mut child = Command::new(exe)
        .arg("--project")
        .arg(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut call = |id: u64, method: &str, params: Value| -> Value {
        let line =
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut resp = String::new();
        assert!(
            out.read_line(&mut resp).unwrap() > 0,
            "asset-pipeline-mcp 关闭了 stdout"
        );
        serde_json::from_str(&resp).unwrap()
    };
    call(
        1,
        "initialize",
        json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "g4", "version": "0" } }),
    );
    let r = call(
        2,
        "tools/call",
        json!({ "name": "asset_import", "arguments": { "sourcePaths": [source.to_string_lossy()], "destFolder": dest } }),
    );
    let _ = child.kill();
    let _ = child.wait();
    let text = r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("asset_import 返回:{r}"));
    let v: Value =
        serde_json::from_str(text).unwrap_or_else(|_| panic!("asset_import 文本不是 JSON:{text}"));
    v["imported"][0]["guid"]
        .as_str()
        .unwrap_or_else(|| panic!("asset_import 无 guid:{v}"))
        .to_string()
}

/// (x, y) 为中心、边长 2r+1 的方框里的平均 RGB。
pub fn mean_box(px: &[u8], w: u32, x: u32, y: u32, r: u32) -> [f64; 3] {
    let mut s = [0f64; 3];
    let mut n = 0f64;
    for yy in y.saturating_sub(r)..=y + r {
        for xx in x.saturating_sub(r)..=x + r {
            let i = ((yy * w + xx) * 4) as usize;
            for c in 0..3 {
                s[c] += px[i + c] as f64;
            }
            n += 1.0;
        }
    }
    s.map(|v| v / n)
}

pub fn at(px: &[u8], w: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * w + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2]]
}

/// (最大通道差, 全像素 RGB 平均绝对差, 差 > 2 的像素比例)。
pub fn stats(a: &[u8], b: &[u8]) -> (u8, f64, f64) {
    let (mut max, mut sum, mut gt2, mut n) = (0u8, 0f64, 0usize, 0usize);
    for (p, q) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let d: Vec<u8> = (0..3).map(|i| p[i].abs_diff(q[i])).collect();
        let m = *d.iter().max().unwrap();
        max = max.max(m);
        sum += d.iter().map(|x| *x as f64).sum::<f64>() / 3.0;
        gt2 += usize::from(m > 2);
        n += 1;
    }
    (max, sum / n as f64, gt2 as f64 / n as f64)
}

// ───────────── 模型夹具(ModelBundle JSON,camelCase;字段见 crates/assetd/src/model.rs)─────────────

/// XY 平面、中心在原点、边长 `size` 的 quad(法线 +Z,uv 左下 (0,1),indices 逆时针)。
pub fn quad_prim(id: &str, material: usize, size: f32) -> Value {
    let h = size / 2.0;
    json!({
        "id": id,
        "positions": [[-h, -h, 0.0], [h, -h, 0.0], [h, h, 0.0], [-h, h, 0.0]],
        "normals": [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
        "tangents": [[1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]],
        "uv0": [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        "indices": [0, 1, 2, 0, 2, 3],
        "joints": [], "weights": [], "material": material
    })
}

pub fn material(guid: &str, base: [f32; 4]) -> Value {
    json!({
        "guid": guid, "name": guid, "baseColor": base, "metallic": 0.0, "roughness": 1.0, "emissive": [0.0, 0.0, 0.0],
        "baseColorTexture": null, "normalTexture": null, "metallicRoughnessTexture": null, "occlusionTexture": null,
        "emissiveTexture": null, "normalScale": 1.0, "occlusionStrength": 1.0, "doubleSided": false,
        "alphaMode": "OPAQUE", "alphaCutoff": 0.5, "unlit": false
    })
}

pub fn texture(id: &str, w: u32, h: u32, rgba: Vec<u8>, nearest: bool) -> Value {
    let f = if nearest { 9728 } else { 9729 };
    json!({ "id": id, "guid": id, "assetPath": "", "width": w, "height": h, "rgba": rgba,
            "wrapS": 33071, "wrapT": 33071, "magFilter": f, "minFilter": f })
}

pub fn node(id: &str, prims: &[usize], t: [f32; 3], rot: [f32; 4]) -> Value {
    json!({ "id": id, "name": id, "children": [], "primitives": prims, "translation": t, "rotation": rot,
            "scale": [1.0, 1.0, 1.0], "matrix": null, "skin": null, "collision": false })
}

pub fn model(
    guid: &str,
    nodes: Vec<Value>,
    prims: Vec<Value>,
    mats: Vec<Value>,
    texs: Vec<Value>,
) -> Value {
    let roots: Vec<usize> = (0..nodes.len()).collect();
    json!({ "version": 1, "guid": guid, "revision": 1, "name": guid, "sourceId": guid, "sourceHash": "g4", "kind": "prop",
            "roots": roots, "primitives": prims, "nodes": nodes, "materials": mats, "textures": texs,
            "skins": [], "animations": [], "idleClip": "", "walkClip": "" })
}

/// 写进 `<project>/Content/Models/<name>.rxmodel`,返回场景里用的引用(Content 相对路径)。
pub fn write_model(project: &Path, name: &str, v: &Value) -> String {
    let dir = project.join("Content").join("Models");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{name}.rxmodel")),
        serde_json::to_vec(v).unwrap(),
    )
    .unwrap();
    format!("Models/{name}.rxmodel")
}

/// 纯色 w×h RGBA8;`f(x, y)` 给每个 texel。
pub fn rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| f(x, y))
        .collect()
}

/// 在两边(rurix 与 godot)用同一组 RPC 布场:scene.new + 若干实体 + 相机。
pub fn build(r: &mut Rpc, entities: &[Value], cam: &Value) {
    r.call("scene.new", json!({ "name": "g4" }));
    for e in entities {
        r.call("entity.create", e.clone());
    }
    r.call("viewport.setCamera", cam.clone());
}

pub fn model_entity(name: &str, model_ref: &str, t: [f32; 3], extra: Vec<Value>) -> Value {
    let mut comps =
        vec![json!({ "type": "ModelRenderer", "enabled": true, "props": { "model": model_ref } })];
    comps.extend(extra);
    json!({ "name": name, "translation": t, "components": comps })
}

/// XY 平面、中心在原点、边长 1 的 quad glTF(POSITION + u16 索引,逆时针朝 +Z),内嵌 base64 buffer。
pub fn quad_gltf() -> String {
    let mut buf = Vec::new();
    for p in [
        [-0.5f32, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ] {
        for f in p {
            buf.extend_from_slice(&f.to_le_bytes());
        }
    }
    for i in [0u16, 1, 2, 0, 2, 3] {
        buf.extend_from_slice(&i.to_le_bytes());
    }
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &buf);
    json!({
        "asset": { "version": "2.0" }, "scene": 0, "scenes": [{ "nodes": [0] }], "nodes": [{ "mesh": 0 }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1, "mode": 4 }] }],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "max": [0.5, 0.5, 0.0], "min": [-0.5, -0.5, 0.0] },
            { "bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR" }
        ],
        "bufferViews": [{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }, { "buffer": 0, "byteOffset": 48, "byteLength": 12 }],
        "buffers": [{ "byteLength": 60, "uri": format!("data:application/octet-stream;base64,{b64}") }]
    })
    .to_string()
}
