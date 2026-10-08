//! Stage 5 集成测试的公共部分(g5_*.rs 共用;common / g4util 原样复用,不改它们)。
//! - 夹具:模型腿的 quad 模型(白 / 红 / 镜面金属 / 自发光),PNG 贴图 + .meta(存储型 deflate,不加依赖);
//! - 取帧:同一进程里按 component.set 开关特性,前后各取一帧(时间性特性连取 n 帧取最后一帧);
//! - 判定:区域均值 / 亮度 / 方差,`render.capabilities.coverage.unsupported` 的特性键。
#![allow(dead_code)]

use std::path::Path;

use serde_json::{json, Value};

use crate::common::Rpc;
use crate::g4util::{material, model, node, quad_prim, rgba, write_model};

pub const W: u32 = 320;
pub const H: u32 = 180;

/// 正交相机:半高 1.5 → 60 px / 世界单位(320×180),画面中心 = (160, 90)。
pub fn ortho_cam() -> Value {
    json!({ "target": [0.0, 0.0, 0.0], "yaw": 0.0, "pitch": 0.0, "dist": 10.0, "ortho": true, "orthoSize": 1.5 })
}

pub fn persp_cam(target: [f32; 3], yaw: f32, pitch: f32, dist: f32) -> Value {
    json!({ "target": target, "yaw": yaw, "pitch": pitch, "dist": dist })
}

/// 世界坐标 (x, y) 在 ortho_cam 下的像素。
pub fn px_of(x: f32, y: f32) -> (u32, u32) {
    (
        (160.0 + 60.0 * x).round() as u32,
        (90.0 - 60.0 * y).round() as u32,
    )
}

/// 单个 quad 模型(XY 平面、朝 +Z、边长 1);`mat` = g4util::material 的 JSON(可再改字段)。
pub fn quad_model(project: &Path, name: &str, mat: Value, textures: Vec<Value>) -> String {
    let guid = format!("00000000-0000-4000-8000-{:012x}", fnv(name));
    let m = model(
        &guid,
        vec![node("q", &[0], [0.0; 3], [0.0, 0.0, 0.0, 1.0])],
        vec![quad_prim("q", 0, 1.0)],
        vec![mat],
        textures,
    );
    write_model(project, name, &m)
}

pub fn white(project: &Path) -> String {
    quad_model(project, "white", material("white", [1.0; 4]), vec![])
}

pub fn colored(project: &Path, name: &str, c: [f32; 4]) -> String {
    quad_model(project, name, material(name, c), vec![])
}

/// 镜面金属(metallic 1、roughness r)。
pub fn metal(project: &Path, name: &str, base: [f32; 4], roughness: f32) -> String {
    let mut m = material(name, base);
    m["metallic"] = json!(1.0);
    m["roughness"] = json!(roughness);
    quad_model(project, name, m, vec![])
}

pub fn emissive(project: &Path, name: &str, e: [f32; 3]) -> String {
    let mut m = material(name, [0.0, 0.0, 0.0, 1.0]);
    m["emissive"] = json!(e);
    quad_model(project, name, m, vec![])
}

pub fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    }) & 0xffff_ffff_ffff
}

/// 实体:ModelRenderer(可为 None)+ 其余组件;rotation = 四元数 [x, y, z, w]。
pub fn ent(
    name: &str,
    model_ref: Option<&str>,
    t: [f32; 3],
    rot: [f32; 4],
    scale: [f32; 3],
    extra: Vec<Value>,
) -> Value {
    let mut comps: Vec<Value> = model_ref
        .map(|m| json!({ "type": "ModelRenderer", "enabled": true, "props": { "model": m } }))
        .into_iter()
        .collect();
    comps.extend(extra);
    json!({ "name": name, "translation": t, "rotation": rot, "scale": scale, "components": comps })
}

pub fn comp(ctype: &str, props: Value) -> Value {
    json!({ "type": ctype, "enabled": true, "props": props })
}

/// 绕 X 轴转 a 弧度的四元数。
pub fn rot_x(a: f32) -> [f32; 4] {
    [(a / 2.0).sin(), 0.0, 0.0, (a / 2.0).cos()]
}

pub fn rot_y(a: f32) -> [f32; 4] {
    [0.0, (a / 2.0).sin(), 0.0, (a / 2.0).cos()]
}

pub const ID: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// 关掉缺省灯(有 Light 实体就不生成缺省灯):一盏强度 0 的方向光。
pub fn no_light() -> Value {
    ent(
        "nolight",
        None,
        [0.0; 3],
        ID,
        [1.0; 3],
        vec![comp(
            "Light",
            json!({ "kind": "directional", "color": [1.0, 1.0, 1.0], "intensity": 0.0, "castShadow": false }),
        )],
    )
}

/// 场景:scene.new + 实体 + 相机;返回每个实体的 id(按传入顺序)。
pub fn scene(r: &mut Rpc, name: &str, ents: &[Value], cam: &Value) -> Vec<u64> {
    r.call("scene.new", json!({ "name": name }));
    let ids = ents
        .iter()
        .map(|e| r.call("entity.create", e.clone())["id"].as_u64().unwrap())
        .collect();
    r.call("viewport.setCamera", cam.clone());
    ids
}

/// component.set 全量替换 props(未给的字段回到缺省)。
pub fn set(r: &mut Rpc, id: u64, ctype: &str, props: Value) {
    r.call(
        "component.set",
        json!({ "id": id, "type": ctype, "props": props }),
    );
}

pub fn enable(r: &mut Rpc, id: u64, ctype: &str, on: bool) {
    r.call(
        "component.set",
        json!({ "id": id, "type": ctype, "enabled": on }),
    );
}

/// 连取 n 帧,返回最后一帧(时间性特性:SDFGI、自动曝光、反射探针、体积雾)。
pub fn frame_n(r: &mut Rpc, n: usize) -> Vec<u8> {
    let mut px = Vec::new();
    for _ in 0..n.max(1) {
        px = r.frame(W, H).1;
    }
    px
}

pub fn mean_rect(px: &[u8], x0: u32, y0: u32, x1: u32, y1: u32) -> [f64; 3] {
    let (mut s, mut n) = ([0f64; 3], 0f64);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * W + x) * 4) as usize;
            for c in 0..3 {
                s[c] += px[i + c] as f64;
            }
            n += 1.0;
        }
    }
    s.map(|v| v / n.max(1.0))
}

pub fn lum(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

pub fn mean_all(px: &[u8]) -> f64 {
    lum(mean_rect(px, 0, 0, W, H))
}

/// 区域亮度方差(模糊前后比较)。
pub fn variance(px: &[u8], x0: u32, y0: u32, x1: u32, y1: u32) -> f64 {
    let m = lum(mean_rect(px, x0, y0, x1, y1));
    let (mut s, mut n) = (0f64, 0f64);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * W + x) * 4) as usize;
            let l = lum([px[i] as f64, px[i + 1] as f64, px[i + 2] as f64]);
            s += (l - m) * (l - m);
            n += 1.0;
        }
    }
    s / n.max(1.0)
}

pub fn unsupported(caps: &Value) -> Vec<String> {
    features(caps, "unsupported")
}

fn features(caps: &Value, key: &str) -> Vec<String> {
    caps["coverage"][key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|u| u["feature"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// 期望:该特性在本配置支持 → 画面按方向变化;不支持 → 画面逐字节不变且 capabilities 里标出;
/// 受限(coverage.limited,实现与 F+ 不同)→ 只要求画面有变化,方向不作断言。
pub fn check(
    method: &str,
    caps: &Value,
    feature: &str,
    supported_delta: f64,
    off: &[u8],
    on: &[u8],
    expect: &str,
) {
    let listed = unsupported(caps).iter().any(|f| f == feature);
    let limited = features(caps, "limited").iter().any(|f| f == feature);
    let same = off == on;
    eprintln!("G5 {method} {feature}: delta={supported_delta:.3} listed_unsupported={listed} limited={limited} identical={same} ({expect})");
    if listed {
        assert!(
            same,
            "{method} {feature}:能力表标为不支持,画面却变了(delta {supported_delta:.3})"
        );
    } else if limited {
        assert!(!same, "{method} {feature}:能力表标为受限支持,画面应有变化");
    } else {
        assert!(
            supported_delta > 0.0,
            "{method} {feature}:应该{expect},实测 delta {supported_delta:.3}"
        );
    }
}
fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// 无压缩(stored deflate)的 RGBA8 PNG。
pub fn png(w: u32, h: u32, px: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * 4 + 1) as usize * h as usize);
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&px[(y * w * 4) as usize..((y + 1) * w * 4) as usize]);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = b.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |ty: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

/// 贴图资产:`<project>/Content/Textures/<name>.png` + `.png.meta`(guid 由名字确定),返回 guid。
pub fn write_texture(
    project: &Path,
    name: &str,
    w: u32,
    h: u32,
    f: impl Fn(u32, u32) -> [u8; 4],
) -> String {
    let dir = project.join("Content").join("Textures");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.png"));
    std::fs::write(&path, png(w, h, &rgba(w, h, f))).unwrap();
    let guid = format!("5e5e5e5e-0000-4000-8000-{:012x}", fnv(name));
    std::fs::write(
        dir.join(format!("{name}.png.meta")),
        format!("guid: {guid}\ntype: texture\nimporter: png\n"),
    )
    .unwrap();
    guid
}

/// 一个开关用例:同一个实体上的 `ctype` 组件,off / on 两组 props(None = 把组件禁用)。
pub struct Toggle {
    pub feature: &'static str,
    pub ctype: &'static str,
    pub off: Option<Value>,
    pub on: Option<Value>,
    pub frames: usize,
    pub metric: fn(&[u8], &[u8]) -> f64,
    pub min: f64,
    pub expect: &'static str,
}

fn apply_state(r: &mut Rpc, id: u64, ctype: &str, props: &Option<Value>) {
    match props {
        Some(p) => {
            set(r, id, ctype, p.clone());
            enable(r, id, ctype, true);
        }
        None => enable(r, id, ctype, false),
    }
}

/// 逐个用例:off 取帧 → on 取帧 → check(支持 → 按方向变化;能力表不支持 → 逐字节不变)。返回 (特性, 变化量)。
pub fn toggles(
    r: &mut Rpc,
    method: &str,
    caps: &Value,
    id: u64,
    cases: &[Toggle],
) -> Vec<(&'static str, f64)> {
    let mut out = Vec::new();
    for c in cases {
        apply_state(r, id, c.ctype, &c.off);
        let off = frame_n(r, c.frames);
        apply_state(r, id, c.ctype, &c.on);
        let on = frame_n(r, c.frames);
        let m = (c.metric)(&off, &on);
        check(method, caps, c.feature, m - c.min, &off, &on, c.expect);
        out.push((c.feature, m));
    }
    out
}

/// rurix 参照宿主,可带额外 env(g4util::RurixAt 固定清掉 FORGE_GPU_PARTICLES)。
pub struct RurixEnv {
    pub child: std::process::Child,
    pub port: u16,
}

impl RurixEnv {
    pub fn start(root: &Path, env: &[(&str, &str)]) -> RurixEnv {
        use std::io::BufRead;
        let exe = crate::common::repo_root()
            .join("target")
            .join("debug")
            .join("engine-host.exe");
        let mut cmd = std::process::Command::new(exe);
        cmd.args(["--port", "0"])
            .env("FORGE_PROJECT_ROOT", root)
            .env_remove("FORGE_GPU_PARTICLES")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line
            .trim()
            .strip_prefix("FORGE_HOST_LISTENING port=")
            .expect("rurix 就绪行")
            .parse()
            .unwrap();
        RurixEnv { child, port }
    }

    pub fn rpc(&self) -> Rpc {
        Rpc::connect(self.port)
    }
}

impl Drop for RurixEnv {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 边缘上"半覆盖"像素数(亮度落在 (lo + 8, hi − 8) 之间):抗锯齿 / 缩放模糊后变多。
pub fn partial(px: &[u8], lo: f64, hi: f64) -> f64 {
    px.chunks_exact(4)
        .filter(|p| {
            let l = lum([p[0] as f64, p[1] as f64, p[2] as f64]);
            l > lo + 8.0 && l < hi - 8.0
        })
        .count() as f64
}

pub fn changed(a: &[u8], b: &[u8]) -> f64 {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(p, q)| p[..3] != q[..3])
        .count() as f64
}
