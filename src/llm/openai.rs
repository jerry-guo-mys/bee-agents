//! OpenAI 兼容 API 客户端
//!
//! 通过 async_openai 调用任意 OpenAI 兼容端点（可配置 base_url）；支持 DeepSeek、OpenAI、自建代理等。

use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use async_trait::async_trait;
use futures_util::stream;

use crate::llm::LlmClient;
use crate::memory::Message;

/// Token 使用统计（累计值）
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Arc<AtomicU64>,
    pub completion_tokens: Arc<AtomicU64>,
    pub total_tokens: Arc<AtomicU64>,
}

impl TokenUsage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, prompt: u64, completion: u64) {
        self.prompt_tokens.fetch_add(prompt, Ordering::Relaxed);
        self.completion_tokens.fetch_add(completion, Ordering::Relaxed);
        self.total_tokens.fetch_add(prompt + completion, Ordering::Relaxed);
    }

    pub fn get(&self) -> (u64, u64, u64) {
        (
            self.prompt_tokens.load(Ordering::Relaxed),
            self.completion_tokens.load(Ordering::Relaxed),
            self.total_tokens.load(Ordering::Relaxed),
        )
    }
}

/// OpenAI 兼容客户端：持有 Client 与 model 名，complete 时转 Message 为 API 格式并取首条 content
pub struct OpenAiClient {
    client: Client<OpenAIConfig>,
    model: String,
    /// 累计 token 使用统计
    pub usage: TokenUsage,
}

impl OpenAiClient {
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
            usage: TokenUsage::new(),
        }
    }

    /// 获取累计 token 使用统计
    pub fn token_usage(&self) -> (u64, u64, u64) {
        self.usage.get()
    }

    fn to_openai_messages(&self, messages: &[Message]) -> Vec<ChatCompletionRequestMessage> {
        messages
            .iter()
            .map(|m| match m.role {
                crate::memory::Role::System => ChatCompletionRequestMessage::System(
                    ChatCompletionRequestSystemMessageArgs::default()
                        .content(m.content.clone())
                        .build()
                        .unwrap(),
                ),
                crate::memory::Role::User => ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(m.content.clone())
                        .build()
                        .unwrap(),
                ),
                crate::memory::Role::Assistant => ChatCompletionRequestMessage::Assistant(
                    ChatCompletionRequestAssistantMessageArgs::default()
                        .content(m.content.clone())
                        .build()
                        .unwrap(),
                ),
            })
            .collect()
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    fn token_usage(&self) -> (u64, u64, u64) {
        self.usage.get()
    }

    async fn complete(&self, messages: &[Message]) -> Result<String, String> {
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(self.to_openai_messages(messages))
            .build()
            .map_err(|e| e.to_string())?;

        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| e.to_string())?;

        // 提取 token 使用统计
        if let Some(usage) = &response.usage {
            self.usage.add(
                usage.prompt_tokens as u64,
                usage.completion_tokens as u64,
            );
        }

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(content)
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
    ) -> Result<std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<String, String>> + Send>>, String> {
        let content = self.complete(messages).await?;
        Ok(Box::pin(stream::iter(vec![Ok(content)])))
    }
}
