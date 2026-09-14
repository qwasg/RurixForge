//! skill 包辅助:SKILL.md frontmatter 解析与正文完整性检查(06 §2)。
//!
//! 比 agentd 里那份「只认 name/description 两行」的实现健壮:支持引号剥离、行内数组、
//! 未知键宽容忽略、注释行跳过。但**不引 YAML 依赖**——frontmatter 是受控的扁平键值面,
//! 手写行解析既够用又不给 skill 包一个「任意 YAML 反序列化」的攻击面。
//!
//! 错误语义:结构性问题(缺 `---`、缺 name/description、name 形态非法)→
//! `STORE_MANIFEST_INVALID`;`validate_skill_doc` 的正文缺节同样用该码,但消息以
//! **「正文缺失」**开头,便于上层映射成 `SKILL_BODY_INCOMPLETE`(11 §5 技能族)。

use crate::{Result, StoreError, STORE_MANIFEST_INVALID};

/// 正文必备小节关键词(06 §2:skill 必须写清怎么做、产出什么、失败了怎么办)。
pub const REQUIRED_BODY_SECTIONS: [&str; 3] = ["执行流程", "输出约束", "失败回退"];

/// frontmatter 解析结果(未知键已被忽略)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub allowed_tools: Vec<String>,
}

fn invalid(msg: impl Into<String>) -> StoreError {
    StoreError::new(STORE_MANIFEST_INVALID, msg)
}

/// 剥掉 value 两端**成对**的单/双引号(只剥一层;不成对则原样保留)。
fn unquote(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2 {
        let b = t.as_bytes();
        if (b[0] == b'"' && b[t.len() - 1] == b'"') || (b[0] == b'\'' && b[t.len() - 1] == b'\'') {
            return &t[1..t.len() - 1];
        }
    }
    t
}

/// 解析列表值:行内数组 `[a, b, c]` 或逗号分隔 `a, b`。
fn parse_list(v: &str) -> Vec<String> {
    let t = v.trim();
    let inner = t
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .unwrap_or(t);
    inner
        .split(',')
        .map(|s| unquote(s).to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 切一行:返回(不含换行符的行, 剩余文本);兼容 `\n` 与 `\r\n`。
fn take_line(s: &str) -> (&str, &str) {
    match s.find('\n') {
        Some(i) => {
            let line = s[..i].strip_suffix('\r').unwrap_or(&s[..i]);
            (line, &s[i + 1..])
        }
        None => (s, ""),
    }
}

/// 切出 frontmatter 与正文。返回 (键值行切片, 正文)。
fn split_doc(text: &str) -> Result<(Vec<&str>, &str)> {
    // 容忍 UTF-8 BOM 与 CRLF(SKILL.md 常由 Windows 编辑器写出)。
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.trim().is_empty() {
        return Err(invalid("SKILL.md 为空"));
    }
    let (first, mut rest) = take_line(text);
    if first.trim() != "---" {
        return Err(invalid(format!(
            "SKILL.md 首行须为 `---`(frontmatter 起始),实为: {}",
            first.trim()
        )));
    }
    let mut kv = Vec::new();
    loop {
        if rest.is_empty() {
            return Err(invalid("SKILL.md frontmatter 未闭合(缺结束的 `---`)"));
        }
        let (line, after) = take_line(rest);
        rest = after;
        if line.trim() == "---" {
            return Ok((kv, rest));
        }
        kv.push(line);
    }
}

/// 解析 frontmatter。缺 name/description 或 name 形态非法 → `STORE_MANIFEST_INVALID`。
pub fn parse_frontmatter(text: &str) -> Result<SkillFrontmatter> {
    let (kv, _) = split_doc(text)?;
    let mut name = None;
    let mut description = None;
    let mut version = None;
    let mut license = None;
    let mut tags = Vec::new();
    let mut allowed_tools = Vec::new();

    for line in kv {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let (k, v) = match t.split_once(':') {
            Some(p) => p,
            // 无冒号的行不是键值对(如续行/列表项):宽容忽略,不因此判整份非法。
            None => continue,
        };
        let key = k.trim().to_ascii_lowercase().replace('_', "-");
        let val = unquote(v);
        match key.as_str() {
            "name" => name = Some(val.to_string()),
            "description" => description = Some(val.to_string()),
            "version" => version = Some(val.to_string()),
            "license" => license = Some(val.to_string()),
            "tags" => tags = parse_list(v),
            "allowed-tools" | "allowedtools" => allowed_tools = parse_list(v),
            // 未知键宽容忽略(上游 skill 生态字段会长,不该因此装不上)。
            _ => {}
        }
    }

    let name = name.filter(|s| !s.is_empty()).ok_or_else(|| {
        invalid("SKILL.md frontmatter 缺必填键 `name`")
    })?;
    let description = description.filter(|s| !s.is_empty()).ok_or_else(|| {
        invalid("SKILL.md frontmatter 缺必填键 `description`")
    })?;
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(invalid(format!(
            "SKILL.md frontmatter `name` 只允许 [a-z0-9-]: {name}"
        )));
    }
    Ok(SkillFrontmatter { name, description, version, license, tags, allowed_tools })
}

/// 完整校验:frontmatter + 正文三节(执行流程 / 输出约束 / 失败回退)。
/// 正文缺节时消息以「正文缺失」开头,上层据此映射 `SKILL_BODY_INCOMPLETE`。
pub fn validate_skill_doc(text: &str) -> Result<SkillFrontmatter> {
    let fm = parse_frontmatter(text)?;
    let (_, body) = split_doc(text)?;
    let missing: Vec<&str> = REQUIRED_BODY_SECTIONS
        .iter()
        .copied()
        .filter(|k| !body.contains(k))
        .collect();
    if !missing.is_empty() {
        return Err(invalid(format!(
            "正文缺失:SKILL.md 正文须含 {} 小节(缺 {})",
            REQUIRED_BODY_SECTIONS.join(" / "),
            missing.join(" / ")
        )));
    }
    Ok(fm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// workspace 根(CARGO_MANIFEST_DIR = crates/forge-store,上两级)。
    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)")
            .to_path_buf()
    }

    #[test]
    fn parses_real_repo_skill() {
        let p = workspace_root().join("skills").join("asset-cleanup").join("SKILL.md");
        let text = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("读 {} 失败: {e}", p.display()));
        let fm = validate_skill_doc(&text).expect("仓内真实 skill 须通过完整校验");
        assert_eq!(fm.name, "asset-cleanup");
        assert!(fm.description.contains("素材整理"), "{}", fm.description);
        assert!(fm.tags.is_empty(), "该 skill 未写 tags");
    }

    const SYNTHETIC: &str = r#"---
# 这行是注释,应被忽略
name: "my-skill"
description: '带引号的描述,含:冒号'
version: 1.2.0
license: MIT
tags: [a, "b", 'c']
allowed-tools: [mcp__asset-pipeline__asset_list, mcp__forge__x]
unknown-key: 随便写
另起一行无冒号内容
---

# 标题

## 执行流程
1. 做事

## 输出约束
- 如实

## 失败回退策略
- 重试
"#;

    #[test]
    fn parses_quotes_arrays_and_ignores_unknown_keys() {
        let fm = validate_skill_doc(SYNTHETIC).unwrap();
        assert_eq!(fm.name, "my-skill");
        assert_eq!(fm.description, "带引号的描述,含:冒号", "只按首个冒号切,值内冒号保留");
        assert_eq!(fm.version.as_deref(), Some("1.2.0"));
        assert_eq!(fm.license.as_deref(), Some("MIT"));
        assert_eq!(fm.tags, vec!["a", "b", "c"]);
        assert_eq!(
            fm.allowed_tools,
            vec!["mcp__asset-pipeline__asset_list", "mcp__forge__x"]
        );
    }

    #[test]
    fn crlf_and_bom_are_tolerated() {
        let crlf = format!("\u{feff}{}", SYNTHETIC.replace('\n', "\r\n"));
        let fm = validate_skill_doc(&crlf).expect("BOM + CRLF 须能解析");
        assert_eq!(fm.name, "my-skill");
        assert_eq!(fm.tags, vec!["a", "b", "c"]);
    }

    #[test]
    fn rejects_missing_keys_bad_name_and_broken_frontmatter() {
        let e = parse_frontmatter("---\ndescription: 只有描述\n---\n正文").unwrap_err();
        assert_eq!(e.code, STORE_MANIFEST_INVALID);
        assert!(e.message.contains("`name`"), "消息须注明缺哪个键: {}", e.message);

        let e = parse_frontmatter("---\nname: x\n---\n正文").unwrap_err();
        assert!(e.message.contains("`description`"), "{}", e.message);

        let e = parse_frontmatter("---\nname: My_Skill\ndescription: d\n---\n").unwrap_err();
        assert_eq!(e.code, STORE_MANIFEST_INVALID);
        assert!(e.message.contains("[a-z0-9-]"), "{}", e.message);

        let e = parse_frontmatter("# 没有 frontmatter\n正文").unwrap_err();
        assert!(e.message.contains("首行"), "{}", e.message);

        let e = parse_frontmatter("---\nname: x\ndescription: d\n没有结束线").unwrap_err();
        assert!(e.message.contains("未闭合"), "{}", e.message);

        assert!(parse_frontmatter("").is_err());
    }

    #[test]
    fn body_incomplete_is_flagged_distinctly() {
        let doc = "---\nname: x\ndescription: d\n---\n\n## 执行流程\n做事\n";
        // frontmatter 本身合法。
        assert_eq!(parse_frontmatter(doc).unwrap().name, "x");
        let e = validate_skill_doc(doc).unwrap_err();
        assert_eq!(e.code, STORE_MANIFEST_INVALID);
        assert!(
            e.message.starts_with("正文缺失"),
            "消息须以「正文缺失」开头以便映射 SKILL_BODY_INCOMPLETE: {}",
            e.message
        );
        assert!(e.message.contains("输出约束") && e.message.contains("失败回退"), "{}", e.message);
    }
}
