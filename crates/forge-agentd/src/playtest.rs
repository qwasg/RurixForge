//! F6 wave.1 playtest 工具面(D-F6-A):断言库四类(entity_count / component_field /
//! transform_near / screenshot_ssim)+ 断言矩阵执行器(经 mcp::call_tool 编排:
//! scene_load → camera? → play_enter → play_pause → inputs 注入 → play_step×N →
//! 逐 case 求值 → play_exit → 结构化报告)。SSIM 自实现(8x8 块亮度统计,确定性 f64)。
//! 诚实纪律:断言求值内部错误(工具失败/实体缺失)= case fail 如实标红,不中断矩阵;
//! 矩阵级错误(场景加载失败)= 整体 Err。

use serde::Deserialize;
use serde_json::{json, Value};

/// 工具调用面(owned 参数规避 HRTB;生产 = mcp::call_tool 信封解包,测试 = 桩)。
pub type ToolResult = Result<Value, String>;

/// workspace 根相对路径解析(绝对路径直用;playtest_run 与 golden/assertion 共用)。
pub fn resolve_workspace_path(rel: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(rel);
    if p.is_absolute() {
        return p;
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace 根")
        .join(p)
}

// ---------- 矩阵 schema ----------

#[derive(Debug, Deserialize)]
pub struct Matrix {
    /// 场景路径(scene_load 直接吃:workspace 相对或绝对)。
    pub scene: String,
    /// 可选相机(viewport_set_camera 子集:target/yaw/pitch/dist/fovY)。
    #[serde(default)]
    pub camera: Option<Value>,
    /// 是否进 PIE(默认 true;纯编辑态断言显式 false)。
    #[serde(default = "default_true")]
    pub enter_play: bool,
    /// play_enter 后输入注入序列:[{action, value, settle?}]——
    /// 引擎输入队列每逻辑帧取空(578 行语义),故逐条注入 + 各自 settle 步进(缺省 1 帧)。
    #[serde(default)]
    pub inputs: Vec<Value>,
    /// 输入序列结束后的追加 settle 帧数(默认 0;等 tween/触发收尾用)。
    #[serde(default)]
    pub settle_frames: u32,
    pub cases: Vec<Case>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct Case {
    pub name: String,
    #[serde(rename = "assert")]
    pub assert_: Value,
}

// ---------- 报告 ----------

#[derive(Debug, Clone)]
pub struct CaseResult {
    pub name: String,
    pub kind: String,
    pub pass: bool,
    pub actual: Value,
    pub expected: Value,
    pub detail: String,
}

#[derive(Debug)]
pub struct MatrixReport {
    pub scene: String,
    pub ok: bool,
    pub passed: usize,
    pub failed: usize,
    pub duration_ms: u128,
    pub cases: Vec<CaseResult>,
}

impl MatrixReport {
    pub fn to_json(&self) -> Value {
        json!({
            "scene": self.scene,
            "ok": self.ok,
            "passed": self.passed,
            "failed": self.failed,
            "durationMs": self.duration_ms,
            "cases": self.cases.iter().map(|c| json!({
                "name": c.name,
                "kind": c.kind,
                "pass": c.pass,
                "actual": c.actual,
                "expected": c.expected,
                "detail": c.detail,
            })).collect::<Vec<_>>(),
        })
    }
}

// ---------- MCP 信封解包(与 client unwrapToolResult 同语义)----------

pub fn unwrap_envelope(result: &Value) -> ToolResult {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("工具级错误(无详情)");
        return Err(format!("工具级 isError: {text}"));
    }
    if let Some(t) = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
    {
        return Ok(serde_json::from_str(t).unwrap_or_else(|_| json!(t)));
    }
    if let Some(sc) = result.get("structuredContent") {
        return Ok(sc.clone());
    }
    Ok(result.clone())
}

// ---------- 断言求值 ----------

/// 实体解析:assert.entity 数值 → id 直用;字符串 → entity_list 按名找(找不到/多义 = fail)。
async fn resolve_entity<F, Fut>(a: &Value, call: &mut F) -> Result<u64, String>
where
    F: FnMut(String, Value) -> Fut,
    Fut: std::future::Future<Output = ToolResult>,
{
    let e = a
        .get("entity")
        .ok_or_else(|| "断言缺 entity 字段".to_string())?;
    if let Some(id) = e.as_u64() {
        return Ok(id);
    }
    let name = e
        .as_str()
        .ok_or_else(|| "entity 须为数值 id 或名称字符串".to_string())?;
    let list = call("mcp__engine-scene__entity_list".into(), json!({})).await?;
    let entities = list
        .get("entities")
        .and_then(Value::as_array)
        .ok_or_else(|| "entity_list 响应缺 entities".to_string())?;
    let matches: Vec<u64> = entities
        .iter()
        .filter(|en| en.get("name").and_then(Value::as_str) == Some(name))
        .filter_map(|en| en.get("id").and_then(Value::as_u64))
        .collect();
    match matches.len() {
        0 => Err(format!("实体「{name}」不存在")),
        1 => Ok(matches[0]),
        n => Err(format!("实体「{name}」多义({n} 个同名)")),
    }
}

/// JSON 值按点路径下钻(props.transform.x 等)。
fn dig<'a>(mut v: &'a Value, path: &str) -> Option<&'a Value> {
    for seg in path.split('.') {
        v = v.get(seg)?;
    }
    Some(v)
}

/// 数值/JSON 比较:eq/neq 走 f64(数值)或 JSON 相等;gt/ge/lt/le 仅数值。
fn compare(op: &str, actual: &Value, expected: &Value) -> Result<bool, String> {
    let num_pair = || -> Option<(f64, f64)> { Some((actual.as_f64()?, expected.as_f64()?)) };
    match op {
        "eq" => Ok(match num_pair() {
            Some((a, b)) => (a - b).abs() < f64::EPSILON,
            None => actual == expected,
        }),
        "neq" => Ok(match num_pair() {
            Some((a, b)) => (a - b).abs() >= f64::EPSILON,
            None => actual != expected,
        }),
        "gt" | "ge" | "lt" | "le" => {
            let (a, b) = num_pair().ok_or_else(|| format!("op {op} 须数值比较,实: {actual} vs {expected}"))?;
            Ok(match op {
                "gt" => a > b,
                "ge" => a >= b,
                "lt" => a < b,
                _ => a <= b,
            })
        }
        other => Err(format!("未知 op「{other}」(eq/neq/gt/ge/lt/le)")),
    }
}

/// 求值单条断言 → (pass, actual, expected, detail)。内部错误一律 case fail(不 Err)。
pub async fn eval_assertion<F, Fut>(a: &Value, call: &mut F) -> (bool, Value, Value, String)
where
    F: FnMut(String, Value) -> Fut,
    Fut: std::future::Future<Output = ToolResult>,
{
    match eval_inner(a, call).await {
        Ok(r) => r,
        Err(detail) => (false, Value::Null, a.get("expected").cloned().unwrap_or(Value::Null), detail),
    }
}

async fn eval_inner<F, Fut>(a: &Value, call: &mut F) -> Result<(bool, Value, Value, String), String>
where
    F: FnMut(String, Value) -> Fut,
    Fut: std::future::Future<Output = ToolResult>,
{
    let kind = a
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "断言缺 kind".to_string())?;
    match kind {
        "entity_count" => {
            let expected = a.get("expected").cloned().unwrap_or(Value::Null);
            let sum = call("mcp__engine-scene__scene_summary".into(), json!({})).await?;
            let actual = sum.get("entityCount").cloned().unwrap_or(Value::Null);
            let pass = compare(a.get("op").and_then(Value::as_str).unwrap_or("eq"), &actual, &expected)?;
            Ok((pass, actual, expected, String::new()))
        }
        "component_field" => {
            let id = resolve_entity(a, call).await?;
            let ctype = a
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| "component_field 缺 type".to_string())?;
            let field = a
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| "component_field 缺 field".to_string())?;
            let comp = call(
                "mcp__engine-scene__component_get".into(),
                json!({ "id": id, "type": ctype }),
            )
            .await?;
            let actual = dig(&comp, field)
                .cloned()
                .ok_or_else(|| format!("字段路径「{field}」在 {ctype} 响应中不存在"))?;
            let expected = a.get("expected").cloned().unwrap_or(Value::Null);
            let pass = compare(a.get("op").and_then(Value::as_str).unwrap_or("eq"), &actual, &expected)?;
            Ok((pass, actual, expected, String::new()))
        }
        "transform_near" => {
            let id = resolve_entity(a, call).await?;
            let t = call("mcp__engine-scene__transform_get".into(), json!({ "id": id })).await?;
            let actual = t
                .get("translation")
                .cloned()
                .ok_or_else(|| "transform_get 响应缺 translation".to_string())?;
            let expected = a.get("translation").cloned().ok_or_else(|| "transform_near 缺 translation".to_string())?;
            let tol = a.get("tolerance").and_then(Value::as_f64).unwrap_or(0.1);
            let aa = actual.as_array().ok_or_else(|| "actual translation 非数组".to_string())?;
            let ee = expected.as_array().ok_or_else(|| "expected translation 非数组".to_string())?;
            if aa.len() != 3 || ee.len() != 3 {
                return Err(format!("translation 须三元组,实: {} vs {}", aa.len(), ee.len()));
            }
            let mut max_dev = 0.0f64;
            for i in 0..3 {
                let d = (aa[i].as_f64().unwrap_or(f64::NAN) - ee[i].as_f64().unwrap_or(f64::NAN)).abs();
                max_dev = max_dev.max(d);
            }
            let pass = max_dev <= tol;
            Ok((pass, actual, expected, format!("maxDeviation={max_dev:.6}, tolerance={tol}")))
        }
        "screenshot_ssim" => {
            let golden_rel = a
                .get("golden")
                .and_then(Value::as_str)
                .ok_or_else(|| "screenshot_ssim 缺 golden".to_string())?;
            let threshold = a.get("threshold").and_then(Value::as_f64).unwrap_or(0.98);
            let w = a.get("width").and_then(Value::as_u64).unwrap_or(960) as u32;
            let h = a.get("height").and_then(Value::as_u64).unwrap_or(540) as u32;
            let frame = call(
                "mcp__engine-scene__viewport_frame".into(),
                json!({ "width": w, "height": h }),
            )
            .await?;
            let fw = frame.get("width").and_then(Value::as_u64).unwrap_or(0) as usize;
            let fh = frame.get("height").and_then(Value::as_u64).unwrap_or(0) as usize;
            let b64 = frame
                .get("pixelsB64")
                .and_then(Value::as_str)
                .ok_or_else(|| "viewport_frame 响应缺 pixelsB64".to_string())?;
            use base64::Engine as _;
            let rgba = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| format!("pixelsB64 解码失败: {e}"))?;
            // golden:相对路径以 workspace 根为基。
            let golden_path = resolve_workspace_path(golden_rel);
            let golden_img = image::open(&golden_path)
                .map_err(|e| format!("golden 读取失败 {}: {e}", golden_path.display()))?
                .to_rgba8();
            let (gw, gh) = (golden_img.width() as usize, golden_img.height() as usize);
            if gw != fw || gh != fh {
                return Err(format!("golden 尺寸 {gw}x{gh} ≠ 帧 {fw}x{fh}(v1 不缩放)"));
            }
            let score = ssim_luma(&rgba, golden_img.as_raw(), fw, fh);
            let pass = score >= threshold;
            Ok((pass, json!(score), json!(threshold), format!("ssim={score:.6}, threshold={threshold}")))
        }
        other => Err(format!("未知断言 kind「{other}」(entity_count/component_field/transform_near/screenshot_ssim)")),
    }
}

// ---------- SSIM(8x8 块亮度;C1/C2 标准常数;确定性 f64)----------

/// rgba8 双图 SSIM(luma = 0.299R+0.587G+0.114B;8x8 非重叠块,边缘块裁掉)。
/// 同图 = 1.0;尺寸须一致(调用方保证)。
pub fn ssim_luma(a: &[u8], b: &[u8], w: usize, h: usize) -> f64 {
    assert_eq!(a.len(), w * h * 4, "a 长度不符 rgba8");
    assert_eq!(b.len(), w * h * 4, "b 长度不符 rgba8");
    const C1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
    const C2: f64 = (0.03 * 255.0) * (0.03 * 255.0);
    let luma = |buf: &[u8], x: usize, y: usize| {
        let i = (y * w + x) * 4;
        0.299 * buf[i] as f64 + 0.587 * buf[i + 1] as f64 + 0.114 * buf[i + 2] as f64
    };
    let mut sum = 0.0;
    let mut n = 0usize;
    let mut by = 0;
    while by + 8 <= h {
        let mut bx = 0;
        while bx + 8 <= w {
            let mut ma = 0.0;
            let mut mb = 0.0;
            for y in by..by + 8 {
                for x in bx..bx + 8 {
                    ma += luma(a, x, y);
                    mb += luma(b, x, y);
                }
            }
            ma /= 64.0;
            mb /= 64.0;
            let mut va = 0.0;
            let mut vb = 0.0;
            let mut cov = 0.0;
            for y in by..by + 8 {
                for x in bx..bx + 8 {
                    let da = luma(a, x, y) - ma;
                    let db = luma(b, x, y) - mb;
                    va += da * da;
                    vb += db * db;
                    cov += da * db;
                }
            }
            va /= 64.0;
            vb /= 64.0;
            cov /= 64.0;
            let s = ((2.0 * ma * mb + C1) * (2.0 * cov + C2)) / ((ma * ma + mb * mb + C1) * (va + vb + C2));
            sum += s;
            n += 1;
            bx += 8;
        }
        by += 8;
    }
    if n == 0 {
        return 1.0; // 小于 8x8 的图无块可算:同尺寸同长度假定下不扣分(如实注释)
    }
    sum / n as f64
}

// ---------- 矩阵执行器 ----------

/// 跑整张矩阵:编排 PIE 生命周期;逐 case 求值;play_exit 兜底(best-effort)。
pub async fn run_matrix<F, Fut>(m: &Matrix, call: &mut F) -> Result<MatrixReport, String>
where
    F: FnMut(String, Value) -> Fut,
    Fut: std::future::Future<Output = ToolResult>,
{
    let t0 = std::time::Instant::now();
    call("mcp__engine-scene__scene_load".into(), json!({ "path": m.scene }))
        .await
        .map_err(|e| format!("scene_load 失败 {}: {e}", m.scene))?;
    if let Some(cam) = &m.camera {
        call("mcp__engine-scene__viewport_set_camera".into(), cam.clone())
            .await
            .map_err(|e| format!("viewport_set_camera 失败: {e}"))?;
    }
    let mut results: Vec<CaseResult> = Vec::new();
    if m.enter_play {
        call("mcp__engine-scene__play_enter".into(), json!({}))
            .await
            .map_err(|e| format!("play_enter 失败: {e}"))?;
        // 确定性 settle:暂停后逐条注入 + 各自步进(F4 wave.3 规范序语义;
        // 引擎输入队列每逻辑帧取空,故注入与步进必须交错,不能先全注再统一步进)。
        let _ = call("mcp__engine-scene__play_pause".into(), json!({})).await;
        for (i, input) in m.inputs.iter().enumerate() {
            let settle = input
                .get("settle")
                .and_then(Value::as_u64)
                .unwrap_or(1) as u32;
            if let Err(e) = call("mcp__engine-scene__logic_inject_input".into(), input.clone()).await {
                results.push(CaseResult {
                    name: format!("(inputs[{i}] 注入)"),
                    kind: "inject".into(),
                    pass: false,
                    actual: Value::Null,
                    expected: input.clone(),
                    detail: e,
                });
            }
            for _ in 0..settle {
                let _ = call("mcp__engine-scene__play_step".into(), json!({})).await;
            }
        }
        // 输入序列后的追加 settle(tween/触发收尾)。
        for _ in 0..m.settle_frames {
            let _ = call("mcp__engine-scene__play_step".into(), json!({})).await;
        }
    }
    for c in &m.cases {
        let kind = c
            .assert_
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        let (pass, actual, expected, detail) = eval_assertion(&c.assert_, call).await;
        results.push(CaseResult {
            name: c.name.clone(),
            kind,
            pass,
            actual,
            expected,
            detail,
        });
    }
    if m.enter_play {
        // 兜底退出 PIE(编辑态原样恢复;失败不掩盖已得报告)。
        let _ = call("mcp__engine-scene__play_exit".into(), json!({})).await;
    }
    let passed = results.iter().filter(|r| r.pass).count();
    let failed = results.len() - passed;
    Ok(MatrixReport {
        scene: m.scene.clone(),
        ok: failed == 0,
        passed,
        failed,
        duration_ms: t0.elapsed().as_millis(),
        cases: results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 桩调用面:工具全名 → 响应(未 mock 即 Err,如实)。
    fn stub(map: HashMap<&'static str, Value>) -> impl FnMut(String, Value) -> std::future::Ready<ToolResult> {
        move |tool: String, _args: Value| {
            let r = map.get(tool.as_str()).cloned();
            std::future::ready(r.ok_or_else(|| format!("未 mock 的工具: {tool}")))
        }
    }

    fn entity_list_fixture() -> Value {
        json!({ "entities": [
            { "id": 1, "name": "Player", "transform": { "translation": [0.0, 0.5, 0.0], "rotation": [0,0,0,1], "scale": [1,1,1] }, "components": [] },
            { "id": 2, "name": "Goal", "transform": { "translation": [3.0, 0.5, 3.0], "rotation": [0,0,0,1], "scale": [1,1,1] }, "components": [] }
        ] })
    }

    #[tokio::test]
    async fn entity_count_pass_and_fail() {
        let mut c = stub(HashMap::from([
            ("mcp__engine-scene__scene_summary", json!({ "name": "s", "entityCount": 2, "playState": "play_paused", "render": {} })),
        ]));
        let (pass, actual, ..) = eval_assertion(&json!({ "kind": "entity_count", "expected": 2 }), &mut c).await;
        assert!(pass);
        assert_eq!(actual, 2);
        let (pass2, ..) = eval_assertion(&json!({ "kind": "entity_count", "expected": 5 }), &mut c).await;
        assert!(!pass2);
    }

    #[tokio::test]
    async fn component_field_ops_and_paths() {
        let mut c = stub(HashMap::from([
            ("mcp__engine-scene__entity_list", entity_list_fixture()),
            ("mcp__engine-scene__component_get", json!({ "id": 1, "type": "Script", "props": { "graphRef": "g.rxgraph", "props": { "keys": 1, "speed": 2.5 } } })),
        ]));
        // 嵌套路径 props.props.keys eq 1。
        let (pass, actual, ..) = eval_assertion(
            &json!({ "kind": "component_field", "entity": "Player", "type": "Script", "field": "props.props.keys", "expected": 1 }),
            &mut c,
        ).await;
        assert!(pass, "keys eq 1 应过: actual={actual}");
        // gt 数值比较。
        let (pass2, ..) = eval_assertion(
            &json!({ "kind": "component_field", "entity": "Player", "type": "Script", "field": "props.props.speed", "op": "gt", "expected": 2.0 }),
            &mut c,
        ).await;
        assert!(pass2);
        // 实体不存在 = fail 如实。
        let (pass3, _, _, detail) = eval_assertion(
            &json!({ "kind": "component_field", "entity": "Ghost", "type": "Script", "field": "props", "expected": 1 }),
            &mut c,
        ).await;
        assert!(!pass3);
        assert!(detail.contains("不存在"), "{detail}");
        // 字段路径不存在 = fail 如实。
        let (pass4, _, _, detail4) = eval_assertion(
            &json!({ "kind": "component_field", "entity": "Player", "type": "Script", "field": "props.nope", "expected": 1 }),
            &mut c,
        ).await;
        assert!(!pass4);
        assert!(detail4.contains("不存在"), "{detail4}");
    }

    #[tokio::test]
    async fn transform_near_tolerance() {
        let mut c = stub(HashMap::from([
            ("mcp__engine-scene__entity_list", entity_list_fixture()),
            ("mcp__engine-scene__transform_get", json!({ "id": 1, "translation": [0.05, 0.5, -0.03], "rotation": [0,0,0,1], "scale": [1,1,1] })),
        ]));
        let (pass, _, _, detail) = eval_assertion(
            &json!({ "kind": "transform_near", "entity": "Player", "translation": [0.0, 0.5, 0.0], "tolerance": 0.1 }),
            &mut c,
        ).await;
        assert!(pass, "{detail}");
        let (pass2, _, _, detail2) = eval_assertion(
            &json!({ "kind": "transform_near", "entity": "Player", "translation": [0.0, 0.5, 0.0], "tolerance": 0.01 }),
            &mut c,
        ).await;
        assert!(!pass2, "{detail2}");
        assert!(detail2.contains("maxDeviation"), "{detail2}");
    }

    #[test]
    fn ssim_identical_is_one_and_different_is_low() {
        let (w, h) = (16usize, 16usize);
        let a: Vec<u8> = (0..w * h * 4).map(|i| ((i / 4) % 256) as u8).collect();
        assert_eq!(ssim_luma(&a, &a, w, h), 1.0, "同图 SSIM 须=1.0");
        // 常数图 0 vs 常数图 255:方差/协方差=0,均值差异主导 → 极低分。
        let black = vec![0u8; w * h * 4];
        let white = vec![255u8; w * h * 4];
        let s = ssim_luma(&black, &white, w, h);
        assert!(s < 0.01, "黑 vs 白 SSIM 应极低: {s}");
    }

    #[tokio::test]
    async fn screenshot_ssim_pass_and_fail() {
        use base64::Engine as _;
        let (w, h) = (16usize, 16usize);
        let frame_rgba: Vec<u8> = (0..w * h * 4).map(|i| ((i * 7) % 256) as u8).collect();
        let golden_path = std::env::temp_dir().join(format!("f6-golden-{}.png", std::process::id()));
        image::save_buffer(
            &golden_path,
            &frame_rgba,
            w as u32,
            h as u32,
            image::ColorType::Rgba8,
        )
        .expect("golden 写盘失败");
        let same_frame = json!({ "width": w, "height": h, "pixelsB64": base64::engine::general_purpose::STANDARD.encode(&frame_rgba) });
        let diff_rgba: Vec<u8> = frame_rgba.iter().map(|v| 255u8.wrapping_sub(*v)).collect();
        let diff_frame = json!({ "width": w, "height": h, "pixelsB64": base64::engine::general_purpose::STANDARD.encode(&diff_rgba) });

        let golden = golden_path.to_string_lossy().replace('\\', "/");
        let a_pass = json!({ "kind": "screenshot_ssim", "golden": golden, "threshold": 0.99, "width": w, "height": h });
        let mut c1 = stub(HashMap::from([("mcp__engine-scene__viewport_frame", same_frame)]));
        let (pass, actual, _, detail) = eval_assertion(&a_pass, &mut c1).await;
        assert!(pass, "同图应过: {detail}");
        assert!((actual.as_f64().unwrap() - 1.0).abs() < 1e-9, "ssim 须 1.0: {actual}");

        let mut c2 = stub(HashMap::from([("mcp__engine-scene__viewport_frame", diff_frame)]));
        let (pass2, _, _, detail2) = eval_assertion(&a_pass, &mut c2).await;
        assert!(!pass2, "反色图应 FAIL: {detail2}");
        std::fs::remove_file(&golden_path).ok();
    }

    #[tokio::test]
    async fn run_matrix_lifecycle_order_and_aggregation() {
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let calls2 = calls.clone();
        let map: HashMap<&'static str, Value> = HashMap::from([
            ("mcp__engine-scene__scene_load", json!({ "loaded": true })),
            ("mcp__engine-scene__play_enter", json!({ "state": "play_running" })),
            ("mcp__engine-scene__play_pause", json!({ "state": "play_paused" })),
            ("mcp__engine-scene__logic_inject_input", json!({ "injected": true })),
            ("mcp__engine-scene__play_step", json!({ "state": "play_paused" })),
            ("mcp__engine-scene__play_exit", json!({ "state": "edit" })),
            ("mcp__engine-scene__scene_summary", json!({ "name": "s", "entityCount": 2, "playState": "play_paused", "render": {} })),
        ]);
        let mut caller = move |tool: String, _args: Value| {
            calls2.borrow_mut().push(tool.clone());
            let r = map.get(tool.as_str()).cloned();
            std::future::ready(r.ok_or_else(|| format!("未 mock 的工具: {tool}")))
        };
        let m = Matrix {
            scene: "Content/Scenes/x.rxscene".into(),
            camera: None,
            enter_play: true,
            inputs: vec![json!({ "action": "forward", "value": 1.0, "settle": 2 })],
            settle_frames: 3,
            cases: vec![
                Case { name: "计数过".into(), assert_: json!({ "kind": "entity_count", "expected": 2 }) },
                Case { name: "计数红".into(), assert_: json!({ "kind": "entity_count", "expected": 9 }) },
            ],
        };
        let report = run_matrix(&m, &mut caller).await.expect("矩阵应完成");
        assert!(!report.ok, "含红 case 矩阵 ok=false");
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert_eq!(report.cases[1].name, "计数红");
        // 生命周期序:scene_load → play_enter → play_pause → inject → step×2(输入自带 settle)→ step×3(尾部 settle)→ (case 调用) → play_exit。
        let seq = calls.borrow();
        let find = |pat: &str| seq.iter().position(|t| t == pat).expect(pat);
        assert!(find("mcp__engine-scene__scene_load") < find("mcp__engine-scene__play_enter"));
        assert!(find("mcp__engine-scene__play_enter") < find("mcp__engine-scene__play_pause"));
        assert!(find("mcp__engine-scene__play_pause") < find("mcp__engine-scene__logic_inject_input"));
        assert!(find("mcp__engine-scene__logic_inject_input") < find("mcp__engine-scene__play_step"));
        let steps = seq.iter().filter(|t| *t == "mcp__engine-scene__play_step").count();
        assert_eq!(steps, 5, "输入 settle=2 + 尾部 settle_frames=3 须 step×5,实 {steps}");
        let last_summary = seq.iter().rposition(|t| t == "mcp__engine-scene__scene_summary").unwrap();
        let exit_pos = find("mcp__engine-scene__play_exit");
        assert!(last_summary < exit_pos, "play_exit 须在最后: {seq:?}");
        // 报告 JSON 面。
        let v = report.to_json();
        assert_eq!(v["ok"], false);
        assert_eq!(v["cases"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn unwrap_envelope_paths() {
        // isError → Err 如实。
        let err = unwrap_envelope(&json!({ "isError": true, "content": [{ "type": "text", "text": "{\"error\":\"X\"}" }] }));
        assert!(err.is_err());
        // content text JSON 二次解析。
        let ok = unwrap_envelope(&json!({ "content": [{ "type": "text", "text": "{\"a\":1}" }] })).unwrap();
        assert_eq!(ok["a"], 1);
        // structuredContent 退化。
        let sc = unwrap_envelope(&json!({ "structuredContent": { "b": 2 } })).unwrap();
        assert_eq!(sc["b"], 2);
    }
}
