//! Mock LLM 客户端（用于测试，无需 API）
//!
//! 取最后一条 User 消息，回显为 JSON Tool Call（echo），便于本地跑通 ReAct 流程。

use std::pin::Pin;

use async_trait::async_trait;
use futures_util::stream;

use crate::llm::LlmClient;
use crate::memory::Message;

/// Mock 客户端：回显用户最后一条消息
#[derive(Debug, Default)]
pub struct MockLlmClient;

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(&self, messages: &[Message]) -> Result<String, String> {
        let last_user = messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, crate::memory::Role::User))
            .map(|m| m.content.as_str())
            .unwrap_or("(no input)");

        Ok(format!(
            r#"{{"tool": "echo", "args": {{"text": "Echo from Mock: {}"}}}}"#,
            last_user
        ))
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
    ) -> Result<std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<String, String>> + Send>>, String> {
        let content = self.complete(messages).await?;
        Ok(Box::pin(stream::iter(vec![Ok(content)])))
    }
}
