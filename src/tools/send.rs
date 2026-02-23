//! send 工具：assistant 向另一个 assistant 发送消息（Phase 2）
//!
//! 创建/复用 P2P 群，写入消息。发送方来自 task_local（process_message_stream 设置）。

use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

tokio::task_local! {
    /// 当前执行 ReAct 的 assistant_id，由 process_message_stream 设置
    pub static CURRENT_ASSISTANT_ID: Option<String>;
}

/// P2P 群 ID 前缀
const P2P_PREFIX: &str = "p2p_";

/// 生成 P2P 群 id：按字母序排列 (a, b) 保证唯一
fn p2p_group_id(a: &str, b: &str) -> String {
    let (x, y) = if a <= b { (a, b) } else { (b, a) };
    format!("{}{}_{}", P2P_PREFIX, x, y)
}

/// send 工具：向另一 assistant 发私信
pub struct SendTool {
    groups_path: std::path::PathBuf,
    sessions_dir: std::path::PathBuf,
}

impl SendTool {
    pub fn new(workspace: &Path) -> Self {
        Self {
            groups_path: workspace.join("groups.json"),
            sessions_dir: workspace.join("sessions"),
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
        if let Ok(json) = serde_json::to_string_pretty(groups) {
            let _ = std::fs::write(&self.groups_path, json);
        }
    }

    fn load_group_messages(&self, group_id: &str) -> Vec<GroupMessage> {
        let path = self.group_session_path(group_id);
        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };
        let snap: GroupSnapshot = serde_json::from_str(&data).unwrap_or(GroupSnapshot {
            messages: vec![],
            max_turns: 20,
        });
        snap.messages
    }

    fn save_group_messages(&self, group_id: &str, messages: &[GroupMessage]) {
        let path = self.group_session_path(group_id);
        let snap = GroupSnapshot {
            messages: messages.to_vec(),
            max_turns: 20,
        };
        if let Ok(json) = serde_json::to_string_pretty(&snap) {
            let _ = std::fs::write(&path, json);
        }
    }

    fn group_session_path(&self, group_id: &str) -> std::path::PathBuf {
        let safe: String = group_id
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.sessions_dir.join(format!("group_{}.json", safe))
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct GroupInfo {
    id: String,
    name: Option<String>,
    member_ids: Vec<String>,
    created_at: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct GroupMessage {
    role: String,
    content: String,
    assistant_id: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GroupSnapshot {
    messages: Vec<GroupMessage>,
    #[serde(default = "default_max_turns")]
    max_turns: usize,
}

fn default_max_turns() -> usize {
    20
}

#[async_trait]
impl Tool for SendTool {
    fn name(&self) -> &str {
        "send"
    }

    fn description(&self) -> &str {
        "Send a direct message to another assistant. Use when you need to delegate, ask for help, or hand off a task. Args: to (assistant_id), content (string)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Target assistant id (e.g. default, code, research)"
                },
                "content": {
                    "type": "string",
                    "description": "Message content to send"
                }
            },
            "required": ["to", "content"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let to = args
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if to.is_empty() {
            return Err("send: 'to' is required".to_string());
        }
        if content.is_empty() {
            return Err("send: 'content' is required".to_string());
        }

        let from = CURRENT_ASSISTANT_ID
            .try_with(|s| s.clone())
            .unwrap_or(None)
            .unwrap_or_else(|| "default".to_string());

        if from == to {
            return Err("send: cannot send message to yourself".to_string());
        }

        let group_id = p2p_group_id(&from, &to);

        let mut groups = self.load_groups();
        if !groups.contains_key(&group_id) {
            groups.insert(
                group_id.clone(),
                GroupInfo {
                    id: group_id.clone(),
                    name: Some(format!("P2P {} ↔ {}", from, to)),
                    member_ids: vec![from.clone(), to.clone()],
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
            );
            self.save_groups(&groups);
        }

        let mut messages = self.load_group_messages(&group_id);
        messages.push(GroupMessage {
            role: "assistant".to_string(),
            content: format!("[来自 {}] {}", from, content),
            assistant_id: Some(from.clone()),
        });
        self.save_group_messages(&group_id, &messages);

        Ok(format!(
            "Message sent to {} in P2P group {}. The recipient may process it when their inbox is checked.",
            to, group_id
        ))
    }
}
