//! 三层记忆协调
//!
//! 将短期（Conversation）、中期（Working）、长期（LongTerm）统一为 ContextManager，
//! 供 ReAct 循环拼 system prompt（working_memory_section + long_term_section）与写入长期记忆。

use std::sync::Arc;

use crate::memory::{ConversationMemory, LongTermMemory, Message, WorkingMemory};

/// 上下文管理器：整合短期/中期/长期记忆，提供 to_llm_messages、working_memory_section、long_term_section
#[derive(Clone)]
pub struct ContextManager {
    pub conversation: ConversationMemory,
    pub working: WorkingMemory,
    pub long_term: Option<Arc<dyn LongTermMemory>>,
}

impl ContextManager {
    pub fn new(max_turns: usize) -> Self {
        Self {
            conversation: ConversationMemory::new(max_turns),
            working: WorkingMemory::new(),
            long_term: None,
        }
    }

    pub fn with_long_term(mut self, long_term: Arc<dyn LongTermMemory>) -> Self {
        self.long_term = Some(long_term);
        self
    }

    pub fn push_message(&mut self, msg: Message) {
        self.conversation.push(msg);
    }

    pub fn messages(&self) -> &[Message] {
        self.conversation.messages()
    }

    pub fn to_llm_messages(&self) -> Vec<Message> {
        self.conversation.messages().to_vec()
    }

    /// 构建带 Working Memory 的 Prompt 后缀
    pub fn working_memory_section(&self) -> String {
        self.working.to_prompt_section()
    }

    /// 构建长期记忆检索段落（Relevant Past Knowledge）
    pub fn long_term_section(&self, query: &str) -> String {
        let Some(ref lt) = self.long_term else {
            return String::new();
        };
        if !lt.enabled() {
            return String::new();
        }
        let hits = lt.search(query, 5);
        if hits.is_empty() {
            return String::new();
        }
        let block = hits.join("\n\n");
        format!("## Relevant Past Knowledge\n{block}")
    }

    /// 将重要内容写入长期记忆（如最终回复摘要）
    pub fn push_to_long_term(&self, text: &str) {
        if let Some(ref lt) = self.long_term {
            lt.add(text);
        }
    }
}
