//! DeepSeek API 客户端（OpenAI 兼容格式）
//!
//! DeepSeek 提供与 OpenAI 完全兼容的 API 接口。
//! - Base URL: https://api.deepseek.com
//! - 模型: deepseek-chat (常规对话), deepseek-reasoner (思考模式)

use crate::llm::OpenAiClient;

/// DeepSeek API 常量
pub const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
pub const DEEPSEEK_CHAT: &str = "deepseek-chat";
pub const DEEPSEEK_REASONER: &str = "deepseek-reasoner";

/// 创建 DeepSeek 客户端
///
/// - 优先使用环境变量 `DEEPSEEK_API_KEY`
/// - 模型可通过 `model` 参数或 `DEEPSEEK_MODEL` 环境变量指定
///   - `deepseek-chat`: 常规对话，响应快
///   - `deepseek-reasoner`: 思考模式，适合复杂推理
pub fn create_deepseek_client(model: Option<&str>) -> OpenAiClient {
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .unwrap_or_else(|| "sk-placeholder".to_string());

    let model = model
        .map(String::from)
        .or_else(|| std::env::var("DEEPSEEK_MODEL").ok())
        .unwrap_or_else(|| DEEPSEEK_CHAT.to_string());

    OpenAiClient::new(Some(DEEPSEEK_BASE_URL), &model, Some(api_key.as_str()))
}
