//! registry 源抽象与两个实现:`FileSource`(file:// 本地目录)/ `HttpSource`(http(s) 远程)。
//!
//! **阻塞式纪律**:`HttpSource` 用 `ureq` 同步调用(与 `gend::media` / `gend::embed` 同族),
//! 异步调用方(agentd / MCP)必须自套 `spawn_blocking`,本库不引 async 运行时。
//!
//! **R-5 红线**:私有源 token 只进 `Authorization: Bearer` 头;任何错误消息、Debug 输出、
//! 落盘配置都不得回显 token 或请求头(`SourceConfig` 的 token 字段 `#[serde(skip)]`,
//! `HttpSource` 手写 Debug 脱敏)。
//!
//! **响应体上限**:远程源不可信,JSON 8MiB / blob 256MiB 封顶。超限显式报错而非
//! 静默截断(截断会让 sha256 校验以「校验失败」的形式误导排障方向)。
//!
//! `FileSource` 磁盘布局:
//! ```text
//! <root>/index.json                       SourceIndex + { "packages": [PackageSummary...] }
//! <root>/packages/<pkgId>/<version>.json  PackageManifest
//! <root>/blobs/<sha256 前 2 位>/<sha256>   blob 字节
//! ```

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::manifest::{
    compare_versions, is_sha256_hex, License, PackageKind, PackageManifest, Pricing, Publisher,
};
use crate::{
    parse_err, ser_err, Result, StoreError, STORE_MANIFEST_INVALID, STORE_PACKAGE_NOT_FOUND,
    STORE_PUBLISH_REJECTED, STORE_SOURCE_NOT_FOUND, STORE_SOURCE_UNREACHABLE,
    STORE_VERSION_NOT_FOUND,
};

/// 源协议版本(index.json `protocol`;不匹配时由上层决定是否降级展示)。
pub const PROTOCOL_VERSION: u32 = 1;
/// 远程 JSON 响应体上限(8MiB)。
pub const JSON_BODY_MAX: u64 = 8 * 1024 * 1024;
/// 远程 blob 响应体上限(256MiB)。
pub const BLOB_BODY_MAX: u64 = 256 * 1024 * 1024;
/// 远程请求超时。
pub const HTTP_TIMEOUT_SECS: u64 = 30;
/// 搜索缺省 / 最大页大小。
pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 200;

fn default_true() -> bool {
    true
}

fn default_protocol() -> u32 {
    PROTOCOL_VERSION
}

/// 源配置(token 由调用方从 keystore 取后注入,**不随本结构落盘**)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 私有源访问令牌。`skip` = 永不进 store-sources.json(R-5)。
    #[serde(skip)]
    pub token: Option<String>,
}

/// 源索引头(index.json 的非 packages 部分)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceIndex {
    pub name: String,
    pub protocol: u32,
    pub package_count: u64,
    pub updated_at: String,
}

/// index.json 全文(索引头 + 摘要清单)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexFile {
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_protocol")]
    pub protocol: u32,
    #[serde(default)]
    pub package_count: u64,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub packages: Vec<PackageSummary>,
}

impl IndexFile {
    pub fn new(name: &str) -> Self {
        IndexFile {
            name: name.to_string(),
            protocol: PROTOCOL_VERSION,
            package_count: 0,
            updated_at: forge_util::timeutil::utc_now_iso8601(),
            packages: Vec::new(),
        }
    }

    pub fn head(&self) -> SourceIndex {
        SourceIndex {
            name: self.name.clone(),
            protocol: self.protocol,
            package_count: self.package_count,
            updated_at: self.updated_at.clone(),
        }
    }
}

/// 搜索请求(page 从 1 起;page=0 视作 1,page_size=0 用缺省)。
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub q: String,
    pub kind: Option<PackageKind>,
    pub page: u32,
    pub page_size: u32,
}

impl SearchQuery {
    pub fn new(q: &str) -> Self {
        SearchQuery { q: q.to_string(), kind: None, page: 1, page_size: DEFAULT_PAGE_SIZE }
    }

    /// 归一化后的 (page, page_size)。
    pub fn normalized(&self) -> (u32, u32) {
        let page = self.page.max(1);
        let size = if self.page_size == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            self.page_size.min(MAX_PAGE_SIZE)
        };
        (page, size)
    }
}

/// 搜索结果页。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    #[serde(default)]
    pub items: Vec<PackageSummary>,
}

/// 包摘要(列表页;不含 files/dependencies 等重字段)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageSummary {
    pub id: String,
    pub name: String,
    pub kind: PackageKind,
    #[serde(default)]
    pub description: String,
    pub latest_version: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub license: Option<License>,
    #[serde(default)]
    pub pricing: Option<Pricing>,
    #[serde(default)]
    pub publisher: Option<Publisher>,
    /// 缩略图 blob 的 sha256(不是 URL;内容寻址,离线可取)。
    #[serde(default)]
    pub thumbnail: Option<String>,
}

impl PackageSummary {
    pub fn from_manifest(m: &PackageManifest) -> Self {
        PackageSummary {
            id: m.id.clone(),
            name: m.name.clone(),
            kind: m.kind,
            description: m.description.clone(),
            latest_version: m.version.clone(),
            tags: m.tags.clone(),
            license: m.license.clone(),
            pricing: m.pricing.clone(),
            publisher: m.publisher.clone(),
            thumbnail: m.preview.as_ref().and_then(|p| p.thumbnail.clone()),
        }
    }

    /// 关键词命中(name / id / description / tags 小写子串)。
    pub fn matches(&self, q: &str) -> bool {
        if q.is_empty() {
            return true;
        }
        let needle = q.to_lowercase();
        self.name.to_lowercase().contains(&needle)
            || self.id.to_lowercase().contains(&needle)
            || self.description.to_lowercase().contains(&needle)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&needle))
    }
}

/// 包详情(摘要 + 全部版本,新版在前)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDetail {
    pub summary: PackageSummary,
    #[serde(default)]
    pub versions: Vec<String>,
}

/// registry 源接口(实现方须保证:任何失败都是结构化错误,不 panic、不静默空返回)。
pub trait RegistrySource {
    fn id(&self) -> &str;
    fn index(&self) -> Result<SourceIndex>;
    fn search(&self, q: &SearchQuery) -> Result<SearchPage>;
    fn detail(&self, pkg_id: &str) -> Result<PackageDetail>;
    fn manifest(&self, pkg_id: &str, version: &str) -> Result<PackageManifest>;
    fn blob(&self, sha256: &str) -> Result<Vec<u8>>;
    fn publish(&self, manifest: &PackageManifest, blobs: &[(String, Vec<u8>)]) -> Result<()>;
}

// ---------- 入参形态校验(拼路径 / 拼 URL 前必过)----------

/// 包 id 形态校验。注意 `..` 在 `[a-z0-9.-]` 字符集内**是合法字符组合**,
/// 必须单独拒绝,否则 `packages/<id>/` 可被穿越。
fn check_pkg_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(StoreError::new(STORE_PACKAGE_NOT_FOUND, "包 id 不可空"));
    }
    if id.contains("..")
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return Err(StoreError::new(
            STORE_MANIFEST_INVALID,
            format!("包 id 形态非法(只允许 [a-z0-9.-] 且不含 `..`): {id}"),
        ));
    }
    Ok(())
}

/// 版本串形态校验(要落成文件名 / URL 段)。
fn check_version(v: &str) -> Result<()> {
    if v.is_empty()
        || v.contains("..")
        || !v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err(StoreError::new(
            STORE_MANIFEST_INVALID,
            format!("版本串形态非法(只允许 [A-Za-z0-9.-] 且不含 `..`): {v}"),
        ));
    }
    Ok(())
}

/// blob 键形态校验(直接当文件名/URL 段用,必须是 64 位小写 hex)。
fn check_sha(sha: &str) -> Result<()> {
    if !is_sha256_hex(sha) {
        return Err(StoreError::new(
            STORE_MANIFEST_INVALID,
            format!("blob 键须为 64 位小写 hex: {sha}"),
        ));
    }
    Ok(())
}

/// 内存内过滤 + 分页(FileSource 与测试共用)。
fn paginate(all: Vec<PackageSummary>, q: &SearchQuery) -> SearchPage {
    let (page, size) = q.normalized();
    let mut hit: Vec<PackageSummary> = all
        .into_iter()
        .filter(|s| q.kind.map(|k| k == s.kind).unwrap_or(true))
        .filter(|s| s.matches(&q.q))
        .collect();
    // 确定性顺序(同一查询多次调用结果稳定,便于上层缓存与测试)。
    hit.sort_by(|a, b| a.id.cmp(&b.id));
    let total = hit.len() as u64;
    let start = ((page - 1) as usize).saturating_mul(size as usize);
    let items = hit.into_iter().skip(start).take(size as usize).collect();
    SearchPage { total, page, page_size: size, items }
}

// ---------- FileSource ----------

/// 本地目录源(file:// 或裸绝对路径)。开发期官方源、离线镜像、测试夹具共用此实现。
pub struct FileSource {
    id: String,
    root: PathBuf,
}

impl FileSource {
    pub fn new(id: &str, root: PathBuf) -> Self {
        FileSource { id: id.to_string(), root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    pub fn manifest_path(&self, pkg_id: &str, version: &str) -> PathBuf {
        self.root
            .join("packages")
            .join(pkg_id)
            .join(format!("{version}.json"))
    }

    pub fn blob_path(&self, sha256: &str) -> PathBuf {
        self.root.join("blobs").join(&sha256[..2]).join(sha256)
    }

    /// 读 index.json;缺文件 / 读失败 → `STORE_SOURCE_UNREACHABLE`(源不可用,不是空源)。
    pub fn read_index(&self) -> Result<IndexFile> {
        let p = self.index_path();
        let text = std::fs::read_to_string(&p).map_err(|e| {
            StoreError::new(
                STORE_SOURCE_UNREACHABLE,
                format!("源 {} 索引不可读({}): {e}", self.id, p.display()),
            )
        })?;
        serde_json::from_str(&text).map_err(|e| parse_err(&format!("源 {} index.json", self.id), e))
    }

    fn write_index(&self, idx: &IndexFile) -> Result<()> {
        crate::write_json_atomic(&self.index_path(), "index.json", idx)
    }

    /// 目录内已有版本(新版在前)。
    fn versions_of(&self, pkg_id: &str) -> Result<Vec<String>> {
        let dir = self.root.join("packages").join(pkg_id);
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => {
                return Err(StoreError::new(
                    STORE_PACKAGE_NOT_FOUND,
                    format!("源 {} 无包 {pkg_id}", self.id),
                ))
            }
        };
        let mut versions: Vec<String> = rd
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("json") {
                    return None;
                }
                p.file_stem().and_then(|s| s.to_str()).map(str::to_string)
            })
            .collect();
        versions.sort_by(|a, b| compare_versions(b, a));
        if versions.is_empty() {
            return Err(StoreError::new(
                STORE_PACKAGE_NOT_FOUND,
                format!("源 {} 的包 {pkg_id} 无任何版本清单", self.id),
            ));
        }
        Ok(versions)
    }
}

impl RegistrySource for FileSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn index(&self) -> Result<SourceIndex> {
        Ok(self.read_index()?.head())
    }

    fn search(&self, q: &SearchQuery) -> Result<SearchPage> {
        Ok(paginate(self.read_index()?.packages, q))
    }

    fn detail(&self, pkg_id: &str) -> Result<PackageDetail> {
        check_pkg_id(pkg_id)?;
        let versions = self.versions_of(pkg_id)?;
        // 摘要优先取索引条目(展示字段以索引为准);索引缺条目则回落到最新版清单。
        let summary = match self.read_index()?.packages.into_iter().find(|s| s.id == pkg_id) {
            Some(s) => s,
            None => PackageSummary::from_manifest(&self.manifest(pkg_id, &versions[0])?),
        };
        Ok(PackageDetail { summary, versions })
    }

    fn manifest(&self, pkg_id: &str, version: &str) -> Result<PackageManifest> {
        check_pkg_id(pkg_id)?;
        check_version(version)?;
        let dir = self.root.join("packages").join(pkg_id);
        if !dir.is_dir() {
            return Err(StoreError::new(
                STORE_PACKAGE_NOT_FOUND,
                format!("源 {} 无包 {pkg_id}", self.id),
            ));
        }
        let p = self.manifest_path(pkg_id, version);
        let text = std::fs::read_to_string(&p).map_err(|_| {
            StoreError::new(
                STORE_VERSION_NOT_FOUND,
                format!("源 {} 的包 {pkg_id} 无版本 {version}", self.id),
            )
        })?;
        serde_json::from_str(&text)
            .map_err(|e| parse_err(&format!("{pkg_id}@{version} 清单"), e))
    }

    fn blob(&self, sha256: &str) -> Result<Vec<u8>> {
        check_sha(sha256)?;
        let p = self.blob_path(sha256);
        std::fs::read(&p).map_err(|e| {
            StoreError::new(
                STORE_PACKAGE_NOT_FOUND,
                format!("源 {} 缺 blob {sha256}: {e}", self.id),
            )
        })
    }

    fn publish(&self, manifest: &PackageManifest, blobs: &[(String, Vec<u8>)]) -> Result<()> {
        manifest.validate()?;
        check_pkg_id(&manifest.id)?;
        check_version(&manifest.version)?;

        // 清单声明的每个 blob 必须「随发布提供」或「源上已存在」,否则包发出去也装不上。
        for f in &manifest.files {
            let provided = blobs.iter().any(|(sha, _)| sha == &f.sha256);
            if !provided && !self.blob_path(&f.sha256).is_file() {
                return Err(StoreError::new(
                    STORE_PUBLISH_REJECTED,
                    format!("清单声明的文件 {} 缺对应 blob({})", f.path, f.sha256),
                ));
            }
        }
        // 写 blob 前逐条复核 sha(发布方算错了要当场拒,不能污染内容寻址库)。
        for (sha, bytes) in blobs {
            check_sha(sha)?;
            let got = forge_util::hashutil::sha256_hex(bytes);
            if &got != sha {
                return Err(StoreError::new(
                    crate::STORE_CHECKSUM_MISMATCH,
                    format!("发布 blob 声明 {sha} 实算 {got}"),
                ));
            }
        }
        for (sha, bytes) in blobs {
            let p = self.blob_path(sha);
            if p.is_file() {
                continue; // 内容寻址:同 sha 即同字节,不重复写
            }
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&p, bytes)?;
        }
        crate::write_json_atomic(
            &self.manifest_path(&manifest.id, &manifest.version),
            "包清单",
            manifest,
        )?;

        // 更新 index.json(缺文件 = 首次发布,新建)。
        let mut idx = match self.read_index() {
            Ok(i) => i,
            Err(e) if e.code == STORE_SOURCE_UNREACHABLE => IndexFile::new(&self.id),
            Err(e) => return Err(e),
        };
        let mut summary = PackageSummary::from_manifest(manifest);
        match idx.packages.iter_mut().find(|s| s.id == manifest.id) {
            Some(old) => {
                // latest_version 只进不退(发历史版本不该把最新版顶掉)。
                if compare_versions(&manifest.version, &old.latest_version)
                    == std::cmp::Ordering::Less
                {
                    summary.latest_version = old.latest_version.clone();
                }
                *old = summary;
            }
            None => idx.packages.push(summary),
        }
        idx.packages.sort_by(|a, b| a.id.cmp(&b.id));
        idx.package_count = idx.packages.len() as u64;
        idx.updated_at = forge_util::timeutil::utc_now_iso8601();
        if idx.name.is_empty() {
            idx.name = self.id.clone();
        }
        self.write_index(&idx)
    }
}

// ---------- HttpSource ----------

/// 远程源(http/https)。**阻塞式**:异步调用方须自套 `spawn_blocking`。
pub struct HttpSource {
    id: String,
    base: String,
    /// 私有源令牌。只用于拼 Authorization 头;不进 Debug、不进错误消息、不落盘(R-5)。
    token: Option<String>,
}

/// Debug 脱敏(R-5;照 `gend::embed::RemoteEmbedder` 纪律)。
impl std::fmt::Debug for HttpSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpSource")
            .field("id", &self.id)
            .field("base", &self.base)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl HttpSource {
    pub fn new(id: &str, base_url: &str, token: Option<String>) -> Self {
        HttpSource {
            id: id.to_string(),
            base: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn index_url(&self) -> String {
        format!("{}/v1/index.json", self.base)
    }

    pub fn search_url(&self, q: &SearchQuery) -> String {
        let (page, size) = q.normalized();
        let mut url = format!(
            "{}/v1/packages?q={}&page={page}&pageSize={size}",
            self.base,
            percent_encode(&q.q)
        );
        if let Some(k) = q.kind {
            url.push_str(&format!("&kind={}", k.as_str()));
        }
        url
    }

    pub fn detail_url(&self, pkg_id: &str) -> String {
        format!("{}/v1/packages/{}", self.base, percent_encode(pkg_id))
    }

    pub fn manifest_url(&self, pkg_id: &str, version: &str) -> String {
        format!(
            "{}/v1/packages/{}/{}",
            self.base,
            percent_encode(pkg_id),
            percent_encode(version)
        )
    }

    pub fn blob_url(&self, sha256: &str) -> String {
        format!("{}/v1/blobs/{sha256}", self.base)
    }

    pub fn publish_url(&self) -> String {
        format!("{}/v1/packages", self.base)
    }

    fn agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
    }

    /// GET → 字节。404 → `STORE_PACKAGE_NOT_FOUND`;其余非 2xx / 连接失败 →
    /// `STORE_SOURCE_UNREACHABLE`。错误消息只带 URL 与状态码,**绝不带头部/令牌**。
    fn get(&self, url: &str, limit: u64, what: &str) -> Result<Vec<u8>> {
        let mut req = self.agent().get(url);
        if let Some(t) = &self.token {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        match req.call() {
            Ok(r) => read_capped(r, limit, what),
            Err(ureq::Error::Status(404, _)) => Err(StoreError::new(
                STORE_PACKAGE_NOT_FOUND,
                format!("源 {} 上不存在({what}):HTTP 404 {url}", self.id),
            )),
            Err(ureq::Error::Status(code, _)) => Err(StoreError::new(
                STORE_SOURCE_UNREACHABLE,
                format!("源 {} {what} 返回 HTTP {code}: {url}", self.id),
            )),
            Err(ureq::Error::Transport(t)) => Err(StoreError::new(
                STORE_SOURCE_UNREACHABLE,
                format!("源 {} 连接失败({what}): {t}", self.id),
            )),
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str, what: &str) -> Result<T> {
        let bytes = self.get(url, JSON_BODY_MAX, what)?;
        serde_json::from_slice(&bytes).map_err(|e| parse_err(what, e))
    }
}

impl RegistrySource for HttpSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn index(&self) -> Result<SourceIndex> {
        let idx: IndexFile = self.get_json(&self.index_url(), "源索引")?;
        Ok(idx.head())
    }

    fn search(&self, q: &SearchQuery) -> Result<SearchPage> {
        self.get_json(&self.search_url(q), "搜索结果")
    }

    fn detail(&self, pkg_id: &str) -> Result<PackageDetail> {
        check_pkg_id(pkg_id)?;
        self.get_json(&self.detail_url(pkg_id), "包详情")
    }

    fn manifest(&self, pkg_id: &str, version: &str) -> Result<PackageManifest> {
        check_pkg_id(pkg_id)?;
        check_version(version)?;
        self.get_json(&self.manifest_url(pkg_id, version), "包清单")
    }

    fn blob(&self, sha256: &str) -> Result<Vec<u8>> {
        check_sha(sha256)?;
        self.get(&self.blob_url(sha256), BLOB_BODY_MAX, "包内容")
    }

    fn publish(&self, manifest: &PackageManifest, blobs: &[(String, Vec<u8>)]) -> Result<()> {
        manifest.validate()?;
        let payload = serde_json::json!({
            "manifest": manifest,
            "blobs": blobs.iter().map(|(sha, bytes)| serde_json::json!({
                "sha256": sha,
                "base64": base64_encode(bytes),
            })).collect::<Vec<_>>(),
        });
        let body = serde_json::to_string(&payload).map_err(|e| ser_err("发布请求体", e))?;
        let mut req = self
            .agent()
            .post(&self.publish_url())
            .set("Content-Type", "application/json");
        if let Some(t) = &self.token {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        match req.send_string(&body) {
            Ok(_) => Ok(()),
            // 4xx = 源方主动拒绝(鉴权/重名/配额),与「源不可达」区分开,便于上层给出不同引导。
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                Err(StoreError::new(
                    STORE_PUBLISH_REJECTED,
                    format!("源 {} 拒绝发布:HTTP {code}", self.id),
                ))
            }
            Err(ureq::Error::Status(code, _)) => Err(StoreError::new(
                STORE_SOURCE_UNREACHABLE,
                format!("源 {} 发布返回 HTTP {code}", self.id),
            )),
            Err(ureq::Error::Transport(t)) => Err(StoreError::new(
                STORE_SOURCE_UNREACHABLE,
                format!("源 {} 发布连接失败: {t}", self.id),
            )),
        }
    }
}

/// 读响应体并封顶。`take(limit + 1)` 而非 `take(limit)`——后者会把超限响应**静默截断**成
/// 一份「校验失败」的字节,把排障引向错误方向;这里超一个字节就显式报错。
fn read_capped(resp: ureq::Response, limit: u64, what: &str) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    resp.into_reader()
        .take(limit + 1)
        .read_to_end(&mut buf)
        .map_err(|e| {
            StoreError::new(STORE_SOURCE_UNREACHABLE, format!("读{what}响应体失败: {e}"))
        })?;
    if buf.len() as u64 > limit {
        return Err(StoreError::new(
            STORE_MANIFEST_INVALID,
            format!("{what}响应体超上限 {limit} 字节"),
        ));
    }
    Ok(buf)
}

/// URL 查询串百分号编码(RFC 3986 unreserved 之外全编码)。
/// 手写而非引 urlencoding:本波不新增第三方依赖。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 标准 base64 编码(带 `=` 填充)。手写 12 行而非引 base64 crate:本波不新增第三方依赖,
/// 而 JSON 发布体需要把 blob 字节装进文本字段。
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b0 = c[0] as u32;
        let b1 = *c.get(1).unwrap_or(&0) as u32;
        let b2 = *c.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// ---------- base_url 分派 ----------

/// 本地路径 → `file://` URL(Windows 盘符形态 `file:///D:/x`)。
pub fn path_to_file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

/// `file://` URL 或裸绝对路径 → 本地根目录;不是本地形态 → None。
pub fn file_root_from_url(base_url: &str) -> Option<PathBuf> {
    let s = match base_url.strip_prefix("file://") {
        Some(rest) => {
            // file:///D:/x → /D:/x → D:/x(Windows 盘符);file:///home/x 保留前导 /。
            let t = rest.strip_prefix('/').unwrap_or(rest);
            if t.len() >= 2 && t.as_bytes()[1] == b':' {
                t.to_string()
            } else {
                rest.to_string()
            }
        }
        None => {
            let p = Path::new(base_url);
            if !p.is_absolute() {
                return None;
            }
            base_url.to_string()
        }
    };
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
}

/// 按 base_url scheme 分派源实现。无法识别 → `STORE_SOURCE_NOT_FOUND`(显式拒绝,
/// 不猜成本地路径)。
pub fn open_source(cfg: &SourceConfig) -> Result<Box<dyn RegistrySource>> {
    let url = cfg.base_url.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(Box::new(HttpSource::new(&cfg.id, url, cfg.token.clone())));
    }
    match file_root_from_url(url) {
        Some(root) => Ok(Box::new(FileSource::new(&cfg.id, root))),
        None => Err(StoreError::new(
            STORE_SOURCE_NOT_FOUND,
            format!(
                "源 {} 的 baseUrl 无法识别(须为 http(s):// / file:// / 本地绝对路径): {}",
                cfg.id, cfg.base_url
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::FileEntry;
    use crate::test_temp_dir;

    fn seed_source(root: &Path) -> (PackageManifest, Vec<u8>) {
        let bytes = b"hello store".to_vec();
        let sha = forge_util::hashutil::sha256_hex(&bytes);
        let mut m = PackageManifest::minimal("acme.props", "中世纪道具", "1.0.0", PackageKind::AssetPack);
        m.description = "木箱与酒桶".into();
        m.tags = vec!["props".into(), "中世纪".into()];
        m.files = vec![FileEntry { path: "Textures/wood.png".into(), sha256: sha.clone(), size: bytes.len() as u64 }];
        m.validate().unwrap();

        std::fs::create_dir_all(root.join("packages").join(&m.id)).unwrap();
        std::fs::write(
            root.join("packages").join(&m.id).join("1.0.0.json"),
            serde_json::to_string_pretty(&m).unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(root.join("blobs").join(&sha[..2])).unwrap();
        std::fs::write(root.join("blobs").join(&sha[..2]).join(&sha), &bytes).unwrap();

        let mut idx = IndexFile::new("测试源");
        idx.packages.push(PackageSummary::from_manifest(&m));
        idx.package_count = 1;
        std::fs::write(root.join("index.json"), serde_json::to_string_pretty(&idx).unwrap()).unwrap();
        (m, bytes)
    }

    #[test]
    fn file_source_full_chain() {
        let dir = test_temp_dir("filesrc");
        let (m, bytes) = seed_source(&dir);
        let src = FileSource::new("local", dir.clone());

        let head = src.index().unwrap();
        assert_eq!(head.name, "测试源");
        assert_eq!(head.protocol, PROTOCOL_VERSION);
        assert_eq!(head.package_count, 1);

        // 命中 name / id / description / tags 四个字段。
        for q in ["中世纪", "acme", "酒桶", "props", ""] {
            let page = src.search(&SearchQuery::new(q)).unwrap();
            assert_eq!(page.total, 1, "查询 {q:?} 应命中");
            assert_eq!(page.items[0].id, "acme.props");
        }
        // 不命中 + kind 过滤。
        assert_eq!(src.search(&SearchQuery::new("赛博朋克")).unwrap().total, 0);
        let mut q = SearchQuery::new("");
        q.kind = Some(PackageKind::Skill);
        assert_eq!(src.search(&q).unwrap().total, 0, "kind 过滤须生效");
        // 分页切片。
        let mut q = SearchQuery::new("");
        q.page = 2;
        q.page_size = 1;
        let page = src.search(&q).unwrap();
        assert_eq!((page.total, page.page, page.page_size), (1, 2, 1));
        assert!(page.items.is_empty(), "第二页应为空但 total 如实为 1");

        let d = src.detail("acme.props").unwrap();
        assert_eq!(d.versions, vec!["1.0.0"]);
        assert_eq!(d.summary.latest_version, "1.0.0");

        let got = src.manifest("acme.props", "1.0.0").unwrap();
        assert_eq!(got.id, m.id);
        assert_eq!(got.files[0].path, "Textures/wood.png");
        assert_eq!(src.blob(&got.files[0].sha256).unwrap(), bytes);

        // 缺包 / 缺版本 / 缺 blob 逐条报对码。
        assert_eq!(src.detail("acme.none").unwrap_err().code, STORE_PACKAGE_NOT_FOUND);
        assert_eq!(
            src.manifest("acme.none", "1.0.0").unwrap_err().code,
            STORE_PACKAGE_NOT_FOUND
        );
        assert_eq!(
            src.manifest("acme.props", "9.9.9").unwrap_err().code,
            STORE_VERSION_NOT_FOUND
        );
        assert_eq!(src.blob(&"b".repeat(64)).unwrap_err().code, STORE_PACKAGE_NOT_FOUND);
        // 形态非法的入参在拼路径前就被拒(目录穿越防线)。
        assert_eq!(src.detail("..").unwrap_err().code, STORE_MANIFEST_INVALID);
        assert_eq!(
            src.manifest("acme.props", "../../x").unwrap_err().code,
            STORE_MANIFEST_INVALID
        );
        assert_eq!(src.blob("../x").unwrap_err().code, STORE_MANIFEST_INVALID);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_source_missing_index_is_unreachable() {
        let dir = test_temp_dir("filesrc-empty");
        let src = FileSource::new("local", dir.join("no-such-root"));
        assert_eq!(src.index().unwrap_err().code, STORE_SOURCE_UNREACHABLE);
        assert_eq!(
            src.search(&SearchQuery::new("x")).unwrap_err().code,
            STORE_SOURCE_UNREACHABLE
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_source_publish_then_searchable() {
        let dir = test_temp_dir("filesrc-pub");
        let src = FileSource::new("local", dir.clone());
        let bytes = b"skill body".to_vec();
        let sha = forge_util::hashutil::sha256_hex(&bytes);
        let mut m = PackageManifest::minimal("acme.tool", "工具包", "1.0.0", PackageKind::Skill);
        m.files = vec![FileEntry { path: "SKILL.md".into(), sha256: sha.clone(), size: bytes.len() as u64 }];

        // 首次发布(index.json 尚不存在 → 自动新建)。
        src.publish(&m, &[(sha.clone(), bytes.clone())]).unwrap();
        let page = src.search(&SearchQuery::new("工具")).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].latest_version, "1.0.0");
        assert_eq!(src.blob(&sha).unwrap(), bytes);
        assert_eq!(src.index().unwrap().package_count, 1);

        // 发新版本 → latest 前进,版本表两条。
        let mut m2 = m.clone();
        m2.version = "1.10.0".into();
        src.publish(&m2, &[(sha.clone(), bytes.clone())]).unwrap();
        let d = src.detail("acme.tool").unwrap();
        assert_eq!(d.versions, vec!["1.10.0", "1.0.0"], "版本表须按数值序新版在前");
        assert_eq!(d.summary.latest_version, "1.10.0");
        assert_eq!(src.index().unwrap().package_count, 1, "同 id 不应重复计数");

        // 补发历史版本 → latest 不回退。
        let mut m0 = m.clone();
        m0.version = "0.9.0".into();
        src.publish(&m0, &[(sha.clone(), bytes.clone())]).unwrap();
        assert_eq!(src.detail("acme.tool").unwrap().summary.latest_version, "1.10.0");

        // blob 与声明不符 → 当场拒绝,不污染内容寻址库。
        let err = src.publish(&m, &[(sha.clone(), b"tampered".to_vec())]).unwrap_err();
        assert_eq!(err.code, crate::STORE_CHECKSUM_MISMATCH);
        // 声明了文件却不给 blob → 发布被拒。
        let mut m3 = m.clone();
        m3.version = "2.0.0".into();
        m3.files[0].sha256 = "c".repeat(64);
        assert_eq!(src.publish(&m3, &[]).unwrap_err().code, STORE_PUBLISH_REJECTED);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn http_source_builds_urls_without_network() {
        let s = HttpSource::new("official", "https://store.example.com/", Some("tok-SECRET".into()));
        assert_eq!(s.index_url(), "https://store.example.com/v1/index.json");
        assert_eq!(s.detail_url("acme.props"), "https://store.example.com/v1/packages/acme.props");
        assert_eq!(
            s.manifest_url("acme.props", "1.2.0"),
            "https://store.example.com/v1/packages/acme.props/1.2.0"
        );
        assert_eq!(
            s.blob_url(&"a".repeat(64)),
            format!("https://store.example.com/v1/blobs/{}", "a".repeat(64))
        );
        assert_eq!(s.publish_url(), "https://store.example.com/v1/packages");

        let mut q = SearchQuery::new("木 wood");
        q.page = 3;
        q.page_size = 50;
        q.kind = Some(PackageKind::AssetPack);
        assert_eq!(
            s.search_url(&q),
            "https://store.example.com/v1/packages?q=%E6%9C%A8%20wood&page=3&pageSize=50&kind=asset-pack"
        );
        // 页参数归一化(0 → 1 / 缺省页大小;超上限截到 MAX)。
        let mut q = SearchQuery::new("x");
        q.page = 0;
        q.page_size = 0;
        assert_eq!(q.normalized(), (1, DEFAULT_PAGE_SIZE));
        q.page_size = 9999;
        assert_eq!(q.normalized().1, MAX_PAGE_SIZE);

        // R-5:Debug 不得泄漏 token。
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("tok-SECRET"), "Debug 泄漏 token: {dbg}");
        assert!(dbg.contains("redacted"), "{dbg}");
    }

    #[test]
    fn open_source_dispatches_by_scheme() {
        let http = SourceConfig {
            id: "official".into(),
            name: "官方源".into(),
            base_url: "https://store.example.com".into(),
            enabled: true,
            token: None,
        };
        assert_eq!(open_source(&http).unwrap().id(), "official");

        let dir = test_temp_dir("dispatch");
        let (_m, _b) = seed_source(&dir);
        let file_cfg = SourceConfig {
            id: "local".into(),
            name: "本地".into(),
            base_url: path_to_file_url(&dir),
            enabled: true,
            token: None,
        };
        let s = open_source(&file_cfg).unwrap();
        assert_eq!(s.id(), "local");
        assert_eq!(s.index().unwrap().name, "测试源", "file:// 须解析回同一目录");

        // 裸绝对路径同样收。
        let bare = SourceConfig { base_url: dir.to_string_lossy().to_string(), ..file_cfg.clone() };
        assert_eq!(open_source(&bare).unwrap().index().unwrap().package_count, 1);

        // 无法识别的 scheme → 显式拒绝(不猜)。
        // `Box<dyn RegistrySource>` 无 Debug,故用 .err() 而非 .unwrap_err()。
        let bad = SourceConfig { base_url: "ftp://x/y".into(), ..file_cfg.clone() };
        assert_eq!(open_source(&bad).err().unwrap().code, STORE_SOURCE_NOT_FOUND);
        let rel = SourceConfig { base_url: "some/relative/dir".into(), ..file_cfg };
        assert_eq!(open_source(&rel).err().unwrap().code, STORE_SOURCE_NOT_FOUND);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_url_roundtrip_and_token_never_serialized() {
        let p = Path::new(if cfg!(windows) { r"D:\游戏引擎\registry" } else { "/tmp/registry" });
        let url = path_to_file_url(p);
        assert!(url.starts_with("file://"), "{url}");
        let back = file_root_from_url(&url).unwrap();
        assert_eq!(back, PathBuf::from(p.to_string_lossy().replace('\\', "/")));
        assert!(file_root_from_url("https://x/y").is_none());
        assert!(file_root_from_url("relative/dir").is_none());

        // R-5:token 不进落盘 JSON。
        let cfg = SourceConfig {
            id: "priv".into(),
            name: "私有源".into(),
            base_url: "https://x".into(),
            enabled: true,
            token: Some("tok-SECRET".into()),
        };
        let text = serde_json::to_string(&cfg).unwrap();
        assert!(!text.contains("tok-SECRET"), "token 落盘(R-5): {text}");
        let back: SourceConfig = serde_json::from_str(&text).unwrap();
        assert!(back.token.is_none());
        assert!(back.enabled);
    }

    #[test]
    fn encoders_are_standard() {
        assert_eq!(percent_encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(percent_encode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(percent_encode("木"), "%E6%9C%A8");
        // RFC 4648 测试向量。
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
