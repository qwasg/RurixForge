//! 索引持久化:<project>/.forge/cache/index/{docs.jsonl, lexical.json,
//! vectors.jsonl, manifest.json}。全部原子写(tmp + rename,同 agentd 配置纪律)。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::IndexDoc;
use crate::lexical::LexicalIndex;
use crate::vector::VectorRecord;
use crate::{IndexError, Result};

/// 索引目录布局。
#[derive(Debug, Clone)]
pub struct IndexPaths {
    pub dir: PathBuf,
}

impl IndexPaths {
    /// <project>/.forge/cache/index。
    pub fn for_project(project_root: &Path) -> Self {
        IndexPaths { dir: project_root.join(".forge").join("cache").join("index") }
    }

    pub fn docs(&self) -> PathBuf {
        self.dir.join("docs.jsonl")
    }
    pub fn lexical(&self) -> PathBuf {
        self.dir.join("lexical.json")
    }
    pub fn vectors(&self) -> PathBuf {
        self.dir.join("vectors.jsonl")
    }
    pub fn manifest(&self) -> PathBuf {
        self.dir.join("manifest.json")
    }

    pub fn exists(&self) -> bool {
        self.manifest().is_file() && self.docs().is_file() && self.lexical().is_file()
    }
}

/// 索引清单(单一事实源:档位/构建时间/hash 表)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub built_at: String,
    pub doc_count: usize,
    /// lexical | hybrid(I-5:检索返回同源标注)。
    pub tier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed_model: Option<String>,
    /// doc id → content_hash(向量增量判定)。
    #[serde(default)]
    pub content_hashes: BTreeMap<String, String>,
}

fn atomic_write(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn save_docs(paths: &IndexPaths, docs: &[IndexDoc]) -> Result<()> {
    let mut out = String::new();
    for d in docs {
        out.push_str(&serde_json::to_string(d).map_err(|e| {
            IndexError::new("DOC_SER", format!("IndexDoc 序列化失败: {e}"))
        })?);
        out.push('\n');
    }
    atomic_write(&paths.docs(), &out)
}

pub fn load_docs(paths: &IndexPaths) -> Result<Vec<IndexDoc>> {
    let p = paths.docs();
    if !p.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&p)?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line).map_err(|e| {
            IndexError::new("DOC_PARSE", format!("docs.jsonl 解析失败: {e}"))
        })?);
    }
    Ok(out)
}

pub fn save_lexical(paths: &IndexPaths, idx: &LexicalIndex) -> Result<()> {
    let text = serde_json::to_string(idx)
        .map_err(|e| IndexError::new("LEX_SER", format!("lexical 序列化失败: {e}")))?;
    atomic_write(&paths.lexical(), &text)
}

pub fn load_lexical(paths: &IndexPaths) -> Result<LexicalIndex> {
    let p = paths.lexical();
    if !p.is_file() {
        return Ok(LexicalIndex::default());
    }
    let text = std::fs::read_to_string(&p)?;
    serde_json::from_str(&text)
        .map_err(|e| IndexError::new("LEX_PARSE", format!("lexical.json 解析失败: {e}")))
}

pub fn save_vectors(paths: &IndexPaths, records: &[VectorRecord]) -> Result<()> {
    let mut out = String::new();
    for r in records {
        out.push_str(&serde_json::to_string(r).map_err(|e| {
            IndexError::new("VEC_SER", format!("VectorRecord 序列化失败: {e}"))
        })?);
        out.push('\n');
    }
    atomic_write(&paths.vectors(), &out)
}

pub fn load_vectors(paths: &IndexPaths) -> Result<Vec<VectorRecord>> {
    let p = paths.vectors();
    if !p.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&p)?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line).map_err(|e| {
            IndexError::new("VEC_PARSE", format!("vectors.jsonl 解析失败: {e}"))
        })?);
    }
    Ok(out)
}

pub fn save_manifest(paths: &IndexPaths, m: &Manifest) -> Result<()> {
    let text = serde_json::to_string_pretty(m)
        .map_err(|e| IndexError::new("MANIFEST_SER", format!("manifest 序列化失败: {e}")))?;
    atomic_write(&paths.manifest(), &text)
}

pub fn load_manifest(paths: &IndexPaths) -> Result<Option<Manifest>> {
    let p = paths.manifest();
    if !p.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p)?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| IndexError::new("MANIFEST_PARSE", format!("manifest.json 解析失败: {e}")))
}
