//! 状态定义：UiState 投影与 InternalState
//!
//! UI 只持有轻量的 UiState（阶段、历史、锁、错误）；内部完整状态由 Orchestrator 维护并投影到 UiState。

use serde::Serialize;

use crate::memory::Message;

/// UI 看到的「投影」状态，轻量且易于渲染
#[derive(Clone, Debug, Serialize)]
pub struct UiState {
    pub phase: AgentPhase,
    pub history: Vec<Message>,
    pub active_tool: Option<String>,
    pub input_locked: bool,
    pub error_message: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            phase: AgentPhase::Idle,
            history: Vec::new(),
            active_tool: None,
            input_locked: false,
            error_message: None,
        }
    }
}

/// Agent 阶段（UI 投影用）
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum AgentPhase {
    Idle,
    Thinking,
    Streaming,
    ToolExecuting,
    Responding,
    Error,
}

/// 内部状态快照（用于投影）
#[derive(Clone, Debug)]
pub struct InternalStateSnapshot {
    pub step: usize,
    pub retries: u8,
    pub context_tokens: usize,
    pub phase: AgentPhase,
    pub active_tool: Option<String>,
}

impl InternalStateSnapshot {
    /// 将内部快照与最新历史/锁/错误合并，得到 UI 可渲染的 UiState
    pub fn project(&self, history: Vec<Message>, input_locked: bool, error_message: Option<String>) -> UiState {
        UiState {
            phase: self.phase.clone(),
            history,
            active_tool: self.active_tool.clone(),
            input_locked,
            error_message,
        }
    }
}
