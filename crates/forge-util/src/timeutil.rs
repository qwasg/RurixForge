//! UTC ISO8601 时间戳(零依赖;SystemTime → 民用历法换算)。
//! 原 engine-host / gend / engine-scene-mcp 三份逐字重复,合并于此(gend 版多 unix_millis)。

use std::time::{SystemTime, UNIX_EPOCH};

/// 当前 UTC 时间,ISO8601 形如 `2026-08-16T05:30:00Z`(秒级)。
pub fn utc_now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_utc(secs)
}

/// unix 毫秒(产物文件名用)。
pub fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// unix 秒 → `YYYY-MM-DDTHH:MM:SSZ`(Howard Hinnant 民用历法算法)。
fn format_unix_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let day_secs = secs % 86_400;
    let (hh, mm, ss) = (day_secs / 3600, (day_secs % 3600) / 60, day_secs % 60);
    // days since 1970-01-01 → 年月日
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_timestamps() {
        assert_eq!(format_unix_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(format_unix_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn now_is_well_formed() {
        let s = utc_now_iso8601();
        assert!(s.ends_with('Z') && s.len() == 20, "格式须为秒级 ISO8601:{s}");
    }

    #[test]
    fn unix_millis_smoke_positive() {
        assert!(unix_millis() > 0, "unix_millis 冒烟:返回值须 > 0");
    }
}
