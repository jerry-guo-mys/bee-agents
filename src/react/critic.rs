//! Critic：结果反思与校验（解决问题 4.3：配置化与模型分离）
//!
//! 在将工具 Observation 喂回 Planner 前，可选一次轻量 LLM 调用判断「是否符合预期」，
//! 若不符合则返回 Correction 作为下一轮上下文，减少重复犯错。
//!
//! 通过配置可以：
//! - 启用/禁用 Critic
//! - 使用与 Planner 不同的模型（避免自我认同）
//! - 仅评估特定工具（减少 token 开销）

use std::collections::HashSet;
use std::sync::Arc;

use crate::config::CriticSection;
use crate::llm::LlmClient;
use crate::memory::Message;

/// Critic 评估结果：通过或需修正
#[derive(Debug, Clone)]
pub enum CriticResult {
    /// 通过
    Approved,
    /// 需要修正
    Correction(String),
    /// 跳过评估（该工具不在评估列表中）
    Skipped,
}

/// Critic：持有 LLM 与 prompt 模板，evaluate(goal, tool, observation) 返回 Approved / Correction / Skipped
pub struct Critic {
    llm: Arc<dyn LlmClient>,
    prompt_template: String,
    /// 是否评估所有工具
    evaluate_all_tools: bool,
    /// 仅评估的工具集合（evaluate_all_tools=false 时生效）
    evaluate_tools: HashSet<String>,
}

impl Critic {
    /// 从配置创建 Critic（需要外部传入 LLM 实例）
    pub fn from_config(llm: Arc<dyn LlmClient>, config: &CriticSection) -> Self {
        Self {
            llm,
            prompt_template: config.prompt_template.clone(),
            evaluate_all_tools: config.evaluate_all_tools,
            evaluate_tools: config.evaluate_tools.iter().cloned().collect(),
        }
    }

    /// 直接创建 Critic（向后兼容）
    pub fn new(llm: Arc<dyn LlmClient>, prompt_template: impl Into<String>) -> Self {
        Self {
            llm,
            prompt_template: prompt_template.into(),
            evaluate_all_tools: true,
            evaluate_tools: HashSet::new(),
        }
    }

    /// 设置仅评估特定工具
    pub fn with_evaluate_tools(mut self, tools: Vec<String>) -> Self {
        self.evaluate_all_tools = false;
        self.evaluate_tools = tools.into_iter().collect();
        self
    }

    /// 设置评估所有工具
    pub fn with_evaluate_all(mut self) -> Self {
        self.evaluate_all_tools = true;
        self
    }

    /// 检查是否应该评估此工具
    fn should_evaluate(&self, tool: &str) -> bool {
        if self.evaluate_all_tools {
            return true;
        }
        if self.evaluate_tools.is_empty() {
            return true;
        }
        self.evaluate_tools.contains(tool)
    }

    pub async fn evaluate(
        &self,
        goal: &str,
        tool: &str,
        observation: &str,
    ) -> Result<CriticResult, String> {
        if !self.should_evaluate(tool) {
            return Ok(CriticResult::Skipped);
        }

        let prompt = self
            .prompt_template
            .replace("{goal}", goal)
            .replace("{tool}", tool)
            .replace("{observation}", observation);

        let messages = vec![Message::user(prompt)];
        let response = self.llm.complete(&messages).await.map_err(|e| e.to_string())?;
        let response = response.trim().to_uppercase();

        if response.starts_with("OK") || response.is_empty() {
            Ok(CriticResult::Approved)
        } else {
            Ok(CriticResult::Correction(response))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmClient;

    #[test]
    fn test_should_evaluate_all() {
        let critic = Critic::new(Arc::new(MockLlmClient), "test");
        assert!(critic.should_evaluate("any_tool"));
    }

    #[test]
    fn test_should_evaluate_specific() {
        let critic = Critic::new(Arc::new(MockLlmClient), "test")
            .with_evaluate_tools(vec!["shell".to_string(), "code_edit".to_string()]);
        assert!(critic.should_evaluate("shell"));
        assert!(critic.should_evaluate("code_edit"));
        assert!(!critic.should_evaluate("cat"));
    }
}
