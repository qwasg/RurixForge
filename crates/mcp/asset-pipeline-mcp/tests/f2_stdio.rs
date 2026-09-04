//! asset-pipeline-mcp 集成测试:真实 spawn + stdio NDJSON 往返。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::{json, Value};

struct McpChild {
    child: Child,
    stdin: ChildStdin,
    lines: std::io::Lines<BufReader<std::process::ChildStdout>>,
    next_id: i64,
}

impl McpChild {
    fn spawn(project: &std::path::Path) -> Self {
        let bin = PathBuf::from(env!("CARGO_BIN_EXE_asset-pipeline-mcp"));
        let mut child = Command::new(bin)
            .arg("--project")
            .arg(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn asset-pipeline-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut c = McpChild { child, stdin, lines: BufReader::new(stdout).lines(), next_id: 0 };
        c.call("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }));
        c.notify("notifications/initialized", json!({}));
        c
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{}", serde_json::to_string(&req).unwrap()).unwrap();
        self.stdin.flush().unwrap();
        loop {
            let line = self.lines.next().unwrap().unwrap();
            let msg: Value = serde_json::from_str(&line).unwrap();
            if msg.get("id").and_then(Value::as_i64) == Some(id) {
                return msg;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        let req = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        writeln!(self.stdin, "{}", serde_json::to_string(&req).unwrap()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.call("tools/call", json!({ "name": name, "arguments": args }));
        let text = resp.pointer("/result/content/0/text").and_then(Value::as_str).expect("缺 content text");
        serde_json::from_str(text).expect("工具返回非 JSON")
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tmp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("asset_mcp_test_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_conformance(name: &str, dest_dir: &std::path::Path) -> PathBuf {
    let src = PathBuf::from("H:/rurix/conformance/asset/gltf/accept").join(name);
    let dst = dest_dir.join(name);
    std::fs::copy(&src, &dst).unwrap();
    dst
}

#[test]
fn stdio_import_list_status() {
    let root = tmp_project("stdio");
    let mut c = McpChild::spawn(&root);

    // 导入 tri_min.gltf
    let src = copy_conformance("tri_min.gltf", &root);
    let out = c.tool("asset_import", json!({
        "sourcePaths": [src.to_string_lossy()],
        "destFolder": "Meshes"
    }));
    let imported = out["imported"].as_array().expect("缺 imported");
    assert_eq!(imported.len(), 1);
    let guid = imported[0]["guid"].as_str().expect("缺 guid");
    assert!(!guid.is_empty());
    assert_eq!(imported[0]["cacheHit"], false);

    // asset_list
    let list = c.tool("asset_list", json!({}));
    let assets = list["assets"].as_array().expect("缺 assets");
    assert!(assets.iter().any(|a| a["guid"] == guid), "list 应见新资产");

    // asset_get_meta
    let meta = c.tool("asset_get_meta", json!({ "assetPath": "Meshes/tri_min.gltf" }));
    assert_eq!(meta["meta"]["guid"], guid);
    assert_eq!(meta["meta"]["type"], "mesh");

    // asset_build_status
    let status = c.tool("asset_build_status", json!({ "assetPaths": ["Meshes/tri_min.gltf"] }));
    let items = status["items"].as_array().expect("缺 items");
    assert_eq!(items[0]["state"], "current");

    // 二次导入 → cache hit
    let out2 = c.tool("asset_import", json!({
        "sourcePaths": [src.to_string_lossy()],
        "destFolder": "Meshes"
    }));
    let imported2 = out2["imported"].as_array().unwrap();
    assert_eq!(imported2[0]["cacheHit"], true, "二次导入应命中缓存");

    let _ = std::fs::remove_dir_all(&root);
}

/// F10-RAG:asset_set_description 写后自动增量进语义索引——不重建即可检索,
/// 响应 indexed/tier 如实标注(词法档;embedding 配置经隔离 data 目录屏蔽)。
#[test]
fn stdio_set_description_auto_indexes() {
    let root = tmp_project("descidx");
    std::fs::create_dir_all(root.join("Content/Scripts")).unwrap();
    std::fs::write(
        root.join("Content/Scripts/door.rx"),
        "// 开门逻辑\n#[export(c)]\npub fn door_open_angle() -> f32 { 90.0 }\n",
    )
    .unwrap();
    // 首建索引(词法档;upsert 只负责增量,首建仍归 context_index_build/build_index)。
    let opts = forge_index::extract::ExtractOptions { project_root: root.clone(), docs_root: None };
    forge_index::build::build_index(&opts, None, false).unwrap();

    // 隔离 embedding 配置(子进程继承;防宿主 data/ 已配渠道时测试触网)。
    let isolated = root.join("data-isolated");
    std::env::set_var("FORGE_GEN_DATA_DIR", &isolated);
    let mut c = McpChild::spawn(&root);
    let r = c.tool("asset_set_description", json!({
        "assetPath": "Scripts/door.rx",
        "description": "开门逻辑脚本,输出门的开启角度,适合机关谜题",
        "tags": ["逻辑", "门"],
        "source": "human"
    }));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["indexed"], true, "{r}");
    assert_eq!(r["tier"], "lexical", "{r}");
    drop(c);
    std::env::remove_var("FORGE_GEN_DATA_DIR");

    // 不重建索引,直接检索命中简介文本。
    let paths = forge_index::store::IndexPaths::for_project(&root);
    let out = forge_index::search::search(
        &paths,
        "机关谜题",
        5,
        &forge_index::search::SearchFilter::default(),
        None,
    )
    .unwrap();
    assert!(!out.hits.is_empty(), "写后须即时可检: {out:?}");
    assert!(
        out.hits.iter().any(|h| h.doc.path == "Scripts/door.rx"),
        "命中应含 door.rx: {out:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
/// F-GAME-4:精灵工具面端到端——合成品红底图集 → 导入 → autoslice → sprite_create
/// (autoslice)→ sprite_get → sprite_set(合法/坏文档)。
#[test]
fn stdio_sprite_tools_end_to_end() {
    let root = tmp_project("sprite");
    let mut c = McpChild::spawn(&root);

    // 合成图集:32x16 品红底,两行三帧(10x6 / 8x5 / 12x4)。
    let mut img = image::RgbaImage::from_pixel(32, 16, image::Rgba([255, 0, 255, 255]));
    let mut fill = |x0: u32, y0: u32, w: u32, h: u32| {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                img.put_pixel(x, y, image::Rgba([40, 180, 60, 255]));
            }
        }
    };
    fill(1, 1, 10, 6);
    fill(14, 2, 8, 5);
    fill(2, 9, 12, 4);
    let sheet = root.join("zombie_walk.png");
    img.save(&sheet).unwrap();

    let out = c.tool("asset_import", json!({
        "sourcePaths": [sheet.to_string_lossy()],
        "destFolder": "Textures"
    }));
    let tex_guid = out["imported"][0]["guid"].as_str().expect("缺贴图 guid").to_string();

    // autoslice(只读):3 帧,行序。
    let sl = c.tool("sprite_autoslice", json!({ "assetPath": "Textures/zombie_walk.png" }));
    assert_eq!(sl["width"], 32);
    let boxes = sl["boxes"].as_array().expect("缺 boxes");
    assert_eq!(boxes.len(), 3, "应检出 3 帧: {sl}");
    assert_eq!(boxes[0], json!([1, 1, 10, 6]));
    assert_eq!(boxes[2], json!([2, 9, 12, 4]));

    // sprite_create + autoslice → 3 帧落盘。
    let created = c.tool("sprite_create", json!({
        "name": "ZombieWalk", "texture": tex_guid, "autoslice": true
    }));
    assert_eq!(created["assetPath"], "Sprites/ZombieWalk.rxsprite");
    assert_eq!(created["frameCount"], 3);
    let sprite_guid = created["guid"].as_str().expect("缺 sprite guid").to_string();
    assert!(!sprite_guid.is_empty());

    // sprite_get:规范形态(缺省脚底锚)。
    let got = c.tool("sprite_get", json!({ "assetPath": "Sprites/ZombieWalk.rxsprite" }));
    assert_eq!(got["guid"], sprite_guid.as_str());
    assert_eq!(got["doc"]["pivot"], json!([0.5, 1.0]));
    assert_eq!(got["doc"]["frames"].as_object().unwrap().len(), 3);

    // sprite_set:补 clip(合法)→ ok;坏文档(clip 引用不存在帧)→ 如实拒。
    let mut doc = got["doc"].clone();
    doc["clips"] = json!({ "walk": { "frames": ["frame_0", "frame_1", "frame_2"], "fps": 8, "loop": true } });
    let set = c.tool("sprite_set", json!({ "assetPath": "Sprites/ZombieWalk.rxsprite", "doc": doc }));
    assert_eq!(set["ok"], true);
    assert_eq!(set["clipCount"], 1);
    let mut bad = got["doc"].clone();
    bad["clips"] = json!({ "walk": { "frames": ["nosuch"] } });
    let rejected = c.tool("sprite_set", json!({ "assetPath": "Sprites/ZombieWalk.rxsprite", "doc": bad }));
    assert_eq!(rejected["error"], "SPRITE_INVALID", "坏文档须如实拒: {rejected}");

    // 未知贴图 GUID 创建 → 如实错。
    let bad_create = c.tool("sprite_create", json!({ "name": "Bad", "texture": "no-such-guid" }));
    assert!(bad_create["error"].is_string(), "未知 GUID 须报错: {bad_create}");

    // 引用边:sprite→texture(重建后可查)。
    let refs = c.tool("asset_refs", json!({ "assetPath": "Sprites/ZombieWalk.rxsprite", "direction": "refs" }));
    let edges = refs["edges"].as_array().expect("缺 edges");
    assert!(
        edges.iter().any(|e| e["to"] == tex_guid.as_str() && e["type"] == "sprite→texture"),
        "应有 sprite→texture 边: {refs}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
