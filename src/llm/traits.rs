//! LLM 客户端抽象
//!
//! 所有后端（OpenAI 兼容 / DeepSeek / Mock）实现 LlmClient：complete（非流式）、complete_stream（流式 Token）。

use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;
use thiserror::Error;

use crate::memory::Message;

/// LLM 错误类型（解决问题 3.2：错误类型化以支持精确恢复）
#[derive(Error, Debug, Clone)]
pub enum LlmError {
    /// 认证失败（API Key 无效或过期）
    #[error("Authentication failed: {0}")]
    AuthError(String),

    /// 速率限制（需要等待后重试）
    #[error("Rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    /// 模型不存在
    #[error("Model not found: {model}")]
    ModelNotFound { model: String },

    /// 上下文长度超限
    #[error("Context length exceeded: {tokens} tokens (max: {max_tokens})")]
    ContextLengthExceeded { tokens: usize, max_tokens: usize },

    /// 网络错误
    #[error("Network error: {0}")]
    NetworkError(String),

    /// 请求超时
    #[error("Request timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    /// 服务端错误（5xx）
    #[error("Server error ({status}): {message}")]
    ServerError { status: u16, message: String },

    /// 无效请求（4xx，非认证/限流/上下文超限）
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// 响应解析错误
    #[error("Failed to parse response: {0}")]
    ParseError(String),

    /// 其他 API 错误
    #[error("API error: {0}")]
    ApiError(String),
}

impl LlmError {
    /// 是否可重试
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LlmError::RateLimited { .. }
                | LlmError::NetworkError(_)
                | LlmError::Timeout { .. }
                | LlmError::ServerError { .. }
        )
    }

    /// 建议的重试等待时间（毫秒）
    pub fn retry_delay_ms(&self) -> Option<u64> {
        match self {
            LlmError::RateLimited { retry_after_ms } => Some(*retry_after_ms),
            LlmError::NetworkError(_) => Some(1000),
            LlmError::Timeout { .. } => Some(2000),
            LlmError::ServerError { .. } => Some(3000),
            _ => None,
        }
    }

    /// 从字符串错误消息解析 LlmError（兼容旧代码）
    pub fn from_string(s: &str) -> Self {
        let s_lower = s.to_lowercase();
        if s_lower.contains("unauthorized") || s_lower.contains("invalid api key") || s_lower.contains("authentication") {
            LlmError::AuthError(s.to_string())
        } else if s_lower.contains("rate limit") || s_lower.contains("too many requests") {
            LlmError::RateLimited { retry_after_ms: 60000 }
        } else if s_lower.contains("context length") || s_lower.contains("maximum context") || s_lower.contains("token limit") {
            LlmError::ContextLengthExceeded { tokens: 0, max_tokens: 0 }
        } else if s_lower.contains("model") && s_lower.contains("not found") {
            LlmError::ModelNotFound { model: s.to_string() }
        } else if s_lower.contains("timeout") {
            LlmError::Timeout { timeout_ms: 30000 }
        } else if s_lower.contains("network") || s_lower.contains("connection") {
            LlmError::NetworkError(s.to_string())
        } else {
            LlmError::ApiError(s.to_string())
        }
    }
}

/// 从 String 转换为 LlmError（向后兼容）
impl From<String> for LlmError {
    fn from(s: String) -> Self {
        LlmError::from_string(&s)
    }
}

/// LlmError 转换为 String（向后兼容）
impl From<LlmError> for String {
    fn from(e: LlmError) -> Self {
        e.to_string()
    }
}

/// LLM 客户端 trait：非流式完成与流式完成（返回 Token 流）
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 非流式完成
    async fn complete(&self, messages: &[Message]) -> Result<String, LlmError>;

    /// 流式完成，返回 Token 流
    async fn complete_stream(
        &self,
        messages: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>, LlmError>;

    /// 获取累计 token 使用统计：(prompt_tokens, completion_tokens, total_tokens)
    /// 默认返回 (0, 0, 0)，具体实现可覆盖
    fn token_usage(&self) -> (u64, u64, u64) {
        (0, 0, 0)
    }
}

/// 重试配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_retries: u32,
    /// 初始等待时间（毫秒）
    pub initial_delay_ms: u64,
    /// 最大等待时间（毫秒）
    pub max_delay_ms: u64,
    /// 指数退避倍数
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// 计算第 n 次重试的等待时间
    pub fn delay_for_retry(&self, retry_count: u32, error: &LlmError) -> u64 {
        if let Some(suggested) = error.retry_delay_ms() {
            return suggested.min(self.max_delay_ms);
        }
        let delay = (self.initial_delay_ms as f64 * self.backoff_multiplier.powi(retry_count as i32)) as u64;
        delay.min(self.max_delay_ms)
    }
}

/// 带自动重试的 LLM 客户端包装器（解决问题 3.3：重试策略）
pub struct RetryingLlmClient<C: LlmClient> {
    inner: C,
    config: RetryConfig,
}

impl<C: LlmClient> RetryingLlmClient<C> {
    /// 创建带重试的客户端
    pub fn new(inner: C) -> Self {
        Self {
            inner,
            config: RetryConfig::default(),
        }
    }

    /// 使用自定义重试配置
    pub fn with_config(inner: C, config: RetryConfig) -> Self {
        Self { inner, config }
    }

    /// 获取内部客户端引用
    pub fn inner(&self) -> &C {
        &self.inner
    }
}

#[async_trait]
impl<C: LlmClient> LlmClient for RetryingLlmClient<C> {
    async fn complete(&self, messages: &[Message]) -> Result<String, LlmError> {
        let mut last_error = None;

        for retry in 0..=self.config.max_retries {
            match self.inner.complete(messages).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if !e.is_retryable() || retry == self.config.max_retries {
                        return Err(e);
                    }
                    let delay = self.config.delay_for_retry(retry, &e);
                    tracing::warn!(
                        "LLM request failed (attempt {}/{}): {}, retrying in {}ms",
                        retry + 1,
                        self.config.max_retries + 1,
                        e,
                        delay
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(LlmError::ApiError("Unknown error".to_string())))
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>, LlmError> {
        let mut last_error = None;

        for retry in 0..=self.config.max_retries {
            match self.inner.complete_stream(messages).await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    if !e.is_retryable() || retry == self.config.max_retries {
                        return Err(e);
                    }
                    let delay = self.config.delay_for_retry(retry, &e);
                    tracing::warn!(
                        "LLM stream request failed (attempt {}/{}): {}, retrying in {}ms",
                        retry + 1,
                        self.config.max_retries + 1,
                        e,
                        delay
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(LlmError::ApiError("Unknown error".to_string())))
    }

    fn token_usage(&self) -> (u64, u64, u64) {
        self.inner.token_usage()
    }
}
