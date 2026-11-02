//! 安装规则(与 tools/installer/installer.nsh 的目录检查保持一致)和进度估算。

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// 与 NSIS 目录页一致:所选路径里(不区分大小写)没有产品名时,实际装到 `<所选>\<产品名>`。
pub fn resolve_install_dir(chosen: &Path, product: &str) -> PathBuf {
    let chosen = trim_trailing_separators(chosen);
    let text = chosen.to_string_lossy().to_lowercase();
    if text.contains(&product.to_lowercase()) {
        chosen
    } else {
        chosen.join(product)
    }
}

fn trim_trailing_separators(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches(['\\', '/']);
    // 盘符根(`D:\`)保留分隔符,`D:` 本身表示「该盘当前目录」。
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        PathBuf::from(format!("{trimmed}\\"))
    } else if trimmed.is_empty() {
        path.to_path_buf()
    } else {
        PathBuf::from(trimmed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirState {
    /// 不存在或是空文件夹。
    Fresh,
    /// 已经是本产品的安装目录(升级 / 重装)。
    Existing,
    /// 有其他文件:卸载和升级会删除整个安装目录,不允许装进去。
    Foreign,
    /// 路径指向一个文件。
    NotADirectory,
    /// 不是完整的绝对路径。
    Invalid,
}

impl DirState {
    pub fn as_str(self) -> &'static str {
        match self {
            DirState::Fresh => "fresh",
            DirState::Existing => "existing",
            DirState::Foreign => "foreign",
            DirState::NotADirectory => "file",
            DirState::Invalid => "invalid",
        }
    }

    pub fn installable(self) -> bool {
        matches!(self, DirState::Fresh | DirState::Existing)
    }
}

/// 绝对路径且带盘符或 UNC 前缀(`C:\…`、`\\server\share\…`)。
pub fn is_full_path(path: &Path) -> bool {
    path.is_absolute() && matches!(path.components().next(), Some(Component::Prefix(_)))
}

pub fn check_install_dir(dir: &Path, exe_name: &str) -> DirState {
    // 盘符或共享根本不存在时同样算无效路径。
    if !is_full_path(dir) || !dir.ancestors().any(Path::exists) {
        return DirState::Invalid;
    }
    match fs::metadata(dir) {
        Err(_) => DirState::Fresh,
        Ok(meta) if !meta.is_dir() => DirState::NotADirectory,
        Ok(_) => {
            if dir.join(exe_name).is_file() {
                DirState::Existing
            } else if fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none()) {
                DirState::Fresh
            } else {
                DirState::Foreign
            }
        }
    }
}

/// 逐段比较(不区分大小写):`child` 是否等于 `parent` 或位于其中。
pub fn is_within(child: &Path, parent: &Path) -> bool {
    let norm = |p: &Path| -> Vec<String> {
        p.components()
            .filter(|c| !matches!(c, Component::CurDir))
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let child = norm(child);
    let parent = norm(parent);
    !parent.is_empty() && child.len() >= parent.len() && child[..parent.len()] == parent[..]
}

/// 同一路径(逐段比较,不区分大小写,忽略末尾分隔符)。
pub fn same_path(a: &Path, b: &Path) -> bool {
    is_within(a, b) && is_within(b, a)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataState {
    Ok,
    /// 放在安装目录里:卸载或升级时会被删掉。
    InsideInstall,
    NotADirectory,
    Invalid,
}

impl DataState {
    pub fn as_str(self) -> &'static str {
        match self {
            DataState::Ok => "ok",
            DataState::InsideInstall => "inside-install",
            DataState::NotADirectory => "file",
            DataState::Invalid => "invalid",
        }
    }
}

pub fn check_data_dir(data_dir: &Path, install_dir: &Path) -> DataState {
    if !is_full_path(data_dir) {
        return DataState::Invalid;
    }
    if is_within(data_dir, install_dir) {
        return DataState::InsideInstall;
    }
    match fs::metadata(data_dir) {
        Ok(meta) if !meta.is_dir() => DataState::NotADirectory,
        _ => DataState::Ok,
    }
}

/// 一次安装的选择。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
    pub install_dir: PathBuf,
    pub desktop_shortcut: bool,
    pub start_menu_shortcut: bool,
}

/// 传给内层 NSIS 安装包的参数。`/D=` 必须放在最后且不能加引号(路径含空格也一样),
/// 所以单独返回,由调用方原样拼到命令行末尾。
pub fn nsis_arguments(choices: &Choices) -> (Vec<&'static str>, String) {
    let mut args = vec!["/S"];
    if !choices.desktop_shortcut {
        args.push("--no-desktop-shortcut");
    }
    if !choices.start_menu_shortcut {
        args.push("--no-start-menu-shortcut");
    }
    (args, format!("/D={}", choices.install_dir.display()))
}

/// 合并 `%APPDATA%\<产品名>\launcher.json`:只改 `dataDir`,端口等其他记录原样保留。
/// `data_dir` 为 None 表示用缺省位置(删掉 `dataDir`)。格式与 apps/desktop/src/backend.cjs 的 writeJson 一致。
pub fn merge_launcher_json(existing: Option<&str>, data_dir: Option<&Path>) -> String {
    let mut root = existing
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(|value| value.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    let map = root.as_object_mut().expect("object");
    match data_dir {
        Some(dir) => {
            map.insert("dataDir".into(), serde_json::Value::String(dir.display().to_string()));
        }
        None => {
            map.remove("dataDir");
        }
    }
    format!("{}\n", serde_json::to_string_pretty(&root).expect("json"))
}

/// 读 launcher.json 里用户选过的数据位置。
pub fn launcher_data_dir(text: &str) -> Option<PathBuf> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let dir = PathBuf::from(value.get("dataDir")?.as_str()?);
    is_full_path(&dir).then_some(dir)
}

/// 从卸载信息的 `UninstallString`(`"C:\…\Uninstall RurixForge.exe" /currentuser`)取出安装目录。
pub fn install_dir_from_uninstall_string(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    let exe = if let Some(rest) = text.strip_prefix('"') {
        &rest[..rest.find('"')?]
    } else {
        let lower = text.to_lowercase();
        &text[..lower.find(".exe")? + 4]
    };
    let dir = Path::new(exe).parent()?.to_path_buf();
    is_full_path(&dir).then_some(dir)
}

/// 目录下所有文件的字节数(不跟随联接点和符号链接);读不到的项跳过。
pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 解出内层安装包。
    Prepare,
    /// 升级:旧版本卸载中(安装目录在变小)。
    Remove,
    /// 复制新文件(安装目录在变大)。
    Copy,
    /// 写快捷方式、注册信息与设置。
    Finish,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Prepare => "prepare",
            Phase::Remove => "remove",
            Phase::Copy => "copy",
            Phase::Finish => "finish",
        }
    }
}

pub const PREPARE_END: f64 = 5.0;
pub const REMOVE_END: f64 = 12.0;
pub const COPY_END: f64 = 96.0;

/// 内层安装包静默运行时没有进度输出,只能看安装目录的大小:升级时先变小(卸载旧版),再变大(解压新版)。
#[derive(Debug)]
pub struct Tracker {
    expected: u64,
    start: u64,
    floor: u64,
    min_seen: u64,
    phase: Phase,
    percent: f64,
}

impl Tracker {
    /// `expected` = 装好后的大小;`start` = 开始时安装目录已有的大小(全新安装为 0)。
    pub fn new(expected: u64, start: u64) -> Self {
        Tracker {
            expected: expected.max(1),
            start,
            floor: 0,
            min_seen: start,
            phase: if start > 0 { Phase::Remove } else { Phase::Copy },
            percent: PREPARE_END,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 喂入当前目录大小与已用时间,返回(阶段, 百分比)。百分比只增不减。
    pub fn update(&mut self, size: u64, elapsed: Duration) -> (Phase, f64) {
        const GROWTH: u64 = 8 * 1024 * 1024;
        let target = match self.phase {
            Phase::Remove => {
                self.min_seen = self.min_seen.min(size);
                let removed = 1.0 - size as f64 / self.start.max(1) as f64;
                let copying = size > self.min_seen + GROWTH
                    || size.saturating_mul(50) < self.start
                    || elapsed > Duration::from_secs(120);
                if copying {
                    self.phase = Phase::Copy;
                    self.floor = self.min_seen.min(size);
                    REMOVE_END
                } else {
                    PREPARE_END + (REMOVE_END - PREPARE_END) * removed.clamp(0.0, 1.0)
                }
            }
            Phase::Copy => {
                let base = if self.start > 0 { REMOVE_END } else { PREPARE_END };
                let span = self.expected.saturating_sub(self.floor).max(1) as f64;
                let done = size.saturating_sub(self.floor) as f64 / span;
                base + (COPY_END - base) * done.clamp(0.0, 1.0)
            }
            Phase::Prepare | Phase::Finish => self.percent,
        };
        self.percent = self.percent.max(target);
        (self.phase, self.percent)
    }

    /// 进程退出、开始收尾。
    pub fn finish(&mut self) -> (Phase, f64) {
        self.phase = Phase::Finish;
        self.percent = self.percent.max(COPY_END);
        (self.phase, self.percent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_dir_gets_product_folder_unless_named() {
        let p = "RurixForge";
        assert_eq!(resolve_install_dir(Path::new(r"D:\Apps"), p), PathBuf::from(r"D:\Apps\RurixForge"));
        assert_eq!(resolve_install_dir(Path::new(r"D:\Apps\"), p), PathBuf::from(r"D:\Apps\RurixForge"));
        assert_eq!(resolve_install_dir(Path::new(r"D:\"), p), PathBuf::from(r"D:\RurixForge"));
        assert_eq!(resolve_install_dir(Path::new(r"D:\rurixforge"), p), PathBuf::from(r"D:\rurixforge"));
        assert_eq!(resolve_install_dir(Path::new(r"E:\Tools\RurixForge-beta"), p), PathBuf::from(r"E:\Tools\RurixForge-beta"));
    }

    #[test]
    fn install_dir_states() {
        let root = std::env::temp_dir().join(format!("forge-setup-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::create_dir_all(root.join("existing")).unwrap();
        fs::write(root.join("existing").join("RurixForge.exe"), b"x").unwrap();
        fs::create_dir_all(root.join("foreign")).unwrap();
        fs::write(root.join("foreign").join("Cargo.toml"), b"x").unwrap();
        fs::write(root.join("file"), b"x").unwrap();
        let exe = "RurixForge.exe";
        assert_eq!(check_install_dir(&root.join("missing"), exe), DirState::Fresh);
        assert_eq!(check_install_dir(&root.join("empty"), exe), DirState::Fresh);
        assert_eq!(check_install_dir(&root.join("existing"), exe), DirState::Existing);
        assert_eq!(check_install_dir(&root.join("foreign"), exe), DirState::Foreign);
        assert_eq!(check_install_dir(&root.join("file"), exe), DirState::NotADirectory);
        assert_eq!(check_install_dir(Path::new(r"relative\dir"), exe), DirState::Invalid);
        assert_eq!(check_install_dir(Path::new(r"\no-drive"), exe), DirState::Invalid);
        assert_eq!(check_install_dir(Path::new(r"\\no-such-server-forge\share\RurixForge"), exe), DirState::Invalid);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn data_dir_must_live_outside_install_dir() {
        let install = Path::new(r"D:\Apps\RurixForge");
        assert_eq!(check_data_dir(Path::new(r"D:\Apps\RurixForge\data"), install), DataState::InsideInstall);
        assert_eq!(check_data_dir(Path::new(r"d:\apps\rurixforge"), install), DataState::InsideInstall);
        assert_eq!(check_data_dir(Path::new(r"D:\Apps\RurixForgeData"), install), DataState::Ok);
        assert_eq!(check_data_dir(Path::new(r"data"), install), DataState::Invalid);
        assert!(is_within(Path::new(r"C:\a\b\c"), Path::new(r"c:\A\b")));
        assert!(!is_within(Path::new(r"C:\a\bc"), Path::new(r"C:\a\b")));
    }

    #[test]
    fn nsis_arguments_put_unquoted_dir_last() {
        let choices = Choices {
            install_dir: PathBuf::from(r"D:\My Apps\RurixForge"),
            desktop_shortcut: false,
            start_menu_shortcut: true,
        };
        let (args, dir) = nsis_arguments(&choices);
        assert_eq!(args, vec!["/S", "--no-desktop-shortcut"]);
        assert_eq!(dir, r"/D=D:\My Apps\RurixForge");
        let all = Choices { desktop_shortcut: true, start_menu_shortcut: false, ..choices };
        assert_eq!(nsis_arguments(&all).0, vec!["/S", "--no-start-menu-shortcut"]);
    }

    #[test]
    fn launcher_json_keeps_other_keys() {
        let merged = merge_launcher_json(Some("{\"hostPort\":3080,\"agentdPort\":8103}"), Some(Path::new(r"E:\Forge Data")));
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["hostPort"], 3080);
        assert_eq!(value["dataDir"], r"E:\Forge Data");
        assert!(merged.ends_with("}\n"));
        assert_eq!(launcher_data_dir(&merged), Some(PathBuf::from(r"E:\Forge Data")));

        let reset = merge_launcher_json(Some(&merged), None);
        let value: serde_json::Value = serde_json::from_str(&reset).unwrap();
        assert!(value.get("dataDir").is_none());
        assert_eq!(value["agentdPort"], 8103);

        assert_eq!(merge_launcher_json(Some("not json"), None), "{}\n");
        assert_eq!(launcher_data_dir("{\"dataDir\":\"relative\"}"), None);
    }

    #[test]
    fn uninstall_string_gives_install_dir() {
        assert_eq!(
            install_dir_from_uninstall_string(r#""C:\Users\me\AppData\Local\Programs\RurixForge\Uninstall RurixForge.exe" /currentuser"#),
            Some(PathBuf::from(r"C:\Users\me\AppData\Local\Programs\RurixForge"))
        );
        assert_eq!(
            install_dir_from_uninstall_string(r"D:\RF\Uninstall.exe /S"),
            Some(PathBuf::from(r"D:\RF"))
        );
        assert_eq!(install_dir_from_uninstall_string("garbage"), None);
    }

    #[test]
    fn tracker_follows_fresh_install() {
        let mut t = Tracker::new(1000, 0);
        assert_eq!(t.phase(), Phase::Copy);
        let (_, p0) = t.update(0, Duration::from_secs(1));
        assert_eq!(p0, PREPARE_END);
        let (_, p1) = t.update(500, Duration::from_secs(2));
        assert!((p1 - (PREPARE_END + (COPY_END - PREPARE_END) * 0.5)).abs() < 1e-9);
        let (_, p2) = t.update(300, Duration::from_secs(3));
        assert_eq!(p2, p1, "never goes backwards");
        let (_, p3) = t.update(5000, Duration::from_secs(4));
        assert_eq!(p3, COPY_END);
        assert_eq!(t.finish(), (Phase::Finish, COPY_END));
    }

    #[test]
    fn tracker_follows_upgrade_remove_then_copy() {
        let mb = 1024 * 1024;
        let mut t = Tracker::new(1000 * mb, 1000 * mb);
        assert_eq!(t.phase(), Phase::Remove);
        let (phase, p) = t.update(500 * mb, Duration::from_secs(1));
        assert_eq!(phase, Phase::Remove);
        assert!(p > PREPARE_END && p < REMOVE_END);
        let (phase, _) = t.update(10 * mb, Duration::from_secs(2));
        assert_eq!(phase, Phase::Copy, "almost empty: copying starts");
        let (_, p) = t.update(505 * mb, Duration::from_secs(3));
        assert!(p > REMOVE_END + 30.0 && p < COPY_END);
    }
}
