//! IndexDoc 统一文档模型:一切可检索素材的最小公共形态。
//! id 命名空间:asset:<guid> / entity:<scene_rel>#<id> / graph:<rel> /
//! symbol:<rel>#<fn> / doc:<file>#<n>。

use serde::{Deserialize, Serialize};

/// 文档种类(检索时可按 kinds 过滤)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    /// Content/ 下的资产文件(mesh/texture/material/scene/script…)。
    Asset,
    /// .rxscene 内的实体。
    Entity,
    /// .rxgraph 节点图(图级文档,facts 含全部节点语义展开)。
    Graph,
    /// .rx 导出函数 / 脚本文件。
    Symbol,
    /// 工作区根 *.md 文档分块。
    Doc,
}

impl DocKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DocKind::Asset => "asset",
            DocKind::Entity => "entity",
            DocKind::Graph => "graph",
            DocKind::Symbol => "symbol",
            DocKind::Doc => "doc",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "asset" => DocKind::Asset,
            "entity" => DocKind::Entity,
            "graph" => DocKind::Graph,
            "symbol" => DocKind::Symbol,
            "doc" => DocKind::Doc,
            _ => return None,
        })
    }
}

/// 统一索引文档。description 来自 .meta semantic(人写/agent 写),
/// facts 来自抽取器的客观事实(尺寸/顶点数/组件/节点/签名…)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDoc {
    pub id: String,
    pub kind: DocKind,
    pub title: String,
    /// 相对项目根(资产/场景/图/脚本)或工作区根(md)的正斜杠路径。
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guid: Option<String>,
    /// 资产类型(kind=asset 专用:mesh/texture/material/scene/script/audio)。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub atype: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub facts: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// 关联 GUID(引用图出边等)。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    /// sha256(text_for_index),用于向量增量与 stale 判定。
    pub content_hash: String,
}

impl IndexDoc {
    /// 进索引/进 embedding 的全文(title+path+description+tags+facts)。
    pub fn text_for_index(&self) -> String {
        let mut t = String::with_capacity(
            self.title.len() + self.path.len() + self.description.len() + self.facts.len() + 64,
        );
        t.push_str(&self.title);
        t.push('\n');
        t.push_str(&self.path);
        t.push('\n');
        if !self.description.is_empty() {
            t.push_str(&self.description);
            t.push('\n');
        }
        if !self.tags.is_empty() {
            t.push_str(&self.tags.join(" "));
            t.push('\n');
        }
        if !self.facts.is_empty() {
            t.push_str(&self.facts);
        }
        t
    }

    /// 回填 content_hash = sha256(text_for_index)。
    pub fn finalize_hash(&mut self) {
        self.content_hash = crate::sha256_hex(self.text_for_index().as_bytes());
    }
}
