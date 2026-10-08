//! UltraPlan browser probe: a fixed Node/Playwright runner, isolated loopback Demo host,
//! real keyboard/mouse inputs and persisted evidence. Missing tooling never means PASS.
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

fn failure(message: impl Into<String>, unavailable: bool) -> Value {
    json!({"ok": false, "unavailable": unavailable, "inputVerified": false,
        "errors": [message.into()], "screenshot": null, "assertions": [], "console": []})
}

/// Resolve a child directory without allowing lexical traversal or existing links to escape.
fn inside(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("WORKSPACE_UNAVAILABLE: {e}"))?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if absolute
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("PROBE_PATH_INVALID".into());
    }
    let mut ancestor = absolute.as_path();
    let mut tail = Vec::new();
    while !ancestor.exists() {
        tail.push(
            ancestor
                .file_name()
                .ok_or_else(|| "PROBE_PATH_INVALID".to_string())?,
        );
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "PROBE_PATH_INVALID".to_string())?;
    }
    let mut resolved = ancestor
        .canonicalize()
        .map_err(|e| format!("PROBE_PATH_INVALID: {e}"))?;
    if !resolved.starts_with(&root) {
        return Err("PROBE_PATH_OUTSIDE_WORKSPACE".into());
    }
    for part in tail.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn packaged_runtime(directory: &Path) -> Result<(PathBuf, PathBuf), String> {
    let node = directory.join(if cfg!(windows) { "node.exe" } else { "node" });
    let runner = directory.join("web-demo-probe.mjs");
    if !node.is_file()
        || !runner.is_file()
        || !directory
            .join("node_modules/playwright-core/package.json")
            .is_file()
        || !directory.join("runtime-manifest.json").is_file()
    {
        return Err(format!("PROBE_RUNTIME_INCOMPLETE: {}", directory.display()));
    }
    Ok((node, runner))
}

fn resolve_runtime() -> Result<(PathBuf, PathBuf), String> {
    if let Some(directory) = std::env::var_os("FORGE_ULTRAPLAN_RUNTIME_DIR") {
        // An explicit deployment setting must fail visibly instead of falling back.
        return packaged_runtime(Path::new(&directory));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            for directory in [
                parent.join("ultraplan-runtime"),
                parent.join("resources/ultraplan-runtime"),
                parent.join("../resources/ultraplan-runtime"),
            ] {
                if directory.exists() {
                    return packaged_runtime(&directory);
                }
            }
        }
    }
    // This fallback is deliberately limited to a developer checkout. Release
    // placement above takes precedence and includes its own Node executable.
    let root = crate::workspace_root();
    let staged = root.join("apps/desktop/dist/ultraplan-runtime");
    if staged.exists() {
        return packaged_runtime(&staged);
    }
    let runner = root.join("tools/e2e/web-demo-probe.mjs");
    if runner.is_file()
        && runner
            .parent()
            .unwrap()
            .join("node_modules/playwright-core/package.json")
            .is_file()
    {
        return Ok((PathBuf::from("node"), runner));
    }
    Err("PROBE_RUNTIME_UNAVAILABLE: 安装 ultraplan-runtime (Node + Playwright)，或设置 FORGE_ULTRAPLAN_RUNTIME_DIR".into())
}

/// `request` is probe.json data: script hooks, real browser inputs, state assertions.
/// Empty request reads `<demo_dir>/probe.json`. Artifacts have a unique subdirectory.
pub async fn probe(
    workspace_root: &Path,
    demo_dir: &Path,
    token: &str,
    output_dir: &Path,
    request: &Value,
) -> Value {
    let result = probe_inner(workspace_root, demo_dir, token, output_dir, request).await;
    result.unwrap_or_else(|(message, unavailable)| failure(message, unavailable))
}

async fn probe_inner(
    workspace_root: &Path,
    demo_dir: &Path,
    token: &str,
    output_dir: &Path,
    request: &Value,
) -> Result<Value, (String, bool)> {
    let fail = |message: String| (message, false);
    let root = workspace_root
        .canonicalize()
        .map_err(|e| fail(e.to_string()))?;
    let demo = inside(&root, demo_dir).map_err(fail)?;
    let out = inside(&root, output_dir).map_err(fail)?;
    if !demo.join("index.html").is_file() {
        return Err(fail("DEMO_ENTRY_MISSING: index.html".into()));
    }
    if token.is_empty()
        || token.len() > 128
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(fail("DEMO_TOKEN_INVALID".into()));
    }
    let mut request = if request.as_object().is_some_and(|o| !o.is_empty()) {
        request.clone()
    } else {
        let spec = inside(&root, &demo.join("probe.json")).map_err(fail)?;
        let text =
            std::fs::read_to_string(spec).map_err(|e| fail(format!("PROBE_SPEC_MISSING: {e}")))?;
        if text.len() > 65536 {
            return Err(fail("PROBE_SPEC_TOO_LARGE".into()));
        }
        serde_json::from_str(&text).map_err(|e| fail(format!("PROBE_SPEC_INVALID: {e}")))?
    };
    let object = request
        .as_object_mut()
        .ok_or_else(|| fail("PROBE_SPEC_INVALID: expected object".into()))?;
    let registry = crate::demo_host::global();
    registry.register(token, &demo);
    let port = registry.ensure_started().await.map_err(fail)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let out = out.join(format!("probe-{stamp}"));
    std::fs::create_dir_all(&out).map_err(|e| fail(format!("PROBE_OUTPUT_FAILED: {e}")))?;
    let shot = out.join("screenshot.png");
    let report_path = out.join("report.json");
    object.insert(
        "url".into(),
        json!(crate::demo_host::local_url(port, token)),
    );
    object.insert("screenshotPath".into(), json!(shot));
    object.insert("reportPath".into(), json!(report_path));
    let (node, runner) = resolve_runtime().map_err(|message| (message, true))?;
    let mut command = tokio::process::Command::new(node);
    command
        .arg(&runner)
        .current_dir(runner.parent().unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|e| (format!("NODE_UNAVAILABLE: {e}"), true))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| fail("PROBE_STDIN_UNAVAILABLE".into()))?;
    stdin
        .write_all(&serde_json::to_vec(&request).unwrap())
        .await
        .map_err(|e| fail(format!("PROBE_STDIN_FAILED: {e}")))?;
    drop(stdin);
    let output = tokio::time::timeout(Duration::from_secs(80), child.wait_with_output())
        .await
        .map_err(|_| fail("PROBE_TIMEOUT: runner exceeded 80 seconds".into()))?
        .map_err(|e| fail(format!("PROBE_RUNNER_FAILED: {e}")))?;
    let mut report: Value = serde_json::from_slice(&output.stdout).map_err(|e| {
        fail(format!(
            "PROBE_REPORT_INVALID: {e}; {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(1000)
                .collect::<String>()
        ))
    })?;
    if !report.is_object() {
        return Err(fail("PROBE_REPORT_INVALID: expected object".into()));
    }
    if !output.status.success() {
        report["ok"] = json!(false);
    }
    if report["inputVerified"] != true
        || report["assertions"]
            .as_array()
            .map_or(true, |a| a.is_empty())
        || !shot.is_file()
    {
        report["ok"] = json!(false);
    }
    report["screenshot"] = if shot.is_file() {
        json!(relative(&root, &shot))
    } else {
        Value::Null
    };
    report["reportPath"] = json!(relative(&root, &report_path));
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap())
        .map_err(|e| fail(format!("PROBE_REPORT_WRITE_FAILED: {e}")))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_is_scoped_and_missing_tool_is_not_pass() {
        let root = std::env::temp_dir().canonicalize().unwrap();
        assert!(inside(&root, Path::new("demo/probe"))
            .unwrap()
            .starts_with(&root));
        assert!(inside(&root, Path::new("../escape")).is_err());
        assert_eq!(failure("no browser", true)["ok"], false);
        assert_eq!(failure("no browser", true)["unavailable"], true);
    }

    #[test]
    fn incomplete_runtime_is_explicitly_unavailable() {
        assert!(packaged_runtime(&std::env::temp_dir())
            .unwrap_err()
            .contains("PROBE_RUNTIME_INCOMPLETE"));
    }
}
