//! 对话持久化
//!
//! 将对话历史写入/从 JSON 文件加载，用于跨进程或跨会话恢复（可选使用）。

use std::path::Path;

use crate::memory::{Message, Role};

/// 简单的文件持久化：单文件 JSON，每条消息含 role + content
#[derive(Debug)]
pub struct ConversationPersistence {
    path: std::path::PathBuf,
}

impl ConversationPersistence {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// 从 JSON 文件加载对话历史；文件不存在时返回空 Vec
    pub fn load(&self) -> anyhow::Result<Vec<Message>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let data = std::fs::read_to_string(&self.path)?;
        let messages: Vec<SerMessage> = serde_json::from_str(&data)?;
        Ok(messages
            .into_iter()
            .map(|m| Message {
                role: match m.role.as_str() {
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    _ => Role::System,
                },
                content: m.content,
            })
            .collect())
    }

    /// 将对话历史写入 JSON 文件；父目录不存在时自动创建
    pub fn save(&self, messages: &[Message]) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let ser: Vec<SerMessage> = messages
            .iter()
            .map(|m| SerMessage {
                role: match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => "system",
                }
                .to_string(),
                content: m.content.clone(),
            })
            .collect();
        std::fs::write(&self.path, serde_json::to_string_pretty(&ser)?)?;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SerMessage {
    role: String,
    content: String,
}
