//! F-GAME-5 / D-050:asset-pipeline-mcp `rig_*` 11 工具 stdio 端到端集成测试。

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
        c.call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "rig-stdio-test", "version": "0" }
            }),
        );
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
        let text = resp
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("缺 content text: {resp}"));
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
    let dir = std::env::temp_dir().join(format!(
        "asset_mcp_rig_test_{}_{}_{}",
        tag,
        std::process::id(),
        gend::timeutil::unix_millis()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn stdio_rig_pipeline_and_psd_end_to_end() {
    let root = tmp_project("rig_e2e");
    let mut c = McpChild::spawn(&root);

    // 0. tools/list 应声明全部 11 个 rig_* 工具。
    let list_resp = c.call("tools/list", json!({}));
    let tool_names: Vec<&str> = list_resp["result"]["tools"]
        .as_array()
        .expect("tools 数组")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for expected in [
        "rig_create",
        "rig_get",
        "rig_set",
        "rig_patch",
        "rig_mesh_auto",
        "rig_weights_auto",
        "rig_evaluate",
        "rig_preview",
        "rig_import_psd",
        "rig_parts_from_sheet",
        "rig_validate",
    ] {
        assert!(tool_names.contains(&expected), "tools/list 缺 {expected}");
    }

    // 1. 构造 96x32 品红底图集 PNG,含 3 个彩色色块:body(红) / face(绿圆) / eye(蓝)。
    let mut img = image::RgbaImage::from_pixel(96, 32, image::Rgba([255, 0, 255, 255]));
    for y in 4..28 {
        for x in 4..28 {
            img.put_pixel(x, y, image::Rgba([210, 50, 40, 255]));
        }
    }
    for y in 4..28 {
        for x in 36..60 {
            let (dx, dy) = (x as f32 - 47.5, y as f32 - 15.5);
            if dx * dx + dy * dy <= 11.5 * 11.5 {
                img.put_pixel(x, y, image::Rgba([40, 200, 60, 255]));
            }
        }
    }
    for y in 8..24 {
        for x in 68..84 {
            img.put_pixel(x, y, image::Rgba([40, 80, 220, 255]));
        }
    }
    let sheet_path = root.join("hero_parts.png");
    img.save(&sheet_path).unwrap();

    let imported = c.tool(
        "asset_import",
        json!({
            "sourcePaths": [sheet_path.to_string_lossy()],
            "destFolder": "Textures"
        }),
    );
    let tex_guid = imported["imported"][0]["guid"].as_str().expect("缺贴图 guid").to_string();

    // 2. rig_parts_from_sheet:3 部件组装为 Rigs/Hero.rxrig。
    let created = c.tool(
        "rig_parts_from_sheet",
        json!({
            "texture": tex_guid,
            "name": "Hero",
            "canvasSize": [64.0, 64.0],
            "parts": [
                { "name": "body", "bbox": [4, 4, 24, 24], "position": [20.0, 32.0], "drawOrder": 0 },
                { "name": "face", "bbox": [36, 4, 24, 24], "position": [20.0, 10.0], "drawOrder": 10 },
                { "name": "eye", "bbox": [68, 8, 16, 16], "position": [24.0, 14.0], "drawOrder": 20 }
            ]
        }),
    );
    assert_eq!(created["assetPath"], "Rigs/Hero.rxrig", "{created}");
    assert_eq!(created["stats"]["parts"], 3, "{created}");
    let rig_guid = created["guid"].as_str().expect("缺 rig guid").to_string();
    assert!(!rig_guid.is_empty());

    // 3. rig_mesh_auto:按轮廓自动三角化全部 3 个部件。
    let meshed = c.tool(
        "rig_mesh_auto",
        json!({
            "assetPath": "Rigs/Hero.rxrig",
            "interiorSpacing": 8.0
        }),
    );
    let meshed_parts = meshed["parts"].as_array().expect("缺 parts");
    assert_eq!(meshed_parts.len(), 3, "{meshed}");
    for mp in meshed_parts {
        assert!(mp["vertices"].as_u64().unwrap() >= 4, "{mp}");
        assert!(mp["triangles"].as_u64().unwrap() >= 2, "{mp}");
    }

    // 4. rig_patch:合并 bust 模板骨架 + 将 eye 刚性绑定到 head(先 dryRun 校验再落盘)。
    let patch_ops = json!([
        { "op": "merge_scaffold", "template": "bust" },
        { "op": "set_weights", "part": "eye", "bone": "head" }
    ]);
    let dry = c.tool(
        "rig_patch",
        json!({
            "guid": rig_guid,
            "ops": patch_ops,
            "dryRun": true
        }),
    );
    assert_eq!(dry["applied"], 2, "{dry}");
    assert_eq!(dry["dryRun"], true, "{dry}");
    assert!(dry["stats"]["bones"].as_u64().unwrap() > 0);

    let patched = c.tool(
        "rig_patch",
        json!({
            "guid": rig_guid,
            "ops": patch_ops
        }),
    );
    assert_eq!(patched["applied"], 2, "{patched}");
    assert_eq!(patched["dryRun"], false, "{patched}");
    assert!(patched["destructive"].as_array().unwrap().is_empty());

    // 5. rig_weights_auto:未指定 parts 时跳过已刚性绑定的 eye,为 body/face 自动算蒙皮权重。
    let weights = c.tool(
        "rig_weights_auto",
        json!({
            "assetPath": "Rigs/Hero.rxrig",
            "falloff": 2.0,
            "smooth": true
        }),
    );
    assert_eq!(weights["skippedRigid"], json!(["eye"]), "{weights}");
    let wp = weights["parts"].as_array().expect("缺 weights parts");
    assert_eq!(wp.len(), 2, "{weights}");

    // 6. rig_patch(set_keyform):读取 eye 顶点数,给 ParamMouthOpenY=1.0 设顶点偏移 [+6, -3]。
    let got_default_view = c.tool("rig_get", json!({ "guid": rig_guid }));
    let eye_vcount = got_default_view["doc"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "eye")
        .unwrap()["mesh"]["vertexCount"]
        .as_u64()
        .unwrap() as usize;
    let offsets: Vec<[f32; 2]> = vec![[6.0, -3.0]; eye_vcount];
    let kf_patched = c.tool(
        "rig_patch",
        json!({
            "assetPath": "Rigs/Hero.rxrig",
            "ops": [{
                "op": "set_keyform",
                "target": "part:eye",
                "params": ["ParamMouthOpenY"],
                "keys": [[0.0, 1.0]],
                "at": { "ParamMouthOpenY": 1.0 },
                "form": { "offsets": offsets }
            }]
        }),
    );
    assert_eq!(kf_patched["applied"], 1, "{kf_patched}");

    // 7. rig_evaluate:对比 ParamMouthOpenY=0 与 =1 时 eye 顶点位置偏移 [+6, -3]。
    let eval_rest = c.tool(
        "rig_evaluate",
        json!({
            "guid": rig_guid,
            "parts": ["eye"]
        }),
    );
    let eval_open = c.tool(
        "rig_evaluate",
        json!({
            "guid": rig_guid,
            "params": { "ParamMouthOpenY": 1.0, "NoSuchParam": 0.5 },
            "parts": ["eye"]
        }),
    );
    let p0 = &eval_rest["parts"][0]["positions"][0];
    let p1 = &eval_open["parts"][0]["positions"][0];
    let dx = p1[0].as_f64().unwrap() - p0[0].as_f64().unwrap();
    let dy = p1[1].as_f64().unwrap() - p0[1].as_f64().unwrap();
    assert!((dx - 6.0).abs() < 0.05 && (dy - (-3.0)).abs() < 0.05, "偏移应为 [6,-3],实际 [{dx},{dy}]");
    assert_eq!(eval_open["unknown"], json!(["param:NoSuchParam"]));

    // 8. rig_preview:写出 PNG 到 .forge/tmp/,并用 compareTo 校验剪影 IoU。
    let prev = c.tool(
        "rig_preview",
        json!({
            "assetPath": "Rigs/Hero.rxrig",
            "size": 128
        }),
    );
    let file_ref = prev["fileRef"].as_str().expect("缺 fileRef");
    assert!(file_ref.starts_with(".forge/tmp/rig_preview_"), "{prev}");
    let preview_abs = root.join(file_ref);
    assert!(preview_abs.is_file(), "预览 PNG 应落盘: {}", preview_abs.display());
    assert_eq!((prev["width"].as_u64(), prev["height"].as_u64()), (Some(128), Some(128)));

    let ref_path = root.join("ref_pose.png");
    std::fs::copy(&preview_abs, &ref_path).unwrap();
    let prev_cmp = c.tool(
        "rig_preview",
        json!({
            "guid": rig_guid,
            "size": 128,
            "overlay": "mesh",
            "compareTo": "ref_pose.png"
        }),
    );
    let iou = prev_cmp["silhouetteIoU"].as_f64().expect("缺 silhouetteIoU");
    assert!(iou > 0.99, "自身参考图剪影 IoU 应接近 1.0,实际 {iou}");

    // 9. rig_validate + rig_create + rig_set:合法与非法文档。
    let val_ok = c.tool("rig_validate", json!({ "guid": rig_guid }));
    assert_eq!(val_ok["ok"], true, "{val_ok}");
    let val_bad = c.tool(
        "rig_validate",
        json!({
            "doc": { "version": 99, "texture": tex_guid, "canvas": { "size": [64, 64] } }
        }),
    );
    assert_eq!(val_bad["ok"], false, "{val_bad}");
    assert_eq!(val_bad["error"]["code"], "RIG_INVALID", "{val_bad}");

    let created_bust = c.tool(
        "rig_create",
        json!({
            "name": "BustRig",
            "texture": tex_guid,
            "template": "bust"
        }),
    );
    assert_eq!(created_bust["assetPath"], "Rigs/BustRig.rxrig", "{created_bust}");
    assert!(created_bust["stats"]["bones"].as_u64().unwrap() > 0);
    let bust_full = c.tool("rig_get", json!({ "assetPath": "Rigs/BustRig.rxrig", "includeMesh": true }));
    let set_out = c.tool("rig_set", json!({ "assetPath": "Rigs/BustRig.rxrig", "doc": bust_full["doc"] }));
    assert_eq!(set_out["guid"], created_bust["guid"], "{set_out}");

    // 10. rig_get:缺省省略 mesh/weights/offsets,includeMesh=true 返回全量数组。
    let got_elided = c.tool("rig_get", json!({ "assetPath": "Rigs/Hero.rxrig" }));
    assert!(
        got_elided["doc"]["parts"][0]["mesh"]["vertexCount"].is_number()
            && got_elided["doc"]["parts"][0]["mesh"].get("vertices").is_none(),
        "缺省应省略 vertices: {got_elided}"
    );
    assert!(
        got_elided["doc"]["parts"][0]["weights"]["count"].is_number(),
        "缺省应省略 weights 数组: {got_elided}"
    );
    let got_full = c.tool(
        "rig_get",
        json!({
            "assetPath": "Rigs/Hero.rxrig",
            "includeMesh": true,
            "sections": ["parts", "keyforms"]
        }),
    );
    assert!(
        got_full["doc"]["parts"][0]["mesh"]["vertices"].is_array(),
        "includeMesh=true 应含 vertices 数组: {got_full}"
    );
    assert!(got_full["doc"].get("bones").is_none(), "sections 过滤应生效: {got_full}");

    // 11. rig_import_psd:合成 8-bit RGB 3 图层 PSD 导入为图集 + .rxrig。
    let psd_bytes = assetd::rig::build_test_psd(
        48,
        48,
        &[
            ("body", [8, 20, 40, 46], [200, 40, 40], true),
            ("face", [12, 6, 36, 26], [40, 200, 60], true),
            ("eye_L", [16, 12, 22, 18], [40, 80, 220], true),
        ],
    );
    let psd_path = root.join("hero_layers.psd");
    std::fs::write(&psd_path, &psd_bytes).unwrap();

    let psd_out = c.tool(
        "rig_import_psd",
        json!({
            "psdRef": "hero_layers.psd",
            "name": "PsdHero"
        }),
    );
    assert_eq!(psd_out["assetPath"], "Rigs/PsdHero.rxrig", "{psd_out}");
    assert_eq!(psd_out["atlasPath"], "Textures/PsdHero_atlas.png", "{psd_out}");
    let psd_parts = psd_out["parts"].as_array().expect("缺 psd parts");
    assert_eq!(psd_parts.len(), 3, "{psd_out}");
    assert_eq!(psd_parts[0]["id"], "body");
    assert_eq!(psd_parts[1]["id"], "face");
    assert_eq!(psd_parts[2]["id"], "eye_L");

    // 引用边:rig→texture
    let refs = c.tool(
        "asset_refs",
        json!({
            "assetPath": "Rigs/PsdHero.rxrig",
            "direction": "refs"
        }),
    );
    let edges = refs["edges"].as_array().expect("缺 edges");
    assert!(
        edges
            .iter()
            .any(|e| e["to"] == psd_out["atlasGuid"] && e["type"] == "rig→texture"),
        "应有 rig→texture 引用边: {refs}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
