//! gend — Forge 生成接入共享库(F5 wave.1,08 §6.2 / 12 §5 R-5)。
//!
//! 职责:后端注册表(`local-mock` 确定性占位 + `remote-openai-compatible` 真实 HTTP
//! 适配面)、gen 配置(data/gen-backends.json)与 keystore(data/keystore.json,
//! env FORGE_GEN_API_KEY 优先,密钥值无序列化出口)、临时产物目录
//! (<project>/.forge/tmp/gen/)、gen_accept 入 assetd 管线 + provenance 写入。
//! F5 wave.2:accept_asset 泛化(origin="gen-image"|"gen-model",importSettings 透传,
//! 网格带回 .rxmesh artifact/cache_hit)+ meshFileRef 解析。
//! 本库不暴露网络端口,被 gen-image-mcp / gen-model-mcp 内嵌(照 assetd 先例)。

pub mod accept;
pub mod backends;
pub mod config;
pub mod embed;
pub mod keystore;
pub mod media;
pub mod mock;
pub mod remote;
// timeutil 已抽至 forge-util(三份逐字重复合并);重导出保 gend::timeutil 路径兼容
// (gen-image-mcp / gen-model-mcp 经此引用,不改其源码)。
pub use forge_util::timeutil;
pub mod tmpstore;
pub mod video_frames;

use std::fmt;

/// gend 统一错误(结构化 code;工具层映射为 isError:true + {error,message})。
#[derive(Debug)]
pub struct GenError {
    pub code: &'static str,
    pub message: String,
}

/// GEN_* 错误码族(11 §4)。
pub const GEN_BACKEND_NOT_CONFIGURED: &str = "GEN_BACKEND_NOT_CONFIGURED";
pub const GEN_RATE_LIMITED: &str = "GEN_RATE_LIMITED";
pub const GEN_BACKEND_ERROR: &str = "GEN_BACKEND_ERROR";
pub const GEN_FILE_NOT_FOUND: &str = "GEN_FILE_NOT_FOUND";
pub const GEN_BAD_PARAMS: &str = "GEN_BAD_PARAMS";
/// 外部可执行依赖缺失(当前仅 ffmpeg,视频截帧用)。与「后端未配置」同档:
/// 环境缺件是用户可补的配置问题,不是生成失败,更不能伪造帧糊过去(I-5)。
pub const GEN_TOOL_MISSING: &str = "GEN_TOOL_MISSING";

impl GenError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        GenError { code, message: message.into() }
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for GenError {}

impl From<std::io::Error> for GenError {
    fn from(e: std::io::Error) -> Self {
        GenError::new(GEN_BACKEND_ERROR, format!("IO: {e}"))
    }
}

impl From<assetd::AssetError> for GenError {
    fn from(e: assetd::AssetError) -> Self {
        GenError::new(GEN_BACKEND_ERROR, format!("[assetd:{}] {}", e.code, e.message))
    }
}

pub type Result<T> = std::result::Result<T, GenError>;

/// 测试用全局 env 串行锁(FORGE_GEN_API_KEY / FORGE_GEN_DATA_DIR 进程级;
/// 单实例防跨模块互踩,锁中毒容忍)。
#[cfg(test)]
pub static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// FNV-1a 64 哈希(零依赖确定性;跨进程/跨运行字节一致,供 mock 种子派生)。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 多段哈希(顺序敏感)。
pub fn hash_parts(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in *p {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
