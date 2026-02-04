//! 三层记忆协调
//!
//! 将短期（Conversation）、中期（Working）、长期（LongTerm）统一为 ContextManager，
//! 供 ReAct 循环拼 system prompt（working_memory_section + long_term_section + lessons_section）与写入长期记忆。

use std::path::PathBuf;
use std::sync::Arc;

use crate::memory::{
    append_lesson, append_preference, append_procedural, load_lessons, load_preferences,
    load_procedural, ConversationMemory, LongTermMemory, Message, WorkingMemory,
};

/// 上下文管理器：整合短期/中期/长期记忆，提供 to_llm_messages、working_memory_section、long_term_section、lessons_section、procedural_section、preferences_section
#[derive(Clone)]
pub struct ContextManager {
    pub conversation: ConversationMemory,
    pub working: WorkingMemory,
    pub long_term: Option<Arc<dyn LongTermMemory>>,
    /// 行为约束/教训文件路径（memory/lessons.md），用于自我进化：内容会注入 system prompt
    pub lessons_path: Option<PathBuf>,
    /// 程序记忆文件路径（memory/procedural.md），工具成功/失败经验会注入 system prompt
    pub procedural_path: Option<PathBuf>,
    /// 用户偏好文件路径（memory/preferences.md），显式「记住：xxx」会写入并注入 system prompt
    pub preferences_path: Option<PathBuf>,
    /// HallucinatedTool 时是否自动向 lessons.md 追加教训（由 config [evolution] 控制）
    pub auto_lesson_on_hallucination: bool,
    /// 是否将工具调用成功也写入 procedural.md（EVOLUTION §3.5 工具统计）
    pub record_tool_success: bool,
}

impl ContextManager {
    pub fn new(max_turns: usize) -> Self {
        Self {
            conversation: ConversationMemory::new(max_turns),
            working: WorkingMemory::new(),
            long_term: None,
            lessons_path: None,
            procedural_path: None,
            preferences_path: None,
            auto_lesson_on_hallucination: true,
            record_tool_success: false,
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

    /// 设置用户偏好文件路径（自我进化：显式「记住：xxx」写入并注入 system prompt）
    pub fn with_preferences_path(mut self, path: PathBuf) -> Self {
        self.preferences_path = Some(path);
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

    /// 用户偏好段落（从 memory/preferences.md 读取，显式「记住：xxx」会写入该文件）
    pub fn preferences_section(&self) -> String {
        let Some(ref p) = self.preferences_path else {
            return String::new();
        };
        let s = load_preferences(p);
        if s.is_empty() {
            return String::new();
        }
        format!("\n## 用户偏好 / Preferences（请遵守）\n{}\n", s)
    }

    /// 记录一次用户显式偏好（用户说「记住：xxx」时调用）
    pub fn append_preference(&self, content: &str) {
        if let Some(ref p) = self.preferences_path {
            let _ = append_preference(p, content);
        }
    }

    /// 当 Critic 给出修正建议时追加到 lessons.md，供后续对话遵守（EVOLUTION §3.4 Critic → Lessons）
    pub fn append_critic_lesson(&self, suggestion: &str) {
        if suggestion.trim().is_empty() {
            return;
        }
        let Some(ref p) = self.lessons_path else {
            return;
        };
        let line = format!("Critic 建议：{}", suggestion.trim());
        let _ = append_lesson(p, &line);
    }

    /// 当发生 HallucinatedTool 时追加一条教训到 lessons.md，减少后续幻觉（受 auto_lesson_on_hallucination 控制）
    pub fn append_hallucination_lesson(&self, hallucinated_tool: &str, valid_tools: &[String]) {
        if !self.auto_lesson_on_hallucination {
            return;
        }
        let Some(ref p) = self.lessons_path else {
            return;
        };
        let tools_list = valid_tools.join("、");
        let line = format!(
            "仅使用以下已注册工具：{}；不要编造不存在的工具名（例如曾误用「{}」）。",
            tools_list, hallucinated_tool
        );
        let _ = append_lesson(p, &line);
    }

    /// 设置 HallucinatedTool 时是否自动追加教训（与 config [evolution].auto_lesson_on_hallucination 一致）
    pub fn with_auto_lesson_on_hallucination(mut self, enabled: bool) -> Self {
        self.auto_lesson_on_hallucination = enabled;
        self
    }

    /// 设置是否记录工具成功到 procedural（与 config [evolution].record_tool_success 一致）
    pub fn with_record_tool_success(mut self, enabled: bool) -> Self {
        self.record_tool_success = enabled;
        self
    }

    /// 将本轮会话策略（目标 + 使用的工具）写入长期记忆，供后续检索（EVOLUTION §3.5）
    pub fn push_session_strategy_to_long_term(&self, goal: &str, tool_names: &[String]) {
        if tool_names.is_empty() {
            return;
        }
        let tools = tool_names.join(", ");
        let line = format!("Session strategy: goal \"{}\"; tools used: {}.", goal.trim(), tools);
        self.push_to_long_term(&line);
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
