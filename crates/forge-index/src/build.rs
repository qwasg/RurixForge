//! 索引构建管线:抽取 → docs.jsonl + lexical.json 全量重建(毫秒级)
//! → 向量按 content_hash 增量(仅嵌变更项;force 全量重嵌)→ manifest。
//! embedding 失败不静默:词法档照常落盘,embed_error 如实返回,tier 退 lexical。

use std::collections::HashMap;

use crate::extract::{extract_all, ExtractOptions};
use crate::lexical::LexicalIndex;
use crate::store::{self, IndexPaths, Manifest};
use crate::vector::{Embedder, VectorRecord};
use crate::Result;

/// 单批 embedding 条数(控请求体大小)。
const EMBED_BATCH: usize = 32;

/// 构建结果(工具层直接序列化返回)。
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub docs: usize,
    /// 本次真实调用 embedding 的条数。
    pub embedded: usize,
    /// hash 未变复用的条数。
    pub reused: usize,
    /// lexical | hybrid。
    pub tier: String,
    /// embedding 失败详情(不含密钥);None = 无失败。
    pub embed_error: Option<String>,
    pub duration_ms: u64,
}

/// 全量构建(词法必建;向量在传入 embedder 时增量构建)。
pub fn build_index(
    opts: &ExtractOptions,
    embedder: Option<&dyn Embedder>,
    force: bool,
) -> Result<BuildOutcome> {
    let started = std::time::Instant::now();
    let paths = IndexPaths::for_project(&opts.project_root);

    // 1. 抽取(已按 id 排序、hash 已回填)。
    let docs = extract_all(opts)?;

    // 2. 词法索引全量重建。
    let lex_input: Vec<(String, String)> = docs
        .iter()
        .map(|d| (d.id.clone(), d.text_for_index()))
        .collect();
    let lexical = LexicalIndex::build(&lex_input);
    store::save_docs(&paths, &docs)?;
    store::save_lexical(&paths, &lexical)?;

    // 3. 向量增量。
    let mut embedded = 0usize;
    let mut reused = 0usize;
    let mut embed_error: Option<String> = None;
    let mut tier = "lexical".to_string();

    if let Some(embedder) = embedder {
        let model = embedder.model().to_string();
        let old: HashMap<String, VectorRecord> = store::load_vectors(&paths)
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();

        let mut records: Vec<VectorRecord> = Vec::with_capacity(docs.len());
        let mut pending: Vec<(usize, String)> = Vec::new(); // (docs 下标, 文本)
        for (i, d) in docs.iter().enumerate() {
            match old.get(&d.id) {
                Some(r) if !force && r.hash == d.content_hash && r.model == model => {
                    records.push(r.clone());
                    reused += 1;
                }
                _ => pending.push((i, d.text_for_index())),
            }
        }

        let mut failed = false;
        for chunk in pending.chunks(EMBED_BATCH) {
            let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
            match embedder.embed(&texts) {
                Ok(vecs) if vecs.len() == chunk.len() => {
                    for ((i, _), v) in chunk.iter().zip(vecs) {
                        let d = &docs[*i];
                        records.push(VectorRecord::new(
                            d.id.clone(),
                            d.content_hash.clone(),
                            model.clone(),
                            v,
                        ));
                        embedded += 1;
                    }
                }
                Ok(vecs) => {
                    embed_error = Some(format!(
                        "embedding 返回条数不匹配:期望 {} 实得 {}",
                        chunk.len(),
                        vecs.len()
                    ));
                    failed = true;
                    break;
                }
                Err(e) => {
                    embed_error = Some(e);
                    failed = true;
                    break;
                }
            }
        }

        if failed {
            // 不静默:向量文件不落半成品,tier 退 lexical,错误如实上抛给调用方。
            tier = "lexical".into();
        } else {
            records.sort_by(|a, b| a.id.cmp(&b.id));
            store::save_vectors(&paths, &records)?;
            tier = "hybrid".into();
        }
    }

    // 4. manifest。
    let manifest = Manifest {
        version: 1,
        built_at: forge_util::timeutil::utc_now_iso8601(),
        doc_count: docs.len(),
        tier: tier.clone(),
        embed_model: if tier == "hybrid" {
            embedder.map(|e| e.model().to_string())
        } else {
            None
        },
        content_hashes: docs
            .iter()
            .map(|d| (d.id.clone(), d.content_hash.clone()))
            .collect(),
    };
    store::save_manifest(&paths, &manifest)?;

    Ok(BuildOutcome {
        docs: docs.len(),
        embedded,
        reused,
        tier,
        embed_error,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 计数假 embedder:返回确定性 8 维向量,统计真实调用条数。
    struct FakeEmbedder {
        calls: AtomicUsize,
    }

    impl Embedder for FakeEmbedder {
        fn model(&self) -> &str {
            "fake-8d"
        }
        fn embed(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, String> {
            self.calls.fetch_add(texts.len(), Ordering::SeqCst);
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

    fn setup_project(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge-index-build-{tag}-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        let scripts = dir.join("Content").join("Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("a.rx"),
            "// 脚本 A\n#[export(c)]\npub fn fa() -> f32 { 1.0 }\n",
        )
        .unwrap();
        std::fs::write(
            scripts.join("b.rx"),
            "// 脚本 B\n#[export(c)]\npub fn fb() -> f32 { 2.0 }\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn incremental_reembeds_only_changed_docs() {
        let dir = setup_project("incr");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        let emb = FakeEmbedder { calls: AtomicUsize::new(0) };

        // 首建:全部嵌入。
        let o1 = build_index(&opts, Some(&emb), false).unwrap();
        assert_eq!(o1.tier, "hybrid");
        assert!(o1.embedded > 0 && o1.reused == 0, "{o1:?}");
        let first_calls = emb.calls.load(Ordering::SeqCst);

        // 无变更重建:全部复用,零调用。
        let o2 = build_index(&opts, Some(&emb), false).unwrap();
        assert_eq!(o2.embedded, 0, "{o2:?}");
        assert_eq!(o2.reused, o1.docs, "{o2:?}");
        assert_eq!(emb.calls.load(Ordering::SeqCst), first_calls);

        // 改一个文件:只重嵌受影响文档(a.rx 文件级 + fa 函数级 = 2 条)。
        std::fs::write(
            dir.join("Content/Scripts/a.rx"),
            "// 脚本 A 已改\n#[export(c)]\npub fn fa2() -> f32 { 3.0 }\n",
        )
        .unwrap();
        let o3 = build_index(&opts, Some(&emb), false).unwrap();
        assert!(o3.embedded >= 1 && o3.embedded <= 3, "只嵌变更项:{o3:?}");
        assert!(o3.reused >= 2, "未变项应复用:{o3:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lexical_only_when_no_embedder() {
        let dir = setup_project("lex");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        let o = build_index(&opts, None, false).unwrap();
        assert_eq!(o.tier, "lexical");
        assert_eq!(o.embedded, 0);
        let paths = IndexPaths::for_project(&dir);
        assert!(paths.docs().is_file() && paths.lexical().is_file());
        assert!(!paths.vectors().is_file(), "无 embedder 不应产向量文件");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn embed_failure_is_explicit_not_silent() {
        struct FailEmbedder;
        impl Embedder for FailEmbedder {
            fn model(&self) -> &str {
                "fail"
            }
            fn embed(&self, _: &[String]) -> std::result::Result<Vec<Vec<f32>>, String> {
                Err("HTTP 500: 上游失败".into())
            }
        }
        let dir = setup_project("fail");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        let o = build_index(&opts, Some(&FailEmbedder), false).unwrap();
        assert_eq!(o.tier, "lexical", "失败须退词法档:{o:?}");
        assert!(o.embed_error.is_some(), "失败须如实上抛:{o:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
