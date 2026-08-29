//! engine-host 资产→视口断链接线设备测试(2026-08-28 对账波):
//! 临时项目内落 .rxmesh 构建产物(assetd 同款 cache_key)→ FORGE_PROJECT_ROOT 指入 →
//! scene.new + entity.create(mesh=<GUID>) → viewport.frame 断言真实网格出帧。
//!
//! 三态诚实:无 vulkan 设备 → viewport.frame 返回 DEV_ENV_DEGRADE 错误,本测试打印
//! SKIP 并以通过退出(非 fake pass)。host 解析腿见 src/meshres.rs 内联单测(恒跑)。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};

struct HostProc {
    child: Child,
    port: u16,
}

impl Drop for HostProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_host(project_root: &std::path::Path) -> HostProc {
    let exe = env!("CARGO_BIN_EXE_engine-host");
    let mut child = Command::new(exe)
        .args(["--port", "0"])
        .env("FORGE_PROJECT_ROOT", project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn engine-host 失败");
    let stdout = child.stdout.take().expect("无 stdout 管道");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("读就绪行失败");
    let port: u16 = line
        .trim()
        .strip_prefix("FORGE_HOST_LISTENING port=")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("就绪行格式非法:{line:?}"));
    HostProc { child, port }
}

struct Client {
    stream: TcpStream,
    next_id: u64,
}

impl Client {
    fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("连接失败");
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        Client { stream, next_id: 1 }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        match self.call_raw(method, params) {
            (Ok(v), _) => v,
            (Err(e), _) => panic!("{method} 失败:{e}"),
        }
    }

    fn call_raw(&mut self, method: &str, params: Value) -> (Result<Value, String>, Value) {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let payload = serde_json::to_vec(&req).unwrap();
        let len = u32::try_from(payload.len()).unwrap();
        self.stream.write_all(&len.to_le_bytes()).unwrap();
        self.stream.write_all(&payload).unwrap();
        self.stream.flush().unwrap();
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).unwrap();
        let n = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; n];
        self.stream.read_exact(&mut buf).unwrap();
        let resp: Value = serde_json::from_slice(&buf).unwrap();
        if let Some(e) = resp.get("error") {
            (Err(e.get("message").and_then(Value::as_str).unwrap_or("?").to_string()), resp.clone())
        } else {
            (Ok(resp.get("result").cloned().unwrap_or(Value::Null)), resp)
        }
    }
}

const W: u32 = 128;
const H: u32 = 96;

/// 四面体:4 顶点 4 三角形(外法线由绕序保证)。
fn tetra() -> (Vec<[f32; 3]>, Vec<u32>) {
    (
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
    )
}

/// 临时项目:Content/Meshes/tetra.gltf(+.meta)+ 构建产物 .rxmesh(assetd 同款 cache_key)。
fn setup_project() -> PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "f2_mesh_viewport_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let content = tmp.join("Content").join("Meshes");
    std::fs::create_dir_all(&content).unwrap();
    std::fs::create_dir_all(tmp.join(".forge").join("cache").join("rxmesh")).unwrap();
    std::fs::write(content.join("tetra.gltf"), b"fake-gltf-bytes").unwrap();
    std::fs::write(
        content.join("tetra.gltf.meta"),
        "guid: 91111111-2222-4333-8444-555555555555\ntype: mesh\nimporter: gltf\n",
    )
    .unwrap();
    let meta =
        assetd::meta::MetaDoc::load(&content.join("tetra.gltf.meta")).expect("meta 可读");
    let key = meta.cache_key(b"fake-gltf-bytes");
    let (pos, idx) = tetra();
    let dag = rurix_geom_build::build_dag(&rurix_geom_build::TriMesh::new(pos, idx));
    std::fs::write(
        tmp.join(".forge")
            .join("cache")
            .join("rxmesh")
            .join(format!("{key}.rxmesh")),
        rurix_geom_build::write_dag(&dag),
    )
    .unwrap();
    tmp
}

#[test]
fn f2_mesh_viewport_device_leg() {
    let tmp = setup_project();
    let host = spawn_host(&tmp);
    let mut c = Client::connect(host.port);

    // 场景 A:四面体网格实体(GUID 引用)。
    c.call("scene.new", json!({ "name": "mesh" }));
    let created = c.call(
        "entity.create",
        json!({
            "name": "tetra",
            "translation": [0.0, 0.5, 0.0],
            "components": [{ "type": "MeshRenderer", "props": { "mesh": "91111111-2222-4333-8444-555555555555", "material": "m" } }]
        }),
    );
    let tetra_id = created.get("id").and_then(Value::as_u64).expect("create 应回 id");

    // 三态门:首帧探设备。
    let (a, _) = c.call_raw("viewport.frame", json!({ "width": W, "height": H }));
    let a = match a {
        Ok(v) => v,
        Err(e) if e.contains("DEV_ENV_DEGRADE") => {
            eprintln!("[f2_mesh_viewport] SKIP DEV_ENV_DEGRADE: {e}");
            std::fs::remove_dir_all(&tmp).ok();
            return;
        }
        Err(e) => panic!("viewport.frame 非降级错误:{e}"),
    };
    assert_eq!(a["draws"], 1, "应 1 绘");
    assert_eq!(a["triangles"], 4, "四面体应 4 三角(cube 恒 12)");
    assert_eq!(a["meshFallbacks"], 0, "GUID 解析不应回退");
    assert_eq!(a["meshClasses"], 1, "应有 1 个非 cube 网格类");
    assert!(a["nonZeroPixels"].as_u64().unwrap_or(0) > 0, "应有非空像素");
    assert!(a["deviceName"].as_str().unwrap_or("").len() > 0, "应有设备名");
    let _ = tetra_id;

    // 场景 B:同位 cube 实体——同视角两帧应逐字节不同(四面体 ≠ 立方体轮廓)。
    c.call("scene.new", json!({ "name": "cube" }));
    c.call(
        "entity.create",
        json!({
            "name": "cube",
            "translation": [0.0, 0.5, 0.0],
            "components": [{ "type": "MeshRenderer", "props": { "mesh": "cube", "material": "m" } }]
        }),
    );
    let (b, _) = c.call_raw("viewport.frame", json!({ "width": W, "height": H }));
    let b = b.expect("cube 帧应成功");
    assert_eq!(b["triangles"], 12, "cube 应 12 三角");
    let da = base64::engine::general_purpose::STANDARD
        .decode(a["pixelsB64"].as_str().expect("A 帧 pixelsB64"))
        .unwrap();
    let db = base64::engine::general_purpose::STANDARD
        .decode(b["pixelsB64"].as_str().expect("B 帧 pixelsB64"))
        .unwrap();
    assert_eq!(da.len(), db.len(), "两帧尺寸应一致");
    assert_ne!(da, db, "四面体与立方体同位帧应逐字节不同(真实网格几何出帧)");

    std::fs::remove_dir_all(&tmp).ok();
}
