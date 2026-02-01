//! 工具执行器
//!
//! 持有 ToolRegistry 与全局超时，execute(tool_name, args) 在超时内调用 registry.execute，
//! 超时或失败时转为 AgentError（ToolTimeout / ToolExecutionFailed）。

use std::time::Duration;

use tokio::time::timeout;

use crate::core::AgentError;
use crate::tools::ToolRegistry;

/// 工具执行器：对每次调用施加超时，并将结果映射为 AgentError
pub struct ToolExecutor {
    registry: ToolRegistry,
    timeout: Duration,
}

impl ToolExecutor {
    pub fn new(registry: ToolRegistry, timeout_secs: u64) -> Self {
        Self {
            registry,
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// 执行指定工具；超时返回 ToolTimeout，工具返回 Err 则转为 ToolExecutionFailed
    pub async fn execute(&self, tool_name: &str, args: serde_json::Value) -> Result<String, AgentError> {
        let result = timeout(
            self.timeout,
            self.registry.execute(tool_name, args),
        )
        .await;

        match result {
            Ok(Ok(content)) => Ok(content),
            Ok(Err(e)) => Err(AgentError::ToolExecutionFailed(e)),
            Err(_) => Err(AgentError::ToolTimeout(tool_name.to_string())),
        }
    }

    pub fn get_tool(&self, name: &str) -> Option<std::sync::Arc<dyn crate::tools::Tool>> {
        self.registry.get(name)
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.registry.tool_names()
    }
}
