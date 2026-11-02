//! 安装流程:解出内层 NSIS 安装包 → 静默运行(按选项传参)→ 按安装目录大小估算进度 → 写数据位置。

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use forge_setup::payload::{self, Payload};
use forge_setup::plan::{self, Choices, DirState, Phase, Tracker};

pub struct Job {
    /// 安装程序自身(载荷所在文件)。
    pub source: PathBuf,
    pub payload: Payload,
    pub choices: Choices,
    pub data: DataChoice,
    /// %APPDATA%\<产品名>\launcher.json。
    pub launcher_json: PathBuf,
}

/// 数据位置怎么处理。
pub enum DataChoice {
    /// 不动 launcher.json(环境变量 FORGE_DATA_DIR 已指定位置)。
    Keep,
    /// 用缺省位置:去掉以前另选的记录。
    Default,
    /// 另选的位置。
    Custom(PathBuf),
}

pub struct Update {
    pub phase: Phase,
    pub percent: f64,
    pub bytes: u64,
}

#[derive(Debug)]
pub struct Failure {
    pub code: Option<i32>,
    pub message: String,
}

fn failure(message: impl Into<String>) -> Failure {
    Failure { code: None, message: message.into() }
}

/// 临时目录,离开作用域时删除(内层安装包运行结束后才会离开)。
pub struct Scratch(pub PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 把内层安装包解到临时目录。`progress` 收到 0..=1 的完成比例。
pub fn unpack_inner(source: &Path, payload: &Payload, mut progress: impl FnMut(f64)) -> Result<(Scratch, PathBuf), Failure> {
    let meta = &payload.meta;
    let scratch = Scratch(std::env::temp_dir().join(format!("{}-Setup-{}", meta.product_name, std::process::id())));
    fs::create_dir_all(&scratch.0).map_err(|e| failure(format!("无法创建临时文件夹 {}:{e}", scratch.0.display())))?;
    let inner = scratch.0.join(format!("{}-{}-core.exe", meta.product_name, meta.version));
    let total = payload.len.max(1) as f64;
    let mut last = Instant::now();
    payload::extract(source, payload, &inner, |copied| {
        if last.elapsed() >= Duration::from_millis(80) {
            last = Instant::now();
            progress(copied as f64 / total);
        }
    })
    .map_err(|e| failure(format!("无法解出安装内容(临时文件夹所在磁盘空间可能不足):{e}")))?;
    progress(1.0);
    Ok((scratch, inner))
}

fn describe_exit(code: Option<i32>) -> String {
    match code {
        Some(2) => "安装被中止:安装位置不可用,或者正在运行的程序无法关闭。请关闭它后重试,或换一个安装位置。".into(),
        Some(1) => "安装被取消。".into(),
        Some(code) => format!("安装程序返回了错误代码 {code}。"),
        None => "安装程序意外结束。".into(),
    }
}

pub fn run(job: &Job, emit: &dyn Fn(Update)) -> Result<PathBuf, Failure> {
    let meta = &job.payload.meta;
    let dir = &job.choices.install_dir;
    let report = |phase, percent, bytes| emit(Update { phase, percent, bytes });

    report(Phase::Prepare, 0.0, 0);
    let (_scratch, inner) = unpack_inner(&job.source, &job.payload, |done| {
        report(Phase::Prepare, plan::PREPARE_END * done, 0);
    })?;

    if let DataChoice::Custom(data) = &job.data {
        fs::create_dir_all(data).map_err(|e| failure(format!("无法创建数据文件夹 {}:{e}", data.display())))?;
    }

    let start = match plan::check_install_dir(dir, &meta.exe_name) {
        DirState::Existing => plan::dir_size(dir),
        DirState::Fresh => 0,
        other => return Err(failure(format!("安装位置不可用({}):{}", other.as_str(), dir.display()))),
    };
    // 先建好安装文件夹:写不进去(例如需要管理员权限的位置)时在这里就给出明确原因。
    fs::create_dir_all(dir).map_err(|e| {
        failure(format!("无法在 {} 创建安装文件夹:{e}。请换一个当前用户可以写入的位置,例如 D:\\Apps\\{}。", dir.display(), meta.product_name))
    })?;
    let mut tracker = Tracker::new(meta.installed_bytes, start);
    let (args, dir_arg) = plan::nsis_arguments(&job.choices);
    let mut child = Command::new(&inner)
        .args(&args)
        .raw_arg(&dir_arg)
        .spawn()
        .map_err(|e| failure(format!("无法启动安装程序:{e}")))?;

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(failure(format!("等待安装程序时出错:{e}"))),
        }
        let size = plan::dir_size(dir);
        let (phase, percent) = tracker.update(size, started.elapsed());
        report(phase, percent, size);
        std::thread::sleep(Duration::from_millis(300));
    };
    let (phase, percent) = tracker.finish();
    report(phase, percent, plan::dir_size(dir));

    if status.code() != Some(0) {
        return Err(Failure { code: status.code(), message: describe_exit(status.code()) });
    }
    let exe = dir.join(&meta.exe_name);
    if !exe.is_file() {
        return Err(failure(format!("安装程序已结束,但没有找到 {}。", exe.display())));
    }

    let data = match &job.data {
        DataChoice::Keep => None,
        DataChoice::Default => Some(None),
        DataChoice::Custom(dir) => Some(Some(dir.as_path())),
    };
    if let Some(data) = data {
        write_data_dir(&job.launcher_json, data)
            .map_err(|e| failure(format!("程序已安装,但无法保存数据位置设置({}):{e}", job.launcher_json.display())))?;
    }
    report(Phase::Finish, 100.0, plan::dir_size(dir));
    Ok(exe)
}

/// 选了别的数据位置就写进 launcher.json;改回缺省位置时去掉旧记录;两者都不涉及时不碰文件。
pub fn write_data_dir(launcher_json: &Path, data_dir: Option<&Path>) -> std::io::Result<()> {
    let existing = fs::read_to_string(launcher_json).ok();
    let had_custom = existing.as_deref().and_then(plan::launcher_data_dir).is_some();
    if data_dir.is_none() && !had_custom {
        return Ok(());
    }
    if let Some(parent) = launcher_json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(launcher_json, plan::merge_launcher_json(existing.as_deref(), data_dir))
}
