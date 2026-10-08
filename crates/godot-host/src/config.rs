//! 启动配置(02 §6.2):engine-host 核心参数一律走 env,由监督器显式设置;Godot 自己的开关走命令行。
//! 不扫 argv:Godot 进程的 argv 里混着引擎开关,parse_port / parse_game 扫整个 argv 容易误伤。

use std::path::PathBuf;

use engine_host::{ConfigSource, RenderDriver, RenderMethod};

pub struct HostConfig {
    /// FORGE_HOST_PORT(监督器必须显式设置;与 Node 宿主的同名 env 冲突,见 02 §6.1),缺省 17810,0 = 系统分配。
    pub port: u16,
    pub project_root: Option<PathBuf>,
    pub game_scene: Option<String>,
    /// 监督器按 forge.toml / env 请求的渲染方式与驱动(实际生效值以 Godot 读回为准,§7.3 第 3 条)。
    pub requested_method: Option<RenderMethod>,
    pub requested_driver: Option<RenderDriver>,
    pub source: ConfigSource,
}

pub fn from_env() -> Result<HostConfig, String> {
    let port = match std::env::var("FORGE_HOST_PORT") {
        Ok(v) => v.parse().map_err(|_| format!("godot-host: 非法 FORGE_HOST_PORT:{v}"))?,
        Err(_) => 17810,
    };
    let project_root = std::env::var("FORGE_PROJECT_ROOT").ok().filter(|v| !v.is_empty()).map(PathBuf::from);
    let game_scene = std::env::var("FORGE_GAME_SCENE").ok().filter(|v| !v.is_empty());
    let requested_method = std::env::var("FORGE_RENDER_METHOD").ok().and_then(|v| parse_method(&v));
    let requested_driver = std::env::var("FORGE_RENDER_DRIVER").ok().and_then(|v| parse_driver(&v));
    let source = match std::env::var("FORGE_RENDER_SOURCE").as_deref() {
        Ok("forge.toml") => ConfigSource::ForgeToml,
        Ok("env") => ConfigSource::Env,
        Ok("cli") => ConfigSource::Cli,
        _ => ConfigSource::Default,
    };
    Ok(HostConfig { port, project_root, game_scene, requested_method, requested_driver, source })
}

/// forge.toml / `RS::get_current_rendering_method()` 的取值。
pub fn parse_method(s: &str) -> Option<RenderMethod> {
    match s {
        "forward_plus" => Some(RenderMethod::ForwardPlus),
        "mobile" => Some(RenderMethod::Mobile),
        "gl_compatibility" => Some(RenderMethod::GlCompatibility),
        _ => None,
    }
}

/// forge.toml / `RS::get_current_rendering_driver_name()` 的取值(opengl3 的 angle / es 变体都归 opengl3)。
pub fn parse_driver(s: &str) -> Option<RenderDriver> {
    match s {
        "d3d12" => Some(RenderDriver::D3d12),
        "vulkan" => Some(RenderDriver::Vulkan),
        s if s.starts_with("opengl3") => Some(RenderDriver::Opengl3),
        _ => None,
    }
}

pub fn method_str(m: RenderMethod) -> &'static str {
    match m {
        RenderMethod::ForwardPlus => "forward_plus",
        RenderMethod::Mobile => "mobile",
        RenderMethod::GlCompatibility => "gl_compatibility",
    }
}

pub fn driver_str(d: RenderDriver) -> &'static str {
    match d {
        RenderDriver::D3d12 => "d3d12",
        RenderDriver::Vulkan => "vulkan",
        RenderDriver::Opengl3 => "opengl3",
    }
}
