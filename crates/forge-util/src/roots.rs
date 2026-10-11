//! 运行根:源码检出与安装目录共用的定位规则。
//!
//! 源码检出时一切锚在编译它的仓根(与此前各 crate 的 `CARGO_MANIFEST_DIR` 上溯同值);
//! 安装版由桌面启动器设置 `FORGE_ROOT`(随包资源:agents/、skills/、registry/、projects/demo …)
//! 与 `FORGE_DATA_DIR`(用户数据,卸载/升级不动),二进制互相按同目录查找。

use std::path::{Path, PathBuf};

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Forge 根:env `FORGE_ROOT` 优先,否则编译期仓根(forge-util 的 manifest 上两级)。
pub fn forge_root() -> PathBuf {
    env_path("FORGE_ROOT").unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)")
            .to_path_buf()
    })
}

/// 数据根:env `FORGE_DATA_DIR` 优先,否则 `<forge_root>/data`。
pub fn data_root() -> PathBuf {
    data_root_under(&forge_root())
}

/// 显式给定 Forge 根(如 store-mcp 的 `--workspace`)时的数据根,同样先认 `FORGE_DATA_DIR`。
pub fn data_root_under(root: &Path) -> PathBuf {
    env_path("FORGE_DATA_DIR").unwrap_or_else(|| root.join("data"))
}

/// 进程日志目录:env `FORGE_LOG_DIR` 优先(安装版 = 用户数据旁的 logs/),否则 Forge 根(源码检出照旧落仓根)。
pub fn log_dir() -> PathBuf {
    env_path("FORGE_LOG_DIR").unwrap_or_else(forge_root)
}

/// 同套件的另一个可执行文件:先找当前进程同目录(安装版 bin/、同 profile 的 cargo 产物),
/// 找不到再退回 `<forge_root>/target/debug`(测试二进制在 deps/ 下,走的就是这条)。
pub fn companion_exe(file_name: &str) -> PathBuf {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(file_name)))
        .filter(|p| p.is_file());
    sibling.unwrap_or_else(|| forge_root().join("target").join("debug").join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_time_root_is_the_workspace() {
        if std::env::var_os("FORGE_ROOT").is_some() {
            return;
        }
        assert!(forge_root().join("Cargo.toml").is_file());
        assert!(forge_root().join("crates").join("forge-util").is_dir());
    }

    #[test]
    fn data_root_follows_the_given_root() {
        if std::env::var_os("FORGE_DATA_DIR").is_some() {
            return;
        }
        let root = Path::new("forge-root-fixture");
        assert_eq!(data_root_under(root), root.join("data"));
    }

    #[test]
    fn logs_stay_at_the_root_unless_redirected() {
        if std::env::var_os("FORGE_LOG_DIR").is_some() {
            return;
        }
        assert_eq!(log_dir(), forge_root());
    }

    #[test]
    fn missing_companion_falls_back_to_the_debug_target() {
        let name = "forge-no-such-companion.exe";
        assert_eq!(
            companion_exe(name),
            forge_root().join("target").join("debug").join(name)
        );
    }
}
