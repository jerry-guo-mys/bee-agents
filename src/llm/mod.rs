//! LLM 层：客户端抽象与实现（OpenAI 兼容 / DeepSeek / Mock）

pub mod deepseek;
pub mod mock;
pub mod openai;
pub mod traits;

pub use deepseek::{create_deepseek_client, DEEPSEEK_CHAT, DEEPSEEK_REASONER};
pub use mock::MockLlmClient;
pub use openai::{OpenAiClient, TokenUsage};
pub use traits::LlmClient;
