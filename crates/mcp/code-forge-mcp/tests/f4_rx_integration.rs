//! 集成单测:真实 H:\rurix\target\debug\rx.exe / rurixc.exe。
//! exe 不存在则 [SKIP] 并 return(照 forge-agentd main.rs 集成测试先例)。

use std::path::PathBuf;

use serde_json::json;

use code_forge_mcp::rxtool;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace 根")
        .join("tests")
        .join("fixtures")
        .join("f4")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn skip_if_no_rx() -> bool {
    if rxtool::rx_cli().is_err() || rxtool::rurixc().is_err() {
        eprintln!("[SKIP] rx/rurixc 不存在(H:\\rurix\\target\\debug;env FORGE_RX_CLI/FORGE_RURIXC 可覆盖),集成测试跳过");
        return true;
    }
    false
}

#[test]
fn check_hello_clean() {
    if skip_if_no_rx() { return; }
    let v = rxtool::check(&json!({ "file": fixture("hello.rx") })).expect("rx_check hello.rx");
    assert_eq!(v["diagnostics"].as_array().expect("diagnostics 数组").len(), 0, "hello.rx 须零诊断");
}

#[test]
fn check_bad_reports_diag() {
    if skip_if_no_rx() { return; }
    let v = rxtool::check(&json!({ "file": fixture("bad.rx") })).expect("rx_check bad.rx");
    let diags = v["diagnostics"].as_array().expect("diagnostics 数组");
    assert!(!diags.is_empty(), "bad.rx 须有诊断");
    let d = &diags[0];
    assert!(d["code"].as_str().expect("code").starts_with("RX"), "code 须 RX 前缀: {d}");
    assert_eq!(d["severity"], "error");
    assert!(d["file"].as_str().expect("file").ends_with("bad.rx"));
    assert!(d["span"]["start"]["line"].is_u64(), "span.start.line 缺失: {d}");
    assert!(!d["message"].as_str().expect("message").is_empty());
}

#[test]
fn test_unit_tests_pass() {
    if skip_if_no_rx() { return; }
    let v = rxtool::test(&json!({ "file": fixture("unit_tests.rx") })).expect("rx_test unit_tests.rx");
    assert!(v["passed"].as_u64().expect("passed") >= 1, "passed>=1: {v}");
    assert_eq!(v["failed"], 0, "failed=0: {v}");
}

#[test]
fn test_fail_file_captured() {
    if skip_if_no_rx() { return; }
    let v = rxtool::test(&json!({ "file": fixture("unit_tests_fail.rx") })).expect("rx_test unit_tests_fail.rx");
    assert!(v["failed"].as_u64().expect("failed") >= 1, "failed>=1: {v}");
    let failures = v["failures"].as_array().expect("failures 数组");
    assert!(!failures.is_empty(), "failures 非空: {v}");
    assert_eq!(failures[0]["name"], "fails_nonzero");
}

#[test]
fn fmt_check_formatted_file() {
    if skip_if_no_rx() { return; }
    let v = rxtool::fmt(&json!({ "file": fixture("hello.rx"), "checkOnly": true })).expect("rx_fmt --check hello.rx");
    assert_eq!(v["needsFormat"], false, "hello.rx 已格式化: {v}");
}

#[test]
fn test_missing_file_is_usage_error() {
    if skip_if_no_rx() { return; }
    let e = rxtool::test(&json!({})).expect_err("缺 file 须 USAGE");
    assert_eq!(e.code, "USAGE");
}
