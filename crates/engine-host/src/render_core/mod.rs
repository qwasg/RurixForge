//! 后端中立的 CPU 渲染逻辑(02 §3):相机、拾取、精灵变换、屏外裁剪、资产解码缓存。
//!
//! 本模块不 `use rurix_rt`(02 §3 P3);原位置(viewport.rs / modelrender.rs)以
//! `pub use` / `pub(crate) use` 再导出,调用点不改。函数体逐字搬入(P1),浮点运算次序不变。

pub(crate) mod assets;
pub(crate) mod camera;
pub(crate) mod cull;
pub(crate) mod delta;
pub(crate) mod env;
pub(crate) mod extract3d;
pub(crate) mod list;
pub(crate) mod math;
pub(crate) mod model;
pub(crate) mod particles;
pub(crate) mod pick;
pub(crate) mod sprite;
pub(crate) mod text;

// 金值对照经 viewport 原路径调用被测函数,其中部分 re-export 只在 backend-rurix 下存在。
#[cfg(all(test, feature = "backend-rurix"))]
mod golden;
