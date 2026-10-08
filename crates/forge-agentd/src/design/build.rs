//! D-045:元素清单 → 2D 场景(`design_build`),确定性编译。
//!
//! 像素对齐的约定:截帧尺寸 = 定稿 × scale(≤1920×1080),正交相机 orthoSize = 截帧高/2/ppu,
//! 元素素材已按 scale 缩放入库,Sprite scale=1 即贴图原生像素——于是场景里 1 世界单位 = ppu 像素,
//! 截帧与定稿逐像素对应。全部元素放 z=0 同一平面,叠放只靠 sortingOrder(= 清单 z)。
//! 场景是新文件 `Content/Scenes/Design/<slug>.rxscene`;切场景前把当前活动场景另存进流程目录。

use std::sync::Arc;

use serde_json::{json, Value};

use super::layout::{self, Layout};
use super::{DesignRuntime, DesignState};
use crate::llm::ToolFeedback;
use crate::ultraplan::{read_json, write_json_atomic};
use crate::AppState;

/// 每世界单位像素数(与 Sprite / Text 缺省一致)。
pub const PPU: f64 = 100.0;
pub const CAMERA_NAME: &str = "DesignCamera";

pub fn scene_rel_path(slug: &str) -> String {
    format!("Content/Scenes/Design/{slug}.rxscene")
}

/// 生成 entity_batch_apply 的 ops(纯函数,单测覆盖)。`assets` = assets.json 的 elements 映射。
pub fn scene_ops(layout: &Layout, assets: &serde_json::Map<String, Value>, scale: f64) -> Result<Vec<Value>, String> {
    let cw = (f64::from(layout.canvas.width) * scale).round() as u32;
    let ch = (f64::from(layout.canvas.height) * scale).round() as u32;
    let mut ops = vec![json!({
        "op": "create",
        "name": CAMERA_NAME,
        "translation": [0.0, 0.0, 10.0],
        "components": [{ "type": "Camera", "props": {
            "projection": "orthographic",
            "orthoSize": f64::from(ch) / 2.0 / PPU,
            "near": 0.1, "far": 100.0
        }}]
    })];
    let mut elements: Vec<&layout::Element> = layout.elements.iter().collect();
    // 清单序不一定是叠放序;按 z 稳定排序后创建,同 z 保持清单序。
    elements.sort_by_key(|e| e.z);
    for e in elements {
        let sb = layout::scale_bbox(e.bbox, scale);
        let [x, y] = layout::pixel_to_world(sb, (cw, ch), PPU);
        let mut components = Vec::new();
        if e.source == "text" {
            let t = e.text.as_ref().ok_or_else(|| format!("{}: 缺 text", e.id))?;
            let font = e.font(layout).ok_or_else(|| format!("{}: 缺字体", e.id))?;
            let mut props = json!({
                "text": t.content,
                "font": font,
                "size": (t.size * scale).max(1.0),
                "color": t.color,
                "align": t.align,
                "verticalAlign": t.vertical_align,
                "boxSize": [f64::from(sb[2]), f64::from(sb[3])],
                "pixelsPerUnit": PPU,
                "sortingOrder": e.z,
            });
            if let Some(c) = t.outline_color {
                props["outlineColor"] = json!(c);
            }
            if let Some(w) = t.outline_width {
                props["outlineWidth"] = json!(w * scale);
            }
            if let Some(c) = t.shadow_color {
                props["shadowColor"] = json!(c);
            }
            if let Some(o) = t.shadow_offset {
                props["shadowOffset"] = json!([o[0] * scale, o[1] * scale]);
            }
            if let Some(v) = t.letter_spacing {
                props["letterSpacing"] = json!(v * scale);
            }
            if let Some(v) = t.line_height {
                props["lineHeight"] = json!(v);
            }
            components.push(json!({ "type": "Text", "props": props }));
        } else {
            let guid = assets
                .get(&e.id)
                .and_then(|a| a["guid"].as_str())
                .ok_or_else(|| format!("{}: 没有素材(先 design_assets)", e.id))?;
            components.push(json!({ "type": "Sprite", "props": {
                "texture": guid,
                "pixelsPerUnit": PPU,
                "sortingOrder": e.z,
                "chromaKey": "none",
                "blendMode": if e.kind == "background" { "opaque" } else { "alpha" },
            }}));
        }
        components.push(json!({ "type": "Category", "props": { "category": e.category() } }));
        ops.push(json!({
            "op": "create",
            "name": e.id,
            "translation": [x, y, 0.0],
            "components": components,
        }));
    }
    Ok(ops)
}

async fn call(project_root: &std::path::Path, tool: &str, args: Value) -> Result<Value, String> {
    let r = crate::mcp::call_tool_in(project_root, tool, Some(args)).await.map_err(|e| e.to_string())?;
    crate::playtest::unwrap_envelope(&r)
}

pub async fn tool(state: &Arc<AppState>, sid: &str, rid: &str, rt: &DesignRuntime, flow: &DesignState) -> (bool, ToolFeedback) {
    let fail = |t: String| (false, ToolFeedback { text: t, images: Vec::new() });
    let Some(layout) = read_json(&rt.dir_abs.join("layout.json")).and_then(|v| serde_json::from_value::<Layout>(v).ok()) else {
        return fail("还没有元素清单(先 design_layout)".into());
    };
    let Some(assets) = read_json(&rt.dir_abs.join("assets.json")) else {
        return fail("还没有素材(先 design_assets)".into());
    };
    if assets["missing"].as_array().is_some_and(|m| !m.is_empty()) {
        return fail(format!("仍有元素缺素材: {}(先补齐)", assets["missing"]));
    }
    let scale = assets["scale"].as_f64().unwrap_or(1.0);
    let map = assets["elements"].as_object().cloned().unwrap_or_default();
    let ops = match scene_ops(&layout, &map, scale) {
        Ok(o) => o,
        Err(e) => return fail(e),
    };
    let root = rt.project_root.clone();
    // 引擎在 play 态时 scene.new 会被拒;先查,给出可操作的原因。
    if let Ok(s) = call(&root, "mcp__engine-scene__play_state", json!({})).await {
        let ps = s["state"].as_str().or(s["playState"].as_str()).unwrap_or("edit");
        if ps != "edit" {
            return fail(format!("DESIGN_ENGINE_BUSY: 引擎处于 {ps} 态(可能有人在试玩),未切换场景。请用户退出 play 后点「继续复刻」"));
        }
    }
    let backup = rt.dir_abs.join(format!("previous-scene-r{}.rxscene", rt.replication_round));
    let backup_note = match call(&root, "mcp__engine-scene__scene_save", json!({ "path": backup.to_string_lossy() })).await {
        Ok(_) => format!("原活动场景已另存 {}", rt.rel(&backup)),
        Err(e) => format!("原活动场景另存失败({e}),未保存的编辑可能丢失"),
    };
    if let Err(e) = call(&root, "mcp__engine-scene__scene_new", json!({ "name": rt.slug, "mode": "2d", "gravity": [0.0, 0.0, 0.0] })).await {
        return fail(format!("scene_new 失败: {e}"));
    }
    let n = ops.len();
    if let Err(e) = call(&root, "mcp__engine-scene__entity_batch_apply", json!({ "ops": ops })).await {
        return fail(format!("entity_batch_apply 失败: {e}"));
    }
    let rel = scene_rel_path(&rt.slug);
    let abs = root.join(&rel);
    if let Some(p) = abs.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Err(e) = call(&root, "mcp__engine-scene__scene_save", json!({ "path": abs.to_string_lossy() })).await {
        return fail(format!("scene_save 失败: {e}"));
    }
    let _ = call(&root, "mcp__engine-scene__asset_reload", json!({})).await;
    let doc = json!({
        "scenePath": rel, "entities": n, "scale": scale, "ppu": PPU,
        "captureWidth": assets["captureWidth"], "captureHeight": assets["captureHeight"],
        "round": rt.replication_round, "runId": rid, "backup": rt.rel(&backup),
    });
    let _ = write_json_atomic(&rt.dir_abs.join("build.json"), &doc);
    let fid = rt.flow_id.clone();
    let _ = state.sessions.update_design(sid, |slot| match slot.as_mut() {
        Some(d) if d.id == fid => {
            d.scene_path = Some(rel.clone());
            true
        }
        _ => false,
    });
    let _ = flow;
    super::emit(state, sid, "design.scene.built", json!({"runId": rid, "id": rt.flow_id, "scenePath": rel, "entities": n}));
    (
        true,
        ToolFeedback {
            text: format!(
                "已建场景 {rel}({n} 个实体:相机 + {} 个元素;截帧 {}x{});{backup_note}。下一步 design_verify。",
                n - 1,
                assets["captureWidth"],
                assets["captureHeight"]
            ),
            images: Vec::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        serde_json::from_value(json!({
            "canvas": {"width": 400, "height": 200},
            "font": "f1",
            "elements": [
                {"id": "lbl", "kind": "text", "bbox": [100, 80, 200, 40], "z": 20, "source": "text",
                 "text": {"content": "开始", "size": 24, "color": [1, 1, 1, 1], "shadowOffset": [2, 2]}},
                {"id": "bg", "kind": "background", "bbox": [0, 0, 400, 200], "z": 0, "source": "cleanplate"},
                {"id": "btn", "kind": "button", "bbox": [100, 80, 200, 40], "z": 10, "source": "crop"}
            ]
        }))
        .unwrap()
    }

    fn assets() -> serde_json::Map<String, Value> {
        json!({"bg": {"guid": "g-bg"}, "btn": {"guid": "g-btn"}}).as_object().unwrap().clone()
    }

    #[test]
    fn ops_are_ordered_aligned_and_complete() {
        let ops = scene_ops(&layout(), &assets(), 1.0).unwrap();
        assert_eq!(ops.len(), 4);
        assert_eq!(ops[0]["name"], CAMERA_NAME);
        assert_eq!(ops[0]["components"][0]["props"]["orthoSize"], json!(1.0));
        let names: Vec<&str> = ops.iter().map(|o| o["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec![CAMERA_NAME, "bg", "btn", "lbl"]);
        assert_eq!(ops[1]["translation"], json!([0.0, 0.0, 0.0]));
        assert_eq!(ops[2]["translation"], json!([0.0, 0.0, 0.0]));
        assert_eq!(ops[2]["components"][0]["props"]["texture"], "g-btn");
        assert_eq!(ops[2]["components"][1]["props"]["category"], "interaction");
        let text = &ops[3]["components"][0];
        assert_eq!(text["type"], "Text");
        assert_eq!(text["props"]["boxSize"], json!([200.0, 40.0]));
        assert_eq!(text["props"]["font"], "f1");
    }

    /// 引擎在 entity_batch_apply 里按注册表校验每个组件(非法 props 整批回滚),
    /// 所以编译产物必须逐个过 forge_scene::validate_props——含带描边阴影的 Text 与全部 Sprite / Category 字段。
    #[test]
    fn every_compiled_component_passes_the_scene_registry() {
        let mut l = layout();
        let t = l.elements[0].text.as_mut().unwrap();
        t.outline_color = Some([0.0, 0.0, 0.0, 1.0]);
        t.outline_width = Some(2.0);
        t.shadow_color = Some([0.0, 0.0, 0.0, 0.5]);
        t.shadow_offset = Some([2.0, 2.0]);
        t.letter_spacing = Some(1.0);
        t.line_height = Some(1.3);
        for scale in [1.0, 0.5] {
            let ops = scene_ops(&l, &assets(), scale).unwrap();
            let mut seen = std::collections::BTreeSet::new();
            for op in &ops {
                assert_eq!(op["op"], "create");
                assert!(op["translation"].as_array().is_some_and(|t| t.len() == 3 && t.iter().all(|v| v.is_number())));
                for c in op["components"].as_array().unwrap() {
                    let ty = c["type"].as_str().unwrap();
                    forge_scene::validate_props(ty, &c["props"])
                        .unwrap_or_else(|e| panic!("{} 的 {ty} 被注册表拒绝: {e}", op["name"]));
                    forge_scene::normalize_props(ty, &c["props"]).unwrap();
                    seen.insert(ty.to_string());
                }
            }
            assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec!["Camera", "Category", "Sprite", "Text"]);
        }
    }

    #[test]
    fn scale_shrinks_sizes_and_missing_asset_errors() {
        let ops = scene_ops(&layout(), &assets(), 0.5).unwrap();
        assert_eq!(ops[0]["components"][0]["props"]["orthoSize"], json!(0.5));
        assert_eq!(ops[3]["components"][0]["props"]["size"], json!(12.0));
        assert_eq!(ops[3]["components"][0]["props"]["shadowOffset"], json!([1.0, 1.0]));
        let mut a = assets();
        a.remove("btn");
        assert!(scene_ops(&layout(), &a, 1.0).unwrap_err().contains("btn"));
    }
}
