//! Planner：意图规划与 Tool Call 解析
//!
//! 调用 LLM 得到回复或 JSON Tool Call；parse_llm_output 从文本中提取 JSON 并解析为 ToolCall 或直接回复。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::AgentError;
use crate::llm::LlmClient;
use crate::memory::Message;

/// LLM 返回的 Tool Call（简化 JSON：{"tool": "cat", "args": {"path": "..."}}）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    pub args: serde_json::Value,
}

/// Planner 输出
#[derive(Debug, Clone)]
pub enum PlannerOutput {
    /// 直接回复用户
    Response(String),
    /// 需要执行工具
    ToolCall(ToolCall),
}

/// 解析 LLM 输出：若含有效 JSON 且 tool 非空则为 ToolCall，否则为 Response
/// 支持可选的工具名验证
pub fn parse_llm_output(output: &str) -> Result<PlannerOutput, AgentError> {
    parse_llm_output_with_validation(output, None)
}

/// 解析 LLM 输出，带工具名验证
pub fn parse_llm_output_with_validation(
    output: &str,
    valid_tools: Option<&[String]>,
) -> Result<PlannerOutput, AgentError> {
    let trimmed = output.trim();

    // 尝试提取 JSON 块（```json ... ``` 或纯 JSON）
    let json_str = if let Some(start) = trimmed.find("```json") {
        let rest = &trimmed[start + 7..];
        rest.find("```")
            .map(|end| rest[..end].trim())
            .unwrap_or(rest.trim())
    } else if let Some(start) = trimmed.find('{') {
        // 找到第一个 { 和对应的 }，支持嵌套和字符串内的 {}
        match extract_first_json_object(&trimmed[start..]) {
            Some(s) => s,
            None => return Ok(PlannerOutput::Response(trimmed.to_string())),
        }
    } else {
        return Ok(PlannerOutput::Response(trimmed.to_string()));
    };

    // 使用容错解析
    let parsed = match try_parse_json(json_str) {
        Ok(tc) => tc,
        Err(e) => {
            // JSON 解析失败，检查是否是简单的文本响应（含有误导性的 { 字符）
            // 如果 JSON 很短或明显不完整，当作普通文本处理
            if json_str.len() < 10 || !json_str.contains("\"tool\"") {
                return Ok(PlannerOutput::Response(trimmed.to_string()));
            }
            return Err(AgentError::JsonParseError(format!("{}: {}", e, json_str)));
        }
    };

    // 验证工具名
    if !validate_tool_name(&parsed.tool, valid_tools) {
        if parsed.tool.is_empty() {
            Ok(PlannerOutput::Response(trimmed.to_string()))
        } else if valid_tools.is_some() {
            // 工具名不在有效列表中
            Err(AgentError::ToolNotFound(parsed.tool))
        } else {
            Ok(PlannerOutput::ToolCall(parsed))
        }
    } else {
        Ok(PlannerOutput::ToolCall(parsed))
    }
}

/// 提取第一个完整的 JSON 对象（支持嵌套、正确处理字符串内的 `{}` 和转义字符）
fn extract_first_json_object(s: &str) -> Option<&str> {
    let mut depth = 0;
    let mut start = None;
    let mut in_string = false;
    let mut escape_next = false;
    
    for (i, ch) in s.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        
        match ch {
            '\\' if in_string => {
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
            }
            '{' if !in_string => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start_idx) = start {
                        return Some(&s[start_idx..=i]);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// 尝试多种策略解析 JSON，提高容错性
fn try_parse_json(json_str: &str) -> Result<ToolCall, serde_json::Error> {
    // 策略 1：直接解析
    if let Ok(tc) = serde_json::from_str::<ToolCall>(json_str) {
        return Ok(tc);
    }
    
    // 策略 2：去除可能的前后空白和控制字符
    let cleaned = json_str.trim().trim_matches(|c: char| c.is_control());
    if let Ok(tc) = serde_json::from_str::<ToolCall>(cleaned) {
        return Ok(tc);
    }
    
    // 策略 3：处理可能的单引号（非标准 JSON）
    let double_quoted = cleaned.replace('\'', "\"");
    if let Ok(tc) = serde_json::from_str::<ToolCall>(&double_quoted) {
        return Ok(tc);
    }
    
    // 最终：返回原始解析错误
    serde_json::from_str::<ToolCall>(json_str)
}

/// 验证工具名是否有效（若提供工具列表则检查，否则接受任意非空工具名）
pub fn validate_tool_name(tool_name: &str, valid_tools: Option<&[String]>) -> bool {
    if tool_name.is_empty() {
        return false;
    }
    match valid_tools {
        Some(tools) => tools.iter().any(|t| t == tool_name),
        None => true,
    }
}

/// Planner：持有 LLM 与 system prompt，负责 plan / plan_with_system（拼 system + messages 后调用 LLM）
pub struct Planner {
    llm: Arc<dyn LlmClient>,
    system_prompt: String,
}

impl Planner {
    pub fn new(llm: Arc<dyn LlmClient>, system_prompt: impl Into<String>) -> Self {
        Self {
            llm,
            system_prompt: system_prompt.into(),
        }
    }

    pub fn base_system_prompt(&self) -> &str {
        &self.system_prompt
    }

    /// 获取 LLM 累计 token 使用统计
    pub fn token_usage(&self) -> (u64, u64, u64) {
        self.llm.token_usage()
    }

    pub async fn plan(&self, messages: &[Message]) -> Result<String, AgentError> {
        self.plan_with_system(messages, &self.system_prompt).await
    }

    /// 使用动态拼接的 system（含 working memory、long-term 检索等）
    pub async fn plan_with_system(
        &self,
        messages: &[Message],
        system: &str,
    ) -> Result<String, AgentError> {
        let mut full_messages = vec![Message::system(system.to_string())];
        full_messages.extend(messages.to_vec());
        self.llm
            .complete(&full_messages)
            .await
            .map_err(AgentError::LlmError)
    }

    /// 将对话历史压缩为一段摘要（用于 Context Compaction：写入长期记忆后替换当前消息）
    pub async fn summarize(&self, messages: &[Message]) -> Result<String, AgentError> {
        if messages.is_empty() {
            return Ok(String::new());
        }
        let system = "You are a summarizer. Summarize the following conversation in one short paragraph: key facts, decisions, user preferences, and the latest question if any. Use the same language as the conversation. Output only the summary, no preamble.";
        let mut full = vec![Message::system(system.to_string())];
        full.extend(messages.to_vec());
        self.llm
            .complete(&full)
            .await
            .map_err(AgentError::LlmError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_llm_output_tool_call() {
        let output = r#"{"tool": "cat", "args": {"path": "src/main.rs"}}"#;
        let result = parse_llm_output(output).unwrap();
        match result {
            PlannerOutput::ToolCall(tc) => {
                assert_eq!(tc.tool, "cat");
                assert_eq!(tc.args["path"], "src/main.rs");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_llm_output_json_in_markdown() {
        let output = r#"
Let me read that file for you.

```json
{"tool": "cat", "args": {"path": "Cargo.toml"}}
```
"#;
        let result = parse_llm_output(output).unwrap();
        match result {
            PlannerOutput::ToolCall(tc) => {
                assert_eq!(tc.tool, "cat");
                assert_eq!(tc.args["path"], "Cargo.toml");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_llm_output_plain_response() {
        let output = "Hello! How can I help you today?";
        let result = parse_llm_output(output).unwrap();
        match result {
            PlannerOutput::Response(s) => {
                assert_eq!(s, output);
            }
            _ => panic!("Expected Response"),
        }
    }

    #[test]
    fn test_parse_llm_output_empty_tool() {
        let output = r#"{"tool": "", "args": {}}"#;
        let result = parse_llm_output(output).unwrap();
        match result {
            PlannerOutput::Response(_) => {}
            _ => panic!("Expected Response for empty tool"),
        }
    }

    #[test]
    fn test_parse_llm_output_nested_json() {
        let output = r#"{"tool": "shell", "args": {"command": "echo '{\"key\": \"value\"}'"}}"#;
        let result = parse_llm_output(output).unwrap();
        match result {
            PlannerOutput::ToolCall(tc) => {
                assert_eq!(tc.tool, "shell");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_llm_output_invalid_json() {
        // 不完整的 JSON，但包含 "tool" 字段，应该报错
        let output = r#"{"tool": "cat", "args": {"path": "test.txt"}"#; // 缺少最后的 }
        let result = parse_llm_output(output);
        // 由于 extract_first_json_object 找不到匹配的 }，会返回 None
        // 然后由于没有有效 JSON，返回 Response
        match result {
            Ok(PlannerOutput::Response(_)) => {}
            _ => panic!("Expected Response for incomplete JSON"),
        }
    }

    #[test]
    fn test_parse_llm_output_malformed_json() {
        // 格式错误的 JSON（有语法错误但括号匹配）
        let output = r#"{"tool": cat, "args": {}}"#; // cat 没有引号
        let result = parse_llm_output(output);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llm_output_braces_in_text() {
        // 文本中含有 { 但不是有效 JSON
        let output = "I think {this} is interesting";
        let result = parse_llm_output(output).unwrap();
        match result {
            PlannerOutput::Response(s) => {
                assert_eq!(s, output);
            }
            _ => panic!("Expected Response for text with braces"),
        }
    }

    #[test]
    fn test_parse_llm_output_braces_in_string_value() {
        // JSON 字符串值中含有 {}
        let output = r#"{"tool": "echo", "args": {"text": "test {value} here"}}"#;
        let result = parse_llm_output(output).unwrap();
        match result {
            PlannerOutput::ToolCall(tc) => {
                assert_eq!(tc.tool, "echo");
                assert_eq!(tc.args["text"], "test {value} here");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_llm_output_escaped_quotes() {
        // JSON 字符串中含有转义引号
        let output = r#"{"tool": "echo", "args": {"text": "say \"hello\""}}"#;
        let result = parse_llm_output(output).unwrap();
        match result {
            PlannerOutput::ToolCall(tc) => {
                assert_eq!(tc.tool, "echo");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_llm_output_with_validation() {
        let valid_tools = vec!["cat".to_string(), "shell".to_string()];
        
        // 有效工具
        let output = r#"{"tool": "cat", "args": {"path": "test.txt"}}"#;
        let result = parse_llm_output_with_validation(output, Some(&valid_tools)).unwrap();
        assert!(matches!(result, PlannerOutput::ToolCall(_)));
        
        // 无效工具
        let output = r#"{"tool": "invalid_tool", "args": {}}"#;
        let result = parse_llm_output_with_validation(output, Some(&valid_tools));
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_json_with_nested_braces() {
        // 嵌套 JSON
        let s = r#"Some text {"tool": "shell", "args": {"cmd": "{nested}"}} more text"#;
        let result = extract_first_json_object(s);
        assert!(result.is_some());
        let json = result.unwrap();
        assert!(json.contains("shell"));
    }

    #[test]
    fn test_validate_tool_name() {
        let tools = vec!["cat".to_string(), "shell".to_string()];
        
        assert!(validate_tool_name("cat", Some(&tools)));
        assert!(validate_tool_name("shell", Some(&tools)));
        assert!(!validate_tool_name("invalid", Some(&tools)));
        assert!(!validate_tool_name("", Some(&tools)));
        
        // 无验证列表时接受任意非空
        assert!(validate_tool_name("anything", None));
        assert!(!validate_tool_name("", None));
    }
}
