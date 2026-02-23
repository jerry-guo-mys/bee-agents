//! list_agents 工具：列出 workspace 内所有 agent，供统筹 agent 查看

use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct DynamicAgent {
    id: String,
    role: String,
    parent_id: Option<String>,
    guidance: Option<String>,
    created_at: String,
}

/// list_agents 工具：列出动态创建的 agent
pub struct ListAgentsTool {
    workspace: std::path::PathBuf,
}

impl ListAgentsTool {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }

    fn load_agents(&self) -> Vec<DynamicAgent> {
        let path = self.workspace.join("agents.json");
        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };
        serde_json::from_str(&data).unwrap_or_default()
    }
}

#[async_trait]
impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "List all dynamically created agents in the workspace. Returns id, role, guidance for each. Config assistants (default, etc.) also exist but are not listed here."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: Value) -> Result<String, String> {
        let agents = self.load_agents();
        if agents.is_empty() {
            return Ok("No dynamic agents yet. Use create tool to add specialized agents. Config assistants (default, etc.) are always available.".to_string());
        }
        let lines: Vec<String> = agents
            .iter()
            .map(|a| {
                let g = a.guidance.as_deref().unwrap_or("-");
                format!("- {}: role={}, guidance={}", a.id, a.role, g)
            })
            .collect();
        Ok(lines.join("\n"))
    }
}
