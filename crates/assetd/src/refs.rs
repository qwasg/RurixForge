//! 引用图持久化与操作(08 §5;wave.2):JSON 索引,支持 add/remove/query + redirector。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::meta::MetaDoc;
use crate::project::ForgeProject;
use crate::{meta_path_for, normalize_rel, AssetError, Result};

/// 引用边(from → to,带类型)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RefEdge {
    pub from_guid: String,
    pub to_guid: String,
    pub edge_type: String,
}

/// redirector 记录(移动后旧 GUID → 新路径)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Redirector {
    pub guid: String,
    pub old_path: String,
    pub new_path: String,
}

/// 引用图 + redirector 集(全内存,持久化 JSON)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefGraph {
    pub edges: Vec<RefEdge>,
    pub redirectors: Vec<Redirector>,
}

impl RefGraph {
    /// 从项目 .forge/cache/refgraph.json 加载;无文件 → 空图。
    pub fn load(project: &ForgeProject) -> Result<Self> {
        let p = project.cache_root().join("refgraph.json");
        if !p.is_file() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&p)?;
        serde_json::from_str(&text)
            .map_err(|e| AssetError::new("REFGRAPH_PARSE", format!("refgraph.json 解析失败: {e}")))
    }

    pub fn save(&self, project: &ForgeProject) -> Result<()> {
        let p = project.cache_root().join("refgraph.json");
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| AssetError::new("REFGRAPH_SER", format!("refgraph 序列化失败: {e}")))?;
        std::fs::write(p, text)?;
        Ok(())
    }

    /// 加边(幂等)。
    pub fn add(&mut self, from: &str, to: &str, edge_type: &str) {
        let e = RefEdge { from_guid: from.into(), to_guid: to.into(), edge_type: edge_type.into() };
        if !self.edges.contains(&e) {
            self.edges.push(e);
        }
    }

    /// 删除某 GUID 的全部出边。
    pub fn remove_outgoing(&mut self, guid: &str) {
        self.edges.retain(|e| e.from_guid != guid);
    }

    /// 删除某 GUID 的全部入边。
    pub fn remove_incoming(&mut self, guid: &str) {
        self.edges.retain(|e| e.to_guid != guid);
    }

    /// 出边:此 GUID 引用了谁。
    pub fn refs(&self, guid: &str) -> Vec<&RefEdge> {
        self.edges.iter().filter(|e| e.from_guid == guid).collect()
    }

    /// 入边:谁引用了此 GUID。
    pub fn referenced_by(&self, guid: &str) -> Vec<&RefEdge> {
        self.edges.iter().filter(|e| e.to_guid == guid).collect()
    }

    /// 写 redirector(幂等:同 guid 覆盖)。
    pub fn add_redirector(&mut self, guid: &str, old_path: &str, new_path: &str) {
        self.redirectors.retain(|r| r.guid != guid);
        self.redirectors.push(Redirector {
            guid: guid.into(),
            old_path: old_path.into(),
            new_path: new_path.into(),
        });
    }

    /// 移除 redirector(fix 后)。
    pub fn remove_redirector(&mut self, guid: &str) {
        self.redirectors.retain(|r| r.guid != guid);
    }

    /// 查 redirector。
    pub fn redirector_for(&self, guid: &str) -> Option<&Redirector> {
        self.redirectors.iter().find(|r| r.guid == guid)
    }

    /// 重建引用图:扫 Content/ 下 .rxscene(JSON)与 .rxmat,凡出现已知资产 GUID 的
    /// 字符串字段即建边(scene→asset / material→texture)。GUID 强引用,不解析路径。
    pub fn rebuild(project: &ForgeProject) -> Result<Self> {
        let mut graph = RefGraph::default();
        let content = project.content_root();
        // 先收全部已知 GUID → (path, type)。
        let mut known: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();
        for rel in project.scan_content()? {
            let mp = meta_path_for(&content, &rel);
            if mp.is_file() {
                if let Ok(m) = MetaDoc::load(&mp) {
                    known.insert(m.guid.clone(), (rel.clone(), m.atype.clone()));
                }
            }
        }
        // 逐场景/材质文件扫 GUID 出现。
        for (guid, (rel, atype)) in &known {
            let abs = content.join(rel);
            let text = match std::fs::read_to_string(&abs) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for (other_guid, (_other_rel, other_type)) in &known {
                if other_guid == guid {
                    continue;
                }
                if text.contains(other_guid) {
                    let edge_type = match (atype.as_str(), other_type.as_str()) {
                        ("scene", "prefab") => "scene→prefab",
                        ("scene", t) => match t {
                            "mesh" => "scene→mesh",
                            "material" => "scene→material",
                            _ => "scene→asset",
                        },
                        ("prefab", "mesh") => "prefab→mesh",
                        ("prefab", "material") => "prefab→material",
                        ("material", "texture") => "material→texture",
                        (a, b) => match (a, b) {
                            _ => "asset→asset",
                        },
                    };
                    graph.add(guid, other_guid, edge_type);
                }
            }
        }
        graph.save(project)?;
        Ok(graph)
    }
}
