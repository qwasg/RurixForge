use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

use super::bin::{self, Launch};

const LOG_MAX: usize = 8000;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, Default)]
pub(super) struct InstallState {
    pub running: bool,
    pub stage: Option<&'static str>,
    pub started_at: Option<Instant>,
    pub log: String,
    pub error: Option<String>,
    pub finished_at: Option<String>,
    npm_error: Option<&'static str>,
}

impl InstallState {
    pub fn begin() -> Self {
        let mut state = Self {
            running: true,
            stage: Some("starting"),
            started_at: Some(Instant::now()),
            ..Self::default()
        };
        state.append("正在检查 Node.js 和 npm…");
        state
    }

    pub fn append(&mut self, message: &str) {
        let elapsed = self.started_at.map_or(0, |start| start.elapsed().as_secs());
        let line = format!("[+{elapsed}s] {message}");
        eprintln!("[codex setup] {line}");
        self.log.push_str(&line);
        self.log.push('\n');
        if self.log.len() > LOG_MAX {
            let mut cut = self.log.len() - LOG_MAX;
            while !self.log.is_char_boundary(cut) {
                cut += 1;
            }
            if let Some(end_of_line) = self.log[cut..].find('\n') {
                cut += end_of_line + 1;
            }
            self.log.drain(..cut);
        }
    }
}

type Progress = Arc<Mutex<InstallState>>;

fn command(launch: &Launch) -> Command {
    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.prefix_args)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd
}

fn progress(state: &Progress, stage: Option<&'static str>, message: &str) {
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(stage) = stage {
        state.stage = Some(stage);
    }
    state.append(message);
}

fn npm_error(code: &str) -> Option<&'static str> {
    match code {
        "ENOTFOUND" | "EAI_AGAIN" => Some("npm 无法解析下载服务器。请检查 DNS 或代理后重试。"),
        "ETIMEDOUT" | "ESOCKETTIMEDOUT" => Some("npm 下载请求超时。请检查网络或代理后重试。"),
        "ECONNREFUSED" | "ECONNRESET" | "ENETUNREACH" => {
            Some("npm 无法连接下载服务器。请检查网络或代理后重试。")
        }
        "CERT_HAS_EXPIRED"
        | "SELF_SIGNED_CERT_IN_CHAIN"
        | "UNABLE_TO_VERIFY_LEAF_SIGNATURE"
        | "DEPTH_ZERO_SELF_SIGNED_CERT"
        | "UNABLE_TO_GET_ISSUER_CERT_LOCALLY" => {
            Some("npm 无法验证服务器证书。请检查系统时间和代理证书设置后重试。")
        }
        "E401" | "E403" => Some("npm 镜像拒绝访问。请检查当前镜像或代理的访问权限。"),
        "E404" | "ETARGET" => Some("npm 镜像找不到所需软件包版本。请检查镜像同步状态后重试。"),
        "EACCES" | "EPERM" => Some("npm 无法写入安装文件。请检查安装目录权限或占用后重试。"),
        "ENOSPC" => Some("安装目录或 npm 缓存所在磁盘空间不足。请释放空间后重试。"),
        "EINTEGRITY" => Some("npm 下载完整性校验失败。请检查网络与镜像后重试。"),
        _ => None,
    }
}

// npm and its lifecycle scripts can print authenticated URLs, headers or
// environment contents. Only emit known phase labels, numeric HTTP status,
// and allowlisted failures to the API and desktop log.
fn record_npm_line(state: &Progress, line: &[u8]) {
    let text = String::from_utf8_lossy(line);
    let mut words = text.split_ascii_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some("npm"), Some("error" | "ERR!"), Some("code")) => {
            if let Some(message) = words.next().and_then(npm_error) {
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                if state.npm_error != Some(message) {
                    state.npm_error = Some(message);
                    state.append(message);
                }
            }
        }
        (Some("npm"), Some("http"), Some("fetch")) => {
            let status = words.nth(1).and_then(|s| s.parse::<u16>().ok());
            if let Some(status @ 100..=599) = status {
                progress(state, None, &format!("软件包请求返回 HTTP {status}"));
            }
        }
        (Some("npm"), Some("info"), Some("run"))
            if words
                .next()
                .is_some_and(|name| name.starts_with("open-computer-use@"))
                && words.next() == Some("postinstall") =>
        {
            progress(state, Some("preparing"), "正在准备 Computer Use 组件…");
        }
        _ => {}
    }
}

async fn drain_output(mut pipe: impl AsyncRead + Unpin, state: Progress) {
    let mut buffer = [0; 4096];
    let mut line = Vec::with_capacity(2048);
    while let Ok(count @ 1..) = pipe.read(&mut buffer).await {
        for part in buffer[..count].split_inclusive(|byte| *byte == b'\n') {
            let remaining = 2048 - line.len();
            line.extend_from_slice(&part[..part.len().min(remaining)]);
            if part.ends_with(b"\n") {
                record_npm_line(&state, &line);
                line.clear();
            }
        }
    }
    if !line.is_empty() {
        record_npm_line(&state, &line);
    }
}

async fn stop_install(child: &mut Child) {
    #[cfg(windows)]
    if let (Some(pid), Some(windows)) = (child.id(), std::env::var_os("SystemRoot")) {
        let taskkill = std::path::PathBuf::from(windows).join("System32/taskkill.exe");
        let _ = tokio::time::timeout(
            Duration::from_secs(3),
            Command::new(taskkill)
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .status(),
        )
        .await;
    }
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
}

pub(super) async fn run_install(npm: &Launch, root: &Path, state: Progress) -> Result<(), String> {
    std::fs::create_dir_all(root)
        .map_err(|e| format!("无法创建 Codex 安装目录，请检查目录权限和剩余空间。{e}"))?;
    progress(
        &state,
        Some("downloading"),
        "正在从 npm 下载并安装 Codex 组件…",
    );
    let mut cmd = command(npm);
    cmd.args(bin::npm_install_args(
        root,
        &[bin::CODEX_NPM_SPEC, bin::COMPUTER_USE_NPM_SPEC],
    ))
    .current_dir(root)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    if let (Some(node_dir), Some(path)) = (npm.program.parent(), std::env::var_os("PATH")) {
        if let Ok(path) = std::env::join_paths(
            std::iter::once(node_dir.to_path_buf()).chain(std::env::split_paths(&path)),
        ) {
            // npm lifecycle scripts use the bundled Node on a clean Windows machine.
            cmd.env("PATH", path);
        }
    }
    let mut child = cmd.spawn().map_err(|e| {
        format!("无法启动 npm。请检查 Node.js / npm 安装或重新安装 RurixForge。{e}")
    })?;
    let out = tokio::spawn(drain_output(
        child.stdout.take().expect("piped stdout"),
        Arc::clone(&state),
    ));
    let err = tokio::spawn(drain_output(
        child.stderr.take().expect("piped stderr"),
        Arc::clone(&state),
    ));
    let outcome = {
        let wait = child.wait();
        let deadline = tokio::time::sleep(INSTALL_TIMEOUT);
        let mut heartbeat = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(10),
            Duration::from_secs(10),
        );
        tokio::pin!(wait, deadline);
        loop {
            tokio::select! {
                status = &mut wait => break Some(status),
                _ = &mut deadline => break None,
                _ = heartbeat.tick() => progress(&state, None, "安装仍在进行，请保持 RurixForge 打开。"),
            }
        }
    };
    if outcome.is_none() {
        stop_install(&mut child).await;
    }
    let mut out = out;
    let mut err = err;
    if tokio::time::timeout(Duration::from_secs(1), async {
        let _ = tokio::join!(&mut out, &mut err);
    })
    .await
    .is_err()
    {
        out.abort();
        err.abort();
    }
    let status = outcome
        .ok_or_else(|| {
            "安装超过 5 分钟，已结束本次安装。请检查网络或 npm 镜像后重试。".to_string()
        })?
        .map_err(|e| format!("无法读取 npm 安装结果：{e}"))?;
    if !status.success() {
        let detail = state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .npm_error
            .unwrap_or("请检查 npm 镜像、网络和安装目录权限后重试。");
        return Err(format!("npm 安装失败（{status}）。{detail}"));
    }
    progress(
        &state,
        Some("verifying"),
        "下载完成，正在验证 Codex 能否启动…",
    );
    let launch = bin::managed_codex_launch(root).ok_or_else(|| {
        "npm 已结束，但缺少 Codex 启动文件。请检查 npm 镜像后重新安装。".to_string()
    })?;
    let mut verify = command(&launch);
    verify
        .arg("--version")
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = verify
        .spawn()
        .map_err(|e| format!("无法启动已下载的 Codex，请重试安装。{e}"))?;
    match tokio::time::timeout(VERIFY_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!("Codex 启动检查失败（{status}）。请重试安装。")),
        Ok(Err(e)) => Err(format!("无法读取 Codex 启动检查结果：{e}")),
        Err(_) => {
            stop_install(&mut child).await;
            Err("Codex 启动检查超时。请重试安装，或在设置中指定可用的 Codex 路径。".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn streams_safe_progress_and_network_errors_before_process_exit() {
        let state = Arc::new(Mutex::new(InstallState::default()));
        let (reader, mut writer) = tokio::io::duplex(4096);
        let output = tokio::spawn(drain_output(reader, Arc::clone(&state)));
        use tokio::io::AsyncWriteExt;
        writer
            .write_all(b"npm http fetch GET 200 https://secret:password@mirror/a?token=private\n")
            .await
            .unwrap();
        writer
            .write_all(b"npm error code ENOTFOUND\n")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while state.lock().unwrap().npm_error.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let snapshot = state.lock().unwrap().clone();
        assert!(!output.is_finished());
        assert!(snapshot.log.contains("HTTP 200"));
        assert!(snapshot.log.contains("DNS"));
        for secret in ["secret", "password", "token", "mirror", "private"] {
            assert!(!snapshot.log.contains(secret));
        }
        drop(writer);
        output.await.unwrap();
    }
}
