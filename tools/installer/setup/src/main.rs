//! RurixForge 安装程序外壳。
//!
//! - 双击运行:WebView2 安装界面(欢迎 → 选项 → 安装进度 → 完成),实际安装交给内层 NSIS 安装包静默执行。
//! - `/S [--no-desktop-shortcut] [--no-start-menu-shortcut] [--data-dir=<目录>] [/D=<目录>]`:
//!   静默安装,参数原样转给内层安装包,退出码与它一致(`/D=` 必须放在最后、不加引号)。
//! - `--classic`,或系统没有 WebView2 运行时:改用内层安装包自带的经典向导。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod gui;
#[cfg(windows)]
mod install;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
fn main() {
    eprintln!("forge-setup 只支持 Windows");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    use forge_setup::payload;
    use std::path::PathBuf;
    use std::process::exit;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a.eq_ignore_ascii_case(flag));
    let silent = has("/S");
    let classic = has("--classic") || has("/classic");

    let source = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => fatal(silent, &format!("找不到安装程序自身:{e}")),
    };
    let payload = match payload::read(&source) {
        Ok(payload) => payload,
        Err(e) => fatal(silent, &format!("安装包已损坏或不完整,请重新下载。\n\n{e}")),
    };
    let product = payload.meta.product_name.clone();

    if silent {
        let data_dir = args
            .iter()
            .find_map(|a| a.strip_prefix("--data-dir=").map(PathBuf::from))
            .filter(|p| forge_setup::plan::is_full_path(p));
        exit(run_inner(&source, &payload, &win::command_line_tail(), true, data_dir));
    }

    let title = format!("{product} 安装程序");
    if !win::claim_single_instance(&format!("Local\\{product}-Setup-UI")) {
        win::focus_window(&title);
        exit(0);
    }
    if classic || wry::webview_version().is_err() {
        exit(run_inner(&source, &payload, &without_flag(&win::command_line_tail(), "--classic"), false, None));
    }
    if let Err(reason) = gui::run(source.clone(), payload.clone()) {
        eprintln!("安装界面不可用({reason}),改用经典向导");
        exit(run_inner(&source, &payload, &win::command_line_tail(), false, None));
    }
}

#[cfg(windows)]
fn fatal(silent: bool, message: &str) -> ! {
    if silent {
        eprintln!("{message}");
    } else {
        win::message_box("安装程序", message, true);
    }
    std::process::exit(1);
}

/// 解出内层安装包并以 `arguments`(原始命令行片段)运行,返回它的退出码。
#[cfg(windows)]
fn run_inner(
    source: &std::path::Path,
    payload: &forge_setup::payload::Payload,
    arguments: &str,
    silent: bool,
    data_dir: Option<std::path::PathBuf>,
) -> i32 {
    use std::os::windows::process::CommandExt;

    let (_scratch, inner) = match install::unpack_inner(source, payload, |_| {}) {
        Ok(found) => found,
        Err(failure) => fatal(silent, &failure.message),
    };
    let status = std::process::Command::new(&inner).raw_arg(arguments).status();
    let code = match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => fatal(silent, &format!("无法启动安装程序:{e}")),
    };
    if code == 0 {
        if let (Some(dir), Some(roaming)) = (data_dir, win::roaming_app_data()) {
            let launcher = roaming.join(&payload.meta.product_name).join("launcher.json");
            if std::fs::create_dir_all(&dir).is_err() || install::write_data_dir(&launcher, Some(&dir)).is_err() {
                return 3;
            }
        }
    }
    code
}

/// 从原始命令行片段里去掉一个独立的开关(不区分大小写)。
#[cfg(windows)]
fn without_flag(arguments: &str, flag: &str) -> String {
    arguments
        .split(' ')
        .filter(|part| !part.eq_ignore_ascii_case(flag))
        .collect::<Vec<_>>()
        .join(" ")
}
