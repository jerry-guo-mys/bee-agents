//! Critic：结果反思与校验
//!
//! 在将工具 Observation 喂回 Planner 前，可选一次轻量 LLM 调用判断「是否符合预期」，
//! 若不符合则返回 Correction 作为下一轮上下文，减少重复犯错。

use std::sync::Arc;

use crate::llm::LlmClient;
use crate::memory::Message;

/// Critic 评估结果：通过或需修正
#[derive(Debug, Clone)]
pub enum CriticResult {
    /// 通过
    Approved,
    /// 需要修正
    Correction(String),
}

/// Critic：持有 LLM 与 prompt 模板，evaluate(goal, tool, observation) 返回 Approved / Correction
pub struct Critic {
    llm: Arc<dyn LlmClient>,
    prompt_template: String,
}

impl Critic {
    pub fn new(llm: Arc<dyn LlmClient>, prompt_template: impl Into<String>) -> Self {
        Self {
            llm,
            prompt_template: prompt_template.into(),
        }
    }

    pub async fn evaluate(
        &self,
        goal: &str,
        tool: &str,
        observation: &str,
    ) -> Result<CriticResult, String> {
        let prompt = self
            .prompt_template
            .replace("{goal}", goal)
            .replace("{tool}", tool)
            .replace("{observation}", observation);

        let messages = vec![Message::user(prompt)];
        let response = self.llm.complete(&messages).await?;
        let response = response.trim().to_uppercase();

        if response.starts_with("OK") || response.is_empty() {
            Ok(CriticResult::Approved)
        } else {
            Ok(CriticResult::Correction(response))
        }
    }
}
