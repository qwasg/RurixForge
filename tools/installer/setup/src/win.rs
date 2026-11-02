//! 安装外壳用到的 Win32 调用。

use std::ffi::{c_void, OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Environment::GetCommandLineW;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Shell::{
    SHGetKnownFolderPath, ShellExecuteW, FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Programs,
    FOLDERID_RoamingAppData,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, MessageBoxW, SetForegroundWindow, ShowWindow, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
    MB_SETFOREGROUND, SW_RESTORE, SW_SHOWNORMAL,
};

use forge_setup::plan;

fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

unsafe fn from_wide_ptr(ptr: *const u16) -> OsString {
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    OsString::from_wide(std::slice::from_raw_parts(ptr, len))
}

pub fn message_box(title: &str, text: &str, error: bool) {
    let icon = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe {
        MessageBoxW(null_mut(), wide(text).as_ptr(), wide(title).as_ptr(), MB_OK | icon | MB_SETFOREGROUND);
    }
}

/// 原始命令行里程序名之后的部分(保留引号,原样转给内层安装包:NSIS 的 `/D=` 要求不加引号且在最后)。
pub fn command_line_tail() -> String {
    let line = unsafe { from_wide_ptr(GetCommandLineW()) }.to_string_lossy().into_owned();
    let rest = if let Some(quoted) = line.strip_prefix('"') {
        quoted.find('"').map(|end| &quoted[end + 1..]).unwrap_or("")
    } else {
        line.find(char::is_whitespace).map(|end| &line[end..]).unwrap_or("")
    };
    rest.trim_start().to_string()
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let mut raw: *mut u16 = null_mut();
        let hr = SHGetKnownFolderPath(id, 0, null_mut(), &mut raw);
        if raw.is_null() {
            return None;
        }
        let path = (hr >= 0).then(|| PathBuf::from(from_wide_ptr(raw)));
        CoTaskMemFree(raw as *const c_void);
        path
    }
}

pub fn local_app_data() -> Option<PathBuf> {
    known_folder(&FOLDERID_LocalAppData).or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
}

pub fn roaming_app_data() -> Option<PathBuf> {
    known_folder(&FOLDERID_RoamingAppData).or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
}

pub fn desktop_dir() -> Option<PathBuf> {
    known_folder(&FOLDERID_Desktop)
}

pub fn start_menu_programs_dir() -> Option<PathBuf> {
    known_folder(&FOLDERID_Programs)
}

/// 路径所在磁盘的(可用, 总量)字节数;路径还不存在时看最近的已存在上级。
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    let mut probe = Some(path);
    while let Some(dir) = probe {
        if dir.exists() {
            let (mut available, mut total) = (0u64, 0u64);
            let ok = unsafe { GetDiskFreeSpaceExW(wide(dir).as_ptr(), &mut available, &mut total, null_mut()) };
            return (ok != 0).then_some((available, total));
        }
        probe = dir.parent();
    }
    None
}

pub struct Existing {
    pub dir: PathBuf,
    pub version: String,
}

fn reg_string(key: HKEY, name: &str) -> Option<String> {
    let name = wide(name);
    let mut bytes = 0u32;
    unsafe {
        if RegGetValueW(key, null(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut bytes) != 0 {
            return None;
        }
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2) + 1];
        let mut size = (buf.len() * 2) as u32;
        if RegGetValueW(key, null(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut size) != 0 {
            return None;
        }
        Some(from_wide_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

/// 按卸载信息找已安装的同名产品(electron-builder 按用户安装时写在 HKCU)。
pub fn existing_install(product: &str) -> Option<Existing> {
    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let prefix = format!("{} ", product.to_lowercase());
    unsafe {
        let mut root: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(UNINSTALL).as_ptr(), 0, KEY_READ, &mut root) != 0 {
            return None;
        }
        let mut found = None;
        let mut index = 0;
        loop {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            let status = RegEnumKeyExW(root, index, name.as_mut_ptr(), &mut len, null(), null_mut(), null_mut(), null_mut());
            if status != 0 {
                break;
            }
            index += 1;
            let mut key: HKEY = null_mut();
            if RegOpenKeyExW(root, name.as_ptr(), 0, KEY_READ, &mut key) != 0 {
                continue;
            }
            let display = reg_string(key, "DisplayName").unwrap_or_default().to_lowercase();
            if display == product.to_lowercase() || display.starts_with(&prefix) {
                let dir = reg_string(key, "UninstallString").and_then(|s| plan::install_dir_from_uninstall_string(&s));
                let version = reg_string(key, "DisplayVersion").unwrap_or_default();
                if let Some(dir) = dir.filter(|d| d.is_dir()) {
                    found = Some(Existing { dir, version });
                }
            }
            RegCloseKey(key);
            if found.is_some() {
                break;
            }
        }
        RegCloseKey(root);
        found
    }
}

/// 有没有进程的可执行文件位于 `dir` 之内(安装时内层安装包会关掉它们)。
pub fn running_from(dir: &Path) -> bool {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut running = false;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more && !running {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
            if !process.is_null() {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0 {
                    let image = PathBuf::from(OsString::from_wide(&buf[..len as usize]));
                    running = plan::is_within(&image, dir);
                }
                CloseHandle(process);
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        running
    }
}

/// 同一时间只运行一个安装界面;已有实例时返回 false。互斥量句柄随进程存活。
pub fn claim_single_instance(name: &str) -> bool {
    unsafe {
        let handle = CreateMutexW(null(), 0, wide(name).as_ptr());
        !(handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS)
    }
}

/// 把已打开的同名窗口带到前台。
pub fn focus_window(title: &str) {
    unsafe {
        let hwnd = FindWindowW(null(), wide(title).as_ptr());
        if !hwnd.is_null() {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
    }
}

/// 用系统默认方式打开文件、文件夹或网址;`dir` 是启动程序时的工作目录。
pub fn shell_open(target: &OsStr, dir: Option<&Path>) -> bool {
    let dir = dir.map(wide);
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            wide(target).as_ptr(),
            null(),
            dir.as_ref().map_or(null(), |d| d.as_ptr()),
            SW_SHOWNORMAL,
        )
    };
    result as isize > 32
}

/// Windows 11 上给无边框窗口加系统圆角;旧系统忽略。
pub fn round_corners(hwnd: isize) {
    let preference = DWMWCP_ROUND;
    unsafe {
        DwmSetWindowAttribute(
            hwnd as *mut c_void,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&preference as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}
