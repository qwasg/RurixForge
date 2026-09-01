//! 零依赖词法检索:分词器(ASCII 整词 + camelCase/snake_case 切分;CJK unigram+bigram)
//! + BM25(k1=1.2, b=0.75)。倒排 BTreeMap 保序列化确定性。

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

const K1: f64 = 1.2;
const B: f64 = 0.75;

/// CJK 统一表意文字(基本区 + 扩展 A + 兼容区)。
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
    )
}

/// 分词:ASCII 连续段 → 小写整词 + camelCase 子词;CJK 连续段 → 单字 + 相邻双字。
/// 其余字符(标点/空白/下划线)作分隔符。
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut ascii = String::new();
    let mut cjk: Vec<char> = Vec::new();

    let flush_ascii = |buf: &mut String, out: &mut Vec<String>| {
        if buf.is_empty() {
            return;
        }
        let whole = buf.to_lowercase();
        // camelCase 切分:小写/数字 → 大写边界。
        let mut parts: Vec<String> = Vec::new();
        let mut cur = String::new();
        let chars: Vec<char> = buf.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if i > 0
                && c.is_ascii_uppercase()
                && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit())
            {
                parts.push(cur.to_lowercase());
                cur = String::new();
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            parts.push(cur.to_lowercase());
        }
        if parts.len() > 1 {
            for p in parts {
                if !p.is_empty() {
                    out.push(p);
                }
            }
        }
        out.push(whole);
        buf.clear();
    };
    let flush_cjk = |buf: &mut Vec<char>, out: &mut Vec<String>| {
        if buf.is_empty() {
            return;
        }
        for &c in buf.iter() {
            out.push(c.to_string());
        }
        for w in buf.windows(2) {
            out.push(w.iter().collect());
        }
        buf.clear();
    };

    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk, &mut out);
            ascii.push(c);
        } else if is_cjk(c) {
            flush_ascii(&mut ascii, &mut out);
            cjk.push(c);
        } else {
            flush_ascii(&mut ascii, &mut out);
            flush_cjk(&mut cjk, &mut out);
        }
    }
    flush_ascii(&mut ascii, &mut out);
    flush_cjk(&mut cjk, &mut out);
    out
}

/// BM25 倒排索引(全量重建;千级文档毫秒级,无需增量)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LexicalIndex {
    pub doc_ids: Vec<String>,
    pub doc_lens: Vec<u32>,
    pub avgdl: f64,
    /// term → [(doc 下标, 词频)];BTreeMap 保 JSON 键序确定。
    pub postings: BTreeMap<String, Vec<(u32, u32)>>,
}

impl LexicalIndex {
    /// 从 (id, text) 列表构建。
    pub fn build(docs: &[(String, String)]) -> Self {
        let mut doc_ids = Vec::with_capacity(docs.len());
        let mut doc_lens = Vec::with_capacity(docs.len());
        let mut postings: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
        for (idx, (id, text)) in docs.iter().enumerate() {
            let terms = tokenize(text);
            doc_ids.push(id.clone());
            doc_lens.push(terms.len() as u32);
            let mut tf: HashMap<String, u32> = HashMap::new();
            for t in terms {
                *tf.entry(t).or_insert(0) += 1;
            }
            for (t, n) in tf {
                postings.entry(t).or_default().push((idx as u32, n));
            }
        }
        // posting 内按 doc 下标排序(HashMap 迭代序不定)。
        for plist in postings.values_mut() {
            plist.sort_by_key(|(di, _)| *di);
        }
        let avgdl = if doc_lens.is_empty() {
            0.0
        } else {
            doc_lens.iter().map(|&l| l as f64).sum::<f64>() / doc_lens.len() as f64
        };
        LexicalIndex { doc_ids, doc_lens, avgdl, postings }
    }

    /// BM25 检索,返回 (doc_id, score) 降序;allowed = None 不过滤。
    pub fn search(
        &self,
        query: &str,
        top_k: usize,
        allowed: Option<&std::collections::HashSet<String>>,
    ) -> Vec<(String, f64)> {
        let mut q_terms = tokenize(query);
        q_terms.sort();
        q_terms.dedup();
        let n = self.doc_ids.len() as f64;
        if n == 0.0 {
            return Vec::new();
        }
        let mut scores: HashMap<u32, f64> = HashMap::new();
        for t in &q_terms {
            let Some(plist) = self.postings.get(t) else { continue };
            let df = plist.len() as f64;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for &(di, tf) in plist {
                let dl = self.doc_lens[di as usize] as f64;
                let tf = tf as f64;
                let denom = tf + K1 * (1.0 - B + B * dl / self.avgdl.max(1.0));
                *scores.entry(di).or_insert(0.0) += idf * tf * (K1 + 1.0) / denom;
            }
        }
        let mut hits: Vec<(String, f64)> = scores
            .into_iter()
            .map(|(di, s)| (self.doc_ids[di as usize].clone(), s))
            .filter(|(id, _)| allowed.map(|a| a.contains(id)).unwrap_or(true))
            .collect();
        // 降序;同分按 id 升序保确定性。
        hits.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        hits.truncate(top_k);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_mixed_chinese_english() {
        let toks = tokenize("MeshRenderer 木质餐椅 wood_albedo");
        // camelCase 切分。
        assert!(toks.contains(&"mesh".to_string()), "{toks:?}");
        assert!(toks.contains(&"renderer".to_string()), "{toks:?}");
        assert!(toks.contains(&"meshrenderer".to_string()), "{toks:?}");
        // CJK unigram + bigram。
        assert!(toks.contains(&"木".to_string()), "{toks:?}");
        assert!(toks.contains(&"木质".to_string()), "{toks:?}");
        assert!(toks.contains(&"餐椅".to_string()), "{toks:?}");
        // snake_case 由下划线分隔符自然切开。
        assert!(toks.contains(&"wood".to_string()), "{toks:?}");
        assert!(toks.contains(&"albedo".to_string()), "{toks:?}");
    }

    #[test]
    fn bm25_chinese_recall_ranks_relevant_first() {
        let docs = vec![
            ("a".to_string(), "四腿木质餐椅 家具 椅子 室内".to_string()),
            ("b".to_string(), "石质地板砖 建筑 地面".to_string()),
            ("c".to_string(), "木质圆桌 家具 桌子".to_string()),
        ];
        let idx = LexicalIndex::build(&docs);
        let hits = idx.search("木质椅子", 3, None);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].0, "a", "「木质椅子」应首推餐椅:{hits:?}");
        // 石质地板不含任何查询词 → 不应出现或排最后。
        assert!(hits.iter().all(|(id, s)| id != "b" || *s < hits[0].1));
    }

    #[test]
    fn search_respects_allowed_filter() {
        let docs = vec![
            ("x".to_string(), "wooden chair".to_string()),
            ("y".to_string(), "wooden table".to_string()),
        ];
        let idx = LexicalIndex::build(&docs);
        let allowed: std::collections::HashSet<String> = ["y".to_string()].into();
        let hits = idx.search("wooden", 10, Some(&allowed));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "y");
    }

    #[test]
    fn empty_index_returns_empty() {
        let idx = LexicalIndex::build(&[]);
        assert!(idx.search("任意", 5, None).is_empty());
    }
}
