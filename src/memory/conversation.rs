//! 短期记忆：对话历史
//!
//! 保留最近 N 轮对话（user/assistant 对），超出时智能剪枝，供 LLM 上下文与 UI 渲染使用。
//! 
//! 智能剪枝策略（解决问题 5.3）：
//! - 保留 System 消息不被剪枝
//! - 按重要性评分决定保留哪些（用户消息 > 助手回复 > 工具结果）
//! - 可选：剪枝前将丢弃内容通知回调

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

/// 消息重要性评分（用于智能剪枝）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessageImportance {
    /// 系统消息 - 最高优先级，永不剪枝
    System = 100,
    /// 用户消息 - 高优先级
    User = 80,
    /// 助手回复 - 中等优先级
    Assistant = 60,
    /// 工具调用结果 - 较低优先级，可优先丢弃
    Tool = 40,
}

impl From<&Role> for MessageImportance {
    fn from(role: &Role) -> Self {
        match role {
            Role::System => MessageImportance::System,
            Role::User => MessageImportance::User,
            Role::Assistant => MessageImportance::Assistant,
            Role::Tool => MessageImportance::Tool,
        }
    }
}

/// 剪枝配置
#[derive(Clone, Debug)]
pub struct PruneConfig {
    /// 是否保留所有 System 消息（默认 true）
    pub preserve_system: bool,
    /// 工具结果保留比例（0.0-1.0，默认 0.5 表示最多保留一半工具结果）
    pub tool_result_ratio: f32,
    /// 是否启用智能剪枝（false 则使用简单的 FIFO）
    pub smart_prune: bool,
}

impl Default for PruneConfig {
    fn default() -> Self {
        Self {
            preserve_system: true,
            tool_result_ratio: 0.5,
            smart_prune: true,
        }
    }
}

/// 剪枝结果（可用于写入长期记忆）
#[derive(Debug)]
pub struct PruneResult {
    /// 被剪枝的消息
    pub pruned_messages: Vec<Message>,
    /// 剪枝后保留的消息数
    pub retained_count: usize,
}

/// 短期记忆：最近 N 轮对话（每轮含 user + assistant，故实际保留约 max_turns*2 条消息）
#[derive(Clone, Debug)]
pub struct ConversationMemory {
    messages: Vec<Message>,
    max_turns: usize,
    prune_config: PruneConfig,
}

impl ConversationMemory {
    pub fn new(max_turns: usize) -> Self {
        Self {
            messages: Vec::new(),
            max_turns,
            prune_config: PruneConfig::default(),
        }
    }

    /// 带剪枝配置创建
    pub fn with_config(max_turns: usize, config: PruneConfig) -> Self {
        Self {
            messages: Vec::new(),
            max_turns,
            prune_config: config,
        }
    }

    /// 从已有消息列表恢复（用于持久化后加载）
    pub fn from_messages(messages: Vec<Message>, max_turns: usize) -> Self {
        let mut c = Self {
            messages,
            max_turns,
            prune_config: PruneConfig::default(),
        };
        let _ = c.prune();
        c
    }

    pub fn max_turns(&self) -> usize {
        self.max_turns
    }

    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
        let _ = self.prune();
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
        let _ = self.prune();
    }

    /// 智能剪枝：返回被剪枝的消息（可用于写入长期记忆）
    pub fn prune(&mut self) -> PruneResult {
        let max_messages = self.max_turns * 2;
        
        if self.messages.len() <= max_messages {
            return PruneResult {
                pruned_messages: Vec::new(),
                retained_count: self.messages.len(),
            };
        }

        if !self.prune_config.smart_prune {
            // 简单 FIFO 剪枝
            let keep = max_messages;
            let pruned: Vec<Message> = self.messages.drain(..self.messages.len() - keep).collect();
            return PruneResult {
                pruned_messages: pruned,
                retained_count: self.messages.len(),
            };
        }

        // 智能剪枝
        let mut indexed: Vec<(usize, &Message, MessageImportance)> = self
            .messages
            .iter()
            .enumerate()
            .map(|(i, m)| (i, m, MessageImportance::from(&m.role)))
            .collect();

        // 分离 System 消息
        let (system_msgs, mut other_msgs): (Vec<_>, Vec<_>) = indexed
            .drain(..)
            .partition(|(_, _, imp)| *imp == MessageImportance::System);

        // 计算非 System 消息的目标数量
        let target_non_system = if self.prune_config.preserve_system {
            max_messages.saturating_sub(system_msgs.len())
        } else {
            max_messages
        };

        // 如果非 System 消息超出目标，进行剪枝
        if other_msgs.len() > target_non_system {
            // 按重要性排序（高重要性在前），同等重要性按时间倒序（新的在前）
            other_msgs.sort_by(|a, b| {
                b.2.cmp(&a.2).then_with(|| b.0.cmp(&a.0))
            });

            // 限制工具结果数量
            let tool_limit = (target_non_system as f32 * self.prune_config.tool_result_ratio) as usize;
            let mut tool_count = 0;
            
            other_msgs.retain(|(_, _, imp)| {
                if *imp == MessageImportance::Tool {
                    tool_count += 1;
                    tool_count <= tool_limit
                } else {
                    true
                }
            });

            // 截断到目标数量
            other_msgs.truncate(target_non_system);

            // 按原始顺序重新排列
            other_msgs.sort_by_key(|(i, _, _)| *i);
        }

        // 重建消息列表，保持原始顺序
        let mut kept_indices: Vec<usize> = system_msgs.iter().map(|(i, _, _)| *i).collect();
        kept_indices.extend(other_msgs.iter().map(|(i, _, _)| *i));
        kept_indices.sort();

        let pruned_messages: Vec<Message> = self
            .messages
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                if !kept_indices.contains(&i) {
                    Some(m.clone())
                } else {
                    None
                }
            })
            .collect();

        let new_messages: Vec<Message> = kept_indices
            .iter()
            .map(|&i| self.messages[i].clone())
            .collect();

        self.messages = new_messages;

        PruneResult {
            pruned_messages,
            retained_count: self.messages.len(),
        }
    }

    /// 获取被剪枝的消息的摘要文本（用于写入长期记忆）
    pub fn summarize_pruned(pruned: &[Message]) -> String {
        if pruned.is_empty() {
            return String::new();
        }
        
        let mut summary = String::from("Pruned conversation context:\n");
        for msg in pruned {
            let role = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::System => "System",
                Role::Tool => "Tool",
            };
            let content = if msg.content.len() > 100 {
                format!("{}...", &msg.content[..100])
            } else {
                msg.content.clone()
            };
            summary.push_str(&format!("- {}: {}\n", role, content));
        }
        summary
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_prune() {
        let mut mem = ConversationMemory::new(2); // 最多 4 条消息
        
        mem.push(Message::user("msg1"));
        mem.push(Message::assistant("reply1"));
        mem.push(Message::user("msg2"));
        mem.push(Message::assistant("reply2"));
        mem.push(Message::user("msg3")); // 触发剪枝
        
        assert!(mem.len() <= 4);
    }

    #[test]
    fn test_preserve_system_messages() {
        let mut mem = ConversationMemory::new(2);
        
        mem.push(Message::system("System prompt"));
        mem.push(Message::user("msg1"));
        mem.push(Message::assistant("reply1"));
        mem.push(Message::user("msg2"));
        mem.push(Message::assistant("reply2"));
        mem.push(Message::user("msg3"));
        mem.push(Message::assistant("reply3"));
        
        // System 消息应该被保留
        assert!(mem.messages().iter().any(|m| m.role == Role::System));
    }

    #[test]
    fn test_tool_messages_pruned_first() {
        let config = PruneConfig {
            preserve_system: true,
            tool_result_ratio: 0.25,
            smart_prune: true,
        };
        let mut mem = ConversationMemory::with_config(3, config); // 最多 6 条
        
        mem.push(Message::user("msg1"));
        mem.push(Message::tool("tool result 1"));
        mem.push(Message::tool("tool result 2"));
        mem.push(Message::tool("tool result 3"));
        mem.push(Message::assistant("reply1"));
        mem.push(Message::user("msg2"));
        mem.push(Message::assistant("reply2"));
        mem.push(Message::user("msg3")); // 触发剪枝
        
        // 工具消息应该被优先剪枝
        let tool_count = mem.messages().iter().filter(|m| m.role == Role::Tool).count();
        assert!(tool_count <= 2, "Should have at most 2 tool messages, got {}", tool_count);
    }

    #[test]
    fn test_prune_result() {
        let mut mem = ConversationMemory::new(2);
        
        mem.push(Message::user("msg1"));
        mem.push(Message::assistant("reply1"));
        mem.push(Message::user("msg2"));
        mem.push(Message::assistant("reply2"));
        
        // 手动触发剪枝
        mem.push(Message::user("msg3"));
        let result = mem.prune();
        
        assert!(result.pruned_messages.len() + result.retained_count >= 4);
    }

    #[test]
    fn test_summarize_pruned() {
        let pruned = vec![
            Message::user("Hello"),
            Message::assistant("Hi there!"),
        ];
        
        let summary = ConversationMemory::summarize_pruned(&pruned);
        assert!(summary.contains("User"));
        assert!(summary.contains("Assistant"));
        assert!(summary.contains("Hello"));
    }

    #[test]
    fn test_message_importance() {
        assert!(MessageImportance::System > MessageImportance::User);
        assert!(MessageImportance::User > MessageImportance::Assistant);
        assert!(MessageImportance::Assistant > MessageImportance::Tool);
    }
}
