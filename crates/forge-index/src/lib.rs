//! forge-index — F10 语义索引库。
//!
//! 职责:把项目内全部素材(Content/ 资产、.rxscene 实体、.rxgraph 节点图、
//! .rx 导出函数、工作区根 *.md 文档)抽取为统一 `IndexDoc`,建 BM25 词法索引
//! (零新增依赖,中文双字切分),可选经 `Embedder` trait 注入远程向量档,
//! 检索时词法/向量 RRF 融合。档位显式标注(I-5 不静默回退):
//! tier = "lexical"(未配 embedding) | "hybrid"(已配且向量就绪)。
//! 持久化 `<project>/.forge/cache/index/`,增量按 content_hash 只重算变更项。

pub mod build;
pub mod doc;
pub mod extract;
pub mod lexical;
pub mod search;
pub mod store;
pub mod vector;

use std::fmt;

/// forge-index 统一错误(结构化;工具层映射为 isError,不 panic)。
#[derive(Debug)]
pub struct IndexError {
    pub code: &'static str,
    pub message: String,
}

impl IndexError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        IndexError { code, message: message.into() }
    }
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for IndexError {}

impl From<std::io::Error> for IndexError {
    fn from(e: std::io::Error) -> Self {
        IndexError::new("IO", e.to_string())
    }
}

impl From<assetd::AssetError> for IndexError {
    fn from(e: assetd::AssetError) -> Self {
        IndexError::new("ASSET", format!("[{}] {}", e.code, e.message))
    }
}

pub type Result<T> = std::result::Result<T, IndexError>;

/// SHA-256 hex(与 assetd 缓存键同源实现)。
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = rurix_pkg::sha256::Sha256::new();
    h.update(bytes);
    rurix_pkg::sha256::hex(&h.finalize())
}
