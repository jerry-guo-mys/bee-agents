//! 核心编排层：错误与恢复、状态投影、会话监管、任务调度、主控循环
//!
//! 白皮书 §3.1 命名对应：`MemoryManager` = ContextManager，`ToolBox` = ToolExecutor，
//! `InternalState` 的投影源 = InternalStateSnapshot（memory/tool_box 由 Orchestrator 分别持有）。

pub mod error;
pub mod orchestrator;
pub mod recovery;
pub mod session_supervisor;
pub mod state;
pub mod task_scheduler;

pub use error::{AgentError, RecoveryAction};
pub use orchestrator::{create_agent, Command};
pub use recovery::RecoveryEngine;
pub use session_supervisor::SessionSupervisor;
pub use state::{AgentPhase, InternalStateSnapshot, UiState};
pub use task_scheduler::{TaskKind, TaskScheduler};

/// 白皮书 §3.1：记忆管理器，实现中即 [ContextManager](crate::react::ContextManager)
pub type MemoryManager = crate::react::ContextManager;

/// 白皮书 §3.1：工具箱，实现中即 [ToolExecutor](crate::tools::ToolExecutor)
pub type ToolBox = crate::tools::ToolExecutor;

/// 白皮书 §3.1：内部状态投影源（step/retries/phase 等）；完整 InternalState 的 memory/tool_box 由 Orchestrator 分别持有
pub type InternalState = InternalStateSnapshot;
