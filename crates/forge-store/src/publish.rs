//! 打包发布:项目资产 → 包清单 + blob → 推给源。
//!
//! 关键纪律:`publish_package` **重算** 每个文件的 sha256 与 size 覆盖 `manifest.files`,
//! 不信任调用方传进来的值——清单里的校验和是安装侧唯一的信任锚点,让上游随手填的值
//! 混进去等于把校验做成摆设。

use std::path::PathBuf;

use assetd::meta_path_for;
use assetd::project::ForgeProject;

use crate::manifest::{safe_rel_path, FileEntry, PackageManifest};
use crate::source::RegistrySource;
use crate::{Result, StoreError, STORE_PUBLISH_REJECTED};

/// 发布输入:清单草稿 + (包内相对路径, 磁盘绝对路径) 列表。
#[derive(Debug, Clone)]
pub struct PublishInput {
    pub manifest: PackageManifest,
    pub files: Vec<(String, PathBuf)>,
}

/// 打包并发布。返回**实际发布出去的清单**(files/时间戳已由本函数重算填充)。
pub fn publish_package(src: &dyn RegistrySource, input: &PublishInput) -> Result<PackageManifest> {
    let mut manifest = input.manifest.clone();
    let mut entries: Vec<FileEntry> = Vec::with_capacity(input.files.len());
    let mut blobs: Vec<(String, Vec<u8>)> = Vec::new();

    for (rel, abs) in &input.files {
        let rel = safe_rel_path(rel)?;
        let bytes = std::fs::read(abs).map_err(|e| {
            StoreError::new(
                STORE_PUBLISH_REJECTED,
                format!("打包读文件失败 {}: {e}", abs.display()),
            )
        })?;
        let sha = forge_util::hashutil::sha256_hex(&bytes);
        if entries.iter().any(|e| e.path == rel) {
            return Err(StoreError::new(
                STORE_PUBLISH_REJECTED,
                format!("包内路径重复: {rel}"),
            ));
        }
        entries.push(FileEntry { path: rel, sha256: sha.clone(), size: bytes.len() as u64 });
        // 内容寻址:同字节只传一次。
        if !blobs.iter().any(|(s, _)| s == &sha) {
            blobs.push((sha, bytes));
        }
    }

    manifest.files = entries;
    let now = forge_util::timeutil::utc_now_iso8601();
    if manifest.created_at.is_empty() {
        manifest.created_at = now.clone();
    }
    manifest.updated_at = now;
    manifest.validate()?;
    src.publish(&manifest, &blobs)?;
    Ok(manifest)
}

/// 从项目 Content 下若干资产构造发布输入(自动带上同名 `.meta` 侧车)。
///
/// `.meta` 随包分发是为了保留发布方的 importSettings / semantic 供订阅方检视;安装侧
/// **不会**用它建资产(GUID 由本地导入链新建),见 `install` 模块的说明。
pub fn draft_from_project(
    project: &ForgeProject,
    asset_paths: &[String],
    base: PackageManifest,
) -> Result<PublishInput> {
    if asset_paths.is_empty() {
        return Err(StoreError::new(STORE_PUBLISH_REJECTED, "未选择任何资产,无法打包"));
    }
    let content_root = project.content_root();
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for p in asset_paths {
        let rel = safe_rel_path(p)?;
        let abs = content_root.join(&rel);
        if !abs.is_file() {
            return Err(StoreError::new(
                STORE_PUBLISH_REJECTED,
                format!("待打包资产不存在: {rel}"),
            ));
        }
        if files.iter().any(|(r, _)| r == &rel) {
            continue; // 同一资产被选了两次:去重而非报错(选择面允许重复点选)
        }
        files.push((rel.clone(), abs));
        let meta = meta_path_for(&content_root, &rel);
        if meta.is_file() {
            files.push((format!("{rel}.meta"), meta));
        }
    }
    Ok(PublishInput { manifest: base, files })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PackageKind;
    use crate::source::{FileSource, SearchQuery};
    use crate::test_temp_dir;
    use crate::{STORE_MANIFEST_INVALID, STORE_PACKAGE_NOT_FOUND};

    const RX: &[u8] = b"-- rx script\nfn main() {}\n";

    /// 临时项目 + 两个已登记资产(含 .meta)。
    fn project_with_assets(dir: &std::path::Path) -> ForgeProject {
        let project = ForgeProject::with_defaults(dir.join("project"));
        project.ensure_dirs().unwrap();
        for name in ["a.rx", "b.rx"] {
            let rel = format!("Scripts/{name}");
            std::fs::write(project.content_root().join(&rel), RX).unwrap();
            assetd::meta::ensure_meta(&project.content_root(), &rel).unwrap();
        }
        project
    }

    #[test]
    fn draft_then_publish_roundtrip() {
        let base_dir = test_temp_dir("pub-rt");
        let project = project_with_assets(&base_dir);
        let registry = base_dir.join("registry");
        std::fs::create_dir_all(&registry).unwrap();
        let src = FileSource::new("official", registry);

        let mut draft = PackageManifest::minimal("acme.scripts", "脚本包", "1.0.0", PackageKind::AssetPack);
        draft.description = "两个 rx 脚本".into();
        draft.tags = vec!["脚本".into()];
        let input =
            draft_from_project(&project, &["Scripts/a.rx".into(), "Scripts/b.rx".into()], draft)
                .unwrap();
        // 自动带上同名 .meta。
        let paths: Vec<&str> = input.files.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(
            paths,
            vec!["Scripts/a.rx", "Scripts/a.rx.meta", "Scripts/b.rx", "Scripts/b.rx.meta"]
        );

        let published = publish_package(&src, &input).unwrap();
        assert_eq!(published.files.len(), 4);
        assert!(!published.created_at.is_empty() && !published.updated_at.is_empty());
        let rx_sha = forge_util::hashutil::sha256_hex(RX);
        let a = published.files.iter().find(|f| f.path == "Scripts/a.rx").unwrap();
        assert_eq!(a.sha256, rx_sha);
        assert_eq!(a.size, RX.len() as u64);

        // 从源上查得到、取得回、字节一致。
        let page = src.search(&SearchQuery::new("脚本")).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].id, "acme.scripts");
        let back = src.manifest("acme.scripts", "1.0.0").unwrap();
        assert_eq!(back.files.len(), 4);
        for f in &back.files {
            let bytes = src.blob(&f.sha256).unwrap();
            assert_eq!(forge_util::hashutil::sha256_hex(&bytes), f.sha256, "{} 取回字节须自洽", f.path);
            assert_eq!(bytes.len() as u64, f.size);
        }
        // a.rx 与 b.rx 内容相同 → 内容寻址只存一份 blob。
        let b = back.files.iter().find(|f| f.path == "Scripts/b.rx").unwrap();
        assert_eq!(b.sha256, rx_sha);

        std::fs::remove_dir_all(&base_dir).ok();
    }

    #[test]
    fn recomputes_sha_and_ignores_caller_values() {
        let base_dir = test_temp_dir("pub-sha");
        let project = project_with_assets(&base_dir);
        let registry = base_dir.join("registry");
        std::fs::create_dir_all(&registry).unwrap();
        let src = FileSource::new("official", registry);

        let mut draft = PackageManifest::minimal("acme.scripts", "脚本包", "1.0.0", PackageKind::AssetPack);
        // 调用方塞了一份撒谎的 files:必须被整体覆盖。
        draft.files = vec![FileEntry {
            path: "totally/other.rx".into(),
            sha256: "f".repeat(64),
            size: 999,
        }];
        let input = draft_from_project(&project, &["Scripts/a.rx".into()], draft).unwrap();
        let published = publish_package(&src, &input).unwrap();
        assert!(
            !published.files.iter().any(|f| f.path == "totally/other.rx"),
            "调用方传入的 files 须被重算覆盖"
        );
        assert_eq!(published.files[0].sha256, forge_util::hashutil::sha256_hex(RX));
        assert_eq!(src.blob(&"f".repeat(64)).unwrap_err().code, STORE_PACKAGE_NOT_FOUND);
        std::fs::remove_dir_all(&base_dir).ok();
    }

    #[test]
    fn rejects_missing_asset_empty_selection_and_bad_manifest() {
        let base_dir = test_temp_dir("pub-bad");
        let project = project_with_assets(&base_dir);
        let registry = base_dir.join("registry");
        std::fs::create_dir_all(&registry).unwrap();
        let src = FileSource::new("official", registry);
        let draft = || PackageManifest::minimal("acme.scripts", "脚本包", "1.0.0", PackageKind::AssetPack);

        assert_eq!(
            draft_from_project(&project, &[], draft()).unwrap_err().code,
            STORE_PUBLISH_REJECTED
        );
        let e = draft_from_project(&project, &["Scripts/none.rx".into()], draft()).unwrap_err();
        assert_eq!(e.code, STORE_PUBLISH_REJECTED);
        assert!(e.message.contains("Scripts/none.rx"), "{}", e.message);
        // 路径穿越在打包侧同样被拦。
        assert_eq!(
            draft_from_project(&project, &["../secrets.rx".into()], draft()).unwrap_err().code,
            STORE_MANIFEST_INVALID
        );
        // 一个文件都不给 → validate 判 files 为空。
        let empty = PublishInput { manifest: draft(), files: vec![] };
        assert_eq!(publish_package(&src, &empty).unwrap_err().code, STORE_MANIFEST_INVALID);
        // 磁盘上不存在的源文件 → 打包期拒绝。
        let ghost = PublishInput {
            manifest: draft(),
            files: vec![("a.rx".into(), base_dir.join("no-such.rx"))],
        };
        assert_eq!(publish_package(&src, &ghost).unwrap_err().code, STORE_PUBLISH_REJECTED);

        std::fs::remove_dir_all(&base_dir).ok();
    }
}
