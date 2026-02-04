//! Agent 错误类型与恢复动作
//!
//! 与 RecoveryEngine 配合：根据 AgentError 决定 RetryWithPrompt / SummarizeAndPrune / AskUser / Abort 等。

use thiserror::Error;

/// Agent 运行过程中可能出现的错误（网络、解析、工具、路径逃逸等）
#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Network timeout")]
    NetworkTimeout,

    #[error("Context window exceeded")]
    ContextWindowExceeded,

    #[error("JSON parse error: {0}")]
    JsonParseError(String),

    #[error("Tool execution failed: {0}")]
    ToolExecutionFailed(String),

    #[error("Tool timeout: {0}")]
    ToolTimeout(String),

    #[error("Hallucinated tool: {0}")]
    HallucinatedTool(String),

    #[error("LLM error: {0}")]
    LlmError(String),

    /// 恢复引擎建议降级模型（如 LLM 持续失败时），由上层决定是否切换轻量模型
    #[error("Suggest downgrade model: {0}")]
    SuggestDowngradeModel(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Path escape attempt: {0}")]
    PathEscape(String),
}

/// 恢复引擎根据错误类型给出的建议动作
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// 将提示注入下一轮，让 LLM 重试（如 JSON 格式错误）
    RetryWithPrompt(String),
    /// 压缩上下文后继续（如超长上下文）
    SummarizeAndPrune,
    /// 需要用户决策（如幻觉工具、超时）
    AskUser(String),
    DowngradeModel,
    /// 终止当前任务
    Abort,
}
