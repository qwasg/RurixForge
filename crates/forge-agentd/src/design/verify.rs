//! D-045:复刻验收(`design_verify`)——服务端截帧、与定稿逐元素对比、落证据。
//!
//! 截帧固定用 `viewport_frame{camera:"scene", exact:true}`:编辑态也走场景相机、尺寸严格等于
//! 定稿 × scale(IDE 推流在线也不让位)。结论只来自这里算出的指标与场景文件核对,不取模型自报(I-5)。
//! 证据落 `verify/<n>/{frame,mockup,diff}.png + report.json`,截图按 sha256 记账。

use std::sync::Arc;

use base64::Engine as _;
use serde_json::{json, Value};

use super::compare::{self, Img};
use super::layout::{self, Layout};
use super::{data_url, DesignRuntime, DesignState, VerifySummary, MAX_VERIFY_CALLS};
use crate::llm::ToolFeedback;
use crate::ultraplan::{read_json, write_json_atomic};
use crate::AppState;

/// 引擎截帧串行(exact 截帧可能触发会话重建,并发只会互相拖慢)。
static CAPTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 场景文件里按实体名核对:非文字元素有启用的 Sprite;文字元素有 Text 且内容逐字一致。
pub fn check_scene(scene: &Value, layout: &Layout) -> Vec<String> {
    let entities = scene["entities"].as_array().cloned().unwrap_or_default();
    let comp = |name: &str, ty: &str| -> Option<Value> {
        entities.iter().find(|e| e["name"] == name).and_then(|e| {
            e["components"]
                .as_array()
                .and_then(|cs| cs.iter().find(|c| c["type"] == ty && c["enabled"].as_bool() != Some(false)).cloned())
        })
    };
    let mut problems = Vec::new();
    for e in &layout.elements {
        if e.source == "text" {
            let want = e.text.as_ref().map(|t| t.content.as_str()).unwrap_or("");
            match comp(&e.id, "Text") {
                None => problems.push(format!("{}: 场景里没有该文字实体", e.id)),
                Some(c) if c["props"]["text"].as_str() != Some(want) => problems.push(format!(
                    "{}: 文字内容 {:?} ≠ 清单 {:?}",
                    e.id,
                    c["props"]["text"].as_str().unwrap_or(""),
                    want
                )),
                Some(c) if c["props"]["font"].as_str().is_none_or(str::is_empty) => {
                    problems.push(format!("{}: 文字没有字体", e.id))
                }
                _ => {}
            }
        } else if comp(&e.id, "Sprite").is_none() {
            problems.push(format!("{}: 场景里没有该元素实体(或 Sprite 被禁用)", e.id));
        }
    }
    problems
}

async fn capture(project_root: &std::path::Path, w: u32, h: u32) -> Result<(Img, Value), String> {
    let r = crate::mcp::call_tool_in(
        project_root,
        "mcp__engine-scene__viewport_frame",
        Some(json!({ "width": w, "height": h, "camera": "scene", "exact": true })),
    )
    .await
    .map_err(|e| e.to_string())?;
    let v = crate::playtest::unwrap_envelope(&r)?;
    let (fw, fh) = (v["width"].as_u64().unwrap_or(0) as u32, v["height"].as_u64().unwrap_or(0) as u32);
    if (fw, fh) != (w, h) {
        return Err(format!("截帧尺寸 {fw}x{fh} ≠ 请求 {w}x{h}(引擎未按 exact 出帧)"));
    }
    let b64 = v["pixelsB64"].as_str().ok_or("视口未返回 RGBA 像素")?;
    let px = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| format!("像素解码失败: {e}"))?;
    if px.len() != (w * h * 4) as usize {
        return Err("像素长度与尺寸不符".into());
    }
    let meta = json!({"draws": v["draws"], "truncated": v["truncated"], "nonZeroPixels": v["nonZeroPixels"], "deviceName": v["deviceName"]});
    Ok((Img { w, h, px }, meta))
}

pub async fn tool(state: &Arc<AppState>, sid: &str, rid: &str, rt: &DesignRuntime, flow: &DesignState) -> (bool, ToolFeedback) {
    let fail = |t: String| (false, ToolFeedback { text: t, images: Vec::new() });
    if rt.verify_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= MAX_VERIFY_CALLS {
        return fail(format!("DESIGN_VERIFY_LIMIT: 本轮验收已达 {MAX_VERIFY_CALLS} 次,请 design_complete 如实收尾"));
    }
    let Some(layout) = read_json(&rt.dir_abs.join("layout.json")).and_then(|v| serde_json::from_value::<Layout>(v).ok()) else {
        return fail("还没有元素清单".into());
    };
    let Some(scene_rel) = flow.scene_path.clone() else {
        return fail("还没有建场景(先 design_build)".into());
    };
    let scale = layout::capture_scale(layout.canvas);
    let (w, h) = (
        (f64::from(layout.canvas.width) * scale).round() as u32,
        (f64::from(layout.canvas.height) * scale).round() as u32,
    );
    let mockup_full = match std::fs::read(rt.approved_path()).map_err(|e| e.to_string()).and_then(|b| Img::decode(&b)) {
        Ok(i) => i,
        Err(e) => return fail(format!("读取定稿失败: {e}")),
    };
    let mockup = mockup_full.resize(w, h);
    let (frame, meta) = {
        let _g = CAPTURE_LOCK.lock().await;
        match capture(&rt.project_root, w, h).await {
            Ok(v) => v,
            Err(e) => return fail(format!("截帧失败: {e}")),
        }
    };
    // 场景核对以磁盘文件为准(agent 手工修正后须 scene_save 到同一路径)。
    let scene_abs = rt.project_root.join(&scene_rel);
    let scene_problems = match std::fs::read_to_string(&scene_abs).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(scene) => check_scene(&scene, &layout),
        None => vec![format!("场景文件读不出: {scene_rel}")],
    };
    let global_ssim = compare::ssim(&mockup, &frame);
    let global_color = compare::mean_color_diff(&mockup, &frame);
    let regions: Vec<compare::RegionScore> = layout
        .elements
        .iter()
        .filter_map(|e| compare::score_region(&mockup, &frame, &e.id, &e.kind, &e.source, layout::scale_bbox(e.bbox, scale)))
        .collect();
    let failed: Vec<&compare::RegionScore> = regions.iter().filter(|r| !r.passed).collect();
    let global_ok = global_ssim >= compare::GLOBAL_SSIM_MIN && global_color <= compare::GLOBAL_COLOR_DIFF_MAX;
    let truncated = meta["truncated"].as_bool().unwrap_or(false);
    let passed = global_ok && failed.is_empty() && scene_problems.is_empty() && !truncated;
    let n = flow.verify_count + 1;
    let dir = rt.dir_abs.join("verify").join(n.to_string());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(format!("建证据目录失败: {e}"));
    }
    let heat = compare::diff_heatmap(&mockup, &frame);
    let mut shots = serde_json::Map::new();
    for (name, img) in [("frame", &frame), ("mockup", &mockup), ("diff", &heat)] {
        let png = match img.encode_png() {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        let path = dir.join(format!("{name}.png"));
        if let Err(e) = std::fs::write(&path, &png) {
            return fail(format!("写证据失败: {e}"));
        }
        shots.insert(name.into(), json!({"path": rt.rel(&path), "sha256": forge_util::hashutil::sha256_hex(&png)}));
    }
    let report = json!({
        "n": n, "round": rt.replication_round, "runId": rid, "passed": passed,
        "width": w, "height": h, "scale": scale, "scenePath": scene_rel,
        "global": {
            "ssim": compare::round4(global_ssim), "colorDiff": compare::round4(global_color),
            "ssimMin": compare::GLOBAL_SSIM_MIN, "colorDiffMax": compare::GLOBAL_COLOR_DIFF_MAX, "passed": global_ok,
        },
        "elements": regions,
        "sceneProblems": scene_problems,
        "render": meta,
        "screenshots": shots,
        "at": crate::events::now_rfc3339(),
    });
    if let Err(e) = write_json_atomic(&dir.join("report.json"), &report) {
        return fail(format!("写报告失败: {e}"));
    }
    super::record_verify(
        state,
        sid,
        rt,
        VerifySummary { n, passed, global_ssim: compare::round4(global_ssim), failed_elements: failed.len() as u32 },
        Some(scene_rel.clone()),
    );
    super::emit(
        state,
        sid,
        "design.verify.result",
        json!({
            "runId": rid, "id": rt.flow_id, "round": rt.replication_round, "n": n, "passed": passed,
            "global": report["global"], "failed": failed.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            "sceneProblems": report["sceneProblems"], "screenshots": report["screenshots"],
            "reportPath": rt.rel(&dir.join("report.json")),
        }),
    );
    let mut lines = vec![format!(
        "验收 #{n}:{}。全局 SSIM {:.3}(≥{}),色差 {:.1}(≤{});逐元素未通过 {} 个{}。",
        if passed { "通过" } else { "未通过" },
        global_ssim,
        compare::GLOBAL_SSIM_MIN,
        global_color,
        compare::GLOBAL_COLOR_DIFF_MAX,
        failed.len(),
        if truncated { ";⚠ 渲染被截断(超出绘制上限)" } else { "" }
    )];
    for r in &failed {
        lines.push(format!(
            "- {}({}/{}) bbox {:?}:ssim {:.3}(≥{}),色差 {:.1}",
            r.id, r.kind, r.source, r.bbox, r.ssim, r.threshold, r.color_diff
        ));
    }
    for p in &scene_problems {
        lines.push(format!("- 场景:{p}"));
    }
    lines.push(if passed {
        "可以 design_complete 收尾。".into()
    } else {
        "附图依次为:引擎截帧 / 定稿 / 差异热力图(越红差越大)。逐项修正后 scene_save 到同一路径再验收。".into()
    });
    let images = [&frame, &mockup, &heat].iter().filter_map(|i| data_url(i)).collect();
    (true, ToolFeedback { text: lines.join("\n"), images })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_check_finds_missing_and_wrong_text() {
        let layout: Layout = serde_json::from_value(json!({
            "canvas": {"width": 10, "height": 10}, "font": "f",
            "elements": [
                {"id": "bg", "kind": "background", "bbox": [0, 0, 10, 10], "z": 0, "source": "cleanplate"},
                {"id": "t", "kind": "text", "bbox": [1, 1, 5, 5], "z": 1, "source": "text",
                 "text": {"content": "开始", "size": 8, "color": [1, 1, 1, 1]}}
            ]
        }))
        .unwrap();
        let ok = json!({"entities": [
            {"name": "bg", "components": [{"type": "Sprite", "props": {}}]},
            {"name": "t", "components": [{"type": "Text", "props": {"text": "开始", "font": "f"}}]}
        ]});
        assert!(check_scene(&ok, &layout).is_empty());
        let bad = json!({"entities": [
            {"name": "t", "components": [{"type": "Text", "props": {"text": "开 始", "font": "f"}}]}
        ]});
        let p = check_scene(&bad, &layout);
        assert_eq!(p.len(), 2, "{p:?}");
    }
}
