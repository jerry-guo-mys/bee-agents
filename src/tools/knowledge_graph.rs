use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;
use crate::llm::LlmClient;
use crate::memory::Message;

pub struct KnowledgeGraphBuilder {
    llm: Arc<dyn LlmClient>,
}

#[derive(Clone, Debug)]
pub struct KnowledgeNode {
    pub id: String,
    pub label: String,
    pub node_type: String,
    pub properties: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct KnowledgeEdge {
    pub source: String,
    pub target: String,
    pub relationship: String,
}

#[derive(Clone, Debug)]
pub struct KnowledgeGraph {
    pub topic: String,
    pub nodes: Vec<KnowledgeNode>,
    pub edges: Vec<KnowledgeEdge>,
}

impl KnowledgeGraphBuilder {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    #[allow(dead_code)]
    pub fn build(&self, topic: &str, _information: &str) -> KnowledgeGraph {
        KnowledgeGraph {
            topic: topic.to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[async_trait]
impl Tool for KnowledgeGraphBuilder {
    fn name(&self) -> &str {
        "build_knowledge_graph"
    }

    fn description(&self) -> &str {
        "Build a knowledge graph from research information. Extracts entities and relationships. Args: {\"topic\": \"topic\", \"information\": \"text to analyze\"}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let topic = args
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        let information = args
            .get("information")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if topic.is_empty() || information.is_empty() {
            return Err("Missing topic or information".to_string());
        }

        let prompt = format!(
            r#"Extract entities and relationships from the following text to build a knowledge graph.

Topic: {}

Information:
{}

Output format (JSON):
{{
    "nodes": [
        {{"id": "entity1", "label": "Entity Label", "type": "concept|person|organization|event", "properties": {{"key": "value"}}}}
    ],
    "edges": [
        {{"source": "entity1", "target": "entity2", "relationship": "related_to"}}
    ]
}}

Knowledge graph:"#,
            topic,
            information
        );

        let messages = vec![Message::user(&prompt)];
        let response = self.llm.complete(&messages).await
            .map_err(|e| format!("LLM error: {}", e))?;

        let graph_data: Value = serde_json::from_str(&response)
            .map_err(|e| format!("Failed to parse graph: {}", e))?;

        let output = serde_json::json!({
            "topic": topic,
            "graph": graph_data,
            "visualization_hint": "Use a force-directed graph layout for visualization"
        });

        Ok(output.to_string())
    }
}
