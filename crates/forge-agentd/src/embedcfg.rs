//! F10 embedding 渠道 REST 面(照 llm.rs openai-compat 先例):
//! POST /api/forge/llm/embedding/config {baseUrl, model, key?} +
//! GET  /api/forge/llm/embedding/status → {configured, baseUrl, model, keyConfigured}。
//! R-5:key 只进 keystore,响应/日志/配置 JSON 绝无 key。

use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingConfigRequest {
    /// OpenAI 兼容端点根(调用时拼 /v1/embeddings);必填,空 → 400 EMPTY_BASE_URL。
    #[serde(default)]
    base_url: String,
    /// embedding 模型名;必填,空 → 400 EMPTY_MODEL。
    #[serde(default)]
    model: String,
    /// API Key;可省略 = 只改 baseUrl/model;非空 → 写 keystore["embedding"](永不回显)。
    #[serde(default)]
    key: Option<String>,
}

/// POST /api/forge/llm/embedding/config。
pub async fn set_embedding_config(
    axum::Json(req): axum::Json<EmbeddingConfigRequest>,
) -> axum::response::Response {
    let base_url = req.base_url.trim().to_string();
    if base_url.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(
                json!({ "error": { "code": "EMPTY_BASE_URL", "message": "baseUrl 不可空" } }),
            ),
        )
            .into_response();
    }
    let model = req.model.trim().to_string();
    if model.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": { "code": "EMPTY_MODEL", "message": "model 不可空" } })),
        )
            .into_response();
    }
    if let Err(e) = gend::embed::save_embedding_file(&gend::embed::EmbeddingFile {
        base_url: base_url.clone(),
        model: model.clone(),
    }) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
        )
            .into_response();
    }
    if let Some(k) = req.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        if let Err(e) = gend::keystore::set_key(gend::embed::EMBEDDING_KEYSTORE_ID, k) {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
            )
                .into_response();
        }
    }
    axum::Json(status_json()).into_response()
}

/// GET /api/forge/llm/embedding/status。
pub async fn embedding_status_handler() -> axum::Json<Value> {
    axum::Json(status_json())
}

fn status_json() -> Value {
    let st = gend::embed::embedding_status();
    json!({
        "ok": true,
        "configured": st.configured,
        "baseUrl": st.base_url,
        "model": st.model,
        "keyConfigured": st.key_configured,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 隔离数据目录守卫(照 llm.rs DataDirGuard 纪律)。
    struct DataDirGuard {
        dir: std::path::PathBuf,
    }
    impl DataDirGuard {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "agentd-embedcfg-{tag}-{}-{}",
                std::process::id(),
                gend::timeutil::unix_millis()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
            DataDirGuard { dir }
        }
    }
    impl Drop for DataDirGuard {
        fn drop(&mut self) {
            std::env::remove_var("FORGE_GEN_DATA_DIR");
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    #[tokio::test]
    async fn embedding_config_roundtrip_no_key_leak() {
        // 环境变量互斥(FORGE_GEN_DATA_DIR 进程级;与 llm/agent 测试共锁防互踩)。
        let _g = crate::llm::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FORGE_GEN_API_KEY");
        let guard = DataDirGuard::new("rt");
        let secret = "sk-embed-agentd-REDLINE";
        // 未配置前置。
        let v = embedding_status_handler().await.0;
        assert_eq!(v["configured"], false);
        // POST 配齐。
        let resp = set_embedding_config(axum::Json(EmbeddingConfigRequest {
            base_url: "http://127.0.0.1:9400".into(),
            model: "bge-m3".into(),
            key: Some(secret.into()),
        }))
        .await;
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["configured"], true);
        assert_eq!(v["baseUrl"], "http://127.0.0.1:9400");
        assert_eq!(v["model"], "bge-m3");
        assert_eq!(v["keyConfigured"], true);
        assert!(!v.to_string().contains(secret), "响应回显密钥(R-5): {v}");
        // 配置 JSON 无 key。
        let text = std::fs::read_to_string(guard.dir.join("llm-embedding.json")).unwrap();
        assert!(!text.contains(secret), "配置 JSON 落密钥(R-5): {text}");
        // 空 baseUrl → 400。
        let resp = set_embedding_config(axum::Json(EmbeddingConfigRequest {
            base_url: " ".into(),
            model: "m".into(),
            key: None,
        }))
        .await;
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, axum::http::StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "EMPTY_BASE_URL");
    }
}
