//! 中期记忆：当前任务目标、已尝试方案、失败原因
//!
//! 在 ReAct 单次对话内有效，用于拼入 system prompt（Current Goal / What has been tried / Failures），减少重复犯错。

#[derive(Clone, Debug, Default)]
pub struct WorkingMemory {
    pub goal: Option<String>,
    pub attempts: Vec<String>,
    pub failures: Vec<String>,
}

impl WorkingMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_goal(&mut self, goal: impl Into<String>) {
        self.goal = Some(goal.into());
    }

    pub fn add_attempt(&mut self, attempt: impl Into<String>) {
        self.attempts.push(attempt.into());
    }

    pub fn add_failure(&mut self, failure: impl Into<String>) {
        self.failures.push(failure.into());
    }

    pub fn clear(&mut self) {
        self.goal = None;
        self.attempts.clear();
        self.failures.clear();
    }

    /// 从本轮的 attempts（格式 "tool -> observation"）中提取工具名列表，用于策略沉淀（EVOLUTION §3.5）
    pub fn tool_names_used(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .attempts
            .iter()
            .filter_map(|a| a.split(" -> ").next().map(|s| s.trim().to_string()))
            .collect();
        names.dedup();
        names
    }

    /// 构建供 Planner 使用的 Prompt 片段（Current Goal / What has been tried / Failures）
    pub fn to_prompt_section(&self) -> String {
        let mut s = String::new();
        if let Some(goal) = &self.goal {
            s.push_str(&format!("## Current Goal\n{}\n\n", goal));
        }
        if !self.attempts.is_empty() {
            s.push_str("## What has been tried\n");
            for a in &self.attempts {
                s.push_str(&format!("- {}\n", a));
            }
            s.push('\n');
        }
        if !self.failures.is_empty() {
            s.push_str("## Failures\n");
            for f in &self.failures {
                s.push_str(&format!("- {}\n", f));
            }
            s.push('\n');
        }
        s
    }
}
