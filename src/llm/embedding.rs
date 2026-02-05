//! 嵌入 API：供向量长期记忆使用，调用 OpenAI 兼容的 /embeddings 端点

use std::sync::Arc;

use async_openai::config::OpenAIConfig;
use async_openai::types::embeddings::{CreateEmbeddingRequestArgs, EmbeddingInput};
use async_openai::Client;

/// 可从 sync 上下文调用的嵌入提供方（内部用 block_on 执行 async 调用）
pub trait EmbeddingProvider: Send + Sync {
    /// 将文本编码为向量；失败时返回错误字符串
    fn embed_sync(&self, text: &str) -> Result<Vec<f32>, String>;
}

/// 使用 async-openai 调用 OpenAI 兼容的 embeddings API
pub struct OpenAiEmbedder {
    client: Client<OpenAIConfig>,
    model: String,
}

impl OpenAiEmbedder {
    /// 从环境变量与可选 base_url 创建（与 LLM 共用 OPENAI_API_KEY / base_url）
    pub fn new(base_url: Option<&str>, model: &str, api_key: Option<&str>) -> Self {
        let api_key = api_key
            .map(String::from)
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .unwrap_or_else(|| "sk-placeholder".to_string());

        let config = if let Some(url) = base_url {
            OpenAIConfig::new()
                .with_api_base(url)
                .with_api_key(api_key)
        } else {
            OpenAIConfig::new().with_api_key(api_key)
        };

        Self {
            client: Client::with_config(config),
            model: model.to_string(),
        }
    }

    pub async fn embed_async(&self, text: &str) -> Result<Vec<f32>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(vec![]);
        }
        let request = CreateEmbeddingRequestArgs::default()
            .model(&self.model)
            .input(EmbeddingInput::String(text.to_string()))
            .build()
            .map_err(|e| e.to_string())?;
        let response = self
            .client
            .embeddings()
            .create(request)
            .await
            .map_err(|e| e.to_string())?;
        let vec = response
            .data
            .first()
            .map(|e| e.embedding.clone())
            .unwrap_or_default();
        Ok(vec)
    }
}

impl EmbeddingProvider for OpenAiEmbedder {
    fn embed_sync(&self, text: &str) -> Result<Vec<f32>, String> {
        let text = text.to_string();
        let this = self.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(this.embed_async(&text))
        })
    }
}

impl Clone for OpenAiEmbedder {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            model: self.model.clone(),
        }
    }
}

/// 从应用配置创建嵌入提供方；未配置或未启用时返回 None
pub fn create_embedder_from_config(
    base_url: Option<&str>,
    model: &str,
    api_key: Option<&str>,
) -> Option<Arc<dyn EmbeddingProvider>> {
    let key = api_key
        .map(String::from)
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());
    if key.as_deref().unwrap_or("").is_empty() || key.as_deref() == Some("sk-placeholder") {
        tracing::debug!("embedding skipped: no OPENAI_API_KEY");
        return None;
    }
    Some(Arc::new(OpenAiEmbedder::new(
        base_url,
        model,
        key.as_deref(),
    )))
}
