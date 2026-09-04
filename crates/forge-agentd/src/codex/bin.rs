//! codex 可执行文件的决议与托管安装。
//!
//! 决议顺序(先命中先用):
//! 1. `FORGE_CODEX_BIN`(测试/脚本面显式点名);
//! 2. `data/codex-config.json.codexBin`(设置页里人工指定的路径);
//! 3. PATH 上的 `codex`(用户自己装过 CLI 就直接用,不重复下载 140MB);
//! 4. 托管安装 `data/codex/node_modules/@openai/codex/bin/codex.js`(用 `node` 启动)。
//!
//! 托管安装走 npm:`npm install --prefix <data>/codex @openai/codex open-computer-use`。
//! 官方 npm 包用 optionalDependencies 分平台带 vendor 二进制(win32-x64 内含
//! `vendor/x86_64-pc-windows-msvc/bin/codex.exe`),`bin/codex.js` 负责挑对应平台的那个,
//! 所以本仓只认这个 JS 入口,不去猜平台三元组。

use std::path::{Path, PathBuf};

/// 托管安装锁定的版本(升级须显式改这里,免得某天上游破坏协议后无声漂移)。
pub const CODEX_NPM_SPEC: &str = "@openai/codex@0.153.2";
/// 桌面级 Computer Use 的开源 MCP 服务(官方运行时只随 ChatGPT 桌面版插件发布,
/// 第三方 app-server 客户端拿不到;这个包跨平台且支持 Windows)。
pub const COMPUTER_USE_NPM_SPEC: &str = "open-computer-use@0.3.3";
/// open-computer-use 的 MCP 子命令入口名(npm bin 名)。
pub const COMPUTER_USE_BIN_NAME: &str = "open-computer-use-mcp";

/// 一条可启动的命令(`program` + 前置参数;调用方在其后追加自己的子命令与参数)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub prefix_args: Vec<String>,
}

impl Launch {
    fn direct(program: PathBuf) -> Self {
        Launch {
            program,
            prefix_args: Vec::new(),
        }
    }

    fn via_node(script: PathBuf) -> Self {
        Launch {
            program: PathBuf::from(node_program()),
            prefix_args: vec![script.to_string_lossy().into_owned()],
        }
    }

    /// 供状态面/日志展示的单行命令(不含密钥,可直接上屏)。
    pub fn display(&self) -> String {
        if self.prefix_args.is_empty() {
            return self.program.to_string_lossy().into_owned();
        }
        format!(
            "{} {}",
            self.program.to_string_lossy(),
            self.prefix_args.join(" ")
        )
    }
}

/// 托管安装根 = `data/codex`(npm --prefix)。
pub fn managed_root() -> PathBuf {
    crate::agent_data_root().join("codex")
}

fn node_program() -> &'static str {
    "node"
}

/// 托管安装的 codex JS 入口。
fn managed_codex_entry() -> PathBuf {
    managed_root()
        .join("node_modules")
        .join("@openai")
        .join("codex")
        .join("bin")
        .join("codex.js")
}

/// npm 在 `node_modules/.bin` 下生成的 shim(Windows 为 `.cmd`)。
fn managed_shim(name: &str) -> PathBuf {
    let dir = managed_root().join("node_modules").join(".bin");
    if cfg!(windows) {
        dir.join(format!("{name}.cmd"))
    } else {
        dir.join(name)
    }
}

/// PATH 上的可执行文件查找(`which` 的最小实现;Windows 按 PATHEXT 补后缀)。
fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_string())
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        Vec::new()
    };
    for dir in std::env::split_paths(&paths) {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        for ext in &exts {
            let cand = dir.join(format!("{name}{ext}"));
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// codex 启动命令决议。`None` = 本机既没装 CLI 也没托管安装过。
pub fn resolve() -> Option<Launch> {
    if let Some(p) = std::env::var_os("FORGE_CODEX_BIN")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
    {
        return Some(launch_for_path(p));
    }
    let cfg = super::config::load();
    if !cfg.codex_bin.is_empty() {
        let p = PathBuf::from(&cfg.codex_bin);
        if p.is_file() {
            return Some(launch_for_path(p));
        }
    }
    if let Some(p) = which("codex") {
        return Some(Launch::direct(p));
    }
    let entry = managed_codex_entry();
    if entry.is_file() {
        return Some(Launch::via_node(entry));
    }
    None
}

/// 路径形态 → 启动命令(`.js` 交给 node,其余直接执行)。
fn launch_for_path(p: PathBuf) -> Launch {
    if p.extension().and_then(|e| e.to_str()) == Some("js") {
        Launch::via_node(p)
    } else {
        Launch::direct(p)
    }
}

/// open-computer-use 的 MCP 启动命令(托管安装优先,其次 PATH)。
pub fn resolve_computer_use() -> Option<Launch> {
    let shim = managed_shim(COMPUTER_USE_BIN_NAME);
    if shim.is_file() {
        return Some(Launch::direct(shim));
    }
    // 包内 bin 是带 shebang 的 node 脚本:Windows 上直接执行不了,统一交给 node。
    let raw = managed_root()
        .join("node_modules")
        .join("open-computer-use")
        .join("bin")
        .join(COMPUTER_USE_BIN_NAME);
    if raw.is_file() && !cfg!(windows) {
        return Some(Launch::direct(raw));
    }
    if raw.is_file() {
        return Some(Launch::via_node(raw));
    }
    which(COMPUTER_USE_BIN_NAME).map(Launch::direct)
}

/// 托管安装是否已就位(codex 与 computer-use 分别报,前端据此显示两行状态)。
pub fn managed_installed() -> (bool, bool) {
    (
        managed_codex_entry().is_file(),
        resolve_computer_use().is_some(),
    )
}

/// npm 启动命令(Windows 上 npm 是 `npm.cmd`,Command 不认无后缀名)。
pub fn npm_launch() -> Option<PathBuf> {
    if cfg!(windows) {
        which("npm.cmd").or_else(|| which("npm"))
    } else {
        which("npm")
    }
}

/// `npm install --prefix <root> <specs...>` 的参数序(单测可断言,免得改错顺序)。
pub fn npm_install_args(root: &Path, specs: &[&str]) -> Vec<String> {
    let mut args = vec![
        "install".to_string(),
        "--prefix".to_string(),
        root.to_string_lossy().into_owned(),
        // 托管目录不是工程,没有 package.json;--no-save 免得 npm 去写它。
        "--no-save".to_string(),
        "--no-audit".to_string(),
        "--no-fund".to_string(),
    ];
    args.extend(specs.iter().map(|s| s.to_string()));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `.js` 路径必须经 node 启动;裸可执行文件直接执行。
    #[test]
    fn js_entry_goes_through_node() {
        let via = launch_for_path(PathBuf::from("/x/bin/codex.js"));
        assert_eq!(via.program, PathBuf::from("node"));
        assert_eq!(via.prefix_args, vec!["/x/bin/codex.js".to_string()]);
        let direct = launch_for_path(PathBuf::from("/x/bin/codex.exe"));
        assert_eq!(direct.program, PathBuf::from("/x/bin/codex.exe"));
        assert!(direct.prefix_args.is_empty());
        assert_eq!(direct.display(), "/x/bin/codex.exe");
    }

    /// npm 参数序:install → --prefix <root> → 免写盘三开关 → specs。
    #[test]
    fn npm_args_shape() {
        let args = npm_install_args(Path::new("/data/codex"), &["a@1", "b@2"]);
        assert_eq!(args[0], "install");
        assert_eq!(args[1], "--prefix");
        assert_eq!(args[2], "/data/codex");
        assert!(args.contains(&"--no-save".to_string()));
        assert_eq!(args[args.len() - 2], "a@1");
        assert_eq!(args[args.len() - 1], "b@2");
    }
}
