//! rx/rurixc 子进程包装核心(F4 wave.1):路径解析、超时、1MB 截断、五工具执行。
//!
//! 诚实纪律:exe 缺失 → RX_CLI_NOT_FOUND;JSON 解析失败 → TOOL_ERROR 附原始输出;
//! 超时 → RX_TIMEOUT 杀进程;不伪造空诊断/空汇总。
//! 实测上游行为(2026-08-17,H:\rurix\target\debug):
//! - rurixc <file> --emit=check --error-format=json:JSON 诊断数组落 stdout;exit 0 → `[]`。
//! - rx build <file> [-o out]:诊断文本落 stderr(`error[RX0008]: msg`);缺省产物 <file>.exe。
//! - rx run <file>:build 后执行并透传产物退出码;**不透传程序参数**(parse_input_out 仅 <input> [-o])。
//! - rx fmt <file>:格式化结果落 stdout;--check 已格式化 → 0,否则 1 + "rx fmt: <file>: 未格式化"。
//! - rx test <file> [--filter s]:stdout "rx test: <name> ..."/"... ok" + "rx test: PASS n/n";
//!   失败时 stderr "rx test: error[RX7011]: <name>: <detail>" + "rx test: FAIL p passed; f failed"。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// stdout/stderr 各截断 1MB(防爆内存;截断后仍排空管道防子进程阻塞)。
pub const CAP: usize = 1024 * 1024;

#[derive(Debug)]
pub struct ToolError {
    pub code: String,
    pub message: String,
}

pub type TResult<T> = Result<T, ToolError>;

fn terr(code: &str, message: impl Into<String>) -> ToolError {
    ToolError { code: code.to_string(), message: message.into() }
}

/// rx.exe:env FORGE_RX_CLI 优先,缺省 H:\rurix\target\debug\rx.exe。
pub fn rx_cli() -> TResult<PathBuf> {
    let p = std::env::var("FORGE_RX_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"H:\rurix\target\debug\rx.exe"));
    if p.is_file() {
        Ok(p)
    } else {
        Err(terr("RX_CLI_NOT_FOUND", format!("rx CLI 不存在: {}(env FORGE_RX_CLI 可覆盖)", p.display())))
    }
}

/// rurixc.exe:env FORGE_RURIXC 优先(rx_check 必须走 rurixc 拿 JSON 诊断,契约决策 D-F4-A)。
pub fn rurixc() -> TResult<PathBuf> {
    let p = std::env::var("FORGE_RURIXC")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"H:\rurix\target\debug\rurixc.exe"));
    if p.is_file() {
        Ok(p)
    } else {
        Err(terr("RX_CLI_NOT_FOUND", format!("rurixc 不存在: {}(env FORGE_RURIXC 可覆盖)", p.display())))
    }
}

/// 子进程一次执行结果。
pub struct RunOut {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// 读流 capped 到 1MB;超限继续读丢弃(排空管道),返截断标记。
pub fn read_capped<R: Read>(mut r: R) -> (String, bool) {
    let mut buf = Vec::with_capacity(8192);
    let mut tmp = [0u8; 8192];
    let mut truncated = false;
    loop {
        match r.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > CAP {
                    buf.extend_from_slice(&tmp[..CAP - buf.len()]);
                    truncated = true;
                } else {
                    buf.extend_from_slice(&tmp[..n]);
                }
            }
            Err(_) => break,
        }
    }
    (String::from_utf8_lossy(&buf).into_owned(), truncated)
}

/// spawn + stdout/stderr 读线程 + 轮询等待;超时杀进程 → RX_TIMEOUT。
pub fn run(bin: &Path, args: &[String], timeout: Duration) -> TResult<RunOut> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| terr("TOOL_ERROR", format!("spawn {} 失败: {e}", bin.display())))?;
    let out_pipe = child.stdout.take().expect("piped stdout");
    let err_pipe = child.stderr.take().expect("piped stderr");
    let t_out = thread::spawn(move || read_capped(out_pipe));
    let t_err = thread::spawn(move || read_capped(err_pipe));
    // std 无 wait_timeout:等待线程 try_wait 轮询 + kill 通道。
    let (kill_tx, kill_rx) = mpsc::channel::<()>();
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let _ = done_tx.send(st.code());
                return;
            }
            Ok(None) => {
                if kill_rx.try_recv().is_ok() {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = done_tx.send(None);
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = done_tx.send(None);
                return;
            }
        }
    });
    let code = match done_rx.recv_timeout(timeout) {
        Ok(c) => c,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = kill_tx.send(());
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
            let _ = t_out.join();
            let _ = t_err.join();
            return Err(terr(
                "RX_TIMEOUT",
                format!("{} {:?} 超时({}ms),已杀进程", bin.display(), args, timeout.as_millis()),
            ));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => None,
    };
    let (stdout, out_t) = t_out.join().unwrap_or_else(|_| (String::new(), false));
    let (stderr, err_t) = t_err.join().unwrap_or_else(|_| (String::new(), false));
    // None = try_wait 报错(超时路径已提前返回,不会到此)。
    let exit_code = code.unwrap_or(-1);
    Ok(RunOut { exit_code, stdout, stderr, stdout_truncated: out_t, stderr_truncated: err_t })
}

/// 截断标记:嵌入文本末尾,如实可辨。
fn marked(s: &str, truncated: bool) -> String {
    if truncated {
        format!("{s}\n...[truncated 1MB]")
    } else {
        s.to_string()
    }
}

fn timeout_of(args: &Value, default_ms: u64) -> Duration {
    let ms = args.get("timeoutMs").and_then(Value::as_u64).unwrap_or(default_ms);
    Duration::from_millis(ms.max(100))
}

fn req_file(args: &Value) -> TResult<&str> {
    args.get("file")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| terr("USAGE", "缺 file 参数(.rx 源文件路径)"))
}

/// rurixc 单条诊断 JSON → 契约形态 {code,severity,file,span?,message,suggestion?}。
/// 实测字段:level/message/code?/labels?/suggestions?;无 file 字段 → 取输入文件。
pub fn map_check_diag(d: &Value, file: &str) -> Value {
    let mut o = json!({
        "code": d.get("code").cloned().unwrap_or(Value::Null),
        "severity": d.get("level").cloned().unwrap_or_else(|| json!("error")),
        "file": file,
        "message": d.get("message").cloned().unwrap_or(Value::Null),
    });
    if let Some(l) = d.get("labels").and_then(Value::as_array).and_then(|ls| ls.first()) {
        o["span"] = json!({ "start": l["start"].clone(), "end": l["end"].clone() });
    }
    if let Some(s) = d.get("suggestions").and_then(Value::as_array).and_then(|ss| ss.first()) {
        o["suggestion"] = json!({ "message": s["message"].clone(), "replacement": s["replacement"].clone() });
    }
    o
}

/// rx_check {file} → rurixc <file> --emit=check --error-format=json(D-F4-A:rx check 不透传
/// --error-format,必须直包 rurixc)。exit 0 → diagnostics=[]。
pub fn check(args: &Value) -> TResult<Value> {
    let file = req_file(args)?;
    let bin = rurixc()?;
    let out = run(
        &bin,
        &[file.to_string(), "--emit=check".into(), "--error-format=json".into()],
        timeout_of(args, 60_000),
    )?;
    let diags: Vec<Value> = serde_json::from_str(out.stdout.trim()).map_err(|e| {
        terr(
            "TOOL_ERROR",
            format!(
                "rurixc JSON 诊断解析失败(exit {}): {e};stdout={};stderr={}",
                out.exit_code,
                marked(&out.stdout, out.stdout_truncated),
                marked(&out.stderr, out.stderr_truncated)
            ),
        )
    })?;
    Ok(json!({ "diagnostics": diags.iter().map(|d| map_check_diag(d, file)).collect::<Vec<_>>() }))
}

/// 从 rx/rurixc 文本 stderr 提取 error[RXnnnn]/warning[RXnnnn] 行(code + message)。
pub fn parse_text_diags(stderr: &str, file: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for raw in stderr.lines() {
        let line = raw.trim_end();
        let (sev, rest) = if let Some(r) = line.strip_prefix("error[") {
            ("error", r)
        } else if let Some(r) = line.strip_prefix("warning[") {
            ("warning", r)
        } else {
            continue;
        };
        let Some(end) = rest.find(']') else { continue };
        let code = &rest[..end];
        let message = rest[end + 1..].trim_start_matches(':').trim();
        out.push(json!({ "code": code, "severity": sev, "file": file, "message": message }));
    }
    out
}

/// rx_build {file, out?} → rx build <file> [-o out];产物存在才填 artifact。
pub fn build(args: &Value) -> TResult<Value> {
    let file = req_file(args)?;
    let bin = rx_cli()?;
    let mut argv = vec!["build".to_string(), file.to_string()];
    let out_path = args.get("out").and_then(Value::as_str);
    if let Some(o) = out_path {
        argv.push("-o".into());
        argv.push(o.into());
    }
    let out = run(&bin, &argv, timeout_of(args, 60_000))?;
    let diagnostics = parse_text_diags(&out.stderr, file);
    let mut v = json!({ "exitCode": out.exit_code, "diagnostics": diagnostics });
    if out.exit_code == 0 {
        let artifact = out_path.map(PathBuf::from).unwrap_or_else(|| Path::new(file).with_extension("exe"));
        if artifact.is_file() {
            v["artifact"] = json!(artifact.to_string_lossy());
        }
    } else if v["diagnostics"].as_array().is_some_and(|d| d.is_empty()) {
        // 非零退出但未解析到结构化诊断:如实附原始 stderr,不伪装空错。
        v["stderr"] = json!(marked(&out.stderr, out.stderr_truncated));
    }
    Ok(v)
}

/// UNIX 秒 → yyyymmdd-hhmmss(UTC;Howard Hinnant civil_from_days,无 chrono 依赖)。
pub fn utc_stamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

/// <workspace>/data/code-runs/<yyyymmdd-hhmmss>-<pid>。
fn runs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("CARGO_MANIFEST_DIR 上三级须存在(workspace 根)")
        .join("data")
        .join("code-runs")
        .join(format!("{}-{}", utc_stamp(SystemTime::now()), std::process::id()))
}

/// rx_run {file, args?, timeoutMs?} → rx run <file>;stdout/stderr 落盘 data/code-runs/ 后返相对路径。
/// 上游 rx run 不透传程序参数(实测 parse_input_out 仅 <input> [-o]):args 非空 → USAGE 如实报错。
pub fn run_tool(args: &Value) -> TResult<Value> {
    let file = req_file(args)?;
    let extra: Vec<String> = args
        .get("args")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    if !extra.is_empty() {
        return Err(terr(
            "USAGE",
            "上游 rx run 不支持透传程序参数(仅 <input> [-o]);args 暂不可用,未静默丢弃",
        ));
    }
    let bin = rx_cli()?;
    let out = run(&bin, &["run".into(), file.to_string()], timeout_of(args, 30_000))?;
    let dir = runs_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| terr("TOOL_ERROR", format!("建落盘目录 {} 失败: {e}", dir.display())))?;
    let stdout_path = dir.join("stdout.txt");
    let stderr_path = dir.join("stderr.txt");
    std::fs::write(&stdout_path, marked(&out.stdout, out.stdout_truncated))
        .map_err(|e| terr("TOOL_ERROR", format!("写 {} 失败: {e}", stdout_path.display())))?;
    std::fs::write(&stderr_path, marked(&out.stderr, out.stderr_truncated))
        .map_err(|e| terr("TOOL_ERROR", format!("写 {} 失败: {e}", stderr_path.display())))?;
    let dir_name = dir.file_name().expect("runs_dir 有末段").to_string_lossy();
    Ok(json!({
        "exitCode": out.exit_code,
        "stdoutRef": format!("data/code-runs/{dir_name}/stdout.txt"),
        "stderrRef": format!("data/code-runs/{dir_name}/stderr.txt"),
    }))
}

/// rx_fmt {file, checkOnly?}:checkOnly → rx fmt --check(exit 1 = 未格式化,附格式化输出);
/// 否则 rx fmt stdout 写回文件(先读备份入内存,写失败回滚)。
pub fn fmt(args: &Value) -> TResult<Value> {
    let file = req_file(args)?;
    let bin = rx_cli()?;
    let check_only = args.get("checkOnly").and_then(Value::as_bool).unwrap_or(false);
    if check_only {
        let out = run(&bin, &["fmt".into(), "--check".into(), file.to_string()], timeout_of(args, 60_000))?;
        if out.exit_code == 0 {
            return Ok(json!({ "formatted": [], "needsFormat": false }));
        }
        // exit 1 = 未格式化(实测):取格式化全文供调用方对比,不改写文件。
        let probe = run(&bin, &["fmt".into(), file.to_string()], timeout_of(args, 60_000))?;
        return Ok(json!({
            "formatted": [],
            "needsFormat": true,
            "diff": "未格式化(rx fmt --check 退出 1)",
            "formattedOutput": marked(&probe.stdout, probe.stdout_truncated),
        }));
    }
    let out = run(&bin, &["fmt".into(), file.to_string()], timeout_of(args, 60_000))?;
    if out.exit_code != 0 {
        return Err(terr(
            "TOOL_ERROR",
            format!("rx fmt 退出 {}: {}", out.exit_code, marked(&out.stderr, out.stderr_truncated)),
        ));
    }
    if out.stdout.trim().is_empty() {
        return Err(terr("TOOL_ERROR", "rx fmt 输出为空,未写回(防截断源文件)"));
    }
    let backup = std::fs::read(file).map_err(|e| terr("TOOL_ERROR", format!("读源文件 {file} 失败: {e}")))?;
    if let Err(e) = std::fs::write(file, out.stdout.as_bytes()) {
        let _ = std::fs::write(file, &backup);
        return Err(terr("TOOL_ERROR", format!("写回 {file} 失败(已回滚): {e}")));
    }
    Ok(json!({ "formatted": [file] }))
}

/// rx test 输出解析:PASS/FAIL 汇总 + RX7011 失败明细。无汇总行 → None(调用方如实 TOOL_ERROR)。
pub fn parse_test_output(stdout: &str, stderr: &str) -> Option<(usize, usize, Vec<(String, String)>)> {
    let mut summary: Option<(usize, usize)> = None;
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("rx test: PASS ") {
            // 形如 "2/2"
            if let Some((a, b)) = rest.split_once('/') {
                if let (Ok(n), Ok(t)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                    summary = Some((n, t - n));
                }
            }
        } else if let Some(rest) = line.strip_prefix("rx test: FAIL ") {
            // 形如 "0 passed; 1 failed"
            let mut p = None;
            let mut f = None;
            for part in rest.split(';') {
                let part = part.trim();
                if let Some(n) = part.strip_suffix(" passed") {
                    p = n.trim().parse().ok();
                }
                if let Some(n) = part.strip_suffix(" failed") {
                    f = n.trim().parse().ok();
                }
            }
            if let (Some(p), Some(f)) = (p, f) {
                summary = Some((p, f));
            }
        }
    }
    let (passed, failed) = summary?;
    let mut failures = Vec::new();
    for line in stderr.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("rx test: error[RX7011]: ") {
            let (name, detail) = rest.split_once(": ").unwrap_or((rest, ""));
            failures.push((name.to_string(), detail.to_string()));
        }
    }
    Some((passed, failed, failures))
}

/// rx_test {file?, filter?, timeoutMs?} → rx test <file> [--filter]。
/// 契约表 rx_test 无 file 参数,但上游 CLI 必须输入文件;file 缺省 → USAGE 如实报错。
pub fn test(args: &Value) -> TResult<Value> {
    let file = args
        .get("file")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| terr("USAGE", "rx_test 缺 file(上游 rx test 需要输入 .rx;契约表未列 file,偏差如实标注)"))?;
    let bin = rx_cli()?;
    let mut argv = vec!["test".to_string(), file.to_string()];
    if let Some(f) = args.get("filter").and_then(Value::as_str) {
        argv.push("--filter".into());
        argv.push(f.into());
    }
    let out = run(&bin, &argv, timeout_of(args, 120_000))?;
    match parse_test_output(&out.stdout, &out.stderr) {
        Some((passed, failed, failures)) => Ok(json!({
            "passed": passed,
            "failed": failed,
            "failures": failures.iter().map(|(n, d)| json!({ "name": n, "detail": d })).collect::<Vec<_>>(),
        })),
        None => Err(terr(
            "TOOL_ERROR",
            format!(
                "rx test 输出无 PASS/FAIL 汇总(exit {}): stdout={} stderr={}",
                out.exit_code,
                marked(&out.stdout, out.stdout_truncated),
                marked(&out.stderr, out.stderr_truncated)
            ),
        )),
    }
}
