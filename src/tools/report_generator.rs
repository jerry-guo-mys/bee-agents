use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;
use crate::llm::LlmClient;
use crate::memory::Message;

pub struct ReportGeneratorTool {
    llm: Arc<dyn LlmClient>,
}

impl ReportGeneratorTool {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Tool for ReportGeneratorTool {
    fn name(&self) -> &str {
        "generate_report"
    }

    fn description(&self) -> &str {
        "Generate a structured research report from research findings. Supports Markdown and JSON formats. Args: {\"topic\": \"research topic\", \"findings\": \"research data\", \"format\": \"markdown|json\" (optional)}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let topic = args
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        let findings = args
            .get("findings")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("markdown");

        if topic.is_empty() || findings.is_empty() {
            return Err("Missing topic or findings".to_string());
        }

        let prompt = if format == "json" {
            format!(
                r#"Generate a structured research report in JSON format.

Topic: {}

Research Findings:
{}

Output JSON structure:
{{
    "title": "report title",
    "executive_summary": "200-300 word summary",
    "key_findings": ["finding 1", "finding 2"],
    "methodology": "brief description",
    "conclusions": ["conclusion 1", "conclusion 2"],
    "recommendations": ["recommendation 1", "recommendation 2"],
    "references": ["source 1", "source 2"]
}}"#,
                topic, findings
            )
        } else {
            format!(
                r#"Generate a comprehensive research report in Markdown format.

Topic: {}

Research Findings:
{}

Format:
# [Report Title]

## Executive Summary
[Brief overview]

## Key Findings
- Finding 1
- Finding 2

## Analysis
[Detailed analysis]

## Conclusions
[Main conclusions]

## Recommendations
[Actionable recommendations]

## References
- Source 1
- Source 2

Report:"#,
                topic, findings
            )
        };

        let messages = vec![Message::user(&prompt)];
        let response = self.llm.complete(&messages).await
            .map_err(|e| format!("LLM error: {}", e))?;

        Ok(response)
    }
}
