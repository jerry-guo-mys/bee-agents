//! 记忆层：短期（对话）、中期（任务目标/尝试/失败）、长期（检索）、持久化

pub mod conversation;
pub mod long_term;
pub mod persistence;
pub mod working;

pub use conversation::{ConversationMemory, Message, Role};
pub use long_term::{InMemoryLongTerm, LongTermMemory, NoopLongTerm};
pub use persistence::ConversationPersistence;
pub use working::WorkingMemory;
