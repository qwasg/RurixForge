//! Agent 三态 profile（参考仓 agent-core/profile.rs 拷入改接）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    General,
    Document,
    Coding,
    Studio,
}

impl Default for AgentKind {
    fn default() -> Self {
        AgentKind::Coding
    }
}

impl AgentKind {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "general" | "chat" | "assistant" => AgentKind::General,
            "document" | "doc" | "writer" => AgentKind::Document,
            "studio" => AgentKind::Studio,
            _ => AgentKind::Coding,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AgentKind::General => "general",
            AgentKind::Document => "document",
            AgentKind::Coding => "coding",
            AgentKind::Studio => "studio",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AgentKind::General => "通用助手",
            AgentKind::Document => "文档处理",
            AgentKind::Coding => "编码",
            AgentKind::Studio => "素材创作",
        }
    }
}

pub struct AgentProfile {
    pub kind: AgentKind,
}

impl AgentProfile {
    pub fn from_kind_str(s: &str) -> Self {
        AgentProfile {
            kind: AgentKind::from_str(s),
        }
    }

    pub fn wants_edit_tools(&self) -> bool {
        matches!(self.kind, AgentKind::Coding)
    }

    pub fn wants_todo_tools(&self) -> bool {
        !matches!(self.kind, AgentKind::General)
    }

    pub fn wants_task_tool(&self) -> bool {
        !matches!(self.kind, AgentKind::Studio)
    }

    pub fn allowed_modes(&self) -> &'static [&'static str] {
        match self.kind {
            AgentKind::Coding => &["build", "plan", "team", "debug", "ask", "multitask"],
            AgentKind::Document | AgentKind::General | AgentKind::Studio => &["ask", "build"],
        }
    }

    /// MCP 工具是否允许。coding 全放行；studio 素材相关;其余只放行只读族。
    pub fn mcp_tool_allowed(&self, name: &str) -> bool {
        match self.kind {
            AgentKind::Coding => true,
            AgentKind::Studio => studio_mcp_allowed(name),
            AgentKind::Document | AgentKind::General => !crate::agent::is_write_tool(name),
        }
    }
}

fn studio_mcp_allowed(name: &str) -> bool {
    name.starts_with("mcp__context__")
        || name.starts_with("mcp__asset-pipeline__")
        || name.starts_with("mcp__store__")
        || name.starts_with("mcp__gen-image__")
        || name.starts_with("mcp__gen-model__")
        || matches!(
            name,
            "mcp__engine-scene__scene_summary"
                | "mcp__engine-scene__template_preview"
                | "mcp__engine-scene__entity_list"
                | "mcp__engine-scene__entity_get"
                | "mcp__engine-scene__scene_index"
                | "mcp__engine-scene__scene_graph_dump"
                | "mcp__engine-scene__viewport_frame"
                | "mcp__engine-scene__component_get"
                | "mcp__engine-scene__component_list_types"
                | "mcp__engine-scene__transform_get"
                | "mcp__engine-scene__play_state"
                | "mcp__code-forge__graph_get"
                | "mcp__code-forge__code_symbol_search"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_parse_and_modes() {
        assert_eq!(AgentKind::from_str("chat"), AgentKind::General);
        assert_eq!(AgentKind::from_str(""), AgentKind::Coding);
        assert_eq!(AgentKind::General.as_str(), "general");
        assert_eq!(AgentKind::Document.label(), "文档处理");
        assert_eq!(
            AgentProfile::from_kind_str("general").allowed_modes(),
            &["ask", "build"]
        );
        // team(游戏制作多代理)仅 Coding 可用。
        assert!(AgentProfile::from_kind_str("coding").allowed_modes().contains(&"team"));
        assert!(!AgentProfile::from_kind_str("general").allowed_modes().contains(&"team"));
        assert!(AgentProfile::from_kind_str("coding").wants_edit_tools());
        assert!(!AgentProfile::from_kind_str("document").wants_edit_tools());
        assert_eq!(AgentKind::from_str("studio"), AgentKind::Studio);
        assert_eq!(
            AgentProfile::from_kind_str("studio").allowed_modes(),
            &["ask", "build"]
        );
        assert!(!AgentProfile::from_kind_str("studio").wants_task_tool());
        assert!(AgentProfile::from_kind_str("studio").mcp_tool_allowed("mcp__store__library_search"));
        assert!(AgentProfile::from_kind_str("studio").mcp_tool_allowed("mcp__store__store_install"));
        assert!(!AgentProfile::from_kind_str("studio").mcp_tool_allowed("mcp__engine-scene__entity_create"));
    }
}
