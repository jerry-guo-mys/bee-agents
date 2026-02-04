//! 记忆层：短期（对话）、中期（任务目标/尝试/失败）、长期（检索）、持久化
//! 支持 Markdown 文件存储：短期按日日志 memory/logs/YYYY-MM-DD.md，长期 memory/long-term.md

pub mod conversation;
pub mod long_term;
pub mod markdown_store;
pub mod persistence;
pub mod working;

pub use conversation::{ConversationMemory, Message, Role};
pub use long_term::{InMemoryLongTerm, LongTermMemory, NoopLongTerm};
pub use markdown_store::{
    append_daily_log, append_lesson, append_preference, append_procedural, consolidate_memory,
    daily_log_path, list_daily_logs_for_llm, load_lessons, load_preferences, load_procedural,
    long_term_path, lessons_path, memory_root, preferences_path, procedural_path,
    ConsolidateResult, FileLongTerm,
};
pub use persistence::ConversationPersistence;
pub use working::WorkingMemory;
