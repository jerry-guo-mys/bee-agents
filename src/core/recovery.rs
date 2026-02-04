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
    pub fn handle(&self, err: &AgentError, _history: &mut Vec<Message>) -> RecoveryAction {
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
            _ => RecoveryAction::Abort,
        }
    }
}
