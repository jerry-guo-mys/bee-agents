//! 记忆层：短期（对话）、中期（任务目标/尝试/失败）、长期（检索）、持久化
//! 支持 Markdown 文件存储：短期按日日志 memory/logs/YYYY-MM-DD.md，长期 memory/long-term.md
//! 自我改进：.learnings/ERRORS.md、LEARNINGS.md、FEATURE_REQUESTS.md

pub mod conversation;
pub mod learnings;
pub mod long_term;
pub mod markdown_store;
pub mod persistence;
pub mod working;

pub use conversation::{ConversationMemory, Message, Role};
pub use long_term::{InMemoryLongTerm, InMemoryVectorLongTerm, LongTermMemory, NoopLongTerm};
pub use markdown_store::{
    append_daily_log, append_lesson, append_preference, append_procedural, consolidate_memory,
    daily_log_path, list_daily_logs_for_llm, load_lessons, load_preferences, load_procedural,
    append_heartbeat_log, heartbeat_log_path, long_term_path, lessons_path, memory_root,
    preferences_path, procedural_path, vector_snapshot_path, ConsolidateResult, FileLongTerm,
};
pub use learnings::{
    agents_path, learnings_root, promote_to_agents, promote_to_soul, promote_to_tools,
    record_error, record_feature_request, record_learning, soul_path, tools_guide_path,
};
pub use persistence::{ConversationPersistence, SqlitePersistence};
pub use working::WorkingMemory;
