//! LLM 客户端抽象
//!
//! 所有后端（OpenAI 兼容 / DeepSeek / Mock）实现 LlmClient：complete（非流式）、complete_stream（流式 Token）。

use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;

use crate::memory::Message;

/// LLM 客户端 trait：非流式完成与流式完成（返回 Token 流）
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 非流式完成
    async fn complete(&self, messages: &[Message]) -> Result<String, String>;

    /// 流式完成，返回 Token 流
    async fn complete_stream(
        &self,
        messages: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, String>> + Send>>, String>;

    /// 获取累计 token 使用统计：(prompt_tokens, completion_tokens, total_tokens)
    /// 默认返回 (0, 0, 0)，具体实现可覆盖
    fn token_usage(&self) -> (u64, u64, u64) {
        (0, 0, 0)
    }
}
