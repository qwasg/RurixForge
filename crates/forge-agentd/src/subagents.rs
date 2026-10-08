//! Subagent profile(04 §6):磁盘 profile `*.md`(front matter:
//! name/description/tools/model/maxSteps + 正文 system prompt),热加载——每次列举
//! 重新读盘(mtime 免缓存),改文件不重启即生效。
//! 两个根目录(D-044,与 `skills/` vs `data/user-skills/` 同构):
//! - 内建 `agents/*.md`(仓库根,随仓库跟踪)——team 编排与 task{subagent_type} 依赖的
//!   工种档案;此前只放在被 gitignore 的 `data/agents/`,新检出即「未知 subagent_type」;
//! - 个人 `data/agents/*.md`(不入库)——自定义工种,或**同名覆盖**内建(个人胜)。
//! 生产路径一律走 `list_all_subagents()`(两根合并);`list_subagents(dir)` 只扫单目录。
//! tools 白名单前缀通配(D-008):`mcp__engine-scene__*`(整族)与
//! `mcp__engine-scene__component.*`(方法族,点号归一为下划线)两形态;
//! `task` 工具恒不可入任何 profile(防递归委派,04 §6 逐字)。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// profile 来源标记(D-044):内建 = 仓库根 `agents/`。
pub const SOURCE_BUILTIN: &str = "builtin";
/// profile 来源标记(D-044):个人 = `data/agents/`(含覆盖内建的同名档案)。
pub const SOURCE_PERSONAL: &str = "personal";

/// 所有子代理（含通用、个人覆盖和网页原型）的最低工作循环预算。
pub const MIN_SUBAGENT_ITERS: usize = 512;

/// 保留档案原配置用于热加载与审计；运行时保证至少 512 轮，更高配置照用。
pub fn loop_max_iters(profile: Option<&SubagentProfile>) -> usize {
    profile
        .map(|p| p.max_steps as usize)
        .unwrap_or(MIN_SUBAGENT_ITERS)
        .max(MIN_SUBAGENT_ITERS)
}

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
    /// 来源(D-044):`builtin` | `personal`;由扫盘方按目录打标,不读 front matter。
    pub source: &'static str,
}

impl SubagentProfile {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "tools": self.tools,
            "model": self.model,
            "maxSteps": self.max_steps,
            "effectiveMaxSteps": loop_max_iters(Some(self)),
            "prompt": self.prompt,
            "source": self.source,
        })
    }
}

/// data/agents 目录(workspace 根/data/agents):个人/覆盖根,不入库。
pub fn agents_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)");
    root.join("data").join("agents")
}

/// 内建 profile 目录(D-044):workspace 根/agents,随仓库跟踪(同 skills/ 的定位方式)。
pub fn builtin_agents_dir() -> PathBuf {
    crate::workspace_root().join("agents")
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
            let arr: Vec<String> =
                serde_json::from_str(v.trim()).map_err(|e| format!("tools 须为 JSON 数组: {e}"))?;
            tools = Some(arr);
        } else if let Some(v) = t.strip_prefix("model:") {
            model = Some(v.trim().to_string());
        } else if let Some(v) = t.strip_prefix("maxSteps:") {
            let steps = v
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("maxSteps 须为正整数: {t}"))?;
            if steps == 0 {
                return Err(format!("maxSteps 须为正整数: {t}"));
            }
            max_steps = Some(steps);
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
        // 解析层不知道文件来自哪个根:缺省个人,内建由 scan_dir 按目录改标。
        source: SOURCE_PERSONAL,
    })
}

/// 单目录扫盘并按来源打标(list_subagents / list_all_subagents 共用)。
/// 同一目录内 `name` 重复(如 qa.md 与 qa-tester.md 都写 `name: qa-tester`)只保留路径排序
/// 靠前的那份,其余如实进 errors(D-044)——否则清单里同名两份,派发取到哪份全看 read_dir
/// 的返回顺序,且没有任何报错。
fn scan_dir(dir: &Path, source: &'static str) -> (Vec<SubagentProfile>, Vec<String>) {
    let mut out: Vec<SubagentProfile> = Vec::new();
    // 与 out 同下标:每份已收档案来自哪个文件(重名报错时指给用户看)。
    let mut kept: Vec<PathBuf> = Vec::new();
    let mut errors = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            paths.push(p);
        }
    }
    // read_dir 顺序不保证:先按路径定序,「重名保留哪份」才可复现。
    paths.sort();
    for p in paths {
        match std::fs::read_to_string(&p) {
            Ok(text) => match parse_profile(&text) {
                Ok(mut prof) => {
                    if let Some(i) = out.iter().position(|o| o.name == prof.name) {
                        errors.push(format!(
                            "{}: name「{}」与 {} 重复,已忽略",
                            p.display(),
                            prof.name,
                            kept[i].display()
                        ));
                        continue;
                    }
                    prof.source = source;
                    out.push(prof);
                    kept.push(p);
                }
                Err(e) => errors.push(format!("{}: {e}", p.display())),
            },
            Err(e) => errors.push(format!("{}: 读取失败 {e}", p.display())),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    (out, errors)
}

/// 热加载列举(单目录):每次重新扫盘(改文件不重启生效);解析失败、目录内重名的文件如实进
/// errors 不遮蔽。来源一律标 `personal`——生产路径请用 `list_all_subagents()`。
pub fn list_subagents(dir: &Path) -> (Vec<SubagentProfile>, Vec<String>) {
    scan_dir(dir, SOURCE_PERSONAL)
}

/// 合并两根(D-044):个人档案按 name 覆盖同名内建(整份替换,不做字段级合并),
/// 结果按 name 排序。来源标记保持入参各自的值。各根内部的重名已由 scan_dir 去重并报错,
/// 故合并结果里每个 name 只有一份。
pub fn merge_profiles(
    builtin: Vec<SubagentProfile>,
    personal: Vec<SubagentProfile>,
) -> Vec<SubagentProfile> {
    let mut out: Vec<SubagentProfile> = builtin
        .into_iter()
        .filter(|b| !personal.iter().any(|p| p.name == b.name))
        .collect();
    out.extend(personal);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 生产路径的工种清单(D-044):内建 `agents/` + 个人 `data/agents/`,同名个人胜。
/// 两根都热加载;两根的解析错误与根内重名都如实进 errors(不遮蔽)。
pub fn list_all_subagents() -> (Vec<SubagentProfile>, Vec<String>) {
    let (builtin, mut errors) = scan_dir(&builtin_agents_dir(), SOURCE_BUILTIN);
    let (personal, personal_errors) = list_subagents(&agents_dir());
    errors.extend(personal_errors);
    (merge_profiles(builtin, personal), errors)
}

/// tools 白名单匹配(D-008 前缀通配):
/// - `task` 恒 false(防递归委派,04 §6 逐字);
/// - 模式尾 `*` → 前缀匹配(模式内点号归一为下划线,适配 component.* 写法);
/// - 否则精确匹配(模式点号同样归一)。
pub fn tool_allowed(allowlist: &[String], tool: &str) -> bool {
    // D-036:dispatch(异步派单)与 task 同列——子代理一律不可再委派。
    if tool == "task" || tool == crate::engine::DISPATCH_TOOL {
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
    fn parse_profile_rejects_zero_and_invalid_step_limits() {
        for value in ["0", "-1", "abc", "4294967296"] {
            let text = SAMPLE.replace("maxSteps: 24", &format!("maxSteps: {value}"));
            assert!(parse_profile(&text).unwrap_err().contains("maxSteps"));
        }
    }

    #[test]
    fn every_subagent_has_at_least_512_iterations() {
        assert_eq!(loop_max_iters(None), 512, "通用子代理");
        for steps in [1, 24, 128, 500, 512, 900] {
            let p = sample_named("personal-worker", steps, SOURCE_PERSONAL);
            assert_eq!(loop_max_iters(Some(&p)), (steps as usize).max(512));
            let wire = p.to_json();
            assert_eq!(wire["maxSteps"], steps, "原配置保留供热加载与审计");
            assert_eq!(wire["effectiveMaxSteps"], (steps as usize).max(512));
        }
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

    fn sample_named(name: &str, steps: u32, source: &'static str) -> SubagentProfile {
        let text = SAMPLE
            .replace("name: scene-builder", &format!("name: {name}"))
            .replace("maxSteps: 24", &format!("maxSteps: {steps}"));
        let mut p = parse_profile(&text).unwrap();
        p.source = source;
        p
    }

    /// D-044:个人档案按 name 整份覆盖同名内建;其余并存;结果按 name 排序,来源各自保留。
    #[test]
    fn merge_personal_overrides_builtin_by_name() {
        let builtin = vec![
            sample_named("scene-builder", 24, SOURCE_BUILTIN),
            sample_named("qa-tester", 48, SOURCE_BUILTIN),
        ];
        let personal = vec![
            sample_named("zz-mine", 8, SOURCE_PERSONAL),
            sample_named("qa-tester", 99, SOURCE_PERSONAL),
        ];
        let merged = merge_profiles(builtin, personal);
        let names: Vec<&str> = merged.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["qa-tester", "scene-builder", "zz-mine"],
            "按 name 排序且同名只留一份"
        );
        let qa = &merged[0];
        assert_eq!(qa.max_steps, 99, "同名个人档案胜");
        assert_eq!(qa.source, SOURCE_PERSONAL);
        assert_eq!(
            merged[1].source, SOURCE_BUILTIN,
            "未被覆盖的内建保留 builtin 标记"
        );
        assert_eq!(merged[1].max_steps, 24);
        assert_eq!(merged[2].source, SOURCE_PERSONAL);
        // 任一侧为空都不丢另一侧。
        let only_builtin = merge_profiles(vec![sample_named("a", 16, SOURCE_BUILTIN)], vec![]);
        assert_eq!(only_builtin.len(), 1);
        let only_personal = merge_profiles(vec![], vec![sample_named("b", 16, SOURCE_PERSONAL)]);
        assert_eq!(only_personal.len(), 1);
        // to_json 带 source(REST /api/forge/subagents 面)。
        assert_eq!(only_builtin[0].to_json()["source"], "builtin");
        assert_eq!(only_personal[0].to_json()["source"], "personal");
    }

    /// 内建工种名单(D-044:仓库根 agents/;04 §6 五工种 + F10 asset-describer +
    /// F-GAME-4 planner/reviewer + D-035 explore + D-044 web-demo-builder = 10)。
    const BUILTIN_NAMES: [&str; 10] = [
        "asset-describer",
        "asset-wrangler",
        "explore",
        "logic-programmer",
        "material-smith",
        "planner",
        "qa-tester",
        "reviewer",
        "scene-builder",
        "web-demo-builder",
    ];

    fn builtin_profiles() -> Vec<SubagentProfile> {
        let (list, errors) = scan_dir(&builtin_agents_dir(), SOURCE_BUILTIN);
        assert!(errors.is_empty(), "内建 profile 解析错误: {errors:?}");
        list
    }

    fn builtin<'a>(list: &'a [SubagentProfile], name: &str) -> &'a SubagentProfile {
        list.iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("缺内建 profile {name}"))
    }

    /// D-044:内建目录的契约——恰好这几个工种,逐项满足 team 编排与 REST 面依赖的约定
    /// (只扫仓库根 agents/,不受 data/agents 个人档案影响,故可断言精确数量)。
    #[test]
    fn builtin_dir_matches_contract() {
        let list = builtin_profiles();
        let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, BUILTIN_NAMES, "内建工种名单(按 name 排序)");
        for p in &list {
            assert_eq!(p.source, SOURCE_BUILTIN);
            assert!(p.description.len() > 4, "{} description 过短", p.name);
            assert!(p.tools.len() >= 2, "{} tools 过少", p.name);
            assert!(p.max_steps >= 512, "{} maxSteps 过小", p.name);
            assert!(
                p.prompt.contains("必须遵守"),
                "{} 正文缺「必须遵守」纪律段",
                p.name
            );
            assert_eq!(p.model, "default", "{} 内建档案不绑定具体模型", p.name);
            // 文件名 = 工种名(覆盖关系按 name 判,文件名不一致会让人找不到该改哪个文件)。
            assert!(
                builtin_agents_dir()
                    .join(format!("{}.md", p.name))
                    .is_file(),
                "{} 的文件名应为 <name>.md",
                p.name
            );
        }
        // 所有内建工种统一至少 500 轮，档案默认 512。
        for name in BUILTIN_NAMES {
            assert_eq!(builtin(&list, name).max_steps, 512, "{name} maxSteps");
        }
        // F-GAME-4 wave.3:planner 只读(无写工具);D-035:explore 只读
        // (plan 模式的并行调研工种;混入写工具就破了只读纪律)。
        for name in ["planner", "explore"] {
            for t in &builtin(&list, name).tools {
                assert!(
                    !t.contains("write") && !t.contains("apply_patch") && !t.contains("edit"),
                    "{name} 白名单混入写工具: {t}"
                );
                assert!(
                    !t.ends_with('*'),
                    "{name} 只读工种不许用通配(会带进写工具): {t}"
                );
                assert!(
                    !crate::agent::is_write_tool(t),
                    "{name} 白名单混入写工具: {t}"
                );
            }
        }
        let has = |name: &str, tool: &str| builtin(&list, name).tools.iter().any(|t| t == tool);
        assert!(has("explore", "grep"));
        assert!(has("explore", "mcp__code-forge__code_references"));
        // reviewer 强制 VERDICT 裁决格式(plan::parse_verdict 据此解析)。
        let reviewer = builtin(&list, "reviewer");
        assert!(reviewer.prompt.contains("VERDICT: APPROVE"));
        assert!(reviewer.prompt.contains("VERDICT: REJECT"));
        assert!(has("reviewer", "mcp__engine-scene__play_*"));
        assert!(has("reviewer", "mcp__engine-scene__viewport_frame"));
        // 逐字白名单抽查(04 §6):logic-programmer 含 component.* 族;qa-tester 有 play 控制面
        // (team 模式自动化试玩:play_* + 输入注入 + viewport_frame 截图断言)。
        assert!(has("logic-programmer", "mcp__engine-scene__component.*"));
        assert!(has("qa-tester", "mcp__engine-scene__play_*"));
        assert!(has("qa-tester", "mcp__engine-scene__logic_inject_input"));
        assert!(has("qa-tester", "mcp__engine-scene__viewport_frame"));
        // qa-tester 强制 QA_RESULT 结论格式(plan::parse_qa_result 据此解析)。
        let qa = builtin(&list, "qa-tester");
        assert!(qa.prompt.contains("QA_RESULT: PASS"));
        assert!(qa.prompt.contains("QA_RESULT: FAIL"));
        // 落盘对账:把关的两个工种必须能拿磁盘文件对编辑态(scene_diff 只读),并能把被测场景
        // 摆上台(scene_load);否则没保存/存错文件的成品会在编辑器现场上测过、审过。
        for name in ["qa-tester", "reviewer"] {
            let p = builtin(&list, name);
            assert!(
                has(name, "mcp__engine-scene__scene_diff"),
                "{name} 缺 scene_diff"
            );
            assert!(
                has(name, "mcp__engine-scene__scene_load"),
                "{name} 缺 scene_load"
            );
            assert!(p.prompt.contains("scene_diff"), "{name} 正文缺落盘对账规程");
        }
        // 试玩三工种的规程口径一致:长时推进用 play_resume(play_step 一次只走一帧);
        // .rx 调用失败事件 logic.call_error 必须在问题事件清单里。
        for name in ["logic-programmer", "qa-tester", "reviewer"] {
            let p = builtin(&list, name);
            assert!(
                tool_allowed(&p.tools, "mcp__engine-scene__play_resume"),
                "{name} 缺 play_resume"
            );
            assert!(
                p.prompt.contains("play_resume"),
                "{name} 试玩规程缺 play_resume"
            );
            assert!(
                p.prompt.contains("logic.call_error"),
                "{name} 问题事件清单缺 logic.call_error"
            );
        }
        // 改场景的三个工种:存盘必须显式传 path(引擎不记当前场景路径,缺省落 data/scene.rxscene)。
        for name in ["scene-builder", "logic-programmer", "material-smith"] {
            let p = builtin(&list, name);
            assert!(
                tool_allowed(&p.tools, "mcp__engine-scene__scene_save"),
                "{name} 缺 scene_save"
            );
            assert!(
                p.prompt.contains("data/scene.rxscene"),
                "{name} 正文缺 scene_save 缺省路径说明"
            );
        }
        // material-smith 的引擎面是 render/component 子集(04 §6),不是整族:不得建/删实体、
        // 换场景、进 play;.rxsprite 图集走 asset-pipeline 的 sprite_create(与 engine-scene 同名工具区分)。
        let ms = builtin(&list, "material-smith");
        assert!(
            !has("material-smith", "mcp__engine-scene__*"),
            "material-smith 不得持 engine-scene 整族"
        );
        for t in [
            "mcp__engine-scene__component_get",
            "mcp__engine-scene__component_set",
            "mcp__engine-scene__viewport_frame",
            "mcp__asset-pipeline__sprite_create",
        ] {
            assert!(tool_allowed(&ms.tools, t), "material-smith 缺 {t}");
        }
        for t in [
            "mcp__engine-scene__sprite_create",
            "mcp__engine-scene__entity_create",
            "mcp__engine-scene__entity_destroy",
            "mcp__engine-scene__scene_new",
            "mcp__engine-scene__scene_load",
            "mcp__engine-scene__play_enter",
        ] {
            assert!(
                !tool_allowed(&ms.tools, t),
                "material-smith 白名单过宽: {t}"
            );
        }
        assert!(ms.prompt.contains("mcp__asset-pipeline__sprite_create"));
    }

    /// D-044:同一目录内两份档案 `name` 相同 → 只留路径排序靠前的一份,另一份进 errors
    /// (此前两份都进清单,派发取到哪份看 read_dir 顺序,且无任何报错)。
    #[test]
    fn scan_dir_dedupes_same_name_and_reports() {
        let dir = std::env::temp_dir().join(format!("d044-subagents-dup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let named = |steps: u32| {
            SAMPLE
                .replace("name: scene-builder", "name: qa-tester")
                .replace("maxSteps: 24", &format!("maxSteps: {steps}"))
        };
        std::fs::write(dir.join("qa.md"), named(11)).unwrap();
        std::fs::write(dir.join("qa-tester.md"), named(22)).unwrap();
        std::fs::write(dir.join("other.md"), SAMPLE).unwrap();
        let (list, errors) = list_subagents(&dir);
        let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["qa-tester", "scene-builder"], "同名只留一份");
        // 路径排序:qa-tester.md < qa.md('-' 0x2D < '.' 0x2E)→ 保留 qa-tester.md 那份。
        assert_eq!(list[0].max_steps, 22);
        assert_eq!(errors.len(), 1, "重名须如实报错: {errors:?}");
        assert!(errors[0].contains("qa.md") && errors[0].contains("qa-tester.md"));
        assert!(errors[0].contains("重复"));
        // 合并后仍然每个 name 一份(个人根的重名不会漏进生产清单)。
        let merged = merge_profiles(vec![sample_named("qa-tester", 48, SOURCE_BUILTIN)], list);
        assert_eq!(merged.iter().filter(|p| p.name == "qa-tester").count(), 1);
        assert_eq!(merged[0].max_steps, 22, "个人胜");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-044:内建白名单里的每个名字都得是真实存在的工具——写错一个字母,该工种就静默少一件
    /// 工具(白名单过滤不报错),只能靠这里守门。
    #[test]
    fn builtin_tools_exist_in_registries() {
        // 子代理专属工具(D-044 契约 §7):由 run_nested_task 按工种追加,不登记在
        // NATIVE_TOOLS / KNOWN_TOOLS。名字只在**真正引入该工具的那一波**加进来
        // (web_demo_probe → W2;playtest_run → W5)——提前放行等于让本守门对还不存在的工具失效。
        // 同理不设「预留通配」豁免:04 §6 的 `mcp__project__*` 没有对应 MCP 服务,
        // 已从 scene-builder 白名单移除(匹配不到任何工具的名字就是死名)。
        const SUBAGENT_EXTRA_TOOLS: &[&str] = &["web_demo_probe", "ultraplan_verify"];
        for p in builtin_profiles() {
            for t in &p.tools {
                assert!(
                    t != "task" && t != crate::engine::DISPATCH_TOOL && !crate::memory::is_tool(t),
                    "{}: {t} 恒不可入子代理白名单",
                    p.name
                );
                let one = [t.clone()];
                if t.ends_with('*') {
                    assert!(
                        crate::mcp::KNOWN_TOOLS
                            .iter()
                            .any(|k| tool_allowed(&one, k)),
                        "{}: 通配 {t} 匹配不到任何已知 MCP 工具",
                        p.name
                    );
                } else if t.starts_with("mcp__") {
                    assert!(
                        crate::mcp::KNOWN_TOOLS
                            .iter()
                            .any(|k| tool_allowed(&one, k)),
                        "{}: {t} 不在 mcp::KNOWN_TOOLS",
                        p.name
                    );
                } else {
                    assert!(
                        crate::engine::NATIVE_TOOLS.contains(&t.as_str())
                            || crate::resources::RESOURCE_TOOLS.contains(&t.as_str())
                            || SUBAGENT_EXTRA_TOOLS.contains(&t.as_str()),
                        "{}: {t} 不是已知的运行时/资源工具",
                        p.name
                    );
                }
            }
        }
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
        std::fs::write(
            dir.join("a.md"),
            SAMPLE.replace("maxSteps: 24", "maxSteps: 99"),
        )
        .unwrap();
        let (v2, _) = list_subagents(&dir);
        assert_eq!(v2[0].max_steps, 99);
        std::fs::remove_dir_all(&dir).ok();
    }
}
