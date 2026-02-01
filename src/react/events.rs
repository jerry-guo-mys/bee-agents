//! ReAct 过程事件：用于流式/SSE 展示思考、工具调用、观察与回复

use serde::Serialize;

/// 单步过程事件（可序列化为 JSON 供前端展示）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReactEvent {
    /// ReAct 步数更新（当前第几步）
    StepUpdate { step: usize, max_steps: usize },
    /// 正在调用 LLM 思考
    Thinking,
    /// LLM 的思考/规划内容（Plan 或推理过程）
    ThinkingContent { text: String },
    /// 调用工具
    ToolCall {
        tool: String,
        args: serde_json::Value,
    },
    /// 工具返回（预览，避免过长）
    Observation {
        tool: String,
        preview: String,
    },
    /// 工具执行失败（记录到 Working Memory）
    ToolFailure { tool: String, reason: String },
    /// 错误恢复动作（RetryWithPrompt / AskUser / Abort 等）
    Recovery { action: String, detail: String },
    /// 使用长期记忆改进回答（检索到的相关内容预览）
    MemoryRecovery { preview: String },
    /// 整理对话到长期记忆（写入内容预览）
    MemoryConsolidation { preview: String },
    /// 最终回复的一小段（流式输出）
    MessageChunk { text: String },
    /// 最终回复结束
    MessageDone,
    /// Token 使用统计（本次对话增量 + 累计）
    TokenUsage {
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        /// 累计 prompt tokens
        cumulative_prompt: u64,
        /// 累计 completion tokens
        cumulative_completion: u64,
        /// 累计 total tokens
        cumulative_total: u64,
    },
    /// 错误
    Error { text: String },
}
