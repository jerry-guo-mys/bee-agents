//! 错误恢复引擎
//!
//! 根据 AgentError 类型返回 RecoveryAction，供 ReAct 循环决定是重试、剪枝、询问用户还是终止。

use crate::core::{AgentError, RecoveryAction};
use crate::memory::Message;

/// 语义化错误恢复：将错误映射为可执行动作（重试提示 / 剪枝 / 问用户 / 终止）
#[derive(Debug, Default)]
pub struct RecoveryEngine;

impl RecoveryEngine {
    pub fn new() -> Self {
        Self
    }

    /// 根据错误类型返回建议的恢复动作；history 预留用于未来「剪枝后重试」等逻辑
    pub fn handle(&self, err: &AgentError, _history: &mut [Message]) -> RecoveryAction {
        match err {
            AgentError::JsonParseError(raw) => RecoveryAction::RetryWithPrompt(format!(
                "上一轮输出的 JSON 格式错误: {raw}。\
                调用工具时你必须只输出一个合法的 JSON 对象，不能输出代码、Markdown 或其它文字。\
                格式必须为: {{\"tool\": \"工具名\", \"args\": {{...}}}}。\
                例如: {{\"tool\": \"echo\", \"args\": {{\"text\": \"hi\"}}}}。请只输出这一行 JSON。"
            )),
            AgentError::ContextWindowExceeded => RecoveryAction::SummarizeAndPrune,
            AgentError::HallucinatedTool(name) => RecoveryAction::AskUser(format!(
                "模型试图调用不存在的工具 '{name}'，是否需要安装或跳过？"
            )),
            AgentError::ToolTimeout(_) => {
                RecoveryAction::AskUser("工具执行超时，是否重试？".to_string())
            }
            AgentError::ToolExecutionFailed(msg) => {
                RecoveryAction::AskUser(format!("工具执行失败: {msg}"))
            }
            AgentError::NetworkTimeout => RecoveryAction::RetryWithPrompt(
                "网络请求超时，请重试。".to_string(),
            ),
            AgentError::LlmError(_) => RecoveryAction::DowngradeModel,
            AgentError::Cancelled => RecoveryAction::Abort,
            _ => RecoveryAction::Abort,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmError;

    #[test]
    fn test_recovery_json_parse_error() {
        let engine = RecoveryEngine::new();
        let err = AgentError::JsonParseError("invalid json".to_string());
        let action = engine.handle(&err, &mut []);
        match action {
            RecoveryAction::RetryWithPrompt(msg) => {
                assert!(msg.contains("JSON"));
            }
            _ => panic!("Expected RetryWithPrompt"),
        }
    }

    #[test]
    fn test_recovery_context_exceeded() {
        let engine = RecoveryEngine::new();
        let err = AgentError::ContextWindowExceeded;
        let action = engine.handle(&err, &mut []);
        assert!(matches!(action, RecoveryAction::SummarizeAndPrune));
    }

    #[test]
    fn test_recovery_hallucinated_tool() {
        let engine = RecoveryEngine::new();
        let err = AgentError::HallucinatedTool("fake_tool".to_string());
        let action = engine.handle(&err, &mut []);
        match action {
            RecoveryAction::AskUser(msg) => {
                assert!(msg.contains("fake_tool"));
            }
            _ => panic!("Expected AskUser"),
        }
    }

    #[test]
    fn test_recovery_tool_timeout() {
        let engine = RecoveryEngine::new();
        let err = AgentError::ToolTimeout("shell".to_string());
        let action = engine.handle(&err, &mut []);
        assert!(matches!(action, RecoveryAction::AskUser(_)));
    }

    #[test]
    fn test_recovery_llm_error() {
        let engine = RecoveryEngine::new();
        let err = AgentError::LlmError(LlmError::RateLimited { retry_after_ms: 1000 });
        let action = engine.handle(&err, &mut []);
        assert!(matches!(action, RecoveryAction::DowngradeModel));
    }

    #[test]
    fn test_recovery_cancelled() {
        let engine = RecoveryEngine::new();
        let err = AgentError::Cancelled;
        let action = engine.handle(&err, &mut []);
        assert!(matches!(action, RecoveryAction::Abort));
    }

    #[test]
    fn test_recovery_network_timeout() {
        let engine = RecoveryEngine::new();
        let err = AgentError::NetworkTimeout;
        let action = engine.handle(&err, &mut []);
        assert!(matches!(action, RecoveryAction::RetryWithPrompt(_)));
    }
}
