//! 工具注册表
//!
//! 所有工具实现 Tool trait（name / description / execute），由 ToolRegistry 按名注册与查找，
//! ToolExecutor 在调用时加超时并统一转 AgentError。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

/// 工具 trait：名称、描述（供 LLM 理解）、异步执行（args 为 JSON）
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, args: Value) -> Result<String, String>;
}

/// 工具注册表：按名称存储 Arc<dyn Tool>，支持 register / get / execute / tool_names
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub async fn execute(&self, name: &str, args: Value) -> Result<String, String> {
        let tool = self.tools.get(name).ok_or_else(|| format!("Unknown tool: {name}"))?;
        tool.execute(args).await
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// 返回 (name, description) 列表，用于生成 prompt 中的 Available tools 段落
    pub fn tool_descriptions(&self) -> Vec<(String, String)> {
        self.tools
            .iter()
            .map(|(name, tool)| (name.clone(), tool.description().to_string()))
            .collect()
    }

    /// 动态生成工具 schema JSON（解决问题 6.1：Schema 与实际注册工具匹配）
    pub fn to_schema_json(&self) -> String {
        let tools: Vec<serde_json::Value> = self
            .tools
            .iter()
            .map(|(name, tool)| {
                serde_json::json!({
                    "name": name,
                    "description": tool.description()
                })
            })
            .collect();
        serde_json::to_string_pretty(&tools).unwrap_or_else(|_| "[]".to_string())
    }
}
