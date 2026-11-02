//! RurixForge 安装程序外壳的平台无关部分:安装包载荷格式、安装位置与数据位置的规则、
//! 传给内层 NSIS 安装包的参数、launcher.json 的合并。界面与 Win32 调用在 main.rs 一侧。

pub mod payload;
pub mod plan;
