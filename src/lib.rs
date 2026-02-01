//! Bee - Rust 个人智能体系统
//!
//! 模块划分：
//! - **agent**: 无头 Agent 运行时（供 WhatsApp / HTTP 等调用）
//! - **config**: 应用配置加载（TOML + 环境变量）
//! - **core**: 编排、状态、恢复、会话监管、任务调度
//! - **llm**: LLM 客户端抽象与实现（OpenAI 兼容 / DeepSeek / Mock）
//! - **memory**: 短期 / 中期 / 长期记忆与持久化
//! - **react**: Planner、Critic、ReAct 主循环
//! - **tools**: 工具箱（cat、ls、shell、search、echo）与执行器
//! - **ui**: Ratatui TUI 界面

pub mod agent;
pub mod config;
pub mod core;
pub mod integrations;
pub mod llm;
pub mod memory;
pub mod observability;
pub mod react;
pub mod tools;
pub mod ui;
