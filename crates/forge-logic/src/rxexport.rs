//! `.rx` `#[export(c)]` 导出表文本扫描器(RD-F4-004 回填波,D-RD4-A)。
//!
//! 诚实边界(照 code_symbol_search 文本级先例):**文本级非语义级**——识别行首
//! `#[export(c)]`(可带 `name = "..."` 覆写)紧跟 `pub fn name(p: T, ...) -> R {` 形态,
//! 签名可跨行;宏生成/条件编译/注释内伪装条目不保证排除。编译期真子集校验由
//! `rurixc --emit=dll` 把关(RX6031/6033 结构化诊断,wave.2 构建腿透传)。
//! 实测锚定(2026-08-18):`rurixc --emit=reflection` 对宿主 fn 产 `entries=[]`
//! (shader-only,RXS-0304),fn 表不能走 reflection——故文本扫描。

/// 单个 `#[export(c)]` 导出函数(文本级事实)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedFn {
    /// .rx fn 名(图 call_function fn pin 的引用面)。
    pub name: String,
    /// C ABI 导出符号名(`name = "..."` 覆写;缺省 = name)。wave.2 dll 运行时按此名 GetProcAddress。
    pub export_name: String,
    /// 形参(名, 类型标记原文 trim)。
    pub params: Vec<(String, String)>,
    /// 返回类型标记原文;"void" = 无 `->` 声明。
    pub ret: String,
}

/// 扫描 .rx 源文本,返回全部 `#[export(c)] pub fn` 条目(源序)。
pub fn scan_export_c_fns(source: &str) -> Vec<ExportedFn> {
    let mut out = Vec::new();
    // Some(name_override?);其他属性行/非空非注释行重置。
    let mut pending: Option<Option<String>> = None;
    let mut lines = source.lines().peekable();
    while let Some(raw) = lines.next() {
        let line = raw.trim();
        if line.starts_with("#[export(") {
            pending = Some(parse_name_override(line));
            continue;
        }
        if line.starts_with("#[") {
            pending = None;
            continue;
        }
        if line.is_empty() || line.starts_with("//") {
            continue; // 空行/注释不重置(属性与 fn 间允许注释/空行)。
        }
        if let Some(ov) = pending.take() {
            if line.starts_with("pub fn ") {
                // 签名可跨行:累积至含 '{' 的行。
                let mut sig = line.to_string();
                while !sig.contains('{') {
                    match lines.next() {
                        Some(next) => {
                            sig.push(' ');
                            sig.push_str(next.trim());
                        }
                        None => break,
                    }
                }
                if let Some(mut f) = parse_signature(&sig) {
                    f.export_name = ov.unwrap_or_else(|| f.name.clone());
                    out.push(f);
                }
            }
            // 非 pub fn 行:pending 已 take 即重置,不产出。
        }
    }
    out
}

/// Native Rust scripts use the same scalar ABI as Rurix. Rustc remains the
/// authority for symbol emission; this scanner only supplies marshalling types.
pub fn scan_rust_c_fns(source: &str) -> Vec<ExportedFn> {
    let normalized = source.lines().map(|line| {
        let trimmed = line.trim();
        if trimmed == "#[no_mangle]" || trimmed == "#[unsafe(no_mangle)]" {
            "#[export(c)]".to_string()
        } else if trimmed.starts_with("pub extern \"C\" fn ") {
            trimmed.replacen("pub extern \"C\" fn ", "pub fn ", 1)
        } else { line.to_string() }
    }).collect::<Vec<_>>().join("\n");
    scan_export_c_fns(&normalized)
}

/// 解析 `#[export(c)]` / `#[export(c, name = "foo")]` 的 name 覆写。
fn parse_name_override(attr: &str) -> Option<String> {
    let inner = attr.strip_prefix("#[export(")?.strip_suffix(")]")?;
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("name") {
            let rest = rest.trim().strip_prefix('=')?.trim();
            return Some(rest.trim_matches('"').to_string());
        }
    }
    None
}

/// 解析签名文本(已含到 '{' 为止):`pub fn name(p0: t0, p1: t1) -> ret {`。
fn parse_signature(sig: &str) -> Option<ExportedFn> {
    let head = sig.split('{').next()?.trim();
    let head = head.strip_prefix("pub fn ")?.trim();
    let lp = head.find('(')?;
    let name = head[..lp].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let rp = head.rfind(')')?;
    if rp < lp {
        return None;
    }
    let params_text = &head[lp + 1..rp];
    let mut params = Vec::new();
    for p in params_text.split(',') {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        let (pn, pt) = p.split_once(':')?;
        params.push((pn.trim().to_string(), pt.trim().to_string()));
    }
    let tail = head[rp + 1..].trim();
    let ret = match tail.strip_prefix("->") {
        Some(t) => t.trim().to_string(),
        None => "void".to_string(),
    };
    Some(ExportedFn { name, export_name: String::new(), params, ret })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rust_exports_require_explicit_c_abi_and_symbol() {
        let source = "#[no_mangle]\npub extern \"C\" fn tick(dt: f32) -> f32 { dt }\n#[unsafe(no_mangle)]\npub extern \"C\" fn reset() {}\npub extern \"C\" fn hidden() {}\n";
        let fns = scan_rust_c_fns(source);
        assert_eq!(fns.len(), 2);
        assert_eq!(fns[0].params, vec![("dt".into(), "f32".into())]);
        assert_eq!(fns[1].ret, "void");
    }

    #[test]
    fn scans_basic_export() {
        let src = "#[export(c)]\npub fn add(a: f32, b: f32) -> f32 { a + b }\n";
        let fns = scan_export_c_fns(src);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name, "add");
        assert_eq!(fns[0].export_name, "add");
        assert_eq!(fns[0].params, vec![("a".to_string(), "f32".to_string()), ("b".to_string(), "f32".to_string())]);
        assert_eq!(fns[0].ret, "f32");
    }

    #[test]
    fn name_override_and_void_ret() {
        let src = "#[export(c, name = \"is_ready_abi\")]\npub fn is_ready(flag: bool) -> bool { flag }\n";
        let fns = scan_export_c_fns(src);
        assert_eq!(fns[0].name, "is_ready");
        assert_eq!(fns[0].export_name, "is_ready_abi");
        assert_eq!(fns[0].ret, "bool");
        // 无 -> 声明 → void。
        let src2 = "#[export(c)]\npub fn poke() { }\n";
        let fns2 = scan_export_c_fns(src2);
        assert_eq!(fns2[0].ret, "void");
        assert!(fns2[0].params.is_empty());
    }

    #[test]
    fn multiline_signature_and_comments_between() {
        let src = "#[export(c)]\n// 导出加法\n\npub fn add(\n    a: f32,\n    b: f32,\n) -> f32 {\n    a + b\n}\n";
        let fns = scan_export_c_fns(src);
        assert_eq!(fns.len(), 1, "{fns:?}");
        assert_eq!(fns[0].params.len(), 2);
    }

    #[test]
    fn non_export_fns_ignored() {
        let src = "pub fn helper(x: i32) -> i32 { x }\n#[test]\nfn t() {}\n#[export(c)]\npub fn mul(a: f64, b: f64) -> f64 { a * b }\n";
        let fns = scan_export_c_fns(src);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name, "mul");
        assert_eq!(fns[0].ret, "f64");
    }

    #[test]
    fn non_pub_fn_after_attr_ignored() {
        // RX6033:属性只能挂 pub fn;文本级如实不产出(编译期由 rurixc 拒)。
        let src = "#[export(c)]\nfn hidden(a: f32) -> f32 { a }\n";
        assert!(scan_export_c_fns(src).is_empty());
    }
}
