//! Echo 工具（测试用）

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

/// Echo 工具：回显文本
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo text (for testing). Args: {\"text\": \"message\"}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("(empty)");
        Ok(text.to_string())
    }
}
