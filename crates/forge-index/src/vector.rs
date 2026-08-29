//! 向量档:Embedder trait(实现方注入,本库不触网)+ 向量记录 base64(f32 LE)编解码
//! + L2 归一化 + 暴力 cosine 检索(千级文档亚毫秒,无需 ANN)。

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// 远程/本地 embedding 抽象(实现在 gend::embed;本库仅依赖 trait,保持可纯单测)。
pub trait Embedder {
    /// 模型标识(进 vectors.jsonl 与 manifest,模型换了 = 全量重嵌)。
    fn model(&self) -> &str;
    /// 批量嵌入;失败返回不含密钥的错误消息(R-5)。
    fn embed(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, String>;
}

/// 单条向量记录(vectors.jsonl 每行一条;v = base64(f32 LE),写入前已 L2 归一化)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorRecord {
    pub id: String,
    /// 对应 IndexDoc.content_hash(增量判定)。
    pub hash: String,
    pub model: String,
    pub dim: usize,
    pub v: String,
}

impl VectorRecord {
    pub fn new(id: String, hash: String, model: String, mut vec: Vec<f32>) -> Self {
        normalize(&mut vec);
        VectorRecord {
            id,
            hash,
            model,
            dim: vec.len(),
            v: encode_vec(&vec),
        }
    }

    pub fn decode(&self) -> Option<Vec<f32>> {
        let v = decode_vec(&self.v)?;
        if v.len() != self.dim {
            return None;
        }
        Some(v)
    }
}

/// f32 切片 → base64(LE 字节)。
pub fn encode_vec(v: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for x in v {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// base64 → f32 向量;长度非 4 倍数或解码失败 → None。
pub fn decode_vec(s: &str) -> Option<Vec<f32>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(s).ok()?;
    if bytes.len() % 4 != 0 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

/// L2 归一化(零向量原样保留)。
pub fn normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// 点积(双方已归一化 → 即 cosine)。
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// 暴力 cosine 检索:query 未归一化则先归一;返回 (id, score) 降序 top_k。
pub fn top_k_similar(
    query: &[f32],
    records: &[(String, Vec<f32>)],
    top_k: usize,
) -> Vec<(String, f64)> {
    let mut q = query.to_vec();
    normalize(&mut q);
    let mut hits: Vec<(String, f64)> = records
        .iter()
        .filter(|(_, v)| v.len() == q.len())
        .map(|(id, v)| (id.clone(), dot(&q, v) as f64))
        .collect();
    hits.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    hits.truncate(top_k);
    hits
}

/// RRF 融合(k=60 业界惯例):score = Σ 1/(k+rank+1),免归一化两路分数量纲。
/// 输入为各路 (id, score) 降序列表;返回融合 (id, rrf_score) 降序。
pub fn rrf_fuse(lists: &[&[(String, f64)]], top_k: usize) -> Vec<(String, f64)> {
    const K: f64 = 60.0;
    let mut fused: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for list in lists {
        for (rank, (id, _)) in list.iter().enumerate() {
            *fused.entry(id.clone()).or_insert(0.0) += 1.0 / (K + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(String, f64)> = fused.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    out.truncate(top_k);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let v = vec![0.1f32, -2.5, 3.75, 0.0];
        let s = encode_vec(&v);
        assert_eq!(decode_vec(&s).unwrap(), v);
    }

    #[test]
    fn normalized_record_cosine() {
        let r = VectorRecord::new("a".into(), "h".into(), "m".into(), vec![3.0, 4.0]);
        let v = r.decode().unwrap();
        // 3-4-5 直角边 → 归一化 [0.6, 0.8]。
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        let sim = top_k_similar(&[3.0, 4.0], &[("a".into(), v)], 1);
        assert!((sim[0].1 - 1.0).abs() < 1e-6, "自身相似度应为 1:{sim:?}");
    }

    #[test]
    fn rrf_prefers_doc_in_both_lists() {
        let lex: Vec<(String, f64)> = vec![("a".into(), 9.0), ("b".into(), 5.0)];
        let vec_: Vec<(String, f64)> = vec![("b".into(), 0.9), ("c".into(), 0.8)];
        let fused = rrf_fuse(&[&lex, &vec_], 3);
        assert_eq!(fused[0].0, "b", "双路命中应居首:{fused:?}");
    }
}
