//! create 工具：assistant 创建 sub-agent（Phase 3）
//!
//! 参数 { role, guidance }，创建动态 agent，建立与 creator 的 P2P 群。

use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;

use super::send::CURRENT_ASSISTANT_ID;
use crate::tools::Tool;

/// 动态 agent 持久化结构
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct DynamicAgent {
    pub id: String,
    pub role: String,
    pub parent_id: Option<String>,
    pub guidance: Option<String>,
    pub created_at: String,
}

const AGENTS_FILE: &str = "agents.json";
const P2P_PREFIX: &str = "p2p_";

fn p2p_group_id(a: &str, b: &str) -> String {
    let (x, y) = if a <= b { (a, b) } else { (b, a) };
    format!("{}{}_{}", P2P_PREFIX, x, y)
}

/// create 工具：创建 sub-agent
pub struct CreateTool {
    workspace: std::path::PathBuf,
}

impl CreateTool {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }

    fn agents_path(&self) -> std::path::PathBuf {
        self.workspace.join(AGENTS_FILE)
    }

    fn groups_path(&self) -> std::path::PathBuf {
        self.workspace.join("groups.json")
    }

    fn sessions_dir(&self) -> std::path::PathBuf {
        self.workspace.join("sessions")
    }

    fn load_agents(&self) -> Vec<DynamicAgent> {
        let data = match std::fs::read_to_string(self.agents_path()) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    fn save_agents(&self, agents: &[DynamicAgent]) {
        std::fs::create_dir_all(&self.workspace).ok();
        if let Ok(json) = serde_json::to_string_pretty(agents) {
            let _ = std::fs::write(self.agents_path(), json);
        }
    }

    fn load_groups(&self) -> std::collections::HashMap<String, GroupInfo> {
        let data = match std::fs::read_to_string(self.groups_path()) {
            Ok(d) => d,
            Err(_) => return std::collections::HashMap::new(),
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    fn save_groups(&self, groups: &std::collections::HashMap<String, GroupInfo>) {
        if let Ok(json) = serde_json::to_string_pretty(groups) {
            let _ = std::fs::write(self.groups_path(), json);
        }
    }

    /// 直接创建 agent（供 API 等非 Tool 场景使用，显式指定 parent_id）
    pub fn create_agent_direct(
        &self,
        role: &str,
        guidance: Option<&str>,
        parent_id: &str,
    ) -> Result<DynamicAgent, String> {
        let role = role.trim();
        if role.is_empty() {
            return Err("role is required".to_string());
        }
        let guidance = guidance.and_then(|s| {
            let t = s.trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
        });
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();
        let agent = DynamicAgent {
            id: id.clone(),
            role: role.to_string(),
            parent_id: Some(parent_id.to_string()),
            guidance: guidance.clone(),
            created_at: created_at.clone(),
        };
        let mut agents = self.load_agents();
        agents.push(agent.clone());
        self.save_agents(&agents);
        let group_id = p2p_group_id(parent_id, &id);
        let mut groups = self.load_groups();
        if !groups.contains_key(&group_id) {
            groups.insert(
                group_id.clone(),
                GroupInfo {
                    id: group_id.clone(),
                    name: Some(format!("P2P {} ↔ {}", parent_id, id)),
                    member_ids: vec![parent_id.to_string(), id.clone()],
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
            );
            self.save_groups(&groups);
        }
        Ok(agent)
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct GroupInfo {
    id: String,
    name: Option<String>,
    member_ids: Vec<String>,
    created_at: String,
}

#[async_trait]
impl Tool for CreateTool {
    fn name(&self) -> &str {
        "create"
    }

    fn description(&self) -> &str {
        "Create a sub-agent with a specific role and guidance. Use when you need to delegate a specialized task. Args: role (string), guidance (string, optional)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "role": {
                    "type": "string",
                    "description": "Role of the sub-agent (e.g. code reviewer, researcher)"
                },
                "guidance": {
                    "type": "string",
                    "description": "Optional guidance or instructions for the sub-agent"
                }
            },
            "required": ["role"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let role = args
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let guidance = args
            .get("guidance")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if role.is_empty() {
            return Err("create: 'role' is required".to_string());
        }

        let parent_id = CURRENT_ASSISTANT_ID
            .try_with(|s| s.clone())
            .unwrap_or(None)
            .unwrap_or_else(|| "default".to_string());

        let id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();

        let agent = DynamicAgent {
            id: id.clone(),
            role: role.clone(),
            parent_id: Some(parent_id.clone()),
            guidance: guidance.clone(),
            created_at: created_at.clone(),
        };

        let mut agents = self.load_agents();
        agents.push(agent.clone());
        self.save_agents(&agents);

        let group_id = p2p_group_id(&parent_id, &id);
        let mut groups = self.load_groups();
        if !groups.contains_key(&group_id) {
            groups.insert(
                group_id.clone(),
                GroupInfo {
                    id: group_id.clone(),
                    name: Some(format!("P2P {} ↔ {}", parent_id, id)),
                    member_ids: vec![parent_id.clone(), id.clone()],
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
            );
            self.save_groups(&groups);
        }

        Ok(format!(
            "Sub-agent created: id={}, role={}. Use send tool to message it: send({{to: \"{}\", content: \"...\"}}).",
            id, role, id
        ))
    }
}
