//! gen_accept:.forge/tmp/gen/ 产物正式导入 Content/(复用 assetd 导入链)+ provenance 写入
//! (08 §6.4:origin="gen-image"|"gen-model",detail 结构化 Value 由调用方组装——backendId/
//! prompt/seed/generatedAt 等来自候选 sidecar;用户自备产物 backendId="user-provided" 如实标注)。
//! F5 wave.2:accept_asset 泛化(图像/网格共用同一 import_assets 构建链;网格带回 .rxmesh
//! artifact 与 cache_hit;importSettings 透传进 .meta import_settings = 缓存键构成)。

use assetd::import::import_assets;
use assetd::meta::{MetaDoc, Provenance};
use assetd::project::ForgeProject;
use assetd::meta_path_for;
use serde_json::Value;

use crate::tmpstore;
use crate::{GenError, Result, GEN_BAD_PARAMS, GEN_BACKEND_ERROR};

/// gen_accept 返回(05 §7/§8)。
#[derive(Debug, Clone)]
pub struct AcceptedAsset {
    /// 相对 Content/ 的正斜杠路径(assetd 族约定,如 "Textures/f5w1_wood.png")。
    pub asset_path: String,
    pub guid: String,
    /// 构建产物相对 .forge/cache/ 路径(网格 = Some(.rxmesh);贴图 = None)。
    pub artifact: Option<String>,
    /// 缓存命中(二次导入零重建,08 §4.3;非网格恒 false)。
    pub cache_hit: bool,
}

/// 产物入管线(泛化):fileRef(.forge/tmp/gen/ 内)→ Content/<destFolder>/<name>.<ext>。
///
/// name 重命名路径(选最简单可靠者):先把产物复制为 .forge/tmp/gen/ 下同目录的
/// `<name>.<ext>`(扩展名继承源文件),再交 assetd::import::import_assets 以目标文件名落
/// Content/(import_assets 以源文件名定落盘名,故 staging 副本即命名载体)。
/// import_settings 透传 import_assets 第三参(merge 进 .meta import_settings,缓存键构成)。
pub fn accept_asset(
    project: &ForgeProject,
    file_ref: &str,
    dest_folder: &str,
    name: &str,
    origin: &str,
    provenance_detail: Value,
    import_settings: Option<&serde_json::Map<String, Value>>,
) -> Result<AcceptedAsset> {
    let src = tmpstore::resolve_any_ref(project, file_ref)?;

    // name 校验:非空、无路径分隔符/扩展名/冒号(落 Content 的文件名片段)。
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name.contains('.')
        || name.starts_with(' ')
    {
        return Err(GenError::new(
            GEN_BAD_PARAMS,
            format!("name 须为纯文件名片段(不含分隔符/扩展名): {name:?}"),
        ));
    }
    if dest_folder.is_empty() {
        return Err(GenError::new(GEN_BAD_PARAMS, "destFolder 不可空"));
    }

    // staging 命名副本(与源同目录,扩展名继承源文件;同名同文件则直接导入,幂等)。
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| !e.is_empty())
        .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, format!("产物无扩展名: {file_ref}")))?;
    let staged_rel = format!(".forge/tmp/gen/{name}.{ext}");
    let staged_abs = project.root.join(&staged_rel);
    if src != staged_abs {
        std::fs::copy(&src, &staged_abs).map_err(|e| {
            GenError::new(GEN_BACKEND_ERROR, format!("staging 复制失败: {e}"))
        })?;
    }

    let outcome = import_assets(project, &[staged_rel], dest_folder, import_settings)?;
    if let Some(f) = outcome.failed.first() {
        return Err(GenError::new(
            GEN_BACKEND_ERROR,
            format!("入管线失败({}): {}", f.source, f.error),
        ));
    }
    let one = outcome
        .imported
        .into_iter()
        .next()
        .ok_or_else(|| GenError::new(GEN_BACKEND_ERROR, "入管线无结果"))?;

    // provenance 覆写(import 链默认 user-import;gen_accept 强制 gen-* origin + 结构化 detail)。
    let meta_path = meta_path_for(&project.content_root(), &one.asset_path);
    let mut meta = MetaDoc::load(&meta_path)?;
    meta.provenance = Some(Provenance {
        origin: origin.into(),
        detail: Some(provenance_detail),
    });
    meta.save(&meta_path)?;

    Ok(AcceptedAsset {
        asset_path: one.asset_path,
        guid: one.guid,
        artifact: one.artifact,
        cache_hit: one.cache_hit,
    })
}

/// 图像 gen_accept(wave.1 兼容面):origin="gen-image",无 importSettings。
pub fn gen_accept(
    project: &ForgeProject,
    image_file_ref: &str,
    dest_folder: &str,
    name: &str,
    provenance_detail: Value,
) -> Result<AcceptedAsset> {
    accept_asset(project, image_file_ref, dest_folder, name, "gen-image", provenance_detail, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmpstore::save_candidate;
    use serde_json::json;

    fn temp_project(tag: &str) -> ForgeProject {
        let dir = std::env::temp_dir().join(format!("gend-acc-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        ForgeProject::with_defaults(dir)
    }

    #[test]
    fn accept_full_chain_with_structured_provenance() {
        let p = temp_project("full");
        // 真实 PNG(mock 产物)走完整导入链(贴图解码尺寸)。
        let png = crate::mock::render_map("wood 木纹", "albedo", 256, 42).unwrap();
        let detail = json!({
            "backendId": "local-mock",
            "prompt": "wood 木纹",
            "seed": 42,
            "sourceRefs": [],
            "generatedAt": "2026-08-18T00:00:00Z",
        });
        let r = save_candidate(&p, &png, 42, 0, &detail).unwrap();
        let acc = gen_accept(&p, &r, "Textures", "f5w1_test", detail.clone()).unwrap();
        assert_eq!(acc.asset_path, "Textures/f5w1_test.png");
        assert!(!acc.guid.is_empty());
        assert!(p.content_root().join("Textures/f5w1_test.png").is_file());
        // .meta provenance 结构化断言(I-7)。
        let meta = MetaDoc::load(&meta_path_for(&p.content_root(), &acc.asset_path)).unwrap();
        let prov = meta.provenance.expect("provenance 必填");
        assert_eq!(prov.origin, "gen-image");
        let d = prov.detail.expect("detail 必填");
        assert_eq!(d["backendId"], "local-mock");
        assert_eq!(d["prompt"], "wood 木纹");
        assert_eq!(d["seed"], 42);
        assert_eq!(d["generatedAt"], "2026-08-18T00:00:00Z");
        assert_eq!(d["sourceRefs"], json!([]));
        // 幂等:再 accept 同名 → GUID 复用(reimport 语义)。
        let acc2 = gen_accept(&p, &r, "Textures", "f5w1_test", detail).unwrap();
        assert_eq!(acc2.guid, acc.guid);
        std::fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn bad_name_and_missing_file() {
        let p = temp_project("bad");
        let err = gen_accept(&p, ".forge/tmp/gen/none.png", "Textures", "x", json!({})).unwrap_err();
        assert_eq!(err.code, crate::GEN_FILE_NOT_FOUND);
        let png = crate::mock::render_map("p", "albedo", 256, 1).unwrap();
        let r = save_candidate(&p, &png, 1, 0, &json!({})).unwrap();
        let err = gen_accept(&p, &r, "Textures", "a/b", json!({})).unwrap_err();
        assert_eq!(err.code, GEN_BAD_PARAMS);
        std::fs::remove_dir_all(&p.root).ok();
    }

    /// 最小三角形 gltf(与 projects/demo/Content/Prefabs/tri_min.gltf 同字节)。
    const TRI_MIN_GLTF: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"mode":4}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","max":[1,1,0],"min":[0,0,0]}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}]}"#;

    /// F5 wave.2:网格 accept_asset 全链(用户自备 gltf → Meshes + .rxmesh artifact +
    /// provenance origin=gen-model + importSettings 透传缓存键;同源二次 accept 缓存命中)。
    #[test]
    fn accept_mesh_full_chain_artifact_and_cache_hit() {
        let p = temp_project("mesh");
        let gen_dir = crate::tmpstore::gen_dir(&p).unwrap();
        std::fs::write(gen_dir.join("user-chair.gltf"), TRI_MIN_GLTF).unwrap();
        let mesh_ref = ".forge/tmp/gen/user-chair.gltf";
        let settings: serde_json::Map<String, Value> =
            serde_json::from_value(json!({ "generateLods": [0.5] })).unwrap();
        let detail = json!({
            "backendId": "user-provided",
            "sourceRefs": [mesh_ref],
            "generatedAt": "2026-08-18T00:00:00Z",
        });
        let acc = accept_asset(&p, mesh_ref, "Meshes", "chair", "gen-model", detail.clone(), Some(&settings)).unwrap();
        assert_eq!(acc.asset_path, "Meshes/chair.gltf");
        assert!(!acc.guid.is_empty());
        assert!(!acc.cache_hit, "首次构建须缓存未命中");
        let art = acc.artifact.as_deref().expect("网格须带 .rxmesh artifact");
        assert!(art.starts_with("rxmesh/") && art.ends_with(".rxmesh"), "{art}");
        assert!(p.cache_root().join(art).is_file(), ".rxmesh 产物须落盘: {art}");
        assert!(p.content_root().join("Meshes/chair.gltf").is_file());
        // .meta:provenance origin=gen-model + import_settings.generateLods 透传。
        let meta = MetaDoc::load(&meta_path_for(&p.content_root(), &acc.asset_path)).unwrap();
        let prov = meta.provenance.expect("provenance 必填");
        assert_eq!(prov.origin, "gen-model");
        let d = prov.detail.expect("detail 必填");
        assert_eq!(d["backendId"], "user-provided");
        assert_eq!(d["sourceRefs"], json!([mesh_ref]));
        assert_eq!(meta.import_settings["generateLods"], json!([0.5]));
        // 同源字节 + 同 importSettings → 同缓存键:改名再 accept,guid 不同但缓存命中。
        let acc2 = accept_asset(&p, mesh_ref, "Meshes", "chair2", "gen-model", detail, Some(&settings)).unwrap();
        assert_ne!(acc2.guid, acc.guid);
        assert_eq!(acc2.asset_path, "Meshes/chair2.gltf");
        assert!(acc2.cache_hit, "同源同设置二次 accept 须缓存命中(08 §4.3)");
        assert_eq!(acc2.artifact, acc.artifact, "同缓存键 → 同 .rxmesh 产物");
        std::fs::remove_dir_all(&p.root).ok();
    }
}
