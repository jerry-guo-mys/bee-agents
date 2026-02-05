//! 工具执行器
//!
//! 持有 ToolRegistry 与全局超时，execute(tool_name, args) 在超时内调用 registry.execute，
//! 超时或失败时转为 AgentError（ToolTimeout / ToolExecutionFailed）；每次调用输出结构化审计日志（JSON）。

use std::time::{Duration, Instant};

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

    /// 执行指定工具；超时返回 ToolTimeout，工具返回 Err 则转为 ToolExecutionFailed；输出 JSON 审计日志
    pub async fn execute(&self, tool_name: &str, args: serde_json::Value) -> Result<String, AgentError> {
        let start = Instant::now();
        let args_preview = args_preview(&args);
        let result = timeout(
            self.timeout,
            self.registry.execute(tool_name, args),
        )
        .await;

        let (ok, outcome): (bool, &str) = match &result {
            Ok(Ok(_)) => (true, "ok"),
            Ok(Err(_)) => (false, "error"),
            Err(_) => (false, "timeout"),
        };
        let duration_ms = start.elapsed().as_millis() as u64;
        let audit = serde_json::json!({
            "event": "tool_audit",
            "tool": tool_name,
            "ok": ok,
            "outcome": outcome,
            "duration_ms": duration_ms,
            "args_preview": args_preview,
        });
        tracing::info!(audit = %audit.to_string(), "tool");

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

fn args_preview(args: &serde_json::Value) -> String {
    let s = args.to_string();
    if s.len() > 200 {
        format!("{}...", s.chars().take(200).collect::<String>())
    } else {
        s
    }
}
