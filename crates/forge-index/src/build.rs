//! 索引构建管线:抽取 → docs.jsonl + lexical.json 全量重建(毫秒级)
//! → 向量按 content_hash 增量(仅嵌变更项;force 全量重嵌)→ manifest。
//! embedding 失败不静默:词法档照常落盘,embed_error 如实返回,tier 退 lexical。

use std::collections::HashMap;

use crate::doc::DocKind;
use crate::extract::{self, extract_all, ExtractOptions};
use crate::lexical::LexicalIndex;
use crate::store::{self, IndexPaths, Manifest};
use crate::vector::{Embedder, VectorRecord};
use crate::{IndexError, Result};

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

// ---------- 单资产增量 upsert(写简介即进检索) ----------

/// upsert 结果(工具层序列化返回;I-5 如实标注)。
#[derive(Debug, Clone)]
pub struct UpsertOutcome {
    /// false = 索引未建(首建仍归 context_index_build),本次仅 .meta 落盘。
    pub indexed: bool,
    /// 本次同步进索引的文档数(asset + graph/symbol 文件级)。
    pub docs: usize,
    /// 本次是否真实调用了 embedding。
    pub embedded: bool,
    /// lexical | hybrid(与 manifest 同源)。
    pub tier: String,
    /// embedding 失败详情;None = 无失败。
    pub embed_error: Option<String>,
}

/// 单资产增量入库:重抽该文件全部文档(asset + graph/symbol 文件级——三者共享
/// .meta semantic)→ docs 按 id 替换/插入 → 词法全量重建(毫秒级)→ manifest hash 表同步。
/// 索引未建时跳过(indexed=false),不替 context_index_build 首建。
///
/// 向量纪律:tier 是全索引属性——仅当索引已是 hybrid 档(向量基座存在)且传入 embedder
/// 时顶量同步变更条;词法档索引不部分向量化(防「2/1000 条有向量」的伪 hybrid),
/// 向量待下次全量构建建立。embed 失败时移除该文件文档的旧向量(避免旧文本向量误导检索),
/// 词法路始终即时可检,失败经 embed_error 如实上抛。
pub fn upsert_asset_docs(
    project_root: &std::path::Path,
    asset_rel: &str,
    embedder: Option<&dyn Embedder>,
) -> Result<UpsertOutcome> {
    let paths = IndexPaths::for_project(project_root);
    let Some(mut manifest) = store::load_manifest(&paths)? else {
        return Ok(UpsertOutcome {
            indexed: false,
            docs: 0,
            embedded: false,
            tier: "lexical".into(),
            embed_error: None,
        });
    };
    let new_docs = extract::extract_file_docs(project_root, asset_rel)?
        .ok_or_else(|| IndexError::new("NO_ASSET", format!("资产不在 Content 扫描内: {asset_rel}")))?;

    // 替换集:新 id 全覆盖 + 同 path 的 asset/graph 文档 + 文件级 symbol(id 可能因
    // ensure_meta 补建 guid 而从 asset:path: 变为 asset:<guid>,旧 id 须清)。
    // 函数级 symbol / entity 文档不携带简介,不在此动。
    let new_ids: std::collections::HashSet<&str> =
        new_docs.iter().map(|d| d.id.as_str()).collect();
    let file_symbol_id = format!("symbol:{asset_rel}");
    let mut docs = store::load_docs(&paths)?;
    let removed_ids: Vec<String> = docs
        .iter()
        .filter(|d| {
            new_ids.contains(d.id.as_str())
                || (d.path == asset_rel && matches!(d.kind, DocKind::Asset | DocKind::Graph))
                || d.id == file_symbol_id
        })
        .map(|d| d.id.clone())
        .collect();
    docs.retain(|d| !removed_ids.contains(&d.id));
    docs.extend(new_docs.iter().cloned());
    docs.sort_by(|a, b| a.id.cmp(&b.id));
    store::save_docs(&paths, &docs)?;

    // 词法索引全量重建(与 build_index 同路径,毫秒级)。
    let lex_input: Vec<(String, String)> = docs
        .iter()
        .map(|d| (d.id.clone(), d.text_for_index()))
        .collect();
    store::save_lexical(&paths, &LexicalIndex::build(&lex_input))?;

    // 向量:仅 hybrid 档索引(向量基座已存在)且 embedder 就绪时顶量同步;
    // 失败移除旧向量 + 如实上报。
    let mut embedded = false;
    let mut embed_error: Option<String> = None;
    if let Some(embedder) = embedder.filter(|_| manifest.tier == "hybrid") {
        let model = embedder.model().to_string();
        let mut records = store::load_vectors(&paths).unwrap_or_default();
        records.retain(|r| !removed_ids.contains(&r.id) && !new_ids.contains(r.id.as_str()));
        let texts: Vec<String> = new_docs.iter().map(|d| d.text_for_index()).collect();
        match embedder.embed(&texts) {
            Ok(vecs) if vecs.len() == new_docs.len() => {
                for (d, v) in new_docs.iter().zip(vecs) {
                    records.push(VectorRecord::new(
                        d.id.clone(),
                        d.content_hash.clone(),
                        model.clone(),
                        v,
                    ));
                }
                records.sort_by(|a, b| a.id.cmp(&b.id));
                store::save_vectors(&paths, &records)?;
                embedded = true;
            }
            Ok(vecs) => {
                store::save_vectors(&paths, &records)?;
                embed_error = Some(format!(
                    "embedding 返回条数不匹配:期望 {} 实得 {};该文件旧向量已移除(词法仍即时可检)",
                    new_docs.len(),
                    vecs.len()
                ));
            }
            Err(e) => {
                store::save_vectors(&paths, &records)?;
                embed_error = Some(format!("{e};该文件旧向量已移除(词法仍即时可检)"));
            }
        }
    }

    // manifest:hash 表同步(清旧 id、写新 id),doc_count/built_at 刷新。
    for id in &removed_ids {
        manifest.content_hashes.remove(id);
    }
    for d in &new_docs {
        manifest
            .content_hashes
            .insert(d.id.clone(), d.content_hash.clone());
    }
    manifest.doc_count = docs.len();
    manifest.built_at = forge_util::timeutil::utc_now_iso8601();
    store::save_manifest(&paths, &manifest)?;

    Ok(UpsertOutcome {
        indexed: true,
        docs: new_docs.len(),
        embedded,
        tier: manifest.tier.clone(),
        embed_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use assetd::project::ForgeProject;
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

    // ---------- upsert_asset_docs ----------

    /// 写简介并 upsert(测试辅助:source=human,与 asset_set_description 同路径)。
    fn set_desc_and_upsert(
        dir: &std::path::Path,
        rel: &str,
        desc: &str,
        embedder: Option<&dyn Embedder>,
    ) -> UpsertOutcome {
        let project = ForgeProject::with_defaults(dir.to_path_buf());
        assetd::ops::set_description(&project, rel, desc, &["测试".to_string()], "human", None, None)
            .unwrap();
        upsert_asset_docs(dir, rel, embedder).unwrap()
    }

    #[test]
    fn upsert_skips_when_index_not_built() {
        let dir = setup_project("up-nobuild");
        let project = ForgeProject::with_defaults(dir.clone());
        assetd::ops::set_description(&project, "Scripts/a.rx", "脚本 A 简介", &[], "human", None, None)
            .unwrap();
        let o = upsert_asset_docs(&dir, "Scripts/a.rx", None).unwrap();
        assert!(!o.indexed, "索引未建须如实跳过:{o:?}");
        assert_eq!(o.docs, 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upsert_makes_description_searchable_without_rebuild() {
        let dir = setup_project("up-search");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        build_index(&opts, None, false).unwrap();

        // 写简介 + upsert(索引已建,词法档)。
        let o = set_desc_and_upsert(&dir, "Scripts/a.rx", "开门角度计算脚本,适合机关谜题", None);
        assert!(o.indexed && !o.embedded, "{o:?}");
        assert_eq!(o.docs, 2, "asset + symbol 文件级两条:{o:?}");

        // 不重建索引,检索即命中简介文本。
        let paths = IndexPaths::for_project(&dir);
        let out = crate::search::search(&paths, "机关谜题", 5, &crate::search::SearchFilter::default(), None)
            .unwrap();
        assert!(!out.hits.is_empty(), "upsert 后词法路须即时可检");
        // asset 文档 id 已从 asset:path: 过渡到 asset:<guid>(ensure_meta 补建)。
        let docs = store::load_docs(&paths).unwrap();
        assert!(!docs.iter().any(|d| d.id == "asset:path:Scripts/a.rx"), "旧 path id 须清除");
        let asset = docs.iter().find(|d| d.kind == DocKind::Asset && d.path == "Scripts/a.rx").unwrap();
        assert!(asset.id.starts_with("asset:") && asset.guid.is_some(), "{asset:?}");
        assert!(asset.description.contains("机关谜题"));
        // symbol 文件级文档同步携带简介(共享 .meta semantic)。
        let sym = docs.iter().find(|d| d.id == "symbol:Scripts/a.rx").unwrap();
        assert!(sym.description.contains("机关谜题"), "{sym:?}");
        // 函数级文档不受影响。
        assert!(docs.iter().any(|d| d.id == "symbol:Scripts/a.rx#fa"));
        // manifest hash 表:旧 id 清、新 id 在。
        let m = store::load_manifest(&paths).unwrap().unwrap();
        assert!(!m.content_hashes.contains_key("asset:path:Scripts/a.rx"));
        assert_eq!(m.content_hashes.get(&asset.id), Some(&asset.content_hash));
        assert_eq!(m.doc_count, docs.len());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upsert_embeds_only_touched_docs() {
        let dir = setup_project("up-embed");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        let emb = FakeEmbedder { calls: AtomicUsize::new(0) };
        build_index(&opts, Some(&emb), false).unwrap();
        let base = emb.calls.load(Ordering::SeqCst);
        assert!(base > 0);

        let o = set_desc_and_upsert(&dir, "Scripts/a.rx", "脚本 A 新简介", Some(&emb));
        assert!(o.indexed && o.embedded, "{o:?}");
        assert_eq!(o.tier, "hybrid");
        // 只嵌本次变更的 2 条(asset + symbol 文件级),不全量重嵌。
        assert_eq!(emb.calls.load(Ordering::SeqCst) - base, 2);
        // 向量记录与 docs 对齐(无旧 id 残留)。
        let paths = IndexPaths::for_project(&dir);
        let recs = store::load_vectors(&paths).unwrap();
        let docs = store::load_docs(&paths).unwrap();
        assert_eq!(recs.len(), docs.len());
        assert!(recs.iter().all(|r| docs.iter().any(|d| d.id == r.id)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upsert_lexical_tier_index_stays_lexical_even_with_embedder() {
        // 词法档索引(建时无 embedder)+ upsert 传 embedder → 不部分向量化:
        // embedded=false,tier 维持 lexical,不产 vectors 文件(向量待全量构建建立)。
        let dir = setup_project("up-lexgate");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        build_index(&opts, None, false).unwrap();
        let emb = FakeEmbedder { calls: AtomicUsize::new(0) };

        let o = set_desc_and_upsert(&dir, "Scripts/a.rx", "词法档索引的简介", Some(&emb));
        assert!(o.indexed && !o.embedded, "{o:?}");
        assert_eq!(o.tier, "lexical", "{o:?}");
        assert_eq!(emb.calls.load(Ordering::SeqCst), 0, "词法档索引不应触发嵌入");
        let paths = IndexPaths::for_project(&dir);
        assert!(!paths.vectors().is_file(), "不应产部分向量文件");
        // 词法路即时可检。
        let out = crate::search::search(&paths, "词法档索引", 5, &crate::search::SearchFilter::default(), None)
            .unwrap();
        assert!(!out.hits.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upsert_embed_failure_removes_stale_vectors() {
        struct FailEmbedder;
        impl Embedder for FailEmbedder {
            fn model(&self) -> &str {
                "fail-model"
            }
            fn embed(&self, _: &[String]) -> std::result::Result<Vec<Vec<f32>>, String> {
                Err("HTTP 502: 上游失败".into())
            }
        }
        let dir = setup_project("up-fail");
        let opts = ExtractOptions { project_root: dir.clone(), docs_root: None };
        let ok = FakeEmbedder { calls: AtomicUsize::new(0) };
        build_index(&opts, Some(&ok), false).unwrap();

        let o = set_desc_and_upsert(&dir, "Scripts/a.rx", "会触发嵌入失败的简介", Some(&FailEmbedder));
        assert!(o.indexed && !o.embedded, "{o:?}");
        assert!(o.embed_error.is_some(), "失败须如实上抛:{o:?}");
        // 被替换文档(asset + symbol 文件级)的旧向量已移除;函数级与其他文档向量保留。
        let paths = IndexPaths::for_project(&dir);
        let recs = store::load_vectors(&paths).unwrap();
        let docs = store::load_docs(&paths).unwrap();
        let replaced_ids: Vec<&str> = docs
            .iter()
            .filter(|d| d.path == "Scripts/a.rx" && !d.id.contains('#'))
            .map(|d| d.id.as_str())
            .collect();
        assert!(replaced_ids.len() == 2, "asset + symbol 文件级两条:{replaced_ids:?}");
        assert!(recs.iter().all(|r| !replaced_ids.contains(&r.id.as_str())), "旧向量须移除");
        assert!(recs.iter().any(|r| r.id == "symbol:Scripts/a.rx#fa"), "函数级向量保留");
        assert!(recs.iter().any(|r| r.id.contains("b.rx")), "其他文档向量保留");
        // 词法路仍即时可检。
        let out = crate::search::search(&paths, "嵌入失败", 5, &crate::search::SearchFilter::default(), None)
            .unwrap();
        assert!(!out.hits.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
