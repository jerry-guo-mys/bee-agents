//! 工具注册表
//!
//! 所有工具实现 Tool trait（name / description / execute），由 ToolRegistry 按名注册与查找，
//! ToolExecutor 在调用时加超时并统一转 AgentError。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

/// 工具 trait：名称、描述（供 LLM 理解）、参数 schema、异步执行（args 为 JSON）
/// 解决问题 6.2：添加 parameters_schema 方法
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具名称（用于 JSON 中的 "tool" 字段）
    fn name(&self) -> &str;

    /// 工具描述（供 LLM 理解功能）
    fn description(&self) -> &str;

    /// 参数 JSON Schema（供 LLM 生成正确的参数格式）
    /// 默认返回空对象，表示无参数或参数格式不限
    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    /// 执行工具
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
    /// 包含参数 schema（解决问题 6.2）
    pub fn to_schema_json(&self) -> String {
        let tools: Vec<serde_json::Value> = self
            .tools
            .iter()
            .map(|(name, tool)| {
                serde_json::json!({
                    "name": name,
                    "description": tool.description(),
                    "parameters": tool.parameters_schema()
                })
            })
            .collect();
        serde_json::to_string_pretty(&tools).unwrap_or_else(|_| "[]".to_string())
    }
}
