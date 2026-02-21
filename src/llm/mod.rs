//! LLM 层：客户端抽象与实现（OpenAI 兼容 / DeepSeek / Mock）

pub mod deepseek;
pub mod embedding;
pub mod mock;
pub mod openai;
pub mod router;
pub mod traits;

pub use deepseek::{create_deepseek_client, DEEPSEEK_CHAT, DEEPSEEK_REASONER};
pub use embedding::{create_embedder_from_config, EmbeddingProvider, OpenAiEmbedder};
pub use mock::MockLlmClient;
pub use openai::{OpenAiClient, TokenUsage};
pub use router::{
    ModelCapabilities, ModelRouter, RoutingLlmClient, RoutingStrategy, TaskClassifier, TaskType,
};
pub use traits::{LlmClient, LlmError, RetryConfig, RetryingLlmClient};
