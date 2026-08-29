//! 后端注册表(08 §6.2 适配器接口):capabilities + generate;可插拔。

use serde_json::Value;

use crate::config::GenConfig;
use crate::keystore::Keystore;
use crate::{mock::LocalMock, remote::RemoteOpenAi, Result};

/// 生成请求(text2img 主面)。
#[derive(Debug, Clone)]
pub struct GenRequest {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    /// 边长(正方形;256/512/1024)。
    pub size: u32,
    /// 基准种子(候选 i 的种子 = seed + i,由适配器派生)。
    pub seed: u64,
    /// 候选数(1..=4)。
    pub n: u32,
}

/// 生成候选(PNG 字节 + 实际种子)。
#[derive(Debug, Clone)]
pub struct GenCandidate {
    pub png_bytes: Vec<u8>,
    pub seed: u64,
}

/// 后端适配器接口(08 §6.2)。
pub trait GenBackend {
    fn id(&self) -> &str;
    /// local | remote。
    fn kind(&self) -> &str;
    /// 配置判定(D-F5-A:gen-backends.json 条目缺失 = false)。
    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool;
    /// 能力面(kinds/sizes/maxBatch 等,如实进 gen_backends_list)。
    fn capabilities(&self) -> Value;
    fn generate(&self, req: &GenRequest, cfg: &GenConfig, keys: &Keystore) -> Result<Vec<GenCandidate>>;
}

/// 注册表(wave.1 两条目)。**顺序即缺省优先级**:真实远程后端排在占位生成器之前,
/// 两者同时 enabled 时缺省走真实出图,占位只兜远程不具备的能力(texture-set/variations)。
pub fn registry() -> Vec<Box<dyn GenBackend>> {
    vec![Box::new(RemoteOpenAi), Box::new(LocalMock)]
}

/// 按 id 查适配器。
pub fn find(id: &str) -> Option<Box<dyn GenBackend>> {
    registry().into_iter().find(|b| b.id() == id)
}

/// 尺寸白名单(D-F5-A:256/512/1024,默认 512)。
pub const SIZES: [u32; 3] = [256, 512, 1024];
pub const DEFAULT_SIZE: u32 = 512;
/// maxBatch = 4(05 §7 n=1..4)。
pub const MAX_BATCH: u32 = 4;
