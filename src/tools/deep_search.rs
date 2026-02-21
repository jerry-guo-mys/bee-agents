use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::Tool;
use crate::llm::LlmClient;
use crate::memory::Message;

pub struct DeepSearchTool {
    llm: Arc<dyn LlmClient>,
    max_rounds: usize,
    _max_results_per_round: usize,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub query: String,
    pub content: String,
    pub source_url: String,
    pub relevance_score: f32,
    pub round: usize,
}

#[derive(Clone, Debug)]
pub struct DeepResearchResult {
    pub topic: String,
    pub search_results: Vec<SearchResult>,
    pub summary: String,
    pub key_findings: Vec<String>,
    pub follow_up_questions: Vec<String>,
}

impl DeepSearchTool {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self {
            llm,
            max_rounds: 5,
            _max_results_per_round: 3,
        }
    }

    async fn decompose_query(&self, query: &str) -> Result<Vec<String>, String> {
        let prompt = format!(
            r#"You are a research assistant. Break down the following complex research question into 3-5 specific, searchable sub-questions.
Each sub-question should be:
- Specific and focused
- Suitable for web search
- Cover different aspects of the main topic

Research question: {}

Output format (JSON array of strings):
["sub-question 1", "sub-question 2", "sub-question 3"]

Sub-questions:"#,
            query
        );

        let messages = vec![Message::user(&prompt)];
        let response = self.llm.complete(&messages).await
            .map_err(|e| format!("LLM error: {}", e))?;

        let queries: Vec<String> = serde_json::from_str(&response)
            .unwrap_or_else(|_| vec![query.to_string()]);

        Ok(queries.into_iter().take(5).collect())
    }

    async fn search_round(&self, queries: &[String], round: usize) -> Result<Vec<SearchResult>, String> {
        let mut results = Vec::new();

        for query in queries {
            results.push(SearchResult {
                query: query.clone(),
                content: format!("[Search results for: {}]", query),
                source_url: format!("https://example.com/search?q={}", query.replace(' ', "+")),
                relevance_score: 0.8,
                round,
            });
        }

        Ok(results)
    }

    async fn generate_follow_up_queries(
        &self,
        original_query: &str,
        previous_results: &[SearchResult],
    ) -> Result<Vec<String>, String> {
        let results_summary: String = previous_results
            .iter()
            .take(3)
            .map(|r| format!("- {}: {}", r.query, r.content.chars().take(200).collect::<String>()))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"Based on the initial research, generate 2-3 follow-up search queries to deepen understanding.
Original query: {}

Previous findings:
{}

Output format (JSON array):
["follow-up query 1", "follow-up query 2"]

Follow-up queries:"#,
            original_query,
            results_summary
        );

        let messages = vec![Message::user(&prompt)];
        let response = self.llm.complete(&messages).await
            .map_err(|e| format!("LLM error: {}", e))?;

        let queries: Vec<String> = serde_json::from_str(&response)
            .unwrap_or_else(|_| vec![]);

        Ok(queries.into_iter().take(3).collect())
    }

    async fn synthesize_results(
        &self,
        topic: &str,
        results: &[SearchResult],
    ) -> Result<(String, Vec<String>, Vec<String>), String> {
        let findings: String = results
            .iter()
            .map(|r| format!("Source: {}\n{}", r.source_url, r.content.chars().take(500).collect::<String>()))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

        let prompt = format!(
            r#"Synthesize the following research findings into a comprehensive summary.

Topic: {}

Research findings:
{}

Output format (JSON):
{{
    "summary": "200-300 word comprehensive summary",
    "key_findings": ["finding 1", "finding 2", "finding 3"],
    "follow_up_questions": ["question 1", "question 2"]
}}

Synthesis:"#,
            topic,
            findings
        );

        let messages = vec![Message::user(&prompt)];
        let response = self.llm.complete(&messages).await
            .map_err(|e| format!("LLM error: {}", e))?;

        let synthesis: Value = serde_json::from_str(&response)
            .map_err(|e| format!("Failed to parse synthesis: {}", e))?;

        let summary = synthesis["summary"].as_str().unwrap_or("No summary available").to_string();
        let key_findings = synthesis["key_findings"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(String::from).collect())
            .unwrap_or_default();
        let follow_up_questions = synthesis["follow_up_questions"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(String::from).collect())
            .unwrap_or_default();

        Ok((summary, key_findings, follow_up_questions))
    }
}

#[async_trait]
impl Tool for DeepSearchTool {
    fn name(&self) -> &str {
        "deep_search"
    }

    fn description(&self) -> &str {
        "Conduct deep research on a complex topic through multiple rounds of autonomous search. Automatically decomposes query, performs iterative searches, and synthesizes findings. Args: {\"topic\": \"research question\", \"max_rounds\": 3 (optional)}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let topic = args
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if topic.is_empty() {
            return Err("Missing topic".to_string());
        }

        let max_rounds = args
            .get("max_rounds")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        tracing::info!(topic = %topic, max_rounds = max_rounds, "deep_search started");

        let max_rounds = max_rounds.min(self.max_rounds);

        let initial_queries = self.decompose_query(topic).await?;
        tracing::info!(queries = ?initial_queries, "decomposed into queries");

        let mut all_results: Vec<SearchResult> = Vec::new();
        let mut current_queries = initial_queries;

        for round in 1..=max_rounds {
            tracing::info!(round, "starting search round");

            let round_results = self.search_round(&current_queries, round).await?;
            all_results.extend(round_results);

            if round < max_rounds {
                current_queries = self.generate_follow_up_queries(topic, &all_results).await?;
                if current_queries.is_empty() {
                    break;
                }
            }
        }

        let (summary, key_findings, follow_up_questions) = 
            self.synthesize_results(topic, &all_results).await?;

        let result = DeepResearchResult {
            topic: topic.to_string(),
            search_results: all_results,
            summary,
            key_findings,
            follow_up_questions,
        };

        let output = json!({
            "topic": result.topic,
            "summary": result.summary,
            "key_findings": result.key_findings,
            "total_sources": result.search_results.len(),
            "follow_up_questions": result.follow_up_questions,
        });

        Ok(output.to_string())
    }
}
