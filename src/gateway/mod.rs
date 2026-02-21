//! 轮毂式（Hub-and-Spoke）网关架构
//!
//! ## Hub（轮毂/中枢）- 核心运行时
//!
//! Hub 是整个系统的大脑，包含：
//! - **LLM 路由网关**：模型选择、负载均衡、fallback
//! - **记忆系统**：短期对话日志 + 长期文件索引
//! - **意图识别**：理解用户意图，路由到合适的能力
//! - **决策引擎**：ReAct 循环、规划、执行
//!
//! ## Spoke（辐条/端点）- 外围接入点
//!
//! Spoke 分为两类：
//!
//! ### 1. 通讯端点（Communication Spokes）
//! - Telegram、Slack、WhatsApp、Discord
//! - 终端命令行（TUI）
//! - Web 浏览器
//! - HTTP API
//!
//! ### 2. 能力端点（Capability Spokes）
//! - Skills 技能（知识增强、模板、脚本）
//! - 本地工具（文件操作、Shell、代码编辑）
//! - API 插件（搜索、浏览器、外部服务）
//! - 自动化脚本（Python/Shell）
//!
//! ## 架构优势
//!
//! - 彻底解耦：通讯层、决策层、能力层分离
//! - 跨平台上下文连贯：任何平台发消息都能保持对话
//! - 后台持续运行：支持异步任务和长时间处理
//! - 统一的会话管理和消息路由

mod hub;
mod intent;
mod message;
#[cfg(feature = "async-sqlite")]
mod persistent_session;
mod runtime;
mod session;
mod session_store;
mod spoke;

pub use hub::{Hub, HubConfig};
pub use intent::{Intent, IntentRecognizer};
pub use message::{GatewayMessage, MessageType, ClientInfo, SpokeType};
#[cfg(feature = "async-sqlite")]
pub use persistent_session::PersistentSessionManager;
pub use runtime::{AgentRuntime, RuntimeConfig};
pub use session::{Session, SessionManager, SessionId};
pub use session_store::{SessionStore, MemorySessionStore, create_session_store};
#[cfg(feature = "async-sqlite")]
pub use session_store::PersistentSessionStore;
pub use spoke::{SpokeAdapter, CommunicationSpoke, CapabilitySpoke, WebSocketSpoke, HttpSpoke};
