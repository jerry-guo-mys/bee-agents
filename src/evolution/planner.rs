use std::sync::Arc;

use crate::llm::LlmClient;
use crate::tools::ToolExecutor;
use crate::evolution::types::{ImprovementPlan, CodeAnalysis};

pub struct ImprovementPlanner {
    llm: Arc<dyn LlmClient>,
    executor: Arc<ToolExecutor>,
}

impl ImprovementPlanner {
    pub fn new(llm: Arc<dyn LlmClient>, executor: Arc<ToolExecutor>) -> Self {
        Self { llm, executor }
    }

    pub async fn plan_improvements(
        &self,
        analysis: &CodeAnalysis,
        plan: &ImprovementPlan,
    ) -> Result<Vec<String>, String> {
        let content = self.read_file(&analysis.file_path).await?;

        let prompt = format!(
            r#"You are an AI assistant tasked with improving Rust code. 

Current file: {}
Overall quality score: {:.2}

Issues found:
{}

Improvement plan:
- Type: {:?}
- Priority: {:?}
- Goal: {}

Code content:
```
{}
```

Please provide specific, actionable steps to improve this code. Each step should be:
1. Atomic - one change per step
2. Verifiable - with expected outcome
3. Safe - doesn't break existing functionality

Return as a numbered list of steps."#,
            analysis.file_path,
            analysis.overall_score,
            self.format_issues(&analysis.issues),
            plan.improvement_type,
            plan.priority,
            plan.expected_outcome,
            content
        );

        let response = self.llm.complete(&[
            crate::memory::Message::system(prompt)
        ]).await.map_err(|e| e.to_string())?;

        let steps = self.parse_steps_from_response(&response);
        Ok(steps)
    }

    async fn read_file(&self, file_path: &str) -> Result<String, String> {
        let args = serde_json::json!({
            "file_path": file_path,
            "limit": 500
        });

        self.executor.execute("code_read", args).await.map_err(|e| e.to_string())
    }

    fn format_issues(&self, issues: &[crate::evolution::types::Issue]) -> String {
        issues.iter()
            .map(|issue| {
                let line_str = if let Some(line) = issue.line_number {
                    format!("Line {}: ", line)
                } else {
                    String::new()
                };
                format!("- {}{} ({:?})", line_str, issue.description, issue.severity)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn parse_steps_from_response(&self, response: &str) -> Vec<String> {
        let mut steps = Vec::new();
        let lines: Vec<&str> = response.lines().collect();

        let mut in_list = false;
        for line in lines {
            let trimmed = line.trim();

            if trimmed.starts_with("1.") || trimmed.starts_with("1)") {
                in_list = true;
            }

            if in_list && (trimmed.starts_with(|c: char| c.is_ascii_digit()) || trimmed.starts_with("- ")) {
                let step = trimmed
                    .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')' || c == '-')
                    .trim()
                    .to_string();
                if !step.is_empty() {
                    steps.push(step);
                }
            } else if in_list && trimmed.is_empty() {
                in_list = false;
            }
        }

        if steps.is_empty() {
            steps.push(response.to_string());
        }

        steps
    }

    pub async fn refine_steps_with_context(
        &self,
        steps: &[String],
        context: &str,
    ) -> Result<Vec<String>, String> {
        let prompt = format!(
            r#"You are reviewing code improvement steps. 

Context: {}

Proposed steps:
{}

Please refine these steps to be more specific and actionable. For each step, include:
1. Exact code location (file, line range if known)
2. What to change (old code -> new code)
3. Why this change improves the code
4. How to verify the change works

Return as a numbered list."#,
            context,
            steps.iter()
                .enumerate()
                .map(|(i, step)| format!("{}. {}", i + 1, step))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let response = self.llm.complete(&[
            crate::memory::Message::system(prompt)
        ]).await.map_err(|e| e.to_string())?;

        let refined_steps = self.parse_steps_from_response(&response);
        Ok(refined_steps)
    }
}
