//! forge-rig2d:Live2D 风格 2D 角色绑定核心(F-GAME-5 / D-050)。
//!
//! 纯计算、无 IO:`.rxrig` 文档模型与校验([`doc`])、预编译([`compile`])、
//! 每帧形变求值([`eval`])、运行时模拟(clip / 物理 / 自动眨眼呼吸 / animator,[`sim`])、
//! agent 辅助算法(自动网格 / 自动权重 / 模板骨架,[`autogen`])、细粒度编辑操作
//! ([`patch`])与 CPU 预览光栅([`raster`])。
//!
//! assetd(资产 IO)、engine-host(运行时与渲染)、godot-host、asset-pipeline-mcp
//! 共用本 crate——求值逻辑全仓唯一一份。

pub mod autogen;
pub mod compile;
pub mod doc;
pub mod eval;
pub mod math;
pub mod patch;
pub mod raster;
pub mod sim;

pub use compile::CompiledRig;
pub use doc::{parse_rxrig, parse_rxrig_str, validate_doc, RigDoc, RigStats};
pub use eval::{evaluate, expand_triangles, pose_from, Deformed, Pose, RigVertex};
pub use sim::{RigCommand, RigSim, RigWarn};

/// 结构化错误(I-5 不静默回退):`code` 为稳定错误码,`message` 带 JSON 路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigError {
    pub code: &'static str,
    pub message: String,
}

impl RigError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
    pub fn invalid(message: String) -> Self {
        Self::new("RIG_INVALID", message)
    }
}

impl std::fmt::Display for RigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RigError {}

pub type Result<T> = std::result::Result<T, RigError>;
