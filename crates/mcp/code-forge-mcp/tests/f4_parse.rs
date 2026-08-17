//! 纯解析单测:不依赖 rx.exe/rurixc.exe(任何机器可跑)。

use std::time::{Duration, UNIX_EPOCH};

use serde_json::json;

use code_forge_mcp::rxtool;

#[test]
fn parse_pass_summary() {
    // 实测形态:stdout 逐测试 "..."/"... ok" + 汇总 "rx test: PASS 2/2"。
    let stdout = "rx test: unit_ok ...\nrx test: unit_ok ... ok\nrx test: returns_zero ...\nrx test: returns_zero ... ok\nrx test: PASS 2/2\n";
    let (passed, failed, failures) = rxtool::parse_test_output(stdout, "").expect("PASS 汇总须解析");
    assert_eq!((passed, failed), (2, 0));
    assert!(failures.is_empty());
}

#[test]
fn parse_fail_summary_and_rx7011() {
    // 实测形态:stdout 只打 "rx test: <name> ...";stderr RX7011 明细 + FAIL 汇总。
    let stdout = "rx test: fails_nonzero ...\n";
    let stderr = "rx test: error[RX7011]: fails_nonzero: 子进程退出 exit code: 1\nrx test: FAIL 0 passed; 1 failed\n";
    let (passed, failed, failures) = rxtool::parse_test_output(stdout, stderr).expect("FAIL 汇总须解析");
    assert_eq!((passed, failed), (0, 1));
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, "fails_nonzero");
    // detail 含 ": " 时只在第一处切分,余下完整保留。
    assert_eq!(failures[0].1, "子进程退出 exit code: 1");
}

#[test]
fn parse_no_summary_is_none() {
    // 发现错误(RX7010)无 PASS/FAIL 汇总 → None(调用方如实 TOOL_ERROR,不伪造 0/0)。
    assert!(rxtool::parse_test_output("", "rx test: error[RX7010]: 未发现匹配的 #[test] 测试\n").is_none());
}

#[test]
fn parse_text_diag_lines() {
    // 实测 rx build bad.rx stderr 首行形态。
    let stderr = "error[RX0008]: expected an expression, found `;`\n --> bad.rx:2:18\n  |\n2 |     let broken = ;\n";
    let diags = rxtool::parse_text_diags(stderr, "bad.rx");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0]["code"], "RX0008");
    assert_eq!(diags[0]["severity"], "error");
    assert_eq!(diags[0]["file"], "bad.rx");
    assert_eq!(diags[0]["message"], "expected an expression, found `;`");
}

#[test]
fn map_check_diag_shape() {
    // 实测 rurixc --error-format=json 单条诊断字段(labels 无 file 字段)。
    let raw = json!({
        "level": "error",
        "message": "expected an expression, found `;`",
        "code": "RX0008",
        "labels": [{ "start": { "line": 1, "character": 17 }, "end": { "line": 1, "character": 18 }, "message": "expected an expression" }]
    });
    let d = rxtool::map_check_diag(&raw, "bad.rx");
    assert_eq!(d["code"], "RX0008");
    assert_eq!(d["severity"], "error");
    assert_eq!(d["file"], "bad.rx");
    assert_eq!(d["span"]["start"]["line"], 1);
    assert_eq!(d["span"]["start"]["character"], 17);
    assert!(d["message"].as_str().unwrap().contains("expected an expression"));
    assert!(d.get("suggestion").is_none());
}

#[test]
fn read_capped_truncates_over_1mb() {
    let small = b"hello".to_vec();
    let (s, t) = rxtool::read_capped(small.as_slice());
    assert_eq!(s, "hello");
    assert!(!t);
    let big = vec![b'x'; rxtool::CAP + 10];
    let (s, t) = rxtool::read_capped(big.as_slice());
    assert!(t);
    assert_eq!(s.len(), rxtool::CAP);
}

#[test]
fn utc_stamp_known_dates() {
    assert_eq!(rxtool::utc_stamp(UNIX_EPOCH), "19700101-000000");
    // 2026-01-01T00:00:00Z = 1767225600(2025 平年 365 天)。
    assert_eq!(rxtool::utc_stamp(UNIX_EPOCH + Duration::from_secs(1_767_225_600)), "20260101-000000");
}

#[test]
fn missing_rx_cli_reports_not_found() {
    // env 指向不存在路径 → RX_CLI_NOT_FOUND(本测试二进制内唯一触碰该 env 的用例)。
    std::env::set_var("FORGE_RX_CLI", r"Z:\no\such\dir\rx.exe");
    let e = rxtool::rx_cli().expect_err("不存在路径须报错");
    assert_eq!(e.code, "RX_CLI_NOT_FOUND");
    assert!(e.message.contains(r"Z:\no\such\dir\rx.exe"));
    std::env::remove_var("FORGE_RX_CLI");
}
