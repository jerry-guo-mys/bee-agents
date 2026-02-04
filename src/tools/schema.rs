//! 工具调用 JSON Schema 生成（白皮书 §4 BOM：schemars 自动生成工具 Schema）
//!
//! 用于将「合法 tool call」的 JSON 结构注入 system prompt，减少 LLM 输出格式错误。

use schemars::{schema_for, JsonSchema};
use std::collections::HashMap;

/// 工具调用请求格式：与 ReAct 解析的 `{"tool": "...", "args": {...}}` 一致（仅用于 Schema 生成）
#[allow(dead_code)]
#[derive(JsonSchema)]
struct ToolCallFormat {
    /// 工具名，如 cat、ls、echo、shell、search
    pub tool: String,
    /// 工具参数，依工具不同而不同（path、command、url、text 等）
    pub args: HashMap<String, String>,
}

/// 返回工具调用的 JSON Schema 字符串，可拼入 system prompt
pub fn tool_call_schema_json() -> String {
    let schema = schema_for!(ToolCallFormat);
    serde_json::to_string_pretty(&schema).unwrap_or_else(|_| String::new())
}
