//! 检索管线:词法档(BM25) / 混合档(BM25 + cosine 经 RRF 融合)。
//! 档位显式标注(I-5):tier + note 同源返回,词法档不假装语义检索。

use std::collections::{HashMap, HashSet};

use crate::doc::{DocKind, IndexDoc};
use crate::store::{self, IndexPaths};
use crate::vector::{rrf_fuse, top_k_similar, Embedder};
use crate::{IndexError, Result};

/// 融合前每路候选数(过取再融合)。
const CANDIDATE_K: usize = 50;

/// 单条命中。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub doc: IndexDoc,
    pub score: f64,
    /// 词法路名次(0 起;None = 该路未命中)。
    pub lexical_rank: Option<usize>,
    /// 向量路名次。
    pub vector_rank: Option<usize>,
}

/// 检索结果(tier 显式;note 在词法档说明原因)。
#[derive(Debug, Clone)]
pub struct SearchOutcome {
    pub tier: String,
    pub note: Option<String>,
    pub hits: Vec<SearchHit>,
}

/// 检索过滤条件。
#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    /// 限定文档种类(空 = 不限)。
    pub kinds: Vec<DocKind>,
    /// 限定资产类型(仅对 kind=asset 有意义;空 = 不限)。
    pub atypes: Vec<String>,
}

/// 主检索入口。embedder = None 或向量缺失 → 词法档(显式标注)。
pub fn search(
    paths: &IndexPaths,
    query: &str,
    top_k: usize,
    filter: &SearchFilter,
    embedder: Option<&dyn Embedder>,
) -> Result<SearchOutcome> {
    if !paths.exists() {
        return Err(IndexError::new(
            "INDEX_NOT_BUILT",
            "索引未构建,先调 context_index_build",
        ));
    }
    let docs = store::load_docs(paths)?;
    let lexical = store::load_lexical(paths)?;
    let by_id: HashMap<&str, &IndexDoc> = docs.iter().map(|d| (d.id.as_str(), d)).collect();

    // 过滤集(kinds/atypes 交集)。
    let allowed: Option<HashSet<String>> = if filter.kinds.is_empty() && filter.atypes.is_empty() {
        None
    } else {
        Some(
            docs.iter()
                .filter(|d| filter.kinds.is_empty() || filter.kinds.contains(&d.kind))
                .filter(|d| {
                    filter.atypes.is_empty()
                        || (d.kind == DocKind::Asset && filter.atypes.contains(&d.atype))
                })
                .map(|d| d.id.clone())
                .collect(),
        )
    };

    let lex_hits = lexical.search(query, CANDIDATE_K, allowed.as_ref());

    // 向量路(embedder 就绪 + 向量文件就绪 + 模型匹配才启用)。
    let mut vec_hits: Vec<(String, f64)> = Vec::new();
    let mut tier = "lexical".to_string();
    let mut note = Some("未配置 embedding 渠道,当前为词法档(BM25);配置后自动升级混合检索".to_string());

    if let Some(embedder) = embedder {
        let records = store::load_vectors(paths)?;
        let usable: Vec<(String, Vec<f32>)> = records
            .iter()
            .filter(|r| r.model == embedder.model())
            .filter(|r| allowed.as_ref().map(|a| a.contains(&r.id)).unwrap_or(true))
            .filter_map(|r| r.decode().map(|v| (r.id.clone(), v)))
            .collect();
        if usable.is_empty() {
            note = Some(
                "已配置 embedding 但索引无匹配向量(先 context_index_build);当前为词法档".to_string(),
            );
        } else {
            match embedder.embed(&[query.to_string()]) {
                Ok(mut vs) if !vs.is_empty() => {
                    let qv = vs.remove(0);
                    vec_hits = top_k_similar(&qv, &usable, CANDIDATE_K);
                    tier = "hybrid".into();
                    note = None;
                }
                Ok(_) => {
                    note = Some("embedding 查询返回空向量,本次退词法档".to_string());
                }
                Err(e) => {
                    // 失败不静默也不阻断:检索退词法档,原因如实带出。
                    note = Some(format!("embedding 查询失败({e}),本次退词法档"));
                }
            }
        }
    }

    let fused: Vec<(String, f64)> = if tier == "hybrid" {
        rrf_fuse(&[&lex_hits, &vec_hits], top_k)
    } else {
        lex_hits.iter().take(top_k).cloned().collect()
    };

    let lex_rank: HashMap<&str, usize> = lex_hits
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.as_str(), i))
        .collect();
    let vec_rank: HashMap<&str, usize> = vec_hits
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.as_str(), i))
        .collect();

    let hits = fused
        .into_iter()
        .filter_map(|(id, score)| {
            by_id.get(id.as_str()).map(|d| SearchHit {
                doc: (*d).clone(),
                score,
                lexical_rank: lex_rank.get(id.as_str()).copied(),
                vector_rank: vec_rank.get(id.as_str()).copied(),
            })
        })
        .collect();

    Ok(SearchOutcome { tier, note, hits })
}

/// 按 id 或 guid 取单文档(context_get)。
pub fn get_doc(paths: &IndexPaths, id_or_guid: &str) -> Result<Option<IndexDoc>> {
    let docs = store::load_docs(paths)?;
    Ok(docs
        .iter()
        .find(|d| d.id == id_or_guid || d.guid.as_deref() == Some(id_or_guid))
        .cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::build_index;
    use crate::extract::ExtractOptions;

    struct FakeEmbedder;
    impl Embedder for FakeEmbedder {
        fn model(&self) -> &str {
            "fake-8d"
        }
        fn embed(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|t| {
                    let h = crate::sha256_hex(t.as_bytes());
                    (0..8)
                        .map(|i| h.as_bytes()[i] as f32 / 255.0)
                        .collect::<Vec<f32>>()
                })
                .collect())
        }
    }

    fn setup(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge-index-search-{tag}-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        let scripts = dir.join("Content").join("Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("door.rx"),
            "// 开门逻辑:角度计算\n#[export(c)]\npub fn door_open_angle() -> f32 { 90.0 }\n",
        )
        .unwrap();
        std::fs::write(
            scripts.join("maze.rx"),
            "// 迷宫规则\n#[export(c)]\npub fn open_at_cell(cell: i32) -> bool { true }\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn lexical_tier_when_no_embedder_with_note() {
        let dir = setup("lex");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        build_index(&opts, None, false).unwrap();
        let paths = IndexPaths::for_project(&dir);
        let out = search(&paths, "开门", 5, &SearchFilter::default(), None).unwrap();
        assert_eq!(out.tier, "lexical");
        assert!(out.note.is_some(), "词法档须显式标注");
        assert!(!out.hits.is_empty());
        assert!(out.hits[0].doc.path.contains("door"), "{:?}", out.hits[0].doc);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hybrid_tier_with_embedder() {
        let dir = setup("hyb");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        build_index(&opts, Some(&FakeEmbedder), false).unwrap();
        let paths = IndexPaths::for_project(&dir);
        let out = search(&paths, "开门角度", 5, &SearchFilter::default(), Some(&FakeEmbedder)).unwrap();
        assert_eq!(out.tier, "hybrid");
        assert!(out.note.is_none());
        assert!(!out.hits.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unbuilt_index_is_explicit_error() {
        let dir = std::env::temp_dir().join(format!(
            "forge-index-nobuild-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = IndexPaths::for_project(&dir);
        let e = search(&paths, "任意", 5, &SearchFilter::default(), None).unwrap_err();
        assert_eq!(e.code, "INDEX_NOT_BUILT");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn kind_filter_narrows_results() {
        let dir = setup("filter");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        build_index(&opts, None, false).unwrap();
        let paths = IndexPaths::for_project(&dir);
        let filter = SearchFilter { kinds: vec![DocKind::Asset], atypes: vec![] };
        let out = search(&paths, "door", 10, &filter, None).unwrap();
        assert!(out.hits.iter().all(|h| h.doc.kind == DocKind::Asset), "{:?}",
            out.hits.iter().map(|h| &h.doc.id).collect::<Vec<_>>());
        std::fs::remove_dir_all(&dir).ok();
    }
}
