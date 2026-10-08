//! 令牌面的纯函数:JWT 载荷读取、RFC3339 解析、登录/续期响应解析。
//! JWT 不验签——这里只读 `exp`(本地排程续期)与 `sid`(识别本机会话),鉴权永远由云端做。

use base64::Engine as _;
use serde_json::Value;

use super::CloudError;

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// JWT 第二段(载荷)→ JSON;格式不对 = None。
pub(crate) fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// RFC3339(`2026-09-26T10:00:00Z` / 带小数秒 / 带 `+08:00` 偏移)→ Unix 秒。
pub(crate) fn parse_rfc3339(s: &str) -> Option<u64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse().ok() };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        rest = &frac[digits..];
    }
    let offset = match rest.as_bytes().first()? {
        b'Z' | b'z' if rest.len() == 1 => 0,
        sign @ (b'+' | b'-') if rest.len() == 6 && rest.as_bytes()[3] == b':' => {
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6)?.parse().ok()?;
            let o = oh * 3600 + om * 60;
            if *sign == b'+' {
                o
            } else {
                -o
            }
        }
        _ => return None,
    };
    // days_from_civil(Howard Hinnant):公历日期 → 1970-01-01 起的天数。
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h * 3600 + mi * 60 + sec - offset;
    u64::try_from(secs).ok()
}

/// access token 的到期时刻(Unix 秒):JWT `exp` > 响应里的 `accessExpiresAt` > 现在 + 14 分钟。
pub(crate) fn access_expiry(token: &str, expires_at: Option<&str>) -> u64 {
    jwt_claims(token)
        .and_then(|c| c.get("exp").and_then(Value::as_u64))
        .or_else(|| expires_at.and_then(parse_rfc3339))
        .unwrap_or_else(|| now_unix() + 14 * 60)
}

/// 本机会话 ID(JWT `sid`)。
pub(crate) fn session_id_of(token: &str) -> Option<String> {
    jwt_claims(token)?
        .get("sid")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn required_str(v: &Value, key: &str) -> Result<String, CloudError> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| CloudError::new(502, "CLOUD_BAD_RESPONSE", format!("云端响应缺 {key}")))
}

/// `/auth/refresh` 与 `LoginResponse` 共有的令牌对。
pub(crate) struct TokenPair {
    pub access_token: String,
    pub access_expires_at: u64,
    pub refresh_token: String,
}

pub(crate) fn parse_token_pair(v: &Value) -> Result<TokenPair, CloudError> {
    let access_token = required_str(v, "accessToken")?;
    let refresh_token = required_str(v, "refreshToken")?;
    let access_expires_at = access_expiry(
        &access_token,
        v.get("accessExpiresAt").and_then(Value::as_str),
    );
    Ok(TokenPair {
        access_token,
        access_expires_at,
        refresh_token,
    })
}

/// `LoginResponse`(注册即登录同形)。
pub(crate) struct LoginGrant {
    pub tokens: TokenPair,
    pub user: Value,
    pub device_key: String,
    pub device_key_prefix: String,
}

pub(crate) fn parse_login(v: &Value) -> Result<LoginGrant, CloudError> {
    let tokens = parse_token_pair(v)?;
    let dk = v
        .get("deviceKey")
        .filter(|d| d.is_object())
        .ok_or_else(|| CloudError::new(502, "CLOUD_BAD_RESPONSE", "云端未签发设备 Key"))?;
    let device_key = required_str(dk, "key")?;
    let device_key_prefix = dk
        .get("prefix")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| device_key.chars().take(12).collect());
    Ok(LoginGrant {
        tokens,
        user: v.get("user").cloned().unwrap_or(Value::Null),
        device_key,
        device_key_prefix,
    })
}

#[cfg(test)]
pub(crate) fn fake_jwt(claims: &Value) -> String {
    let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
    format!(
        "{}.{}.{}",
        enc(br#"{"alg":"HS256","typ":"JWT"}"#),
        enc(claims.to_string().as_bytes()),
        enc(b"signature")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rfc3339_variants() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-09-26T10:00:00Z"), Some(1_790_416_800));
        assert_eq!(
            parse_rfc3339("2026-09-26T10:00:00.123456Z"),
            Some(1_790_416_800)
        );
        assert_eq!(
            parse_rfc3339("2026-09-26T18:00:00+08:00"),
            Some(1_790_416_800)
        );
        assert_eq!(parse_rfc3339("2000-02-29T00:00:00Z"), Some(951_782_400));
        assert_eq!(parse_rfc3339("not a date"), None);
        assert_eq!(parse_rfc3339("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339("2026-09-26T10:00:00"), None);
    }

    #[test]
    fn jwt_exp_and_sid() {
        let t = fake_jwt(&json!({ "sub": "1", "sid": "sess-9", "exp": 4_000_000_000u64 }));
        assert_eq!(
            access_expiry(&t, Some("1970-01-01T00:00:00Z")),
            4_000_000_000
        );
        assert_eq!(session_id_of(&t).as_deref(), Some("sess-9"));
        assert_eq!(
            access_expiry("opaque", Some("2026-09-26T10:00:00Z")),
            1_790_416_800
        );
        assert!(access_expiry("opaque", None) > now_unix());
    }

    #[test]
    fn login_requires_device_key() {
        let base = json!({
            "accessToken": "a", "refreshToken": "rt_1", "user": { "id": 1 },
            "deviceKey": null
        });
        assert_eq!(parse_login(&base).err().unwrap().code, "CLOUD_BAD_RESPONSE");
        let mut ok = base.clone();
        ok["deviceKey"] = json!({ "id": 3, "key": "sk-rf-abcdefghijklmn", "prefix": "sk-rf-abcd" });
        let g = parse_login(&ok).unwrap();
        assert_eq!(g.device_key_prefix, "sk-rf-abcd");
        assert_eq!(g.tokens.refresh_token, "rt_1");
    }
}
