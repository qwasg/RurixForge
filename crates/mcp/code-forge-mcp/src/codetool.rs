//! code_* 三工具(F4 wave.4,05 §4):code_symbol_search / code_references / code_structured_edit。
//!
//! 诚实注记(实测 2026-08-17,H:\rurix\target\debug):
//! - `rurixc <file> --emit=reflection` 产物 = shader 接口文档(RXS-0304/0305):只枚举
//!   vertex/fragment/compute/mesh/kernel entry(普通 fn 不出现),且 JSON 不含 file/span
//!   (禁用面),hello.rx 实测 entries=[] —— 不能充当通用符号表。故 code_symbol_search 走
//!   **文本扫描器**(逐行匹配条目头,非语义级,注记于工具 description),span 为 0 基
//!   line/character(字符计,与上游 LSP 口径一致)。
//! - code_references 走 rurixc --tooling-server 常驻 LSP 会话(lspclient):同源定位定义 →
//!   didOpen → textDocument/references。上游 ToolingSession 单文档语义(session.rs),
//!   refs 限定义所在文件;跨文件引用如实给不出。
//! - code_structured_edit:span(0 基 line/character 文本区间)或 symbolQuery(扫本文件符号
//!   名区间)逐项解析 → 重叠校验 → 倒序应用 → 写回 → rurixc --emit=check 回读诊断。
//!   任一 edit 越界/定位失败/区间重叠 → 不写盘,{applied:false, error} 如实返回。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::lspclient;
use crate::rxtool::{self, TResult, ToolError};

fn terr(code: &str, message: impl Into<String>) -> ToolError {
    ToolError { code: code.to_string(), message: message.into() }
}

// ───────────────────────── 文本符号扫描器 ─────────────────────────

/// 扫描出的条目符号(0 基;line/character 均为字符口径,与上游 LSP 一致)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    pub line: u32,
    pub col_start: u32,
    pub col_end: u32,
}

/// 条目关键字闭集(文本扫描面)。
const ITEM_KEYWORDS: &[&str] = &["fn", "struct", "enum", "mod", "trait", "const", "static", "type"];
/// 着色阶段前缀(仅修饰 fn:`vertex fn vs_main` 等)。
const STAGE_PREFIXES: &[&str] = &["vertex", "fragment", "compute", "mesh", "kernel", "task"];

/// 文本扫描 .rx 源,产出条目符号表。
/// 局限(如实):不解析语法,注释/字符串内的形似行、fn 体内的嵌套定义可能误判;够用口径 =
/// 行首(可空缩进)+ 可选 `pub` + 可选阶段前缀 + 条目关键字 + 标识符。
pub fn scan_symbols(text: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        let mut rest = t;
        if let Some(r) = rest.strip_prefix("pub ") {
            rest = r.trim_start();
        }
        // 可选阶段前缀(后跟 fn 才成立)。
        for st in STAGE_PREFIXES {
            if let Some(r) = rest.strip_prefix(st) {
                let r2 = r.trim_start();
                if r2.starts_with("fn ") {
                    rest = r2;
                }
                break;
            }
        }
        for kw in ITEM_KEYWORDS {
            let Some(after) = rest.strip_prefix(kw) else { continue };
            // 关键字边界:次字符须为空白(挡 `fnord` 类前缀撞车)。
            if !after.chars().next().is_some_and(char::is_whitespace) {
                continue;
            }
            let name_part = after.trim_start();
            let name: String = name_part
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            // 名字起点 → 字符列(byte 偏移 → chars 计)。
            let name_byte_in_line = line.len() - name_part.len();
            let col_start = line[..name_byte_in_line].chars().count() as u32;
            out.push(Symbol {
                col_end: col_start + name.chars().count() as u32,
                col_start,
                name,
                kind: kw,
                line: line_no as u32,
            });
            break;
        }
    }
    out
}

/// 递归收集 .rx 文件(排除 target / vendor / 点开头目录);读目录失败如实计入 skipped。
fn walk_rx(dir: &Path, files: &mut Vec<PathBuf>, skipped: &mut Vec<Value>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            skipped.push(json!({ "file": dir.to_string_lossy(), "reason": format!("读目录失败: {e}") }));
            return;
        }
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            let name = ent.file_name().to_string_lossy().to_string();
            if name == "target" || name == "vendor" || name.starts_with('.') {
                continue;
            }
            walk_rx(&p, files, skipped);
        } else if p.extension().is_some_and(|e| e == "rx") {
            files.push(p);
        }
    }
}

/// 项目相对路径(正斜杠形态,跨平台一致)。
fn rel_slash(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 收项目 .rx 文件并逐文件扫符号;读文件失败计入 skipped(不遮蔽)。
fn scan_project_symbols(root: &Path) -> (Vec<(String, Symbol)>, Vec<Value>) {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    walk_rx(root, &mut files, &mut skipped);
    files.sort();
    let mut out: Vec<(String, Symbol)> = Vec::new();
    for f in files {
        let rel = rel_slash(root, &f);
        match std::fs::read_to_string(&f) {
            Ok(text) => {
                for s in scan_symbols(&text) {
                    out.push((rel.clone(), s));
                }
            }
            Err(e) => skipped.push(json!({ "file": rel, "reason": format!("读文件失败: {e}") })),
        }
    }
    (out, skipped)
}

/// code_symbol_search {query, kinds?} → {symbols:[{name,kind,file,span}], skipped[]}。
/// query = 名字子串(大小写不敏感);kinds = 条目类闭集过滤(fn/struct/enum/...)。
pub fn symbol_search(args: &Value, root: &Path) -> TResult<Value> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| terr("USAGE", "缺 query 参数(符号名子串,大小写不敏感)"))?;
    let kinds: Option<BTreeSet<String>> = match args.get("kinds") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let arr = v.as_array().ok_or_else(|| terr("USAGE", "kinds 须为字符串数组"))?;
            let mut set = BTreeSet::new();
            for k in arr {
                set.insert(
                    k.as_str()
                        .ok_or_else(|| terr("USAGE", "kinds 元素须为字符串"))?
                        .to_string(),
                );
            }
            Some(set)
        }
    };
    let q = query.to_lowercase();
    let (all, skipped) = scan_project_symbols(root);
    let symbols: Vec<Value> = all
        .iter()
        .filter(|(_, s)| s.name.to_lowercase().contains(&q))
        .filter(|(_, s)| kinds.as_ref().is_none_or(|ks| ks.contains(s.kind)))
        .map(|(rel, s)| {
            json!({
                "name": s.name,
                "kind": s.kind,
                "file": rel,
                "span": {
                    "start": { "line": s.line, "character": s.col_start },
                    "end": { "line": s.line, "character": s.col_end }
                }
            })
        })
        .collect();
    Ok(json!({ "symbols": symbols, "skipped": skipped }))
}

/// 定位符号唯一定义:精确(大小写敏感)→ 精确(不敏感)→ 子串(不敏感),逐档收敛;
/// 0 → SYMBOL_NOT_FOUND;>1 → AMBIGUOUS_SYMBOL(附候选,不猜测归属)。
fn locate_symbol(root: &Path, query: &str) -> TResult<(PathBuf, String, Symbol)> {
    let (all, _) = scan_project_symbols(root);
    let ql = query.to_lowercase();
    let tiers: [Box<dyn Fn(&Symbol) -> bool>; 3] = [
        Box::new(|s: &Symbol| s.name == query),
        Box::new(|s: &Symbol| s.name.to_lowercase() == ql),
        Box::new(|s: &Symbol| s.name.to_lowercase().contains(&ql)),
    ];
    for tier in &tiers {
        let hits: Vec<&(String, Symbol)> = all.iter().filter(|(_, s)| tier(s)).collect();
        if hits.len() == 1 {
            let (rel, sym) = hits[0];
            return Ok((root.join(rel), rel.clone(), sym.clone()));
        }
        if hits.len() > 1 {
            let cands = hits
                .iter()
                .map(|(rel, s)| format!("{rel}:{} ({})", s.line, s.name))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(terr(
                "AMBIGUOUS_SYMBOL",
                format!("符号 `{query}` 命中 {} 处定义: {cands}", hits.len()),
            ));
        }
    }
    Err(terr("SYMBOL_NOT_FOUND", format!("符号 `{query}` 无匹配定义(项目 .rx 扫描面内)")))
}

/// 绝对路径 → LSP uri(上游 tooling_smoke 同款:`file:///` + 反斜杠转正斜杠)。
fn uri_for(abs: &Path) -> String {
    format!("file:///{}", abs.to_string_lossy().replace('\\', "/"))
}

/// code_references {symbolQuery} → {refs:[{file,span}]}。
/// 同源逻辑定位定义 → LSP 会话 didOpen + textDocument/references;会话崩 → 重连一次再如实报错。
/// refs 限定义所在文件(上游 ToolingSession 单文档语义)。
pub fn references(args: &Value, root: &Path) -> TResult<Value> {
    let query = args
        .get("symbolQuery")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| terr("USAGE", "缺 symbolQuery 参数(符号名字符串)"))?;
    let (abs, rel, sym) = locate_symbol(root, query)?;
    let text = std::fs::read_to_string(&abs)
        .map_err(|e| terr("TOOL_ERROR", format!("读 {} 失败: {e}", abs.display())))?;
    let uri = uri_for(&abs);
    // 第一次失败(会话崩/超时)→ with_lsp 已作废旧会话,第二次走全新连接;再败如实报。
    let result = match lspclient::references_at(&uri, &text, sym.line, sym.col_start) {
        Ok(v) => v,
        Err(e1) => lspclient::references_at(&uri, &text, sym.line, sym.col_start).map_err(|e2| {
            terr(
                "LSP_ERROR",
                format!("references 两次尝试均败(已重连一次): 一={} {};二={} {}", e1.code, e1.message, e2.code, e2.message),
            )
        })?,
    };
    let arr = result
        .as_array()
        .ok_or_else(|| terr("LSP_PROTO", format!("references result 非数组: {result}")))?;
    let refs: Vec<Value> = arr
        .iter()
        .map(|l| {
            let r = &l["range"];
            let file = match l.get("uri").and_then(Value::as_str) {
                Some(u) if u == uri => rel.clone(),
                // 单文档语义下不应出现异 uri;出现则如实透传原 uri。
                other => other.unwrap_or("").to_string(),
            };
            json!({ "file": file, "span": { "start": r["start"].clone(), "end": r["end"].clone() } })
        })
        .collect();
    Ok(json!({ "refs": refs }))
}

// ───────────────────────── code_structured_edit ─────────────────────────

/// 行首字节偏移表(text 按 \n 切)。
fn line_offsets(text: &str) -> Vec<usize> {
    let mut offs = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            offs.push(i + 1);
        }
    }
    offs
}

/// (line, character)(0 基,character 字符口径)→ 字节偏移;越界 → (code, message)。
fn pos_to_off(text: &str, pos: &Value) -> Result<usize, (String, String)> {
    let line = pos
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| ("USAGE".to_string(), "span 缺 line(u32)".to_string()))? as usize;
    let character = pos
        .get("character")
        .and_then(Value::as_u64)
        .ok_or_else(|| ("USAGE".to_string(), "span 缺 character(u32)".to_string()))? as usize;
    let lines: Vec<&str> = text.split('\n').collect();
    if line >= lines.len() {
        return Err((
            "SPAN_OUT_OF_RANGE".to_string(),
            format!("line {line} 越界(文件共 {} 行)", lines.len()),
        ));
    }
    let content = lines[line].strip_suffix('\r').unwrap_or(lines[line]);
    let char_len = content.chars().count();
    if character > char_len {
        return Err((
            "SPAN_OUT_OF_RANGE".to_string(),
            format!("character {character} 越界(line {line} 共 {char_len} 字符)"),
        ));
    }
    let byte_in_line = content
        .char_indices()
        .nth(character)
        .map(|(i, _)| i)
        .unwrap_or(content.len());
    Ok(line_offsets(text)[line] + byte_in_line)
}

/// 倒序应用(buf[0..e] 此时恒等于原文前缀,见重叠校验注释)。
fn splice(buf: &mut String, start: usize, end: usize, content: &str) {
    let tail = buf[end..].to_owned();
    buf.truncate(start);
    buf.push_str(content);
    buf.push_str(&tail);
}

/// code_structured_edit {file, edits:[{kind:replace|insert|delete, span|symbolQuery, content?}]}
/// → {applied:true, newDiagnostics[]} | {applied:false, error}(不写盘)。
pub fn structured_edit(args: &Value, root: &Path) -> TResult<Value> {
    let file = args
        .get("file")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| terr("USAGE", "缺 file 参数(项目相对路径)"))?;
    let edits = args
        .get("edits")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| terr("USAGE", "缺 edits 参数(非空数组)"))?;
    let abs = match forge_util::pathutil::confine_under(&[root], file) {
        Ok(p) => p,
        Err(e) => {
            return Ok(json!({
                "applied": false,
                "error": { "code": "PATH_OUTSIDE_ROOT", "message": e }
            }));
        }
    };
    let fail = |code: &str, msg: String| Ok(json!({ "applied": false, "error": { "code": code, "message": msg } }));
    let text = match std::fs::read_to_string(&abs) {
        Ok(t) => t,
        Err(e) => return fail("FILE_NOT_FOUND", format!("读 {file} 失败: {e}")),
    };

    // ── 逐项解析为原文区间 (start_off, end_off, content) ──
    let mut resolved: Vec<(usize, usize, String)> = Vec::with_capacity(edits.len());
    for (i, e) in edits.iter().enumerate() {
        let kind = e
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| terr("USAGE", format!("edits[{i}] 缺 kind")))?;
        if !matches!(kind, "replace" | "insert" | "delete") {
            return Err(terr("USAGE", format!("edits[{i}] kind 须为 replace|insert|delete: {kind}")));
        }
        let content = e.get("content").and_then(Value::as_str).unwrap_or("").to_string();
        let (s, t) = if let Some(span) = e.get("span") {
            let s = match pos_to_off(&text, &span["start"]) {
                Ok(o) => o,
                Err((c, m)) if c == "USAGE" => return Err(terr("USAGE", format!("edits[{i}] {m}"))),
                Err((c, m)) => return fail(&c, format!("edits[{i}] {m}")),
            };
            let t = match pos_to_off(&text, &span["end"]) {
                Ok(o) => o,
                Err((c, m)) if c == "USAGE" => return Err(terr("USAGE", format!("edits[{i}] {m}"))),
                Err((c, m)) => return fail(&c, format!("edits[{i}] {m}")),
            };
            if t < s {
                return fail("SPAN_OUT_OF_RANGE", format!("edits[{i}] span end < start"));
            }
            (s, t)
        } else if let Some(sq) = e.get("symbolQuery").and_then(Value::as_str) {
            // 本文件内定位符号名区间(精确 → 不敏感精确 → 不敏感子串)。
            let syms = scan_symbols(&text);
            let sql = sq.to_lowercase();
            let tiers: [Box<dyn Fn(&Symbol) -> bool>; 3] = [
                Box::new(|s: &Symbol| s.name == sq),
                Box::new(|s: &Symbol| s.name.to_lowercase() == sql),
                Box::new(|s: &Symbol| s.name.to_lowercase().contains(&sql)),
            ];
            let mut found: Option<Symbol> = None;
            for tier in &tiers {
                let hits: Vec<&Symbol> = syms.iter().filter(|s| tier(s)).collect();
                if hits.len() == 1 {
                    found = Some((*hits[0]).clone());
                    break;
                }
                if hits.len() > 1 {
                    let names = hits.iter().map(|s| format!("{}:{}", s.line, s.name)).collect::<Vec<_>>().join(", ");
                    return fail("AMBIGUOUS_SYMBOL", format!("edits[{i}] symbolQuery `{sq}` 命中 {} 处: {names}", hits.len()));
                }
            }
            let Some(sym) = found else {
                return fail("SYMBOL_NOT_FOUND", format!("edits[{i}] symbolQuery `{sq}` 在 {file} 无匹配"));
            };
            let s = pos_to_off(&text, &json!({ "line": sym.line, "character": sym.col_start }))
                .map_err(|(c, m)| terr(&c, m))?;
            let t = pos_to_off(&text, &json!({ "line": sym.line, "character": sym.col_end }))
                .map_err(|(c, m)| terr(&c, m))?;
            (s, t)
        } else {
            return Err(terr("USAGE", format!("edits[{i}] 须含 span 或 symbolQuery")));
        };
        match kind {
            "replace" => resolved.push((s, t, content)),
            "delete" => resolved.push((s, t, String::new())),
            // insert:于 span.start 处插入(end 不消费)。
            _ => resolved.push((s, s, content)),
        }
    }

    // ── 重叠校验(排序后相邻 prev.end > cur.start 即重叠;贴边 end==start 不算)──
    // 重叠校验通过后倒序应用时,先应用的 edit 区间恒在当前 edit 之后,
    // 故 buf[0..cur.end] 与原文一致,原文坐标直接可用(splice 前提)。
    let mut order: Vec<usize> = (0..resolved.len()).collect();
    order.sort_by_key(|&i| (resolved[i].0, resolved[i].1));
    for w in order.windows(2) {
        let (a, b) = (&resolved[w[0]], &resolved[w[1]]);
        if a.1 > b.0 {
            return fail(
                "EDIT_OVERLAP",
                format!("edits 区间重叠: [{}, {}) 与 [{}, {})", a.0, a.1, b.0, b.1),
            );
        }
    }

    // ── 应用(倒序)+ 写回(失败回滚,照 rx_fmt 先例)──
    let mut new_text = text.clone();
    for &i in order.iter().rev() {
        let (s, t, c) = &resolved[i];
        splice(&mut new_text, *s, *t, c);
    }
    if new_text != text {
        if let Err(e) = std::fs::write(&abs, new_text.as_bytes()) {
            let _ = std::fs::write(&abs, text.as_bytes());
            return Err(terr("TOOL_ERROR", format!("写回 {file} 失败(已回滚): {e}")));
        }
    }

    // ── rurixc --emit=check 回读诊断(D-F4-A 同款直包)──
    let abs_str = abs.to_string_lossy().into_owned();
    let diags = match rxtool::rurixc().and_then(|bin| {
        rxtool::run(
            &bin,
            &[abs_str.clone(), "--emit=check".into(), "--error-format=json".into()],
            std::time::Duration::from_secs(60),
        )
    }) {
        Ok(out) => match serde_json::from_str::<Vec<Value>>(out.stdout.trim()) {
            Ok(ds) => json!(ds.iter().map(|d| rxtool::map_check_diag(d, file)).collect::<Vec<_>>()),
            Err(e) => {
                return Ok(json!({
                    "applied": true,
                    "newDiagnostics": Value::Null,
                    "diagError": { "code": "TOOL_ERROR", "message": format!("回读诊断 JSON 解析失败(exit {}): {e}", out.exit_code) }
                }));
            }
        },
        Err(e) => {
            return Ok(json!({
                "applied": true,
                "newDiagnostics": Value::Null,
                "diagError": { "code": e.code, "message": e.message }
            }));
        }
    };
    Ok(json!({ "applied": true, "newDiagnostics": diags }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge_codetool_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const TWO_FN: &str = "pub fn smooth_open(x: i32) -> i32 {\n    x * 2 + 1\n}\n\nfn main() {\n    let y = smooth_open(41);\n}\n";

    #[test]
    fn scan_symbols_two_fn_and_filters() {
        let syms = scan_symbols(TWO_FN);
        assert_eq!(syms.len(), 2, "{syms:?}");
        assert_eq!(syms[0].name, "smooth_open");
        assert_eq!(syms[0].kind, "fn");
        assert_eq!((syms[0].line, syms[0].col_start, syms[0].col_end), (0, 7, 18));
        assert_eq!(syms[1].name, "main");
        assert_eq!((syms[1].line, syms[1].col_start), (4, 3));
        // 调用点 `smooth_open(41)` 不得误判为定义(行首非条目关键字)。
        assert!(!syms.iter().any(|s| s.line == 5));
    }

    #[test]
    fn symbol_search_query_and_kinds() {
        let root = tmp_root("search");
        std::fs::create_dir_all(root.join("Content")).unwrap();
        std::fs::write(root.join("Content").join("mathlib.rx"), TWO_FN).unwrap();
        std::fs::write(root.join("other.rx"), "struct Door { open: bool }\n").unwrap();
        // target/vendor 排除。
        std::fs::create_dir_all(root.join("target").join("x")).unwrap();
        std::fs::write(root.join("target").join("x").join("ghost.rx"), "fn smooth_ghost() {}\n").unwrap();

        let v = symbol_search(&json!({ "query": "smooth" }), &root).unwrap();
        let syms = v["symbols"].as_array().unwrap();
        assert_eq!(syms.len(), 1, "{v}");
        assert_eq!(syms[0]["name"], "smooth_open");
        assert_eq!(syms[0]["kind"], "fn");
        assert_eq!(syms[0]["file"], "Content/mathlib.rx");
        assert_eq!(syms[0]["span"]["start"]["line"], 0);

        // kinds 过滤:fn 档掉 struct。
        let v = symbol_search(&json!({ "query": "o", "kinds": ["struct"] }), &root).unwrap();
        let syms = v["symbols"].as_array().unwrap();
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0]["name"], "Door");
        // 无 .rx → 空表而非报错。
        let empty = tmp_root("empty");
        let v = symbol_search(&json!({ "query": "x" }), &empty).unwrap();
        assert_eq!(v["symbols"].as_array().unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn structured_edit_span_replace_and_oob() {
        let root = tmp_root("edit");
        std::fs::write(root.join("a.rx"), TWO_FN).unwrap();
        // span replace:`41` → `7`(line 5,`    let y = smooth_open(41);`,41 在 char 24..26)。
        let v = structured_edit(
            &json!({
                "file": "a.rx",
                "edits": [{ "kind": "replace", "span": { "start": { "line": 5, "character": 24 }, "end": { "line": 5, "character": 26 } }, "content": "7" }]
            }),
            &root,
        )
        .unwrap();
        assert_eq!(v["applied"], true, "{v}");
        let after = std::fs::read_to_string(root.join("a.rx")).unwrap();
        assert!(after.contains("smooth_open(7);"), "{after}");
        // rurixc 在 → newDiagnostics 须为空数组;不在 → diagError 如实(不遮蔽)。
        if rxtool::rurixc().is_ok() {
            assert_eq!(v["newDiagnostics"].as_array().map(Vec::len), Some(0), "{v}");
        } else {
            assert!(v["diagError"].is_object(), "{v}");
        }

        // 越界 span → applied:false 不写盘。
        let before = std::fs::read_to_string(root.join("a.rx")).unwrap();
        let v = structured_edit(
            &json!({
                "file": "a.rx",
                "edits": [{ "kind": "replace", "span": { "start": { "line": 99, "character": 0 }, "end": { "line": 99, "character": 1 } }, "content": "x" }]
            }),
            &root,
        )
        .unwrap();
        assert_eq!(v["applied"], false);
        assert_eq!(v["error"]["code"], "SPAN_OUT_OF_RANGE");
        assert_eq!(std::fs::read_to_string(root.join("a.rx")).unwrap(), before, "失败不写盘");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn structured_edit_symbol_query_and_overlap() {
        let root = tmp_root("editsym");
        std::fs::write(root.join("a.rx"), TWO_FN).unwrap();
        // symbolQuery replace:符号名区间替换(等价 def 点改名,引用点不动 —— 机械区间语义)。
        let v = structured_edit(
            &json!({
                "file": "a.rx",
                "edits": [{ "kind": "replace", "symbolQuery": "smooth_open", "content": "smooth_on" }]
            }),
            &root,
        )
        .unwrap();
        assert_eq!(v["applied"], true, "{v}");
        let after = std::fs::read_to_string(root.join("a.rx")).unwrap();
        assert!(after.contains("fn smooth_on("), "{after}");

        // 两 edit 区间重叠 → applied:false 不写盘。
        let v = structured_edit(
            &json!({
                "file": "a.rx",
                "edits": [
                    { "kind": "replace", "span": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 10 } }, "content": "x" },
                    { "kind": "replace", "span": { "start": { "line": 0, "character": 5 }, "end": { "line": 0, "character": 8 } }, "content": "y" }
                ]
            }),
            &root,
        )
        .unwrap();
        assert_eq!(v["applied"], false);
        assert_eq!(v["error"]["code"], "EDIT_OVERLAP");
        // symbolQuery 不存在 → SYMBOL_NOT_FOUND。
        let v = structured_edit(
            &json!({
                "file": "a.rx",
                "edits": [{ "kind": "replace", "symbolQuery": "ghost_fn", "content": "x" }]
            }),
            &root,
        )
        .unwrap();
        assert_eq!(v["applied"], false);
        assert_eq!(v["error"]["code"], "SYMBOL_NOT_FOUND");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// references 真 LSP 集成(rurixc 缺失跳过):定义 + 调用点 >=2,调用点 line 5。
    #[test]
    fn references_finds_call_site() {
        if rxtool::rurixc().is_err() {
            eprintln!("[SKIP] rurixc 不存在,references 集成测试跳过");
            return;
        }
        let root = tmp_root("refs");
        std::fs::write(root.join("a.rx"), TWO_FN).unwrap();
        let v = references(&json!({ "symbolQuery": "smooth_open" }), &root).unwrap();
        let refs = v["refs"].as_array().unwrap();
        assert!(refs.len() >= 2, "{v}");
        assert!(
            refs.iter().any(|r| r["span"]["start"]["line"] == 5 && r["file"] == "a.rx"),
            "须含调用点(line 5): {v}"
        );
        // 不存在符号 → SYMBOL_NOT_FOUND。
        let e = references(&json!({ "symbolQuery": "ghost_xyz" }), &root).unwrap_err();
        assert_eq!(e.code, "SYMBOL_NOT_FOUND");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn structured_edit_rejects_path_escape() {
        let root = tmp_root("escape");
        std::fs::write(root.join("a.rx"), TWO_FN).unwrap();
        let v = structured_edit(
            &json!({
                "file": "../a.rx",
                "edits": [{ "kind": "replace", "symbolQuery": "smooth_open", "content": "x" }]
            }),
            &root,
        )
        .unwrap();
        assert_eq!(v["applied"], false);
        assert_eq!(v["error"]["code"], "PATH_OUTSIDE_ROOT");
        let _ = std::fs::remove_dir_all(&root);
    }
}
