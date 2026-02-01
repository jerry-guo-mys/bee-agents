//! Planner：意图规划与 Tool Call 解析
//!
//! 调用 LLM 得到回复或 JSON Tool Call；parse_llm_output 从文本中提取 JSON 并解析为 ToolCall 或直接回复。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::AgentError;
use crate::llm::LlmClient;
use crate::memory::Message;

/// LLM 返回的 Tool Call（简化 JSON：{"tool": "cat", "args": {"path": "..."}}）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    pub args: serde_json::Value,
}

/// Planner 输出
#[derive(Debug, Clone)]
pub enum PlannerOutput {
    /// 直接回复用户
    Response(String),
    /// 需要执行工具
    ToolCall(ToolCall),
}

/// 解析 LLM 输出：若含有效 JSON 且 tool 非空则为 ToolCall，否则为 Response
pub fn parse_llm_output(output: &str) -> Result<PlannerOutput, AgentError> {
    let trimmed = output.trim();

    // 尝试提取 JSON 块（```json ... ``` 或纯 JSON）
    let json_str = if let Some(start) = trimmed.find("```json") {
        let rest = &trimmed[start + 7..];
        rest.find("```")
            .map(|end| rest[..end].trim())
            .unwrap_or(rest.trim())
    } else if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        return Ok(PlannerOutput::Response(trimmed.to_string()));
    };

    let parsed: ToolCall = serde_json::from_str(json_str)
        .map_err(|e| AgentError::JsonParseError(format!("{}: {}", e, json_str)))?;

    if parsed.tool.is_empty() {
        Ok(PlannerOutput::Response(trimmed.to_string()))
    } else {
        Ok(PlannerOutput::ToolCall(parsed))
    }
}

/// Planner：持有 LLM 与 system prompt，负责 plan / plan_with_system（拼 system + messages 后调用 LLM）
pub struct Planner {
    llm: Arc<dyn LlmClient>,
    system_prompt: String,
}

impl Planner {
    pub fn new(llm: Arc<dyn LlmClient>, system_prompt: impl Into<String>) -> Self {
        Self {
            llm,
            system_prompt: system_prompt.into(),
        }
    }

    pub fn base_system_prompt(&self) -> &str {
        &self.system_prompt
    }

    /// 获取 LLM 累计 token 使用统计
    pub fn token_usage(&self) -> (u64, u64, u64) {
        self.llm.token_usage()
    }

    pub async fn plan(&self, messages: &[Message]) -> Result<String, AgentError> {
        self.plan_with_system(messages, &self.system_prompt).await
    }

    /// 使用动态拼接的 system（含 working memory、long-term 检索等）
    pub async fn plan_with_system(
        &self,
        messages: &[Message],
        system: &str,
    ) -> Result<String, AgentError> {
        let mut full_messages = vec![Message::system(system.to_string())];
        full_messages.extend(messages.to_vec());
        self.llm
            .complete(&full_messages)
            .await
            .map_err(|e| AgentError::LlmError(e))
    }
}
