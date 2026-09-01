//! Subagent profile(04 §6):磁盘 profile `data/agents/*.md`(front matter:
//! name/description/tools/model/maxSteps + 正文 system prompt),热加载——每次列举
//! 重新读盘(mtime 免缓存),改文件不重启即生效。
//! tools 白名单前缀通配(D-008):`mcp__engine-scene__*`(整族)与
//! `mcp__engine-scene__component.*`(方法族,点号归一为下划线)两形态;
//! `task` 工具恒不可入任何 profile(防递归委派,04 §6 逐字)。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// 单个 subagent profile(04 §6 front matter)。
#[derive(Debug, Clone)]
pub struct SubagentProfile {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub model: String,
    pub max_steps: u32,
    /// 正文 system prompt。
    pub prompt: String,
}

impl SubagentProfile {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "tools": self.tools,
            "model": self.model,
            "maxSteps": self.max_steps,
            "prompt": self.prompt,
        })
    }
}

/// data/agents 目录(workspace 根/data/agents)。
pub fn agents_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("data").join("agents")
}

/// 解析 profile 文本(front matter + 正文)。front matter 行格式:
/// name/description/model/maxSteps 为标量;tools 为 JSON 数组。
pub fn parse_profile(text: &str) -> Result<SubagentProfile, String> {
    // UTF-8 BOM 容忍(Windows 编辑器/PS Set-Content UTF8 常带 BOM)。
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err("缺 front matter 起始 ---".into());
    }
    let mut name = None;
    let mut desc = None;
    let mut tools: Option<Vec<String>> = None;
    let mut model = None;
    let mut max_steps = None;
    let mut body_start = 0usize;
    for (i, line) in text.lines().enumerate().skip(1) {
        let t = line.trim();
        if t == "---" {
            body_start = i + 1;
            break;
        }
        if let Some(v) = t.strip_prefix("name:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = t.strip_prefix("description:") {
            desc = Some(v.trim().to_string());
        } else if let Some(v) = t.strip_prefix("tools:") {
            let arr: Vec<String> = serde_json::from_str(v.trim())
                .map_err(|e| format!("tools 须为 JSON 数组: {e}"))?;
            tools = Some(arr);
        } else if let Some(v) = t.strip_prefix("model:") {
            model = Some(v.trim().to_string());
        } else if let Some(v) = t.strip_prefix("maxSteps:") {
            max_steps = Some(
                v.trim()
                    .parse::<u32>()
                    .map_err(|_| format!("maxSteps 须为正整数: {t}"))?,
            );
        }
    }
    if body_start == 0 {
        return Err("缺 front matter 结束 ---".into());
    }
    let name = name.ok_or("缺 name")?;
    if name == "task" {
        return Err("profile 名不可为 task(防递归委派)".into());
    }
    let prompt = text.lines().skip(body_start).collect::<Vec<_>>().join("\n");
    Ok(SubagentProfile {
        name,
        description: desc.ok_or("缺 description")?,
        tools: tools.ok_or("缺 tools")?,
        model: model.unwrap_or_else(|| "default".into()),
        max_steps: max_steps.ok_or("缺 maxSteps")?,
        prompt,
    })
}

/// 热加载列举:每次重新扫盘(改文件不重启生效);解析失败的文件如实进 errors 不遮蔽。
pub fn list_subagents(dir: &Path) -> (Vec<SubagentProfile>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            match std::fs::read_to_string(&p) {
                Ok(text) => match parse_profile(&text) {
                    Ok(prof) => out.push(prof),
                    Err(e) => errors.push(format!("{}: {e}", p.display())),
                },
                Err(e) => errors.push(format!("{}: 读取失败 {e}", p.display())),
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    (out, errors)
}

/// tools 白名单匹配(D-008 前缀通配):
/// - `task` 恒 false(防递归委派,04 §6 逐字);
/// - 模式尾 `*` → 前缀匹配(模式内点号归一为下划线,适配 component.* 写法);
/// - 否则精确匹配(模式点号同样归一)。
pub fn tool_allowed(allowlist: &[String], tool: &str) -> bool {
    if tool == "task" {
        return false;
    }
    allowlist.iter().any(|p| {
        let p = p.replace('.', "_");
        if let Some(prefix) = p.strip_suffix('*') {
            tool.starts_with(prefix)
        } else {
            tool == p
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"---
name: scene-builder
description: 场景搭建与批量摆放
tools: ["mcp__engine-scene__*", "mcp__project__*", "read_file"]
model: default
maxSteps: 24
---
你是场景搭建专家。先查询后修改。
"#;

    #[test]
    fn parse_profile_full_fields() {
        let p = parse_profile(SAMPLE).unwrap();
        assert_eq!(p.name, "scene-builder");
        assert_eq!(p.description, "场景搭建与批量摆放");
        assert_eq!(p.tools.len(), 3);
        assert_eq!(p.max_steps, 24);
        assert!(p.prompt.contains("场景搭建专家"));
    }

    #[test]
    fn parse_profile_rejects_missing_fields_and_task_name() {
        assert!(parse_profile("no frontmatter").is_err());
        assert!(parse_profile("---\nname: x\n---\nbody").is_err());
        let task = SAMPLE.replace("name: scene-builder", "name: task");
        assert!(parse_profile(&task).unwrap_err().contains("task"));
    }

    #[test]
    fn tool_allowed_prefix_wildcard_whole_family() {
        let allow = vec!["mcp__engine-scene__*".to_string(), "read_file".to_string()];
        assert!(tool_allowed(&allow, "mcp__engine-scene__entity_create"));
        assert!(tool_allowed(&allow, "mcp__engine-scene__component_add"));
        assert!(tool_allowed(&allow, "read_file"));
        assert!(!tool_allowed(&allow, "mcp__asset-pipeline__asset_import"));
        assert!(!tool_allowed(&allow, "fs_write"));
    }

    #[test]
    fn tool_allowed_method_family_dot_form() {
        // 04 §6 字面形态 mcp__engine-scene__component.*(点号)须匹配下划线工具名。
        let allow = vec!["mcp__engine-scene__component.*".to_string()];
        assert!(tool_allowed(&allow, "mcp__engine-scene__component_add"));
        assert!(tool_allowed(&allow, "mcp__engine-scene__component_set"));
        assert!(!tool_allowed(&allow, "mcp__engine-scene__entity_create"));
        assert!(!tool_allowed(&allow, "mcp__engine-scene__componentless"));
    }

    #[test]
    fn tool_allowed_task_never_allowed() {
        let allow = vec!["*".to_string(), "task".to_string()];
        // 即便白名单显式列 task 或全通配,task 也恒 false(04 §6 防递归委派)。
        assert!(!tool_allowed(&allow, "task"));
    }

    #[test]
    fn list_subagents_hot_reads_disk() {
        let dir = std::env::temp_dir().join(format!("f3-subagents-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), SAMPLE).unwrap();
        let (v1, errs) = list_subagents(&dir);
        assert_eq!(v1.len(), 1);
        assert!(errs.is_empty());
        // 热加载:改文件后再列即反映,无需重启。
        std::fs::write(dir.join("a.md"), SAMPLE.replace("maxSteps: 24", "maxSteps: 99")).unwrap();
        let (v2, _) = list_subagents(&dir);
        assert_eq!(v2[0].max_steps, 99);
        std::fs::remove_dir_all(&dir).ok();
    }
}
