//! 包清单 DTO 与校验(F11 商店包契约)。
//!
//! 清单是商店的唯一事实源:索引页、详情页、安装校验、发布回执读的都是它。故本模块
//! 只做**纯函数**校验(不碰磁盘、不联网),并把「路径安全」这条红线钉在最外层——
//! 任何来源(本地目录源 / 远程源 / 用户草稿)的清单进入安装链之前都必须先过
//! `PackageManifest::validate`,安装侧再对每条 path 复核一次 `safe_rel_path`。
//!
//! 设计取舍:不引 semver 依赖,自解析「三段数字 + 可选 `-pre` 后缀」;版本比较失败时
//! 退化为字符串序而非 panic——更新检查是提示面,不该因源上一条脏数据而中断整轮。

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::{Result, StoreError, STORE_MANIFEST_INVALID};

/// 包种类(线上形态 `asset-pack` / `skill`)。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageKind {
    #[serde(rename = "asset-pack")]
    AssetPack,
    #[serde(rename = "skill")]
    Skill,
}

impl PackageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PackageKind::AssetPack => "asset-pack",
            PackageKind::Skill => "skill",
        }
    }

    /// 查询参数 / 清单字符串 → 种类(未知 → None,调用方显式报错)。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "asset-pack" | "assetPack" => Some(PackageKind::AssetPack),
            "skill" => Some(PackageKind::Skill),
            _ => None,
        }
    }
}

/// 发布者(展示面;id 用于同名包归属判别)。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Publisher {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// 许可(id = SPDX 标识,如 "CC-BY-4.0")。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct License {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// 定价(amount = 0 视为免费;本波安装链只放行免费包)。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Pricing {
    pub amount: f64,
    pub currency: String,
}

/// 依赖(要求已安装 id 且版本 >= version)。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub id: String,
    pub version: String,
}

/// 包内单文件(path 相对包根;sha256 同时是内容寻址 blob 键)。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// 预览资源(值为 blob 的 sha256,不是 URL——预览图也走内容寻址,离线可用)。
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<String>,
}

/// 包清单。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackageManifest {
    /// 反域名式 id,如 "acme.medieval-props"(只含 `[a-z0-9.-]`)。
    pub id: String,
    /// 展示名(可中文)。
    pub name: String,
    /// 语义版本 "1.2.0"(允许 `-pre` 后缀)。
    pub version: String,
    pub kind: PackageKind,
    pub description: String,
    #[serde(default)]
    pub publisher: Option<Publisher>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub license: Option<License>,
    #[serde(default)]
    pub pricing: Option<Pricing>,
    #[serde(default)]
    pub engine_version: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    pub files: Vec<FileEntry>,
    #[serde(default)]
    pub preview: Option<Preview>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl PackageManifest {
    /// 最小清单骨架(files 留空待 `publish::publish_package` 重算填充)。
    pub fn minimal(id: &str, name: &str, version: &str, kind: PackageKind) -> Self {
        PackageManifest {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            kind,
            description: String::new(),
            publisher: None,
            tags: Vec::new(),
            license: None,
            pricing: None,
            engine_version: None,
            dependencies: Vec::new(),
            files: Vec::new(),
            preview: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// 结构校验(纯函数;安装/发布前必调)。
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(invalid("id 不可空"));
        }
        if !self
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        {
            return Err(invalid(format!("id 只允许 [a-z0-9.-]: {}", self.id)));
        }
        if parse_version(&self.version).is_none() {
            return Err(invalid(format!(
                "version 须为三段数字(可带 -pre 后缀): {}",
                self.version
            )));
        }
        if self.files.is_empty() {
            return Err(invalid(format!("包 {} 的 files 不可空", self.id)));
        }
        for f in &self.files {
            // 路径安全红线:任何一条不过,整份清单判非法(不做「跳过坏条目」的降级)。
            safe_rel_path(&f.path)?;
            if !is_sha256_hex(&f.sha256) {
                return Err(invalid(format!(
                    "files[{}].sha256 须为 64 位小写 hex: {}",
                    f.path, f.sha256
                )));
            }
        }
        if self.kind == PackageKind::Skill {
            let n = self
                .files
                .iter()
                .filter(|f| last_segment(&f.path) == "SKILL.md")
                .count();
            if n != 1 {
                return Err(invalid(format!(
                    "skill 包须恰含一个 SKILL.md(可在子目录下),实得 {n} 个"
                )));
            }
        }
        Ok(())
    }

    /// 包内 SKILL.md 条目(skill 包 `validate` 通过后必存在)。
    pub fn skill_doc_entry(&self) -> Option<&FileEntry> {
        self.files
            .iter()
            .find(|f| last_segment(&f.path) == "SKILL.md")
    }
}

fn invalid(msg: impl Into<String>) -> StoreError {
    StoreError::new(STORE_MANIFEST_INVALID, msg)
}

/// 相对路径最后一段(用于 SKILL.md 判定)。
pub fn last_segment(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// 64 位小写 hex 判定(sha256 既是校验值也是 blob 文件名,必须先验形态再拼路径)。
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// **路径安全校验(安全红线)**:包内相对路径归一化。
///
/// 规则(比 `assetd::normalize_rel` 更严,因为包内容来自第三方,不可信):
/// 正斜杠归一化;拒绝空串、拒绝含 `..`、拒绝前导 `/` 或 `\`、拒绝含 `:`(Windows 盘符
/// 与 NTFS 数据流)、拒绝任何路径段为空(含尾随斜杠)或为 `.`。
/// 返回归一化后的相对路径,调用方只能用返回值拼盘,不得再用原串。
pub fn safe_rel_path(p: &str) -> Result<String> {
    let s = p.replace('\\', "/");
    if s.is_empty() {
        return Err(invalid("包内路径不可空"));
    }
    if s.starts_with('/') {
        return Err(invalid(format!("包内路径不可为绝对路径: {p}")));
    }
    if s.contains("..") {
        return Err(invalid(format!("包内路径不可含 `..`(目录穿越): {p}")));
    }
    if s.contains(':') {
        return Err(invalid(format!("包内路径不可含 `:`(盘符/数据流): {p}")));
    }
    for seg in s.split('/') {
        if seg.is_empty() {
            return Err(invalid(format!("包内路径含空路径段: {p}")));
        }
        if seg == "." {
            return Err(invalid(format!("包内路径含 `.` 路径段: {p}")));
        }
    }
    Ok(s)
}

/// 版本解析:`x.y.z` 三段数字 + 可选 `-pre` 后缀。非法 → None。
fn parse_version(v: &str) -> Option<([u64; 3], Option<String>)> {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
        Some(_) => return None, // 尾随 '-' 无内容
        None => (v, None),
    };
    let mut it = core.split('.');
    let mut nums = [0u64; 3];
    for slot in nums.iter_mut() {
        let seg = it.next()?;
        if seg.is_empty() || !seg.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = seg.parse().ok()?;
    }
    if it.next().is_some() {
        return None;
    }
    Some((nums, pre))
}

/// 版本比较(三段数字逐段比较;数字相等时「无 pre 后缀」大于「有 pre 后缀」,
/// 同为 pre 则按字符串序)。任一侧解析失败 → 退化为整串字符串比较。
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    match (parse_version(a), parse_version(b)) {
        (Some((na, pa)), Some((nb, pb))) => match na.cmp(&nb) {
            Ordering::Equal => match (pa, pb) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(x), Some(y)) => x.cmp(&y),
            },
            other => other,
        },
        _ => a.cmp(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_sha() -> String {
        "a".repeat(64)
    }

    fn pack(kind: PackageKind, files: Vec<FileEntry>) -> PackageManifest {
        let mut m = PackageManifest::minimal("acme.medieval-props", "中世纪道具", "1.2.0", kind);
        m.files = files;
        m
    }

    fn entry(path: &str) -> FileEntry {
        FileEntry { path: path.into(), sha256: png_sha(), size: 3 }
    }

    #[test]
    fn valid_manifest_passes() {
        let m = pack(PackageKind::AssetPack, vec![entry("Textures/wood.png"), entry("mesh/a.gltf")]);
        m.validate().expect("合法清单须通过");
        // 序列化字段名 camelCase + kind 线上形态。
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["kind"], "asset-pack");
        assert!(v.get("engineVersion").is_some(), "engine_version 须序列化为 engineVersion");
        let back: PackageManifest = serde_json::from_value(v).unwrap();
        assert_eq!(back.id, m.id);
        assert_eq!(back.kind, PackageKind::AssetPack);
    }

    #[test]
    fn rejects_bad_id_version_and_empty_files() {
        let mut m = pack(PackageKind::AssetPack, vec![entry("a.png")]);
        m.id = String::new();
        assert_eq!(m.validate().unwrap_err().code, STORE_MANIFEST_INVALID);

        let mut m = pack(PackageKind::AssetPack, vec![entry("a.png")]);
        m.id = "Acme.Props".into();
        let e = m.validate().unwrap_err();
        assert_eq!(e.code, STORE_MANIFEST_INVALID);
        assert!(e.message.contains("[a-z0-9.-]"), "{}", e.message);

        for bad in ["1.2", "1.2.0.1", "v1.2.0", "1.2.x", "1.2.0-", ""] {
            let mut m = pack(PackageKind::AssetPack, vec![entry("a.png")]);
            m.version = bad.into();
            let e = m.validate().unwrap_err();
            assert_eq!(e.code, STORE_MANIFEST_INVALID, "版本 {bad:?} 应判非法");
            assert!(e.message.contains("version"), "{}", e.message);
        }
        // 合法版本形态。
        for good in ["1.2.0", "0.0.1", "10.20.30", "1.2.0-beta.1"] {
            let mut m = pack(PackageKind::AssetPack, vec![entry("a.png")]);
            m.version = good.into();
            m.validate().unwrap_or_else(|e| panic!("版本 {good:?} 应合法: {e}"));
        }

        let m = pack(PackageKind::AssetPack, vec![]);
        let e = m.validate().unwrap_err();
        assert_eq!(e.code, STORE_MANIFEST_INVALID);
        assert!(e.message.contains("files"), "{}", e.message);
    }

    #[test]
    fn rejects_unsafe_paths_and_bad_sha() {
        for bad in [
            "../etc/passwd",
            "a/../../b.png",
            "C:/x.png",
            "/abs/x.png",
            "\\abs\\x.png",
            "a//b.png",
            "./a.png",
            "a/./b.png",
            "dir/",
            "",
        ] {
            let m = pack(PackageKind::AssetPack, vec![entry(bad)]);
            let e = m.validate().unwrap_err();
            assert_eq!(e.code, STORE_MANIFEST_INVALID, "路径 {bad:?} 应判非法");
            assert!(safe_rel_path(bad).is_err(), "safe_rel_path 应拒绝 {bad:?}");
        }
        // 反斜杠归一化为正斜杠(合法相对路径)。
        assert_eq!(safe_rel_path("a\\b\\c.png").unwrap(), "a/b/c.png");
        assert_eq!(safe_rel_path("SKILL.md").unwrap(), "SKILL.md");

        let mut m = pack(PackageKind::AssetPack, vec![entry("a.png")]);
        m.files[0].sha256 = "abc123".into();
        let e = m.validate().unwrap_err();
        assert_eq!(e.code, STORE_MANIFEST_INVALID);
        assert!(e.message.contains("sha256"), "{}", e.message);
        // 大写 hex 也不收(blob 文件名须唯一形态)。
        m.files[0].sha256 = "A".repeat(64);
        assert_eq!(m.validate().unwrap_err().code, STORE_MANIFEST_INVALID);
        assert!(!is_sha256_hex(&"A".repeat(64)));
        assert!(is_sha256_hex(&"0f".repeat(32)));
    }

    #[test]
    fn skill_pack_requires_exactly_one_skill_doc() {
        // 缺 SKILL.md。
        let m = pack(PackageKind::Skill, vec![entry("readme.md")]);
        let e = m.validate().unwrap_err();
        assert_eq!(e.code, STORE_MANIFEST_INVALID);
        assert!(e.message.contains("SKILL.md"), "{}", e.message);
        // 两份 SKILL.md。
        let m = pack(
            PackageKind::Skill,
            vec![entry("SKILL.md"), entry("nested/SKILL.md")],
        );
        assert_eq!(m.validate().unwrap_err().code, STORE_MANIFEST_INVALID);
        // 子目录下一份 = 合法。
        let m = pack(
            PackageKind::Skill,
            vec![entry("my-skill/SKILL.md"), entry("my-skill/ref.md")],
        );
        m.validate().expect("子目录下的 SKILL.md 须合法");
        assert_eq!(m.skill_doc_entry().unwrap().path, "my-skill/SKILL.md");
    }

    #[test]
    fn version_compare_is_numeric_not_lexical() {
        assert_eq!(compare_versions("1.10.0", "1.9.0"), Ordering::Greater, "1.10.0 须大于 1.9.0");
        assert_eq!(compare_versions("1.9.0", "1.10.0"), Ordering::Less);
        assert_eq!(compare_versions("2.0.0", "1.99.99"), Ordering::Greater);
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
        // 预发布 < 正式。
        assert_eq!(compare_versions("1.0.0", "1.0.0-beta"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0-alpha", "1.0.0-beta"), Ordering::Less);
        // 解析失败退化为字符串序(不 panic)。
        assert_eq!(compare_versions("nightly", "nightly"), Ordering::Equal);
        assert_eq!(compare_versions("a", "b"), Ordering::Less);
    }
}
