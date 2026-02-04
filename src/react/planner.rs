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
        // 找到第一个 { 和对应的 }，支持嵌套
        extract_first_json_object(&trimmed[start..])
            .map(|s| &trimmed[start..start + s.len()])
            .unwrap_or(trimmed)
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

/// 提取第一个完整的 JSON 对象（支持嵌套）
fn extract_first_json_object(s: &str) -> Option<&str> {
    let mut depth = 0;
    let mut start = None;
    
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start_idx) = start {
                        return Some(&s[start_idx..=i]);
                    }
                }
            }
            _ => {}
        }
    }
    None
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
            .map_err(AgentError::LlmError)
    }

    /// 将对话历史压缩为一段摘要（用于 Context Compaction：写入长期记忆后替换当前消息）
    pub async fn summarize(&self, messages: &[Message]) -> Result<String, AgentError> {
        if messages.is_empty() {
            return Ok(String::new());
        }
        let system = "You are a summarizer. Summarize the following conversation in one short paragraph: key facts, decisions, user preferences, and the latest question if any. Use the same language as the conversation. Output only the summary, no preamble.";
        let mut full = vec![Message::system(system.to_string())];
        full.extend(messages.to_vec());
        self.llm
            .complete(&full)
            .await
            .map_err(AgentError::LlmError)
    }
}
