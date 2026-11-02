//! General scene-driven Vulkan renderer. RXS-0408..0411.
//! Assisted-by: Codex:GPT-6.
//! Matrices are column-major, distances metres, colors linear RGB, punctual
//! intensity candela. Output is sRGB RGBA8; all lighting/compositing is HDR.

mod scene;
mod session;
mod shaders;
mod types;

pub use session::RendererSession;
pub use types::*;

#[cfg(test)]
mod tests;
