//! 核心编排层：错误与恢复、状态投影、会话监管、任务调度、主控循环

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
