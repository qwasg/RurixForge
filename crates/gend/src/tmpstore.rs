//! 临时产物目录(08 §6.2 逐字):<project>/.forge/tmp/gen/。
//! imageFileRef = 项目相对路径字符串(如 ".forge/tmp/gen/gen-<ms>-<seed>-<i>.png")。
//! 每个候选附带 sidecar(<file>.png.json)记录生成上下文(backendId/prompt/seed/
//! generatedAt 等),gen_accept 据此写 .meta provenance detail。

use std::path::PathBuf;

use assetd::project::ForgeProject;
use serde_json::Value;

use forge_util::timeutil::unix_millis;
use crate::{GenError, Result, GEN_FILE_NOT_FOUND};

const GEN_SUBDIR: &str = ".forge/tmp/gen";

/// 产物目录(确保存在)。
pub fn gen_dir(project: &ForgeProject) -> Result<PathBuf> {
    let dir = project.root.join(GEN_SUBDIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 保存候选 PNG + sidecar,返回 imageFileRef(项目相对,正斜杠)。
pub fn save_candidate(
    project: &ForgeProject,
    png_bytes: &[u8],
    seed: u64,
    idx: u32,
    sidecar: &Value,
) -> Result<String> {
    save_artifact(project, png_bytes, "png", seed, idx, sidecar)
}

/// 产物扩展名白名单(素材创作波泛化:图片 + 视频/音频/网格)。
pub const ARTIFACT_EXTS: [&str; 6] = ["png", "mp4", "mp3", "wav", "glb", "gltf"];

/// 保存任意媒体产物 + sidecar,返回 fileRef(项目相对,正斜杠;素材创作波泛化)。
/// ext 须在 ARTIFACT_EXTS 白名单内(不带点)。
pub fn save_artifact(
    project: &ForgeProject,
    bytes: &[u8],
    ext: &str,
    seed: u64,
    idx: u32,
    sidecar: &Value,
) -> Result<String> {
    if !ARTIFACT_EXTS.contains(&ext) {
        return Err(GenError::new(
            crate::GEN_BACKEND_ERROR,
            format!("产物扩展名须为 {ARTIFACT_EXTS:?} 之一,实: {ext}"),
        ));
    }
    let dir = gen_dir(project)?;
    let file_name = format!("gen-{}-{seed}-{idx}.{ext}", unix_millis());
    std::fs::write(dir.join(&file_name), bytes)?;
    let sc_path = dir.join(format!("{file_name}.json"));
    let sc_text = serde_json::to_string_pretty(sidecar)
        .map_err(|e| GenError::new(crate::GEN_BACKEND_ERROR, format!("sidecar 序列化失败: {e}")))?;
    std::fs::write(sc_path, sc_text)?;
    Ok(format!("{GEN_SUBDIR}/{file_name}"))
}

/// 产物预览图落盘:与主产物同目录同名加视角后缀(`<产物名>.<label>.png`),不另起 sidecar
/// ——它们是主产物的附属视图,不是独立候选,起 sidecar 会被误当成可 accept 的产物。
/// 供应商给的是会过期的签名 URL,所以必须落盘;label 只收 [a-z0-9-]。
pub fn save_preview(
    project: &ForgeProject,
    artifact_ref: &str,
    label: &str,
    png: &[u8],
) -> Result<String> {
    let abs = resolve_any_ref(project, artifact_ref)?;
    let file_name = abs
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| GenError::new(GEN_FILE_NOT_FOUND, format!("产物名不可读: {artifact_ref}")))?;
    let safe: String = label
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .collect();
    if safe.is_empty() {
        return Err(GenError::new(
            crate::GEN_BAD_PARAMS,
            format!("预览图 label 非法(须含 [a-z0-9-]): {label}"),
        ));
    }
    let name = format!("{file_name}.{safe}.png");
    std::fs::write(abs.with_file_name(&name), png)?;
    Ok(format!("{GEN_SUBDIR}/{name}"))
}

/// fileRef → 绝对路径。限定 .forge/tmp/gen/ 内(拒绝越界/绝对路径);缺失 → GEN_FILE_NOT_FOUND。
/// param 仅用于错误消息标注(imageFileRef/meshFileRef/fileRef)。
fn resolve_gen_ref(project: &ForgeProject, file_ref: &str, param: &str) -> Result<PathBuf> {
    let rel = file_ref.replace('\\', "/");
    if rel.is_empty() || rel.starts_with('/') || rel.contains("..") || rel.contains(':') {
        return Err(GenError::new(
            GEN_FILE_NOT_FOUND,
            format!("{param} 非法或越界: {file_ref}"),
        ));
    }
    if !rel.starts_with(&format!("{GEN_SUBDIR}/")) {
        return Err(GenError::new(
            GEN_FILE_NOT_FOUND,
            format!("{param} 须位于 {GEN_SUBDIR}/ 内: {file_ref}"),
        ));
    }
    let abs = project.root.join(&rel);
    if !abs.is_file() {
        return Err(GenError::new(
            GEN_FILE_NOT_FOUND,
            format!("{param} 产物不存在: {file_ref}"),
        ));
    }
    Ok(abs)
}

/// imageFileRef → 绝对路径(规则同 resolve_gen_ref)。
pub fn resolve_ref(project: &ForgeProject, image_file_ref: &str) -> Result<PathBuf> {
    resolve_gen_ref(project, image_file_ref, "imageFileRef")
}

/// meshFileRef → 绝对路径(同 imageFileRef:.forge/tmp/gen/ 相对路径 + 存在性校验)。
pub fn resolve_mesh_ref(project: &ForgeProject, mesh_file_ref: &str) -> Result<PathBuf> {
    resolve_gen_ref(project, mesh_file_ref, "meshFileRef")
}

/// videoFileRef → 绝对路径(同 imageFileRef;视频截帧管线的输入端)。
pub fn resolve_video_ref(project: &ForgeProject, video_file_ref: &str) -> Result<PathBuf> {
    resolve_gen_ref(project, video_file_ref, "videoFileRef")
}

/// accept_asset 内部用(中性 fileRef 标注;工具层先行校验可带各自参数名)。
pub(crate) fn resolve_any_ref(project: &ForgeProject, file_ref: &str) -> Result<PathBuf> {
    resolve_gen_ref(project, file_ref, "fileRef")
}

/// 读候选 sidecar(无 sidecar = None,如人工预放的 fixture)。
pub fn load_sidecar(project: &ForgeProject, image_file_ref: &str) -> Option<Value> {
    let abs = resolve_ref(project, image_file_ref).ok()?;
    let sc = abs.with_file_name(format!(
        "{}.json",
        abs.file_name()?.to_str()?
    ));
    let text = std::fs::read_to_string(sc).ok()?;
    serde_json::from_str(&text).ok()
}

/// 参考图上限(编码为 data URI 后约 ×4/3;远端建任务 body 装得下)。
pub const IMAGE_REF_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// 项目内图片 → base64 data URI。图生 3D 类供应商只收公网 URL 或 data URI,
/// 而本仓的参考图在本地磁盘(.forge/tmp/gen/ 候选或 Content/ 资产),无公网地址可给。
/// 仅 png/jpg/jpeg(供应商公共交集);越界/缺失 → GEN_FILE_NOT_FOUND,过大 → GEN_BAD_PARAMS。
pub fn image_data_uri(project: &ForgeProject, rel_path: &str) -> Result<String> {
    use base64::Engine as _;
    let abs = resolve_project_file(project, rel_path)?;
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        other => {
            return Err(GenError::new(
                crate::GEN_BAD_PARAMS,
                format!("参考图须为 png/jpg/jpeg,实: {other}({rel_path})"),
            ))
        }
    };
    let size = std::fs::metadata(&abs)?.len();
    if size > IMAGE_REF_MAX_BYTES {
        return Err(GenError::new(
            crate::GEN_BAD_PARAMS,
            format!("参考图 {size} 字节超上限 {IMAGE_REF_MAX_BYTES}: {rel_path}"),
        ));
    }
    let bytes = std::fs::read(&abs)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

/// 项目内任意已存在文件(variations 源图用;拒绝越界)。
pub fn resolve_project_file(project: &ForgeProject, rel_path: &str) -> Result<PathBuf> {
    let rel = rel_path.replace('\\', "/");
    if rel.is_empty() || rel.starts_with('/') || rel.contains("..") || rel.contains(':') {
        return Err(GenError::new(
            GEN_FILE_NOT_FOUND,
            format!("路径非法或越界: {rel_path}"),
        ));
    }
    let abs = project.root.join(&rel);
    if !abs.is_file() {
        return Err(GenError::new(GEN_FILE_NOT_FOUND, format!("文件不存在: {rel_path}")));
    }
    Ok(abs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_project(tag: &str) -> ForgeProject {
        let dir = std::env::temp_dir().join(format!("gend-tmp-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        ForgeProject::with_defaults(dir)
    }

    #[test]
    fn save_and_resolve_roundtrip() {
        let p = temp_project("rt");
        let sc = json!({"backendId":"local-mock","seed":42});
        let r = save_candidate(&p, b"\x89PNG\r\n\x1a\nfake", 42, 0, &sc).unwrap();
        assert!(r.starts_with(".forge/tmp/gen/gen-"));
        assert!(r.ends_with("-42-0.png"));
        let abs = resolve_ref(&p, &r).unwrap();
        assert_eq!(std::fs::read(&abs).unwrap(), b"\x89PNG\r\n\x1a\nfake");
        let sc2 = load_sidecar(&p, &r).unwrap();
        assert_eq!(sc2["seed"], 42);
        std::fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn rejects_traversal_and_missing() {
        let p = temp_project("rj");
        assert!(resolve_ref(&p, "../escape.png").is_err());
        assert!(resolve_ref(&p, "Content/Textures/x.png").is_err());
        let err = resolve_ref(&p, ".forge/tmp/gen/nope.png").unwrap_err();
        assert_eq!(err.code, GEN_FILE_NOT_FOUND);
        // meshFileRef 同规则(F5 wave.2):越界拒绝 + 缺失 GEN_FILE_NOT_FOUND + 存在解析。
        assert!(resolve_mesh_ref(&p, "../escape.gltf").is_err());
        assert!(resolve_mesh_ref(&p, "Content/Prefabs/tri_min.gltf").is_err());
        let err = resolve_mesh_ref(&p, ".forge/tmp/gen/nope.gltf").unwrap_err();
        assert_eq!(err.code, GEN_FILE_NOT_FOUND);
        assert!(err.message.contains("meshFileRef"), "错误标注参数名: {}", err.message);
        let dir = gen_dir(&p).unwrap();
        std::fs::write(dir.join("m.gltf"), b"{}").unwrap();
        assert!(resolve_mesh_ref(&p, ".forge/tmp/gen/m.gltf").unwrap().is_file());
        std::fs::remove_dir_all(&p.root).ok();
    }
}
