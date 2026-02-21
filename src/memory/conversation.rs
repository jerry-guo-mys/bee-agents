//! 短期记忆：对话历史
//!
//! 保留最近 N 轮对话（user/assistant 对），超出时自动剪枝，供 LLM 上下文与 UI 渲染使用。

use serde::{Deserialize, Serialize};

/// 消息角色（与 LLM API 一致）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    System,
    /// 工具调用结果（解决问题 4.2：分离工具调用与对话历史）
    Tool,
}

/// 单条消息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    /// 工具调用结果消息
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
        }
    }
}

/// 短期记忆：最近 N 轮对话（每轮含 user + assistant，故实际保留约 max_turns*2 条消息）
#[derive(Clone, Debug)]
pub struct ConversationMemory {
    messages: Vec<Message>,
    max_turns: usize,
}

impl ConversationMemory {
    pub fn new(max_turns: usize) -> Self {
        Self {
            messages: Vec::new(),
            max_turns,
        }
    }

    /// 从已有消息列表恢复（用于持久化后加载）
    pub fn from_messages(messages: Vec<Message>, max_turns: usize) -> Self {
        let mut c = Self {
            messages,
            max_turns,
        };
        c.prune();
        c
    }

    pub fn max_turns(&self) -> usize {
        self.max_turns
    }

    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
        self.prune();
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// 替换为指定消息列表（用于 Context Compaction：摘要后仅保留摘要消息）
    pub fn set_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        self.prune();
    }

    /// 超出 max_turns*2 时丢弃最旧的消息，保留最近部分
    fn prune(&mut self) {
        if self.messages.len() > self.max_turns * 2 {
            let keep = self.max_turns * 2;
            self.messages.drain(..self.messages.len() - keep);
        }
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}
