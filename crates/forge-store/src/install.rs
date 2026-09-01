//! 安装 / 卸载 / 已装清单 / 更新检查。
//!
//! 安装照 `gend::accept::accept_asset` 的三步法:**staging 命名副本 → `import_assets`
//! → 覆写 provenance**。不自己拷文件进 `Content/`,一律走 assetd 导入链,这样 `.meta`
//! /GUID/缓存键/引用图全都由同一条链维护,商店装进来的资产与手工导入的资产没有二等公民。
//!
//! provenance 契约(08 Errata E-08-001):`origin = "store-install"`,`detail` 为 camelCase
//! 结构化对象 `{ sourceId, sourceName?, packageId, packageVersion, fileSha256, license,
//! publisher, installedAt }`——「这份资产从哪来、什么许可、谁发的」必须可追。
//!
//! 诚实纪律:
//! - 校验**绝不跳过**:字节 sha256 与长度双验,任一不符即 `STORE_CHECKSUM_MISMATCH`;
//! - 付费包(`pricing.amount > 0`)显式 `STORE_PAYMENT_REQUIRED`,不当免费装;
//! - 任一文件落地失败 → 回滚本次新落地的文件与 `.meta`,**不留半装状态**
//!   (回滚只删本次新增的,不碰安装前就存在的同名资产);
//! - 卸载遇引用阻断如实回填 `blocked_by_refs`,**不强删**(force 由上层 Proposal 门把关)。
//!
//! 本波已知边界(如实登记,未做):
//! - asset-pack 只收 assetd 能识别扩展名的文件(`.meta` 侧车除外),含其他文件的包在
//!   resolve 阶段就显式拒绝,不做「跳过不认识的文件」的静默降级;
//! - `force` 重装到**不同版本**时,旧版本遗留的同包资产不会被自动清理。

use std::path::{Path, PathBuf};

use assetd::import::import_assets;
use assetd::meta::{MetaDoc, Provenance};
use assetd::ops::delete_assets;
use assetd::project::ForgeProject;
use assetd::{meta_path_for, AssetType};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::manifest::{compare_versions, safe_rel_path, PackageKind, PackageManifest};
use crate::registry::SourcesConfig;
use crate::skillpkg::parse_frontmatter;
use crate::source::{open_source, RegistrySource, SourceConfig};
use crate::{
    parse_err, ser_err, Result, StoreError, STORE_ALREADY_INSTALLED, STORE_CHECKSUM_MISMATCH,
    STORE_DEPENDENCY_UNRESOLVED, STORE_MANIFEST_INVALID, STORE_NOT_INSTALLED,
    STORE_PAYMENT_REQUIRED, STORE_SOURCE_NOT_FOUND,
};

/// 商店安装写入 `.meta` 的 provenance origin(08 Errata E-08-001)。
pub const PROVENANCE_ORIGIN: &str = "store-install";
/// 已装清单(项目级)。
pub const INSTALLED_DB_REL: &str = ".forge/store/installed.json";
/// 下载暂存区(项目级;安装结束即清理)。
pub const STAGING_REL: &str = ".forge/tmp/store";

/// 单条安装记录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    pub source_id: String,
    pub package_id: String,
    pub version: String,
    /// "asset-pack" | "skill"。
    pub kind: String,
    /// 落地的 Content 相对路径(asset-pack)。
    #[serde(default)]
    pub asset_paths: Vec<String>,
    /// 落地的技能名(skill)。
    #[serde(default)]
    pub skill_names: Vec<String>,
    pub installed_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledDoc {
    #[serde(default)]
    installed: Vec<InstallRecord>,
}

/// 已装清单(`<project>/.forge/store/installed.json`)。
pub struct InstalledDb {
    path: PathBuf,
    records: Vec<InstallRecord>,
}

impl InstalledDb {
    /// 读;缺文件 = 空清单(不算错误)。解析失败显式报错(不当成空,否则会重复安装)。
    pub fn load(project: &ForgeProject) -> Result<Self> {
        let path = project.root.join(INSTALLED_DB_REL);
        let records = match std::fs::read_to_string(&path) {
            Ok(text) => {
                serde_json::from_str::<InstalledDoc>(&text)
                    .map_err(|e| parse_err("installed.json", e))?
                    .installed
            }
            Err(_) => Vec::new(),
        };
        Ok(InstalledDb { path, records })
    }

    /// 原子落盘。
    pub fn save(&self) -> Result<()> {
        let mut doc = InstalledDoc { installed: self.records.clone() };
        doc.installed
            .sort_by(|a, b| (&a.source_id, &a.package_id).cmp(&(&b.source_id, &b.package_id)));
        crate::write_json_atomic(&self.path, "installed.json", &doc)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> &[InstallRecord] {
        &self.records
    }

    pub fn find(&self, source_id: &str, package_id: &str) -> Option<&InstallRecord> {
        self.records
            .iter()
            .find(|r| r.source_id == source_id && r.package_id == package_id)
    }

    /// 写入/覆盖一条(同 source+package 视为同一装机体)。
    pub fn upsert(&mut self, rec: InstallRecord) {
        match self
            .records
            .iter_mut()
            .find(|r| r.source_id == rec.source_id && r.package_id == rec.package_id)
        {
            Some(old) => *old = rec,
            None => self.records.push(rec),
        }
    }

    /// 移除一条,返回被移除的记录。
    pub fn remove(&mut self, source_id: &str, package_id: &str) -> Option<InstallRecord> {
        let i = self
            .records
            .iter()
            .position(|r| r.source_id == source_id && r.package_id == package_id)?;
        Some(self.records.remove(i))
    }
}

/// 安装进度回调(供长任务上报;phase ∈ "resolve"|"download"|"verify"|"import"|"record")。
pub type ProgressFn<'a> = &'a (dyn Fn(&str, u32, u32) + Send + Sync);

/// 安装选项。
pub struct InstallOptions<'a> {
    /// asset-pack 落 `Content/<dest>`;缺省按扩展名分流(Meshes/Textures/...)。
    pub dest_folder: Option<&'a str>,
    /// skill 包落地根(通常 `<workspace>/skills`)。
    pub skills_root: &'a Path,
    /// 已装同版本时是否重装。
    pub force: bool,
    pub progress: Option<ProgressFn<'a>>,
    /// 源展示名(写进 provenance.detail.sourceName)。
    /// 偏离指令的小扩展:契约里 `sourceName` 是可选字段,但 `install_package` 只拿得到
    /// sourceId;为免为一个展示字段多打一次 `index()` 请求,改由调用方按需注入。
    pub source_name: Option<&'a str>,
}

impl<'a> InstallOptions<'a> {
    /// 最简选项(缺省分流 / 不强制 / 无进度回调)。
    pub fn new(skills_root: &'a Path) -> Self {
        InstallOptions {
            dest_folder: None,
            skills_root,
            force: false,
            progress: None,
            source_name: None,
        }
    }
}

fn report(opts: &InstallOptions, phase: &str, done: u32, total: u32) {
    if let Some(p) = opts.progress {
        p(phase, done, total);
    }
}

/// 扩展名 → 缺省落地目录(与 `ForgeProject::ensure_dirs` 建的目录树对齐)。
pub fn default_dest_folder(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "gltf" | "glb" => "Meshes",
        "png" | "jpg" | "jpeg" => "Textures",
        "rxmat" => "Materials",
        "rxscene" => "Scenes",
        "rx" | "rxgraph" => "Scripts",
        _ => "Misc",
    }
}

fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

/// `.meta` 侧车:随包分发供检视,但**不作为资产导入**——GUID 由本地导入链新建,
/// 复用发布方的 GUID 会让两个项目的引用图撞车。
fn is_meta_sidecar(path: &str) -> bool {
    path.ends_with(".meta")
}

/// 安装一个包(包清单由调用方先从源上取到)。
pub fn install_package(
    project: &ForgeProject,
    src: &dyn RegistrySource,
    source_id: &str,
    manifest: &PackageManifest,
    opts: &InstallOptions,
) -> Result<InstallRecord> {
    // ---------- 1. resolve ----------
    report(opts, "resolve", 0, 1);
    manifest.validate()?;
    if let Some(p) = manifest.pricing.as_ref() {
        if p.amount > 0.0 {
            return Err(StoreError::new(
                STORE_PAYMENT_REQUIRED,
                format!(
                    "包 {} 标价 {} {},本波无支付通道,显式拒绝安装(不按免费包处理)",
                    manifest.id, p.amount, p.currency
                ),
            ));
        }
    }
    let mut db = InstalledDb::load(project)?;
    if let Some(rec) = db.find(source_id, &manifest.id) {
        if rec.version == manifest.version && !opts.force {
            return Err(StoreError::new(
                STORE_ALREADY_INSTALLED,
                format!("{} 已安装同版本 {}(force=true 可重装)", manifest.id, rec.version),
            ));
        }
    }
    let missing: Vec<String> = manifest
        .dependencies
        .iter()
        .filter(|d| {
            !db.list().iter().any(|r| {
                r.package_id == d.id
                    && compare_versions(&r.version, &d.version) != std::cmp::Ordering::Less
            })
        })
        .map(|d| format!("{}@>={}", d.id, d.version))
        .collect();
    if !missing.is_empty() {
        return Err(StoreError::new(
            STORE_DEPENDENCY_UNRESOLVED,
            format!("包 {} 的依赖未满足:{}", manifest.id, missing.join(", ")),
        ));
    }
    if manifest.kind == PackageKind::AssetPack {
        // 早失败:不认识的扩展名等下载完再炸只是浪费带宽,也给不出可行动的提示。
        let bad: Vec<&str> = manifest
            .files
            .iter()
            .filter(|f| {
                !is_meta_sidecar(&f.path) && AssetType::from_extension(&ext_of(&f.path)).is_none()
            })
            .map(|f| f.path.as_str())
            .collect();
        if !bad.is_empty() {
            return Err(StoreError::new(
                STORE_MANIFEST_INVALID,
                format!(
                    "asset-pack 含 assetd 不识别的扩展名(本波不支持任意文件随包落地):{}",
                    bad.join(", ")
                ),
            ));
        }
    }
    report(opts, "resolve", 1, 1);

    // ---------- 2. download ----------
    let total = manifest.files.len() as u32;
    let mut payload: Vec<(&crate::manifest::FileEntry, Vec<u8>)> =
        Vec::with_capacity(manifest.files.len());
    for (i, f) in manifest.files.iter().enumerate() {
        let bytes = src.blob(&f.sha256)?;
        payload.push((f, bytes));
        report(opts, "download", i as u32 + 1, total);
    }

    // ---------- 3. verify(绝不跳过)----------
    for (i, (f, bytes)) in payload.iter().enumerate() {
        if bytes.len() as u64 != f.size {
            return Err(StoreError::new(
                STORE_CHECKSUM_MISMATCH,
                format!(
                    "{} 长度不符:清单声明 {} 字节,实收 {} 字节",
                    f.path,
                    f.size,
                    bytes.len()
                ),
            ));
        }
        let got = forge_util::hashutil::sha256_hex(bytes);
        if got != f.sha256 {
            return Err(StoreError::new(
                STORE_CHECKSUM_MISMATCH,
                format!("{} 校验和不符:清单声明 {},实算 {}", f.path, f.sha256, got),
            ));
        }
        report(opts, "verify", i as u32 + 1, total);
    }

    // ---------- 4. import ----------
    let installed_at = forge_util::timeutil::utc_now_iso8601();
    let staging = project.root.join(STAGING_REL).join(&manifest.id);
    let mut asset_paths = Vec::new();
    let mut skill_names = Vec::new();
    match manifest.kind {
        PackageKind::AssetPack => {
            let r = import_asset_pack(
                project,
                manifest,
                source_id,
                opts,
                &payload,
                &staging,
                &installed_at,
            );
            std::fs::remove_dir_all(&staging).ok();
            asset_paths = r?;
        }
        PackageKind::Skill => {
            skill_names.push(import_skill(manifest, &payload, opts.skills_root, opts.force)?);
        }
    }
    report(opts, "import", total, total);

    // ---------- 5. record ----------
    let record = InstallRecord {
        source_id: source_id.to_string(),
        package_id: manifest.id.clone(),
        version: manifest.version.clone(),
        kind: manifest.kind.as_str().to_string(),
        asset_paths,
        skill_names,
        installed_at,
    };
    db.upsert(record.clone());
    db.save()?;
    report(opts, "record", 1, 1);
    Ok(record)
}

/// asset-pack 落地:staging → `import_assets` → 覆写 provenance。失败即回滚。
fn import_asset_pack(
    project: &ForgeProject,
    manifest: &PackageManifest,
    source_id: &str,
    opts: &InstallOptions,
    payload: &[(&crate::manifest::FileEntry, Vec<u8>)],
    staging: &Path,
    installed_at: &str,
) -> Result<Vec<String>> {
    let content_root = project.content_root();
    // (dest 目录, [(staging 项目相对路径, 预期落地路径, 文件 sha)])
    let mut groups: Vec<(String, Vec<(String, String, String)>)> = Vec::new();
    // 安装前就存在的同名资产:回滚时不得误删用户原有文件。
    let mut pre_existing: Vec<String> = Vec::new();

    for (f, bytes) in payload {
        if is_meta_sidecar(&f.path) {
            continue;
        }
        // 路径安全复核(清单已验过一遍;落盘前再验一遍,红线不靠单点)。
        let rel = safe_rel_path(&f.path)?;
        let staged_abs = staging.join(&rel);
        if let Some(parent) = staged_abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&staged_abs, bytes)?;

        let file_name = crate::manifest::last_segment(&rel).to_string();
        let parent_rel = rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        let dest = match opts.dest_folder {
            // 显式目标目录时保留包内子目录结构(同名文件不会互相覆盖)。
            Some(d) => {
                let d = d.trim_matches('/');
                if parent_rel.is_empty() {
                    d.to_string()
                } else {
                    format!("{d}/{parent_rel}")
                }
            }
            None => default_dest_folder(&ext_of(&rel)).to_string(),
        };
        let expect_rel = format!("{dest}/{file_name}");
        if content_root.join(&expect_rel).exists() {
            pre_existing.push(expect_rel.clone());
        }
        let staged_rel = format!("{STAGING_REL}/{}/{}", manifest.id, rel);
        match groups.iter_mut().find(|(d, _)| d == &dest) {
            Some((_, v)) => v.push((staged_rel, expect_rel, f.sha256.clone())),
            None => groups.push((dest, vec![(staged_rel, expect_rel, f.sha256.clone())])),
        }
    }
    if groups.is_empty() {
        return Err(StoreError::new(
            STORE_MANIFEST_INVALID,
            format!("包 {} 无可落地的资产文件(只有 .meta 侧车)", manifest.id),
        ));
    }

    let mut imported: Vec<(String, String)> = Vec::new();
    let mut failure: Option<StoreError> = None;

    'groups: for (dest, entries) in &groups {
        let sources: Vec<String> = entries.iter().map(|(s, _, _)| s.clone()).collect();
        match import_assets(project, &sources, dest, None) {
            Ok(outcome) => {
                if let Some(f) = outcome.failed.first() {
                    failure = Some(StoreError::new(
                        "ASSET",
                        format!("导入失败({}): {}", f.source, f.error),
                    ));
                    break 'groups;
                }
                for (one, (_, _, sha)) in outcome.imported.iter().zip(entries.iter()) {
                    imported.push((one.asset_path.clone(), sha.clone()));
                }
            }
            Err(e) => {
                failure = Some(e.into());
                break 'groups;
            }
        }
    }

    // provenance 覆写(import 链默认 user-import;商店安装强制 store-install + 结构化 detail)。
    if failure.is_none() {
        for (asset_path, sha) in &imported {
            if let Err(e) = write_store_provenance(
                project,
                asset_path,
                sha,
                source_id,
                opts.source_name,
                manifest,
                installed_at,
            ) {
                failure = Some(e);
                break;
            }
        }
    }

    if let Some(e) = failure {
        // 回滚:只删本次新落地的文件与 .meta,安装前就存在的同名资产原样保留。
        for (asset_path, _) in &imported {
            if pre_existing.iter().any(|p| p == asset_path) {
                continue;
            }
            std::fs::remove_file(content_root.join(asset_path)).ok();
            std::fs::remove_file(meta_path_for(&content_root, asset_path)).ok();
        }
        return Err(e);
    }
    Ok(imported.into_iter().map(|(p, _)| p).collect())
}

/// 写商店 provenance(08 Errata E-08-001 契约字段,camelCase)。
fn write_store_provenance(
    project: &ForgeProject,
    asset_path: &str,
    file_sha: &str,
    source_id: &str,
    source_name: Option<&str>,
    manifest: &PackageManifest,
    installed_at: &str,
) -> Result<()> {
    let meta_path = meta_path_for(&project.content_root(), asset_path);
    let mut meta = MetaDoc::load(&meta_path)?;
    let mut detail = serde_json::Map::new();
    detail.insert("sourceId".into(), json!(source_id));
    if let Some(n) = source_name {
        detail.insert("sourceName".into(), json!(n));
    }
    detail.insert("packageId".into(), json!(manifest.id));
    detail.insert("packageVersion".into(), json!(manifest.version));
    detail.insert("fileSha256".into(), json!(file_sha));
    detail.insert(
        "license".into(),
        serde_json::to_value(&manifest.license).map_err(|e| ser_err("license", e))?,
    );
    detail.insert(
        "publisher".into(),
        serde_json::to_value(&manifest.publisher).map_err(|e| ser_err("publisher", e))?,
    );
    detail.insert("installedAt".into(), json!(installed_at));
    meta.provenance = Some(Provenance {
        origin: PROVENANCE_ORIGIN.into(),
        detail: Some(Value::Object(detail)),
    });
    meta.save(&meta_path)?;
    Ok(())
}

/// skill 包落地:`<skills_root>/<frontmatter.name>/`。返回技能名。
fn import_skill(
    manifest: &PackageManifest,
    payload: &[(&crate::manifest::FileEntry, Vec<u8>)],
    skills_root: &Path,
    force: bool,
) -> Result<String> {
    let doc_path = manifest
        .skill_doc_entry()
        .map(|f| f.path.clone())
        .ok_or_else(|| StoreError::new(STORE_MANIFEST_INVALID, "skill 包缺 SKILL.md"))?;
    let doc_bytes = payload
        .iter()
        .find(|(f, _)| f.path == doc_path)
        .map(|(_, b)| b.clone())
        .ok_or_else(|| StoreError::new(STORE_MANIFEST_INVALID, "SKILL.md 内容缺失"))?;
    let text = String::from_utf8(doc_bytes)
        .map_err(|e| StoreError::new(STORE_MANIFEST_INVALID, format!("SKILL.md 非 UTF-8: {e}")))?;
    // 只取名字:正文完整性是发布期/作者期的关口,装机期不因此拒绝第三方 skill。
    let name = parse_frontmatter(&text)?.name;

    let target = skills_root.join(&name);
    if target.exists() {
        if !force {
            return Err(StoreError::new(
                STORE_ALREADY_INSTALLED,
                format!("技能目录已存在: {}(force=true 可覆盖)", target.display()),
            ));
        }
        std::fs::remove_dir_all(&target)?;
    }
    // 包内 SKILL.md 若在子目录下,以该子目录为包根展开,避免多出一层同名目录。
    let base = safe_rel_path(&doc_path)?
        .rsplit_once('/')
        .map(|(p, _)| format!("{p}/"))
        .unwrap_or_default();
    for (f, bytes) in payload {
        let rel = safe_rel_path(&f.path)?;
        let stripped = rel.strip_prefix(&base).unwrap_or(&rel);
        let out = target.join(stripped);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, bytes)?;
    }
    Ok(name)
}

/// 卸载返回。
#[derive(Debug, Clone, Default)]
pub struct UninstallOutcome {
    pub removed_assets: Vec<String>,
    /// (assetPath, [引用方 GUID]);被引用阻断的资产不强删。
    pub blocked_by_refs: Vec<(String, Vec<String>)>,
    pub removed_skills: Vec<String>,
}

/// 卸载。asset-pack 走 `assetd::ops::delete_assets`(默认引用阻断);skill 删目录。
///
/// 偏离指令的取舍:若有资产被引用阻断,**保留** db 记录并把 `asset_paths` 收敛为仍在盘的
/// 那些——否则残留资产会脱离已装清单管理,变成谁也认不出来源的孤儿。全部删净时才移除记录。
pub fn uninstall_package(
    project: &ForgeProject,
    db: &mut InstalledDb,
    source_id: &str,
    package_id: &str,
    skills_root: &Path,
    force: bool,
) -> Result<UninstallOutcome> {
    let record = db
        .find(source_id, package_id)
        .cloned()
        .ok_or_else(|| {
            StoreError::new(
                STORE_NOT_INSTALLED,
                format!("{source_id}/{package_id} 未安装,无法卸载"),
            )
        })?;

    let mut out = UninstallOutcome::default();
    if !record.asset_paths.is_empty() {
        // 已被手工删掉的路径先滤掉(缺 .meta 会让 delete_assets 整批报错)。
        let alive: Vec<String> = record
            .asset_paths
            .iter()
            .filter(|p| meta_path_for(&project.content_root(), p).is_file())
            .cloned()
            .collect();
        if !alive.is_empty() {
            let d = delete_assets(project, &alive, force)?;
            out.removed_assets = d.deleted;
            out.blocked_by_refs = d.blocked_by_refs;
        }
    }
    for name in &record.skill_names {
        let dir = skills_root.join(name);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)?;
        }
        out.removed_skills.push(name.clone());
    }

    if out.blocked_by_refs.is_empty() {
        db.remove(source_id, package_id);
    } else {
        let mut kept = record;
        kept.asset_paths = out.blocked_by_refs.iter().map(|(p, _)| p.clone()).collect();
        kept.skill_names.clear();
        db.upsert(kept);
    }
    db.save()?;
    Ok(out)
}

/// 单包更新信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub source_id: String,
    pub package_id: String,
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    /// 查询失败原因(I-5:源不可达不得伪装成「已是最新」)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 逐条已装记录查源上最新版本。查询失败**不静默跳过**:如实回填 `error`,
/// `has_update=false` 且 `latest=current`,由上层展示「这条查不到,原因是……」。
pub fn check_updates(
    db: &InstalledDb,
    cfg: &SourcesConfig,
    token_of: &dyn Fn(&str) -> Option<String>,
) -> Vec<UpdateInfo> {
    db.list()
        .iter()
        .map(|rec| {
            let mut info = UpdateInfo {
                source_id: rec.source_id.clone(),
                package_id: rec.package_id.clone(),
                current: rec.version.clone(),
                latest: rec.version.clone(),
                has_update: false,
                error: None,
            };
            let entry = match cfg.find(&rec.source_id) {
                Some(e) if e.enabled => e,
                Some(_) => {
                    info.error = Some(format!("源 {} 已禁用", rec.source_id));
                    return info;
                }
                None => {
                    info.error = Some(
                        StoreError::new(
                            STORE_SOURCE_NOT_FOUND,
                            format!("源 {} 未在源清单中", rec.source_id),
                        )
                        .to_string(),
                    );
                    return info;
                }
            };
            let with_token = SourceConfig { token: token_of(&entry.id), ..entry.clone() };
            match open_source(&with_token).and_then(|s| s.detail(&rec.package_id)) {
                Ok(detail) => {
                    let latest = detail
                        .versions
                        .iter()
                        .fold(detail.summary.latest_version.clone(), |acc, v| {
                            if compare_versions(v, &acc) == std::cmp::Ordering::Greater {
                                v.clone()
                            } else {
                                acc
                            }
                        });
                    info.has_update =
                        compare_versions(&latest, &rec.version) == std::cmp::Ordering::Greater;
                    info.latest = latest;
                }
                Err(e) => info.error = Some(e.to_string()),
            }
            info
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{FileEntry, License, Pricing, Publisher};
    use crate::source::{FileSource, SearchQuery};
    use crate::test_temp_dir;
    use crate::STORE_PACKAGE_NOT_FOUND;

    /// 最小合法 1×1 PNG(70 字节,RGBA)。assetd 的贴图导入会 `image_dimensions` 真解码,
    /// 故必须是真 PNG;本仓不给 forge-store 加 image 依赖,所以硬编码字节。
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC,
        0xCF, 0xC0, 0x50, 0x0F, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xA9, 0x8C, 0x21, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    const TINY_RX: &[u8] = b"-- rx script\nfn main() {}\n";
    const SKILL_DOC: &str = "---\nname: store-demo\ndescription: 商店安装冒烟用技能\n---\n\n## 执行流程\n1. 做事\n\n## 输出约束\n如实\n\n## 失败回退策略\n重试\n";

    struct Fixture {
        project: ForgeProject,
        source_root: PathBuf,
        skills_root: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let base = test_temp_dir(tag);
            let project = ForgeProject::with_defaults(base.join("project"));
            project.ensure_dirs().unwrap();
            let source_root = base.join("registry");
            std::fs::create_dir_all(&source_root).unwrap();
            let skills_root = base.join("skills");
            std::fs::create_dir_all(&skills_root).unwrap();
            Fixture { project, source_root, skills_root }
        }

        fn src(&self) -> FileSource {
            FileSource::new("official", self.source_root.clone())
        }

        fn base_dir(&self) -> PathBuf {
            self.project.root.parent().unwrap().to_path_buf()
        }

        /// 发布一个包到临时源;files = [(包内路径, 字节)]。
        fn publish(
            &self,
            id: &str,
            version: &str,
            kind: PackageKind,
            files: &[(&str, &[u8])],
        ) -> PackageManifest {
            let mut m = PackageManifest::minimal(id, "测试包", version, kind);
            m.description = "冒烟用包".into();
            m.license = Some(License { id: "CC0-1.0".into(), url: None });
            m.publisher = Some(Publisher {
                id: "acme".into(),
                name: "Acme 工作室".into(),
                url: None,
            });
            let mut blobs = Vec::new();
            for (path, bytes) in files {
                let sha = forge_util::hashutil::sha256_hex(bytes);
                m.files.push(FileEntry {
                    path: (*path).to_string(),
                    sha256: sha.clone(),
                    size: bytes.len() as u64,
                });
                blobs.push((sha, bytes.to_vec()));
            }
            self.src().publish(&m, &blobs).unwrap();
            m
        }

        fn opts(&self) -> InstallOptions<'_> {
            InstallOptions::new(&self.skills_root)
        }

        fn cleanup(&self) {
            std::fs::remove_dir_all(self.base_dir()).ok();
        }
    }

    #[test]
    fn asset_pack_end_to_end_with_provenance() {
        let fx = Fixture::new("inst-e2e");
        let m = fx.publish(
            "acme.props",
            "1.0.0",
            PackageKind::AssetPack,
            &[("Textures/wood.png", TINY_PNG), ("helper.rx", TINY_RX)],
        );

        let log = std::sync::Mutex::new(Vec::<String>::new());
        let cb = |phase: &str, done: u32, total: u32| {
            log.lock().unwrap().push(format!("{phase} {done}/{total}"));
        };
        let progress: ProgressFn = &cb;
        let mut opts = fx.opts();
        opts.progress = Some(progress);
        opts.source_name = Some("官方源");

        let rec = install_package(&fx.project, &fx.src(), "official", &m, &opts).unwrap();
        assert_eq!(rec.package_id, "acme.props");
        assert_eq!(rec.kind, "asset-pack");
        assert!(rec.skill_names.is_empty());
        // 缺省按扩展名分流。
        let mut got = rec.asset_paths.clone();
        got.sort();
        assert_eq!(got, vec!["Scripts/helper.rx", "Textures/wood.png"]);
        let content = fx.project.content_root();
        assert!(content.join("Textures/wood.png").is_file());
        assert!(content.join("Scripts/helper.rx").is_file());
        assert_eq!(std::fs::read(content.join("Textures/wood.png")).unwrap(), TINY_PNG);

        // provenance 契约(08 Errata E-08-001)。
        let meta = MetaDoc::load(&meta_path_for(&content, "Textures/wood.png")).unwrap();
        let prov = meta.provenance.expect("provenance 必填");
        assert_eq!(prov.origin, PROVENANCE_ORIGIN);
        let d = prov.detail.expect("detail 必填");
        assert_eq!(d["sourceId"], "official");
        assert_eq!(d["sourceName"], "官方源");
        assert_eq!(d["packageId"], "acme.props");
        assert_eq!(d["packageVersion"], "1.0.0");
        assert_eq!(d["fileSha256"], forge_util::hashutil::sha256_hex(TINY_PNG));
        assert_eq!(d["license"]["id"], "CC0-1.0");
        assert_eq!(d["publisher"]["name"], "Acme 工作室");
        assert!(d["installedAt"].as_str().unwrap().ends_with('Z'));

        // 已装清单落盘。
        let db = InstalledDb::load(&fx.project).unwrap();
        assert_eq!(db.list().len(), 1);
        assert_eq!(db.find("official", "acme.props").unwrap().version, "1.0.0");
        assert!(fx.project.root.join(INSTALLED_DB_REL).is_file());
        // staging 已清理,不留垃圾。
        assert!(!fx.project.root.join(STAGING_REL).join("acme.props").exists());

        // 五个阶段都上报过。
        let phases = log.lock().unwrap().join(" | ");
        for p in ["resolve", "download", "verify", "import", "record"] {
            assert!(phases.contains(p), "缺 {p} 进度上报: {phases}");
        }

        // 重复安装 → 显式拒绝;force=true 可重装。
        let err = install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap_err();
        assert_eq!(err.code, STORE_ALREADY_INSTALLED);
        let mut forced = fx.opts();
        forced.force = true;
        let rec2 = install_package(&fx.project, &fx.src(), "official", &m, &forced).unwrap();
        assert_eq!(rec2.asset_paths.len(), 2);
        assert_eq!(InstalledDb::load(&fx.project).unwrap().list().len(), 1, "重装不该长第二条");

        fx.cleanup();
    }

    #[test]
    fn explicit_dest_folder_keeps_subdirs() {
        let fx = Fixture::new("inst-dest");
        let m = fx.publish(
            "acme.deep",
            "1.0.0",
            PackageKind::AssetPack,
            &[("a/b/wood.png", TINY_PNG), ("top.rx", TINY_RX)],
        );
        let mut opts = fx.opts();
        opts.dest_folder = Some("Vendor");
        let rec = install_package(&fx.project, &fx.src(), "official", &m, &opts).unwrap();
        let mut got = rec.asset_paths.clone();
        got.sort();
        assert_eq!(got, vec!["Vendor/a/b/wood.png", "Vendor/top.rx"]);
        assert!(fx.project.content_root().join("Vendor/a/b/wood.png").is_file());
        fx.cleanup();
    }

    #[test]
    fn checksum_mismatch_leaves_no_residue() {
        let fx = Fixture::new("inst-sum");
        let mut m = fx.publish(
            "acme.props",
            "1.0.0",
            PackageKind::AssetPack,
            &[("wood.png", TINY_PNG)],
        );

        // (a) 长度不符。
        let mut bad_size = m.clone();
        bad_size.files[0].size += 1;
        let err = install_package(&fx.project, &fx.src(), "official", &bad_size, &fx.opts())
            .unwrap_err();
        assert_eq!(err.code, STORE_CHECKSUM_MISMATCH);
        assert!(err.message.contains("长度不符"), "{}", err.message);

        // (b) 内容不符:直接往源里塞一个「键与内容对不上」的 blob(被污染/被中间人替换的源)。
        let fake_sha = "d".repeat(64);
        let poisoned = fx.source_root.join("blobs").join(&fake_sha[..2]);
        std::fs::create_dir_all(&poisoned).unwrap();
        std::fs::write(poisoned.join(&fake_sha), b"not the declared bytes").unwrap();
        m.files[0].sha256 = fake_sha;
        m.files[0].size = b"not the declared bytes".len() as u64;
        let err = install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap_err();
        assert_eq!(err.code, STORE_CHECKSUM_MISMATCH);
        assert!(err.message.contains("校验和不符"), "{}", err.message);

        // Content 下无残留 + 未写已装记录。
        assert!(!fx.project.content_root().join("Textures/wood.png").exists());
        assert!(!fx.project.content_root().join("Misc/wood.png").exists());
        assert!(InstalledDb::load(&fx.project).unwrap().list().is_empty());
        fx.cleanup();
    }

    #[test]
    fn paid_package_is_refused_and_deps_are_checked() {
        let fx = Fixture::new("inst-paid");
        let mut m = fx.publish(
            "acme.paid",
            "1.0.0",
            PackageKind::AssetPack,
            &[("wood.png", TINY_PNG)],
        );
        m.pricing = Some(Pricing { amount: 9.9, currency: "CNY".into() });
        let err = install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap_err();
        assert_eq!(err.code, STORE_PAYMENT_REQUIRED);
        assert!(err.message.contains("9.9"), "{}", err.message);
        // 免费(amount = 0)照常放行。
        m.pricing = Some(Pricing { amount: 0.0, currency: "CNY".into() });
        install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap();

        // 依赖未装 → 显式列出缺哪些。
        let mut dep = fx.publish(
            "acme.dependent",
            "1.0.0",
            PackageKind::AssetPack,
            &[("other.rx", TINY_RX)],
        );
        dep.dependencies = vec![crate::manifest::Dependency {
            id: "acme.missing".into(),
            version: "2.0.0".into(),
        }];
        let err =
            install_package(&fx.project, &fx.src(), "official", &dep, &fx.opts()).unwrap_err();
        assert_eq!(err.code, STORE_DEPENDENCY_UNRESOLVED);
        assert!(err.message.contains("acme.missing"), "{}", err.message);
        // 依赖改为已装的 acme.paid@1.0.0 → 放行。
        dep.dependencies = vec![crate::manifest::Dependency {
            id: "acme.paid".into(),
            version: "1.0.0".into(),
        }];
        install_package(&fx.project, &fx.src(), "official", &dep, &fx.opts()).unwrap();
        fx.cleanup();
    }

    #[test]
    fn rejects_unsupported_extension_before_download() {
        let fx = Fixture::new("inst-ext");
        let m = fx.publish(
            "acme.docs",
            "1.0.0",
            PackageKind::AssetPack,
            &[("README.txt", b"hello")],
        );
        let err = install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap_err();
        assert_eq!(err.code, STORE_MANIFEST_INVALID);
        assert!(err.message.contains("README.txt"), "{}", err.message);
        fx.cleanup();
    }

    #[test]
    fn missing_blob_on_source_fails_loudly() {
        let fx = Fixture::new("inst-noblob");
        let mut m = PackageManifest::minimal("acme.ghost", "幽灵包", "1.0.0", PackageKind::AssetPack);
        m.files = vec![FileEntry {
            path: "ghost.rx".into(),
            sha256: "e".repeat(64),
            size: 3,
        }];
        let err = install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap_err();
        assert_eq!(err.code, STORE_PACKAGE_NOT_FOUND);
        assert!(InstalledDb::load(&fx.project).unwrap().list().is_empty());
        fx.cleanup();
    }

    #[test]
    fn uninstall_removes_files_meta_and_record() {
        let fx = Fixture::new("inst-uninst");
        let m = fx.publish(
            "acme.props",
            "1.0.0",
            PackageKind::AssetPack,
            &[("wood.png", TINY_PNG), ("helper.rx", TINY_RX)],
        );
        install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap();
        let content = fx.project.content_root();
        assert!(content.join("Textures/wood.png").is_file());

        let mut db = InstalledDb::load(&fx.project).unwrap();
        let out =
            uninstall_package(&fx.project, &mut db, "official", "acme.props", &fx.skills_root, false)
                .unwrap();
        let mut removed = out.removed_assets.clone();
        removed.sort();
        assert_eq!(removed, vec!["Scripts/helper.rx", "Textures/wood.png"]);
        assert!(out.blocked_by_refs.is_empty());
        assert!(!content.join("Textures/wood.png").exists(), "源文件须被删");
        assert!(
            !meta_path_for(&content, "Textures/wood.png").exists(),
            ".meta 须被删"
        );
        assert!(db.find("official", "acme.props").is_none());
        assert!(InstalledDb::load(&fx.project).unwrap().list().is_empty(), "记录须已落盘移除");

        // 未安装的包卸载 → 显式错误。
        let err =
            uninstall_package(&fx.project, &mut db, "official", "acme.props", &fx.skills_root, false)
                .unwrap_err();
        assert_eq!(err.code, STORE_NOT_INSTALLED);
        fx.cleanup();
    }

    #[test]
    fn skill_package_lands_in_skills_root() {
        let fx = Fixture::new("inst-skill");
        let m = fx.publish(
            "acme.skill",
            "1.0.0",
            PackageKind::Skill,
            &[("SKILL.md", SKILL_DOC.as_bytes()), ("ref/notes.md", "参考".as_bytes())],
        );
        let rec = install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap();
        assert_eq!(rec.kind, "skill");
        assert_eq!(rec.skill_names, vec!["store-demo"]);
        assert!(rec.asset_paths.is_empty());
        let doc = fx.skills_root.join("store-demo").join("SKILL.md");
        assert!(doc.is_file());
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), SKILL_DOC);
        assert!(fx.skills_root.join("store-demo/ref/notes.md").is_file());

        // 同名目录已存在且非 force → 显式拒绝。
        let err = install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap_err();
        assert_eq!(err.code, STORE_ALREADY_INSTALLED);

        // 卸载删目录。
        let mut db = InstalledDb::load(&fx.project).unwrap();
        let out =
            uninstall_package(&fx.project, &mut db, "official", "acme.skill", &fx.skills_root, false)
                .unwrap();
        assert_eq!(out.removed_skills, vec!["store-demo"]);
        assert!(!fx.skills_root.join("store-demo").exists());
        fx.cleanup();
    }

    #[test]
    fn skill_in_subdir_is_flattened_to_skill_name() {
        let fx = Fixture::new("inst-skill-sub");
        let m = fx.publish(
            "acme.nested",
            "1.0.0",
            PackageKind::Skill,
            &[("pkgroot/SKILL.md", SKILL_DOC.as_bytes()), ("pkgroot/a.md", b"a")],
        );
        install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap();
        assert!(fx.skills_root.join("store-demo/SKILL.md").is_file());
        assert!(fx.skills_root.join("store-demo/a.md").is_file());
        assert!(
            !fx.skills_root.join("store-demo/pkgroot").exists(),
            "包内子目录不应多出一层"
        );
        fx.cleanup();
    }

    #[test]
    fn check_updates_reports_newer_and_failures_honestly() {
        let fx = Fixture::new("inst-upd");
        let m = fx.publish(
            "acme.props",
            "1.9.0",
            PackageKind::AssetPack,
            &[("wood.png", TINY_PNG)],
        );
        install_package(&fx.project, &fx.src(), "official", &m, &fx.opts()).unwrap();
        let db = InstalledDb::load(&fx.project).unwrap();

        let cfg = SourcesConfig {
            sources: vec![SourceConfig {
                id: "official".into(),
                name: "官方源".into(),
                base_url: crate::source::path_to_file_url(&fx.source_root),
                enabled: true,
                token: None,
            }],
        };
        // 尚无新版。
        let ups = check_updates(&db, &cfg, &|_| None);
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0].current, "1.9.0");
        assert_eq!(ups[0].latest, "1.9.0");
        assert!(!ups[0].has_update);
        assert!(ups[0].error.is_none());

        // 源上发 1.10.0(数值序,不是字符串序)。
        fx.publish("acme.props", "1.10.0", PackageKind::AssetPack, &[("wood.png", TINY_PNG)]);
        let ups = check_updates(&db, &cfg, &|_| None);
        assert!(ups[0].has_update, "1.10.0 应被认作比 1.9.0 新");
        assert_eq!(ups[0].latest, "1.10.0");
        // 搜索面也能看到该包(源自洽)。
        assert_eq!(fx.src().search(&SearchQuery::new("测试包")).unwrap().total, 1);

        // 源不在清单里 → 如实回填 error,不伪装成「已是最新」。
        let empty = SourcesConfig::default();
        let ups = check_updates(&db, &empty, &|_| None);
        assert!(!ups[0].has_update);
        assert!(ups[0].error.as_deref().unwrap().contains("STORE_SOURCE_NOT_FOUND"));

        // 源被禁用 → 同样如实标注。
        let mut disabled = cfg;
        disabled.sources[0].enabled = false;
        let ups = check_updates(&db, &disabled, &|_| None);
        assert!(ups[0].error.as_deref().unwrap().contains("禁用"));
        fx.cleanup();
    }
}
