//! forge-store — F11 商店 / 个人资产库内核(wave.1 registry 内核)。
//!
//! 职责:把「商店」拆成六层纯库能力,供上层(agentd REST / MCP / IDE)组装,本库
//! 自身不开端口、不起线程、不读全局单例配置:
//! - `manifest`:包清单 DTO + 校验(**路径安全红线**钉在最外层)
//! - `source`:`RegistrySource` 源抽象 + `FileSource`(file:// 本地目录)/ `HttpSource`(http(s))
//! - `registry`:多源聚合搜索 + 源配置持久化(`<data>/store-sources.json`)
//! - `library`:内容寻址个人资产库(`<data>/store/library/`,sha256 去重 + 引用计数)
//! - `install`:安装 / 卸载 / 已装清单 / 更新检查(经 assetd 导入链落 Content/)
//! - `publish` / `skillpkg`:打包发布与 skill 包 frontmatter 解析
//!
//! 诚实纪律(I-5)贯穿全库:能力缺失、源不可达、校验不过一律返回结构化错误并保持
//! 磁盘干净(安装失败不留半装状态),**不静默降级、不伪造成功**。多源聚合时单源失败
//! 也不吞掉——错误随结果一并返回,由上层如实展示。
//!
//! 错误码分两层(与 11 §5 对齐):
//! - `STORE_*` = 契约面语义码,已在 `11_API_CONTRACTS.md §5` 登记,可跨层/跨进程暴露;
//! - 裸码(`IO` / `ASSET` / `PARSE_ERR` / `SERIALIZE`)= 库内技术性失败,沿用
//!   `assetd` / `forge-index` 同族惯例,不新造未登记的 `STORE_*`。

pub mod install;
pub mod library;
pub mod manifest;
pub mod publish;
pub mod registry;
pub mod skillpkg;
pub mod source;

use std::fmt;

/// 商店错误(11 §5 `STORE_*` 前缀;工具层映射为 isError,不 panic)。
#[derive(Debug, Clone)]
pub struct StoreError {
    pub code: &'static str,
    pub message: String,
}

impl StoreError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        StoreError { code, message: message.into() }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::new("IO", e.to_string())
    }
}

impl From<assetd::AssetError> for StoreError {
    fn from(e: assetd::AssetError) -> Self {
        StoreError::new("ASSET", format!("[{}] {}", e.code, e.message))
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

// ---------- 契约错误码(11 §5 商店族;新增须同步该表)----------

/// 源地址连不通 / 索引读不到(本地目录缺 index.json 亦属此类)。
pub const STORE_SOURCE_UNREACHABLE: &str = "STORE_SOURCE_UNREACHABLE";
/// 源未配置或 base_url scheme 无法识别。
pub const STORE_SOURCE_NOT_FOUND: &str = "STORE_SOURCE_NOT_FOUND";
/// 源上无此包(含包内容 blob 缺失)。
pub const STORE_PACKAGE_NOT_FOUND: &str = "STORE_PACKAGE_NOT_FOUND";
/// 包存在但无此版本。
pub const STORE_VERSION_NOT_FOUND: &str = "STORE_VERSION_NOT_FOUND";
/// 清单结构 / 字段 / 路径安全校验不过。
pub const STORE_MANIFEST_INVALID: &str = "STORE_MANIFEST_INVALID";
/// 下载字节的 sha256 或长度与清单声明不符(**绝不跳过校验**)。
pub const STORE_CHECKSUM_MISMATCH: &str = "STORE_CHECKSUM_MISMATCH";
/// 同源同包同版本已安装(force=true 可重装)。
pub const STORE_ALREADY_INSTALLED: &str = "STORE_ALREADY_INSTALLED";
/// 卸载 / 更新的目标未安装。
pub const STORE_NOT_INSTALLED: &str = "STORE_NOT_INSTALLED";
/// 依赖未满足(消息列出缺哪些)。
pub const STORE_DEPENDENCY_UNRESOLVED: &str = "STORE_DEPENDENCY_UNRESOLVED";
/// 付费包(本波无支付通道,显式拒绝而非当免费装)。
pub const STORE_PAYMENT_REQUIRED: &str = "STORE_PAYMENT_REQUIRED";
/// 长任务 id 不存在(上层任务表用)。
pub const STORE_TASK_NOT_FOUND: &str = "STORE_TASK_NOT_FOUND";
/// 发布被源拒绝(缺 blob / 远端 4xx)。
pub const STORE_PUBLISH_REJECTED: &str = "STORE_PUBLISH_REJECTED";

/// JSON 序列化失败 → 结构化错误(统一出口,避免各处 unwrap)。
pub(crate) fn ser_err(what: &str, e: serde_json::Error) -> StoreError {
    StoreError::new("SERIALIZE", format!("{what} 序列化失败: {e}"))
}

/// 本地 JSON 解析失败 → 结构化错误。
pub(crate) fn parse_err(what: &str, e: serde_json::Error) -> StoreError {
    StoreError::new("PARSE_ERR", format!("{what} 解析失败: {e}"))
}

/// 原子写 JSON:同目录 `.tmp` 落盘后 rename 覆盖(避免半截文件被读到)。
pub(crate) fn write_json_atomic<T: serde::Serialize>(
    path: &std::path::Path,
    what: &str,
    value: &T,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| ser_err(what, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 测试隔离目录(进程 id + 毫秒 + 自增序号;并发跑测互不踩,调用方负责清理)。
#[cfg(test)]
pub(crate) fn test_temp_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "forge-store-{tag}-{}-{}-{}",
        std::process::id(),
        forge_util::timeutil::unix_millis(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_and_conversions() {
        let e = StoreError::new(STORE_MANIFEST_INVALID, "id 非法");
        assert_eq!(format!("{e}"), "[STORE_MANIFEST_INVALID] id 非法");
        let io: StoreError = std::io::Error::new(std::io::ErrorKind::NotFound, "缺文件").into();
        assert_eq!(io.code, "IO");
        let a: StoreError = assetd::AssetError::new("NO_META", "缺 .meta: x").into();
        assert_eq!(a.code, "ASSET");
        assert!(a.message.contains("NO_META"), "assetd 原码须保留在消息里: {}", a.message);
    }

    #[test]
    fn store_codes_are_registered_prefix() {
        // 11 §5:商店族一律 STORE_ 前缀(新增码须同步该表)。
        for c in [
            STORE_SOURCE_UNREACHABLE,
            STORE_SOURCE_NOT_FOUND,
            STORE_PACKAGE_NOT_FOUND,
            STORE_VERSION_NOT_FOUND,
            STORE_MANIFEST_INVALID,
            STORE_CHECKSUM_MISMATCH,
            STORE_ALREADY_INSTALLED,
            STORE_NOT_INSTALLED,
            STORE_DEPENDENCY_UNRESOLVED,
            STORE_PAYMENT_REQUIRED,
            STORE_TASK_NOT_FOUND,
            STORE_PUBLISH_REJECTED,
        ] {
            assert!(c.starts_with("STORE_"), "{c}");
        }
    }

    #[test]
    fn atomic_write_leaves_no_tmp_and_roundtrips() {
        let dir = test_temp_dir("atomic");
        let p = dir.join("sub").join("cfg.json");
        write_json_atomic(&p, "cfg", &serde_json::json!({ "a": 1 })).unwrap();
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(back["a"], 1);
        assert!(!p.with_extension("json.tmp").exists(), "tmp 文件须已被 rename 消费");
        // 覆写同路径(Windows rename 覆盖语义)。
        write_json_atomic(&p, "cfg", &serde_json::json!({ "a": 2 })).unwrap();
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(back["a"], 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
