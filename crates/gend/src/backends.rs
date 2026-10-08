//! 后端注册表(08 §6.2 适配器接口):capabilities + generate;可插拔。

use serde_json::Value;

use crate::config::GenConfig;
use crate::keystore::Keystore;
use crate::{
    mock::LocalMock, remote::RemoteOpenAi, GenError, Result, GEN_BACKEND_NOT_CONFIGURED,
    GEN_BAD_PARAMS, GEN_UNSUPPORTED,
};

/// 画幅(D-045):square 沿用 size 边长;landscape/portrait 为 3:2 原生尺寸(1536x1024 / 1024x1536),
/// 设计稿与场景图需要非正方形画布,降采样到正方形会把构图挤坏。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Aspect {
    #[default]
    Square,
    Landscape,
    Portrait,
}

impl Aspect {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "square" => Some(Aspect::Square),
            "landscape" => Some(Aspect::Landscape),
            "portrait" => Some(Aspect::Portrait),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Aspect::Square => "square",
            Aspect::Landscape => "landscape",
            Aspect::Portrait => "portrait",
        }
    }

    /// 输出像素尺寸(square 用调用方给的边长)。
    pub fn dims(self, square_size: u32) -> (u32, u32) {
        match self {
            Aspect::Square => (square_size, square_size),
            Aspect::Landscape => (1536, 1024),
            Aspect::Portrait => (1024, 1536),
        }
    }
}

/// 画幅名白名单(进能力面)。
pub const ASPECTS: [&str; 3] = ["square", "landscape", "portrait"];

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
    /// 画幅(缺省 square = 按 size 出正方形,行为与旧版一致)。
    pub aspect: Aspect,
    /// 画质档(low|medium|high|auto;None = 后端缺省)。
    pub quality: Option<String>,
    /// 背景(transparent|opaque|auto;None = 后端缺省)。
    pub background: Option<String>,
}

impl GenRequest {
    /// 旧形态构造(正方形、无画质/背景偏好)。
    pub fn square(prompt: impl Into<String>, negative_prompt: Option<String>, size: u32, seed: u64, n: u32) -> Self {
        GenRequest {
            prompt: prompt.into(),
            negative_prompt,
            size,
            seed,
            n,
            aspect: Aspect::Square,
            quality: None,
            background: None,
        }
    }

    pub fn dims(&self) -> (u32, u32) {
        self.aspect.dims(self.size)
    }
}

/// 改图请求(img2img,D-045):参考图 + 可选蒙版(透明像素 = 允许重绘区,OpenAI images/edits 语义)。
#[derive(Debug, Clone)]
pub struct EditRequest {
    pub prompt: String,
    /// 参考图(PNG/JPEG 字节,首张为主图,≥1 张)。
    pub images: Vec<Vec<u8>>,
    /// 蒙版 PNG(与主图同尺寸;alpha=0 处可重绘)。
    pub mask: Option<Vec<u8>>,
    pub aspect: Aspect,
    pub seed: u64,
    pub n: u32,
    pub quality: Option<String>,
    pub background: Option<String>,
}

impl EditRequest {
    pub fn validate(&self) -> Result<()> {
        if self.prompt.trim().is_empty() {
            return Err(GenError::new(GEN_BAD_PARAMS, "改图 prompt 不可空"));
        }
        if self.images.is_empty() {
            return Err(GenError::new(GEN_BAD_PARAMS, "改图至少需要一张参考图"));
        }
        if self.n == 0 || self.n > MAX_BATCH {
            return Err(GenError::new(GEN_BAD_PARAMS, format!("n 须 1..={MAX_BATCH},实: {}", self.n)));
        }
        Ok(())
    }
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
    /// 改图(img2img)。缺省如实报不支持(I-5,不拿文生图冒充改图)。
    fn edit(&self, _req: &EditRequest, _cfg: &GenConfig, _keys: &Keystore) -> Result<Vec<GenCandidate>> {
        Err(GenError::new(
            GEN_UNSUPPORTED,
            format!("后端 {} 不支持改图(img2img)", self.id()),
        ))
    }
}

/// 能力面 kinds 是否含该 kind。
pub fn supports_kind(b: &dyn GenBackend, kind: &str) -> bool {
    b.capabilities()["kinds"]
        .as_array()
        .map(|ks| ks.iter().any(|k| k == kind))
        .unwrap_or(false)
}

/// 后端解析(自 gen-image-mcp 上移,供 agentd 复用):指定 id → 须已配置;缺省 → 注册表序首个
/// 「已配置且支持 kind」者;全无 → GEN_BACKEND_NOT_CONFIGURED。
pub fn resolve_backend(
    backend: Option<&str>,
    kind: &str,
    cfg: &GenConfig,
    keys: &Keystore,
) -> Result<Box<dyn GenBackend>> {
    if let Some(id) = backend.map(str::trim).filter(|s| !s.is_empty()) {
        let b = find(id).ok_or_else(|| GenError::new(GEN_BAD_PARAMS, format!("未知后端 id: {id}")))?;
        if !b.configured(cfg, keys) {
            return Err(GenError::new(
                GEN_BACKEND_NOT_CONFIGURED,
                format!("后端未配置或不可用: {id}"),
            ));
        }
        return Ok(b);
    }
    registry()
        .into_iter()
        .find(|b| b.configured(cfg, keys) && supports_kind(b.as_ref(), kind))
        .ok_or_else(|| {
            GenError::new(
                GEN_BACKEND_NOT_CONFIGURED,
                format!("无支持 {kind} 的已配置生成后端(data/gen-backends.json 缺 enabled 条目)"),
            )
        })
}

/// 注册表(wave.1 两条目)。**顺序即缺省优先级**:真实远程后端排在占位生成器之前,
/// 两者同时 enabled 时缺省走真实出图,占位只兜远程不具备的能力(texture-set/variations)。
pub fn registry() -> Vec<Box<dyn GenBackend>> {
    let mut backends = builtin_registry();
    if let Ok(profiles) = crate::profiles::Profiles::load() {
        backends.extend(profiles.profiles.iter().filter_map(crate::profiles::image_backend));
    }
    backends
}

pub fn builtin_registry() -> Vec<Box<dyn GenBackend>> {
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
