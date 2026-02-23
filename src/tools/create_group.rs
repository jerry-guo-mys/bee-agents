//! create_group 工具：创建多 Agent 群聊，供统筹 agent 组队

use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct GroupInfo {
    id: String,
    name: Option<String>,
    member_ids: Vec<String>,
    created_at: String,
}

/// create_group 工具：创建群聊（≥2 人）
pub struct CreateGroupTool {
    groups_path: std::path::PathBuf,
}

impl CreateGroupTool {
    pub fn new(workspace: &Path) -> Self {
        Self {
            groups_path: workspace.join("groups.json"),
        }
    }

    fn load_groups(&self) -> std::collections::HashMap<String, GroupInfo> {
        let data = match std::fs::read_to_string(&self.groups_path) {
            Ok(d) => d,
            Err(_) => return std::collections::HashMap::new(),
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    fn save_groups(&self, groups: &std::collections::HashMap<String, GroupInfo>) {
        std::fs::create_dir_all(self.groups_path.parent().unwrap()).ok();
        if let Ok(json) = serde_json::to_string_pretty(groups) {
            let _ = std::fs::write(&self.groups_path, json);
        }
    }
}

#[async_trait]
impl Tool for CreateGroupTool {
    fn name(&self) -> &str {
        "create_group"
    }

    fn description(&self) -> &str {
        "Create a group chat with 2 or more agents. Use when you need a team to collaborate on a task. Args: member_ids (array of agent ids), name (optional string). Returns group_id."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "member_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Agent ids to include (at least 2)"
                },
                "name": {
                    "type": "string",
                    "description": "Optional group name"
                }
            },
            "required": ["member_ids"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let member_ids: Vec<String> = args
            .get("member_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if member_ids.len() < 2 {
            return Err("create_group: member_ids must have at least 2 agents".to_string());
        }

        let dedup: Vec<String> = member_ids
            .into_iter()
            .fold((std::collections::HashSet::new(), Vec::new()), |(mut set, mut vec), id| {
                if set.insert(id.clone()) {
                    vec.push(id);
                }
                (set, vec)
            })
            .1;

        if dedup.len() < 2 {
            return Err("create_group: need at least 2 distinct agent ids".to_string());
        }

        let id = uuid::Uuid::new_v4().to_string();
        let group = GroupInfo {
            id: id.clone(),
            name: name.or_else(|| Some(format!("群聊 {}", &id[..8]))),
            member_ids: dedup.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let mut groups = self.load_groups();
        groups.insert(id.clone(), group);
        self.save_groups(&groups);

        Ok(format!(
            "Group created: id={}, members=[{}]. Use send to message agents, or users can chat in this group via the UI.",
            id,
            dedup.join(", ")
        ))
    }
}
