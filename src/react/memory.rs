//! 三层记忆协调
//!
//! 将短期（Conversation）、中期（Working）、长期（LongTerm）统一为 ContextManager，
//! 供 ReAct 循环拼 system prompt（working_memory_section + long_term_section + lessons_section）与写入长期记忆。

use std::path::PathBuf;
use std::sync::Arc;

use crate::memory::{
    append_procedural, load_lessons, load_procedural, ConversationMemory, LongTermMemory, Message,
    WorkingMemory,
};

/// 上下文管理器：整合短期/中期/长期记忆，提供 to_llm_messages、working_memory_section、long_term_section、lessons_section、procedural_section
#[derive(Clone)]
pub struct ContextManager {
    pub conversation: ConversationMemory,
    pub working: WorkingMemory,
    pub long_term: Option<Arc<dyn LongTermMemory>>,
    /// 行为约束/教训文件路径（memory/lessons.md），用于自我进化：内容会注入 system prompt
    pub lessons_path: Option<PathBuf>,
    /// 程序记忆文件路径（memory/procedural.md），工具成功/失败经验会注入 system prompt
    pub procedural_path: Option<PathBuf>,
}

impl ContextManager {
    pub fn new(max_turns: usize) -> Self {
        Self {
            conversation: ConversationMemory::new(max_turns),
            working: WorkingMemory::new(),
            long_term: None,
            lessons_path: None,
            procedural_path: None,
        }
    }

    pub fn with_long_term(mut self, long_term: Arc<dyn LongTermMemory>) -> Self {
        self.long_term = Some(long_term);
        self
    }

    /// 设置行为约束/教训文件路径（自我进化：该文件内容会注入 system prompt）
    pub fn with_lessons_path(mut self, path: PathBuf) -> Self {
        self.lessons_path = Some(path);
        self
    }

    /// 设置程序记忆文件路径（自我进化：工具经验会注入 system prompt）
    pub fn with_procedural_path(mut self, path: PathBuf) -> Self {
        self.procedural_path = Some(path);
        self
    }

    /// 行为约束/教训段落（从 memory/lessons.md 读取，供自我进化）
    pub fn lessons_section(&self) -> String {
        let Some(ref p) = self.lessons_path else {
            return String::new();
        };
        let s = load_lessons(p);
        if s.is_empty() {
            return String::new();
        }
        format!("\n## 行为约束 / Lessons（请遵守）\n{}\n", s)
    }

    /// 程序记忆段落（从 memory/procedural.md 读取，工具使用经验，供自我进化）
    pub fn procedural_section(&self) -> String {
        let Some(ref p) = self.procedural_path else {
            return String::new();
        };
        let s = load_procedural(p);
        if s.is_empty() {
            return String::new();
        }
        format!("\n## 程序记忆 / 工具使用经验（请参考，避免重复失败）\n{}\n", s)
    }

    /// 记录一次工具调用结果到程序记忆（失败时调用可减少重复错误）
    pub fn append_procedural_record(&self, tool: &str, success: bool, detail: &str) {
        if let Some(ref p) = self.procedural_path {
            let _ = append_procedural(p, tool, success, detail);
        }
    }

    /// 替换当前对话消息（用于 Context Compaction 后仅保留摘要）
    pub fn set_messages(&mut self, messages: Vec<Message>) {
        self.conversation.set_messages(messages);
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
