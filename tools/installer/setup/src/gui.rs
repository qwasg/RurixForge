//! 安装界面:tao 无边框窗口 + wry(WebView2)渲染 ui/ 下的页面,两边经 IPC 传 JSON。
//!
//! 页面 → 本进程:`window.ipc.postMessage(JSON)`,`type` 见 [`handle_message`]。
//! 本进程 → 页面:`window.__setup.receive(JSON)`。

use std::borrow::Cow;
use std::num::NonZeroIsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use serde_json::{json, Value};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Icon, Window, WindowBuilder};
use wry::http::{header, Request, Response, StatusCode};
use wry::{ScrollBarStyle, WebContext, WebViewBuilder, WebViewBuilderExtWindows};

use forge_setup::payload::Payload;
use forge_setup::plan::{self, Choices};

use crate::install::{self, DataChoice, Job};
use crate::win;

const ORIGIN: &str = "http://setup.localhost";

enum UserEvent {
    Ipc(String),
    Send(Value),
    Show,
    Installed(Result<PathBuf, install::Failure>),
}

/// 安装前就能确定的缺省值与现状。
struct Context {
    source: PathBuf,
    payload: Payload,
    existing: Option<win::Existing>,
    install_dir: PathBuf,
    data_default: PathBuf,
    data_current: PathBuf,
    /// 环境变量 FORGE_DATA_DIR 指定了数据位置,程序运行时以它为准,界面里不让改。
    data_locked: bool,
    launcher_json: PathBuf,
    desktop_shortcut: bool,
    start_menu_shortcut: bool,
}

impl Context {
    fn new(source: PathBuf, payload: Payload) -> Self {
        let product = payload.meta.product_name.clone();
        let existing = win::existing_install(&product);
        let local = win::local_app_data().unwrap_or_else(std::env::temp_dir);
        let roaming = win::roaming_app_data().unwrap_or_else(|| local.clone());
        let user_data = roaming.join(&product);
        let launcher_json = user_data.join("launcher.json");
        let data_default = user_data.join("data");
        let env_data = std::env::var_os("FORGE_DATA_DIR").map(PathBuf::from).filter(|p| plan::is_full_path(p));
        let saved_data = std::fs::read_to_string(&launcher_json).ok().and_then(|t| plan::launcher_data_dir(&t));
        let data_locked = env_data.is_some();
        let data_current = env_data.or(saved_data).unwrap_or_else(|| data_default.clone());
        let shortcut_exists = |dir: Option<PathBuf>| dir.map(|d| d.join(format!("{product}.lnk")).exists()).unwrap_or(true);
        let (desktop_shortcut, start_menu_shortcut) = match &existing {
            // 升级时沿用上次的选择:用户删掉的快捷方式不再加回来。
            Some(_) => (shortcut_exists(win::desktop_dir()), shortcut_exists(win::start_menu_programs_dir())),
            None => (true, true),
        };
        let install_dir = existing
            .as_ref()
            .map(|e| e.dir.clone())
            .unwrap_or_else(|| local.join("Programs").join(&product));
        Context {
            source,
            payload,
            existing,
            install_dir,
            data_default,
            data_current,
            data_locked,
            launcher_json,
            desktop_shortcut,
            start_menu_shortcut,
        }
    }

    fn product(&self) -> &str {
        &self.payload.meta.product_name
    }

    fn init_message(&self) -> Value {
        let meta = &self.payload.meta;
        json!({
            "type": "init",
            "product": meta.product_name,
            "version": meta.version,
            "description": meta.description,
            "homepage": meta.homepage,
            "license": meta.license,
            "requiredBytes": meta.installed_bytes,
            "existing": self.existing.as_ref().map(|e| json!({ "dir": e.dir, "version": e.version })),
            "defaults": {
                "installDir": self.install_dir,
                "dataDir": self.data_current,
                "dataDirDefault": self.data_default,
                "dataDirLocked": self.data_locked,
                "desktopShortcut": self.desktop_shortcut,
                "startMenuShortcut": self.start_menu_shortcut,
                "launchAfter": true,
            },
        })
    }

    /// 校验选择:安装位置(含实际文件夹、可用空间、是否有程序在运行)与数据位置。
    fn validate(&self, seq: &Value, install_dir: &str, data_dir: &str) -> Value {
        let meta = &self.payload.meta;
        let chosen = PathBuf::from(install_dir.trim());
        let resolved = if plan::is_full_path(&chosen) { plan::resolve_install_dir(&chosen, self.product()) } else { chosen };
        let state = plan::check_install_dir(&resolved, &meta.exe_name);
        let install_state = if state.installable() && needs_admin(&resolved) { "protected" } else { state.as_str() };
        let data = PathBuf::from(data_dir.trim());
        let data_state = plan::check_data_dir(&data, &resolved);
        let temp = std::env::temp_dir();
        // 内层安装包先解到临时目录,NSIS 再把自带的压缩包解到同一处:约两倍载荷大小。
        let temp_required = self.payload.len.saturating_mul(2);
        let same_drive = drive_of(&temp) == drive_of(&resolved);
        let space = win::disk_space(&resolved);
        json!({
            "type": "validation",
            "seq": seq,
            "installDir": resolved,
            "installState": install_state,
            "drive": drive_of(&resolved),
            "freeBytes": space.map(|s| s.0),
            "totalBytes": space.map(|s| s.1),
            // 装好后占用的空间;neededBytes 再加上同盘时安装过程中的临时文件,用来判断空间够不够。
            "requiredBytes": meta.installed_bytes,
            "neededBytes": meta.installed_bytes + if same_drive { temp_required } else { 0 },
            "tempFreeBytes": if same_drive { None } else { win::disk_space(&temp).map(|s| s.0) },
            "tempRequiredBytes": temp_required,
            "dataDir": data,
            "dataState": data_state.as_str(),
            "running": state == plan::DirState::Existing && win::running_from(&resolved),
        })
    }
}

/// 按用户安装(不提权)写不进去的系统位置。
fn needs_admin(path: &Path) -> bool {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "SystemRoot"]
        .iter()
        .filter_map(std::env::var_os)
        .any(|root| plan::is_within(path, Path::new(&root)))
}

fn drive_of(path: &Path) -> String {
    match path.components().next() {
        Some(std::path::Component::Prefix(prefix)) => prefix.as_os_str().to_string_lossy().to_uppercase(),
        _ => String::new(),
    }
}

/// rfd 需要的父窗口句柄;只带 HWND 数值,可以在别的线程上弹对话框。
struct Parent(isize);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?;
        // SAFETY:句柄来自本进程仍然存活的主窗口,对话框返回前窗口不会销毁。
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(hwnd))) })
    }
}

impl HasDisplayHandle for Parent {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

struct Asset {
    body: Cow<'static, [u8]>,
    mime: &'static str,
}

fn asset(path: &str) -> Option<Asset> {
    let (name, mime) = match path {
        "/" | "/index.html" => ("index.html", "text/html; charset=utf-8"),
        "/setup.css" => ("setup.css", "text/css; charset=utf-8"),
        "/setup.js" => ("setup.js", "text/javascript; charset=utf-8"),
        "/particles.js" => ("particles.js", "text/javascript; charset=utf-8"),
        _ => return None,
    };
    // 调试界面时可设 FORGE_SETUP_UI_DIR 直接读磁盘上的 ui/,改完刷新即可,不必重新编译。
    if let Some(dir) = std::env::var_os("FORGE_SETUP_UI_DIR") {
        return std::fs::read(Path::new(&dir).join(name)).ok().map(|b| Asset { body: Cow::Owned(b), mime });
    }
    let body: &'static [u8] = match name {
        "index.html" => include_bytes!("../ui/index.html"),
        "setup.css" => include_bytes!("../ui/setup.css"),
        "setup.js" => include_bytes!("../ui/setup.js"),
        _ => include_bytes!("../ui/particles.js"),
    };
    Some(Asset { body: Cow::Borrowed(body), mime })
}

fn serve(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let found = asset(request.uri().path());
    let builder = Response::builder()
        .header(header::CACHE_CONTROL, "no-store")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; script-src 'self'",
        );
    match found {
        Some(asset) => builder.header(header::CONTENT_TYPE, asset.mime).body(asset.body),
        None => builder.status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&b"not found"[..])),
    }
    .expect("response")
}

fn script_for(message: &Value) -> String {
    format!("window.__setup && window.__setup.receive({message})")
}

fn send(proxy: &EventLoopProxy<UserEvent>, message: Value) {
    let _ = proxy.send_event(UserEvent::Send(message));
}

fn center(window: &Window) {
    if let Some(monitor) = window.current_monitor().or_else(|| window.primary_monitor()) {
        let area = monitor.size();
        let origin = monitor.position();
        let size = window.outer_size();
        let x = origin.x + (area.width as i32 - size.width as i32) / 2;
        let y = origin.y + (area.height as i32 - size.height as i32) / 2;
        window.set_outer_position(PhysicalPosition::new(x, y.max(origin.y)));
    }
}

/// WebView2 运行时可用时打开安装界面;此函数不返回。界面建不起来时返回 Err,由调用方改走经典向导。
pub fn run(source: PathBuf, payload: Payload) -> Result<(), String> {
    let context = Context::new(source, payload);
    let title = format!("{} 安装程序", context.product());
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(LogicalSize::new(880.0, 580.0))
        .with_resizable(false)
        .with_maximizable(false)
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_visible(false)
        .with_window_icon(Icon::from_resource(1, None).ok())
        .build(&event_loop)
        .map_err(|e| format!("无法创建窗口:{e}"))?;
    center(&window);
    win::round_corners(window.hwnd());

    let data_dir = std::env::temp_dir().join(format!("{}-Setup-WebView2", context.product()));
    let mut web_context = WebContext::new(Some(data_dir));
    let ipc_proxy = proxy.clone();
    let mut builder = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_custom_protocol("setup".into(), |_id, request| serve(request))
        .with_url(format!("{ORIGIN}/index.html"))
        .with_ipc_handler(move |request: Request<String>| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(request.into_body()));
        })
        .with_navigation_handler(|url| url.starts_with(ORIGIN))
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_background_color((246, 246, 244, 255))
        .with_hotkeys_zoom(false)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        .with_scroll_bar_style(ScrollBarStyle::FluentOverlay)
        .with_devtools(cfg!(debug_assertions));
    // 自动化测试用:经 DevTools 协议驱动界面。
    if let Ok(port) = std::env::var("FORGE_SETUP_DEBUG_PORT") {
        builder = builder.with_additional_browser_args(format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
        ));
    }
    let webview = builder.build(&window).map_err(|e| format!("无法创建网页视图:{e}"))?;

    // 页面没按时报到也要把窗口显示出来。
    let show_proxy = proxy.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        let _ = show_proxy.send_event(UserEvent::Show);
    });

    let installing = Arc::new(AtomicBool::new(false));
    let mut installed: Option<PathBuf> = None;
    let mut shown = false;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // 让 web_context 活到事件循环结束。
        let _ = &web_context;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if installing.load(Ordering::SeqCst) {
                    send(&proxy, json!({ "type": "busy" }));
                } else {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Show) => {
                if !shown {
                    shown = true;
                    window.set_visible(true);
                    window.set_focus();
                }
            }
            Event::UserEvent(UserEvent::Send(message)) => {
                let _ = webview.evaluate_script(&script_for(&message));
            }
            Event::UserEvent(UserEvent::Installed(result)) => {
                installing.store(false, Ordering::SeqCst);
                let message = match result {
                    Ok(exe) => {
                        let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
                        installed = Some(exe);
                        json!({ "type": "done", "installDir": dir })
                    }
                    Err(failure) => json!({ "type": "failed", "code": failure.code, "message": failure.message }),
                };
                let _ = webview.evaluate_script(&script_for(&message));
            }
            Event::UserEvent(UserEvent::Ipc(body)) => {
                let Ok(message) = serde_json::from_str::<Value>(&body) else { return };
                let kind = message["type"].as_str().unwrap_or_default();
                match kind {
                    "ready" => {
                        let _ = webview.evaluate_script(&script_for(&context.init_message()));
                        let _ = proxy.send_event(UserEvent::Show);
                    }
                    "drag" => {
                        let _ = window.drag_window();
                    }
                    "minimize" => window.set_minimized(true),
                    "close" => {
                        if !installing.load(Ordering::SeqCst) {
                            *control_flow = ControlFlow::Exit;
                        }
                    }
                    "validate" => {
                        let reply = context.validate(
                            &message["seq"],
                            message["installDir"].as_str().unwrap_or_default(),
                            message["dataDir"].as_str().unwrap_or_default(),
                        );
                        let _ = webview.evaluate_script(&script_for(&reply));
                    }
                    "pick" => pick_folder(&proxy, window.hwnd(), &message),
                    "open" => {
                        // 只放行网页链接(主页、许可证原文)。
                        if let Some(url) = message["url"].as_str().filter(|u| u.starts_with("https://")) {
                            win::shell_open(url.as_ref(), None);
                        }
                    }
                    "openFolder" => {
                        if let Some(exe) = &installed {
                            if let Some(dir) = exe.parent() {
                                win::shell_open(dir.as_os_str(), None);
                            }
                        }
                    }
                    "launch" => {
                        if let Some(exe) = &installed {
                            win::shell_open(exe.as_os_str(), exe.parent());
                        }
                        *control_flow = ControlFlow::Exit;
                    }
                    "install" => {
                        if installing.swap(true, Ordering::SeqCst) {
                            return;
                        }
                        start_install(&context, &proxy, &message);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    });
}

fn pick_folder(proxy: &EventLoopProxy<UserEvent>, hwnd: isize, message: &Value) {
    let target = message["target"].as_str().unwrap_or("install").to_string();
    let current = PathBuf::from(message["current"].as_str().unwrap_or_default());
    let proxy = proxy.clone();
    std::thread::spawn(move || {
        let title = if target == "data" { "选择数据存放位置" } else { "选择安装位置" };
        // 从当前值最近的已存在上级开始浏览。
        let mut start = Some(current.as_path());
        while let Some(dir) = start {
            if dir.is_dir() {
                break;
            }
            start = dir.parent();
        }
        let mut dialog = rfd::FileDialog::new().set_title(title).set_parent(&Parent(hwnd));
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        let picked = dialog.pick_folder();
        send(&proxy, json!({ "type": "picked", "target": target, "path": picked }));
    });
}

fn start_install(context: &Context, proxy: &EventLoopProxy<UserEvent>, message: &Value) {
    let product = context.product().to_string();
    let chosen = PathBuf::from(message["installDir"].as_str().unwrap_or_default().trim());
    let install_dir = plan::resolve_install_dir(&chosen, &product);
    let data = PathBuf::from(message["dataDir"].as_str().unwrap_or_default().trim());
    // 缺省位置不写进 launcher.json;环境变量指定时程序以环境变量为准,不碰 launcher.json。
    let data = if context.data_locked {
        DataChoice::Keep
    } else if data.as_os_str().is_empty() || plan::same_path(&data, &context.data_default) {
        DataChoice::Default
    } else {
        DataChoice::Custom(data)
    };
    let job = Job {
        source: context.source.clone(),
        payload: context.payload.clone(),
        choices: Choices {
            install_dir,
            desktop_shortcut: message["desktopShortcut"].as_bool().unwrap_or(true),
            start_menu_shortcut: message["startMenuShortcut"].as_bool().unwrap_or(true),
        },
        data,
        launcher_json: context.launcher_json.clone(),
    };
    let total = context.payload.meta.installed_bytes;
    let proxy = proxy.clone();
    std::thread::spawn(move || {
        let progress_proxy = proxy.clone();
        let emit = move |update: install::Update| {
            send(
                &progress_proxy,
                json!({
                    "type": "progress",
                    "phase": update.phase.as_str(),
                    "percent": update.percent,
                    "bytes": update.bytes,
                    "total": total,
                }),
            );
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| install::run(&job, &emit)))
            .unwrap_or_else(|_| Err(install::Failure { code: None, message: "安装过程中出现内部错误。".into() }));
        let _ = proxy.send_event(UserEvent::Installed(result));
    });
}
