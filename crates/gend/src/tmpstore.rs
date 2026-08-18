//! 临时产物目录(08 §6.2 逐字):<project>/.forge/tmp/gen/。
//! imageFileRef = 项目相对路径字符串(如 ".forge/tmp/gen/gen-<ms>-<seed>-<i>.png")。
//! 每个候选附带 sidecar(<file>.png.json)记录生成上下文(backendId/prompt/seed/
//! generatedAt 等),gen_accept 据此写 .meta provenance detail。

use std::path::PathBuf;

use assetd::project::ForgeProject;
use serde_json::Value;

use crate::timeutil::unix_millis;
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
    let dir = gen_dir(project)?;
    let file_name = format!("gen-{}-{seed}-{idx}.png", unix_millis());
    std::fs::write(dir.join(&file_name), png_bytes)?;
    let sc_path = dir.join(format!("{file_name}.json"));
    let sc_text = serde_json::to_string_pretty(sidecar)
        .map_err(|e| GenError::new(crate::GEN_BACKEND_ERROR, format!("sidecar 序列化失败: {e}")))?;
    std::fs::write(sc_path, sc_text)?;
    Ok(format!("{GEN_SUBDIR}/{file_name}"))
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
