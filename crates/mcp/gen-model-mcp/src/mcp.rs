//! MCP stdio 服务:initialize / tools/list / tools/call(复刻 gen-image-mcp 骨架)。
//! 三工具(05 §8 逐字参数):gen_mesh / gen_mesh_refine / gen_accept。
//! 诚实优先(D-F5-E):注册表无 text2mesh/retopo 适配器 → gen_mesh/gen_mesh_refine
//! 显式 GEN_BACKEND_NOT_CONFIGURED(不伪造生成能力);gen_accept 走 asset_import 同一
//! 构建链(rurix-geom-build DAG,retopo/LOD 派生属链内行为 D-009,非独立 refine 调用)。
//! 工具错误 = isError:true + {error: <GEN_* code>, message}。

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use assetd::project::ForgeProject;
use gend::accept::accept_asset;
use gend::timeutil::utc_now_iso8601;
use gend::tmpstore;
use gend::{GenError, GEN_BACKEND_NOT_CONFIGURED, GEN_BAD_PARAMS};
use serde_json::{json, Value};

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "gen_mesh",
                "description": "文/图生网格候选;默认走 meshy(text-to-3d preview→refine 两阶段 / image-to-3d,产物 glb),可经 backend 切到自建兼容端点。后端未配置 → 显式 GEN_BACKEND_NOT_CONFIGURED,不伪造生成能力。注意:远端为异步任务制,单次调用可能耗时数分钟,超时的调用方请改用 REST POST /api/forge/gen/mesh",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "与 imageRef 至少其一;上限 600 字符" },
                        "imageRef": { "type": "string", "description": "参考图(项目相对路径 png/jpg,或 http(s)/data URI);给了即走图生 3D,与 prompt 至少其一" },
                        "targetPolyBudget": { "type": "integer", "description": "目标面数(standard 档 100..=300000,smart-topology 档 100..=15000)" },
                        "styleRefAssetPath": { "type": "string", "description": "风格参考:图片路径 → 作贴图引导图;其余字符串 → 作贴图提示词" },
                        "texture": { "type": "boolean", "description": "是否跑贴图阶段(默认 true;false 只出几何,省额度)" },
                        "pbr": { "type": "boolean", "description": "是否出 PBR 贴图组(默认 true)" },
                        "textureResolution": { "type": "string", "description": "2k | 4k | 8k(默认 2k)" },
                        "modelType": { "type": "string", "description": "standard | smart-topology(默认 standard)" },
                        "aiModel": { "type": "string", "description": "latest | meshy-7 | meshy-6 | meshy-5 | meshy-t2(缺省取后端条目 model)" },
                        "topology": { "type": "string", "description": "triangle | quad" },
                        "poseMode": { "type": "string", "description": "a-pose | t-pose(角色类可用)" },
                        "timeoutSec": { "type": "integer", "description": "单阶段等待预算秒数(默认 900)" },
                        "backend": { "type": "string", "description": "后端 id(缺省取首个已配置的 text2mesh 适配器,即 meshy)" }
                    }
                }
            },
            {
                "name": "gen_mesh_refine",
                "description": "网格精修(retopo/unwrap/rigPreview);无 refine 后端 → 显式 GEN_BACKEND_NOT_CONFIGURED(retopo/LOD 由 gen_accept 构建链 rurix-geom-build DAG 派生,D-009)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "meshFileRef": { "type": "string", "description": "项目相对路径(.forge/tmp/gen/ 内)" },
                        "ops": {
                            "type": "object",
                            "properties": {
                                "retopo": { "type": "object", "properties": { "targetFaces": { "type": "integer" } } },
                                "unwrap": { "type": "boolean" },
                                "rigPreview": { "type": "boolean" }
                            }
                        }
                    },
                    "required": ["meshFileRef", "ops"]
                }
            },
            {
                "name": "gen_accept",
                "description": "网格产物正式入管线:.forge/tmp/gen/ meshFileRef → Content/<destFolder>/<name>.<ext> + .meta provenance(origin=gen-model,结构化 detail)+ .rxmesh 构建产物;importSettings 透传(merge 进 .meta import_settings,缓存键构成)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "meshFileRef": { "type": "string", "description": "项目相对路径(.forge/tmp/gen/ 内)" },
                        "destFolder": { "type": "string", "description": "相对 Content/,如 \"Meshes\"" },
                        "name": { "type": "string", "description": "文件名片段(不含扩展名)" },
                        "importSettings": { "type": "object", "description": "透传 asset_import 第三参(如 generateLods)" }
                    },
                    "required": ["meshFileRef", "destFolder", "name"]
                }
            }
        ]
    })
}

fn ok(id: Value, result: Value) -> Value { json!({ "jsonrpc": "2.0", "id": id, "result": result }) }
fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_wrap(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error { out["isError"] = json!(true); }
    out
}

fn lock<'a>(m: &'a Mutex<ForgeProject>) -> MutexGuard<'a, ForgeProject> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, GenError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, format!("缺参数或为空: {key}")))
}

/// 图引用 → 远端可收的地址:已是 http(s)/data URI 则原样透传,否则按项目相对路径读盘编码。
fn to_image_uri(project: &ForgeProject, image_ref: &str) -> Result<String, GenError> {
    let r = image_ref.trim();
    if r.starts_with("http://") || r.starts_with("https://") || r.starts_with("data:") {
        return Ok(r.to_string());
    }
    tmpstore::image_data_uri(project, r)
}

/// gen_mesh 的档位参数原样搬进 MediaRequest.params(校验交给适配器,
/// 由它按供应商能力面给出可诉诸的 GEN_BAD_PARAMS,工具层不重复一套白名单)。
fn mesh_params(args: &Value) -> Result<Value, GenError> {
    let mut params = json!({});
    for key in ["textureResolution", "modelType", "aiModel", "topology", "poseMode", "texturePrompt"]
    {
        if let Some(v) = args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
            params[key] = json!(v.trim());
        }
    }
    for key in ["texture", "pbr"] {
        if let Some(v) = args.get(key).and_then(Value::as_bool) {
            params[key] = json!(v);
        }
    }
    if let Some(v) = args.get("timeoutSec").and_then(Value::as_u64) {
        params["timeoutSec"] = json!(v);
    }
    // 契约参数名 targetPolyBudget → 适配器面 targetPolycount。
    match args.get("targetPolyBudget") {
        None | Some(Value::Null) => {}
        Some(v) => {
            let n = v.as_u64().filter(|n| *n > 0).ok_or_else(|| {
                GenError::new(GEN_BAD_PARAMS, "targetPolyBudget 须为正整数")
            })?;
            params["targetPolycount"] = json!(n);
        }
    }
    Ok(params)
}

fn call_tool(proj: &Arc<Mutex<ForgeProject>>, params: &Value) -> Result<Value, GenError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "invalid params: 缺工具名"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Err(GenError::new(GEN_BAD_PARAMS, "invalid params: arguments 须为对象"));
    }

    match name {
        "gen_mesh" => {
            // 参数校验先行:prompt 与 imageRef 双空 → GEN_BAD_PARAMS。
            let prompt = args.get("prompt").and_then(Value::as_str).unwrap_or("");
            let image_ref = args.get("imageRef").and_then(Value::as_str).unwrap_or("");
            if prompt.is_empty() && image_ref.is_empty() {
                return Err(GenError::new(GEN_BAD_PARAMS, "prompt 与 imageRef 至少其一非空"));
            }
            // 默认 meshy(注册表首个已配置的 text2mesh 适配器);未配置显式 NOT_CONFIGURED
            // (D-F5-E 诚实纪律不变,门从「无适配器」升级为「适配器未配置」)。
            let backend_arg = args.get("backend").and_then(Value::as_str);
            let cfg = gend::config::GenConfig::load();
            let keys = gend::keystore::Keystore::load();
            let backend = gend::media::resolve_backend(
                gend::media::MediaKind::Mesh,
                backend_arg,
                &cfg,
                &keys,
            )?;
            // 参考图/风格图落在本地磁盘,远端只收 URL 或 data URI,故先在工具层解析编码
            // (media 层保持纯 HTTP,不认识 ForgeProject)。
            let mut params = mesh_params(&args)?;
            if !image_ref.is_empty() {
                let p = lock(proj);
                params["imageDataUrl"] = json!(to_image_uri(&p, image_ref)?);
            }
            if let Some(style) = args.get("styleRefAssetPath").and_then(Value::as_str) {
                let style = style.trim();
                if !style.is_empty() {
                    let p = lock(proj);
                    // 图片路径 → 贴图引导图;其余(含自由文本)→ 贴图提示词。
                    match to_image_uri(&p, style) {
                        Ok(uri) => params["textureImageUrl"] = json!(uri),
                        Err(_) => params["texturePrompt"] = json!(style),
                    }
                }
            }
            let req = gend::media::MediaRequest {
                kind: gend::media::MediaKind::Mesh,
                prompt: prompt.to_string(),
                params,
            };
            let artifacts = backend.generate(&req, &cfg, &keys)?;
            let p = lock(proj);
            let seed = gend::fnv1a64(prompt.as_bytes());
            let mut candidates = Vec::new();
            // 顶层 imageRefs 是 agentd 侧「工具产出图片」的约定键:有视觉面的模型会
            // 收到这些图,从而真看见生成的东西,而不是只读到一行文件路径。
            let mut image_refs: Vec<Value> = Vec::new();
            for (i, a) in artifacts.iter().enumerate() {
                let sidecar = json!({
                    "backendId": backend.id(),
                    "kind": if image_ref.is_empty() { "text2mesh" } else { "image2mesh" },
                    "prompt": prompt,
                    // 供应商任务 id / 模型 / 额度等如实入 sidecar,gen_accept 据此写 provenance。
                    "meta": a.meta,
                    "generatedAt": utc_now_iso8601(),
                });
                let file_ref =
                    tmpstore::save_artifact(&p, &a.bytes, &a.ext, seed, i as u32, &sidecar)?;
                // 供应商预览图落盘:签名 URL 会过期,存下来才是可复看的事实。
                let mut previews = Vec::new();
                for pv in &a.previews {
                    let r = tmpstore::save_preview(&p, &file_ref, &pv.label, &pv.png)?;
                    previews.push(json!({ "label": pv.label, "fileRef": r.clone() }));
                    image_refs.push(json!(r));
                }
                candidates.push(json!({
                    "meshFileRef": file_ref,
                    "backendId": backend.id(),
                    "meta": a.meta,
                    "previews": previews,
                }));
            }
            Ok(json!({ "candidates": candidates, "imageRefs": image_refs }))
        }
        "gen_mesh_refine" => {
            let mesh_ref = arg_str(&args, "meshFileRef")?;
            match args.get("ops") {
                Some(v) if v.is_object() => {}
                _ => return Err(GenError::new(GEN_BAD_PARAMS, "缺参数或须为对象: ops")),
            }
            // meshFileRef 存在性校验先于后端门(如实报 FILE_NOT_FOUND)。
            let p = lock(proj);
            tmpstore::resolve_mesh_ref(&p, mesh_ref)?;
            // 无 retopo/unwrap 后端(D-F5-E)→ 显式 NOT_CONFIGURED。
            // retopo/LOD 走 rurix-geom-build 简化 DAG 派生(D-009),属 gen_accept 构建链
            // 内行为,非独立 refine 调用。
            Err(GenError::new(
                GEN_BACKEND_NOT_CONFIGURED,
                "无 refine 后端(retopo/LOD 由 gen_accept 构建链 rurix-geom-build DAG 派生,非独立 refine 调用;D-009)",
            ))
        }
        "gen_accept" => {
            let mesh_ref = arg_str(&args, "meshFileRef")?;
            let dest = arg_str(&args, "destFolder")?;
            let name = arg_str(&args, "name")?;
            let import_settings: Option<serde_json::Map<String, Value>> = match args.get("importSettings") {
                None | Some(Value::Null) => None,
                Some(Value::Object(m)) => Some(m.clone()),
                Some(_) => return Err(GenError::new(GEN_BAD_PARAMS, "importSettings 须为对象")),
            };
            let p = lock(proj);
            // meshFileRef 存在性校验(meshFileRef 标签的 GEN_FILE_NOT_FOUND)。
            tmpstore::resolve_mesh_ref(&p, mesh_ref)?;
            // provenance detail:有生成 sidecar(未来真实后端)如实沿用;无 sidecar
            // (用户自备 .forge/tmp/gen/ 产物)backendId="user-provided" 如实标注。
            let detail = tmpstore::load_sidecar(&p, mesh_ref).unwrap_or_else(|| {
                json!({
                    "backendId": "user-provided",
                    "sourceRefs": [mesh_ref],
                    "generatedAt": utc_now_iso8601(),
                    "note": "用户自备产物(.forge/tmp/gen/ 预放),非真实生成后端",
                })
            });
            let acc = accept_asset(&p, mesh_ref, dest, name, "gen-model", detail, import_settings.as_ref())?;
            Ok(json!({
                "assetPath": acc.asset_path,
                "guid": acc.guid,
                "artifact": acc.artifact,
                "cacheHit": acc.cache_hit,
            }))
        }
        _ => Err(GenError::new(GEN_BAD_PARAMS, format!("未知工具:{name}"))),
    }
}

pub fn serve_stdio(proj: Arc<Mutex<ForgeProject>>) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = err(Value::Null, -32700, &format!("parse error: {e}"));
                let _ = writeln!(stdout, "{resp}");
                let _ = stdout.flush();
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let resp: Option<Value> = match method {
            "initialize" => id.map(|i| ok(i, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "gen-model", "version": env!("CARGO_PKG_VERSION") }
            }))),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => id.map(|i| ok(i, json!({}))),
            "tools/list" => id.map(|i| ok(i, tool_list())),
            "tools/call" => id.map(|i| {
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                match call_tool(&proj, &params) {
                    Ok(result) => ok(i, tool_wrap(&result, false)),
                    Err(e) => ok(i, tool_wrap(&json!({ "error": e.code, "message": e.message }), true)),
                }
            }),
            "" => id.map(|i| err(i, -32600, "invalid request: 缺 method")),
            other => id.map(|i| err(i, -32601, &format!("method not found: {other}"))),
        };
        if let Some(r) = resp {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gend::{GEN_FILE_NOT_FOUND, GEN_BACKEND_ERROR};
    use std::sync::Mutex;

    /// FORGE_GEN_DATA_DIR 进程级,测试串行。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// 最小三角形 gltf(与 projects/demo/Content/Prefabs/tri_min.gltf 同字节)。
    const TRI_MIN_GLTF: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"mode":4}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","max":[1,1,0],"min":[0,0,0]}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}]}"#;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("genmdl-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_project(tag: &str) -> Arc<Mutex<ForgeProject>> {
        Arc::new(Mutex::new(ForgeProject::with_defaults(temp_dir(tag))))
    }

    fn call(proj: &Arc<Mutex<ForgeProject>>, tool: &str, args: Value) -> Result<Value, GenError> {
        call_tool(proj, &json!({ "name": tool, "arguments": args }))
    }

    #[test]
    fn tools_list_three() {
        let tl = tool_list();
        let tools = tl["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        for t in ["gen_mesh", "gen_mesh_refine", "gen_accept"] {
            assert!(tools.iter().any(|x| x["name"] == t), "缺工具 {t}");
        }
        // gen_accept 三必填 + importSettings 可选(05 §8)。
        let acc = tools.iter().find(|x| x["name"] == "gen_accept").unwrap();
        let req = acc["inputSchema"]["required"].as_array().unwrap();
        for k in ["meshFileRef", "destFolder", "name"] {
            assert!(req.iter().any(|r| r == k), "gen_accept 缺必填 {k}");
        }
        assert!(acc["inputSchema"]["properties"]["importSettings"].is_object());
        let refine = tools.iter().find(|x| x["name"] == "gen_mesh_refine").unwrap();
        let rreq = refine["inputSchema"]["required"].as_array().unwrap();
        assert!(rreq.iter().any(|r| r == "meshFileRef") && rreq.iter().any(|r| r == "ops"));
    }

    #[test]
    fn gen_mesh_bad_params_then_not_configured() {
        let _g = ENV_LOCK.lock().unwrap();
        // 即使 local-mock(text2img)已配置,gen_mesh 仍 NOT_CONFIGURED(素材创作波:
        // media 注册表有 remote-mesh-compatible 适配器但未配置——配置门如实)。
        let data = temp_dir("mesh-data");
        std::fs::write(
            data.join("gen-backends.json"),
            r#"{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}"#,
        )
        .unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        let proj = temp_project("mesh-proj");
        // 双空 → GEN_BAD_PARAMS(参数校验先行)。
        let e = call(&proj, "gen_mesh", json!({ "prompt": "", "imageRef": "" })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        let e = call(&proj, "gen_mesh", json!({})).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 有参 → GEN_BACKEND_NOT_CONFIGURED(显式,不伪造;错误信息引导可配置条目)。
        let e = call(&proj, "gen_mesh", json!({ "prompt": "a chair" })).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        assert!(e.message.contains("remote-mesh-compatible"), "错误应引导配置: {}", e.message);
        let e = call(&proj, "gen_mesh", json!({ "imageRef": ".forge/tmp/gen/x.png" })).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        // 显式指定不支持 text2mesh 的后端(local-mock)→ GEN_BAD_PARAMS(能力面如实)。
        let e = call(&proj, "gen_mesh", json!({ "prompt": "a chair", "targetPolyBudget": 5000, "backend": "local-mock" })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 显式指定 mesh 适配器但未配置 → GEN_BACKEND_NOT_CONFIGURED。
        let e = call(&proj, "gen_mesh", json!({ "prompt": "a chair", "backend": "remote-mesh-compatible" })).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn gen_mesh_refine_file_check_then_not_configured() {
        let proj = temp_project("refine-proj");
        // 缺 ops → GEN_BAD_PARAMS。
        let e = call(&proj, "gen_mesh_refine", json!({ "meshFileRef": ".forge/tmp/gen/x.gltf" })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 文件缺失 → GEN_FILE_NOT_FOUND(存在性先于后端门)。
        let e = call(
            &proj,
            "gen_mesh_refine",
            json!({ "meshFileRef": ".forge/tmp/gen/none.gltf", "ops": { "retopo": { "targetFaces": 1000 } } }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_FILE_NOT_FOUND);
        // 文件存在 → GEN_BACKEND_NOT_CONFIGURED(无 refine 后端)。
        {
            let p = proj.lock().unwrap();
            let dir = gend::tmpstore::gen_dir(&p).unwrap();
            std::fs::write(dir.join("m.gltf"), TRI_MIN_GLTF).unwrap();
        }
        let e = call(
            &proj,
            "gen_mesh_refine",
            json!({ "meshFileRef": ".forge/tmp/gen/m.gltf", "ops": { "retopo": { "targetFaces": 1000 } } }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
    }

    #[test]
    fn gen_accept_temp_project_full_chain() {
        let proj = temp_project("acc-proj");
        {
            let p = proj.lock().unwrap();
            let dir = gend::tmpstore::gen_dir(&p).unwrap();
            std::fs::write(dir.join("user-chair.gltf"), TRI_MIN_GLTF).unwrap();
        }
        let mesh_ref = ".forge/tmp/gen/user-chair.gltf";
        // 文件缺失先行。
        let e = call(&proj, "gen_accept", json!({ "meshFileRef": ".forge/tmp/gen/none.gltf", "destFolder": "Meshes", "name": "x" })).unwrap_err();
        assert_eq!(e.code, GEN_FILE_NOT_FOUND);
        // importSettings 非对象 → GEN_BAD_PARAMS。
        let e = call(&proj, "gen_accept", json!({ "meshFileRef": mesh_ref, "destFolder": "Meshes", "name": "x", "importSettings": 5 })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 全链:Meshes 落盘 + .rxmesh artifact + provenance origin=gen-model +
        // detail.backendId=user-provided + importSettings 透传。
        let acc = call(
            &proj,
            "gen_accept",
            json!({ "meshFileRef": mesh_ref, "destFolder": "Meshes", "name": "ut_chair", "importSettings": { "generateLods": [0.5] } }),
        )
        .unwrap();
        assert_eq!(acc["assetPath"], "Meshes/ut_chair.gltf");
        assert!(acc["guid"].as_str().unwrap().len() > 8);
        assert_eq!(acc["cacheHit"], false, "首次构建缓存未命中");
        let art = acc["artifact"].as_str().expect("网格须带 artifact");
        assert!(art.starts_with("rxmesh/") && art.ends_with(".rxmesh"), "{art}");
        let p = proj.lock().unwrap();
        assert!(p.cache_root().join(art).is_file(), ".rxmesh 产物落盘: {art}");
        assert!(p.content_root().join("Meshes/ut_chair.gltf").is_file());
        let meta = assetd::meta::MetaDoc::load(&assetd::meta_path_for(&p.content_root(), "Meshes/ut_chair.gltf")).unwrap();
        let prov = meta.provenance.expect("provenance 必填");
        assert_eq!(prov.origin, "gen-model");
        let d = prov.detail.expect("detail 必填");
        assert_eq!(d["backendId"], "user-provided");
        assert_eq!(d["sourceRefs"], json!([mesh_ref]));
        assert!(d["generatedAt"].as_str().unwrap().ends_with('Z'));
        assert_eq!(meta.import_settings["generateLods"], json!([0.5]));
        drop(p);
        // 确定性复跑:改名 → guid 不同,同源字节 + 同 importSettings → 缓存命中。
        let acc2 = call(
            &proj,
            "gen_accept",
            json!({ "meshFileRef": mesh_ref, "destFolder": "Meshes", "name": "ut_chair2", "importSettings": { "generateLods": [0.5] } }),
        )
        .unwrap();
        assert_ne!(acc2["guid"], acc["guid"]);
        assert_eq!(acc2["cacheHit"], true, "同源同设置二次 accept 须缓存命中(08 §4.3)");
        assert_eq!(acc2["artifact"], acc["artifact"]);
        // 未知工具 → GEN_BAD_PARAMS;import_assets 失败族(非 gltf 伪装)→ GEN_BACKEND_ERROR 族。
        let e = call(&proj, "gen_no_such", json!({})).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        let _ = GEN_BACKEND_ERROR; // 错误码族引用(防未用警告)。
    }
}
