//! 可观测性（解决问题 7.1）
//!
//! 提供结构化 metrics 和 tracing spans：
//! - LLM 调用次数/延迟/token 消耗/错误率
//! - 工具执行时间
//! - 请求完整生命周期追踪

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use uuid::Uuid;

pub fn init() {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();
}

/// 生成新的请求 ID
pub fn generate_request_id() -> String {
    Uuid::new_v4().to_string()
}

/// 在 tracing span 中注入请求 ID
pub fn with_request_id<F, T>(request_id: &str, f: F) -> T
where
    F: FnOnce() -> T,
{
    let span = tracing::info_span!("request", request_id = %request_id);
    let _guard = span.enter();
    f()
}

pub fn init_metrics() {
    tracing::info!("Metrics system initialized");
}

/// 全局指标收集器
#[derive(Debug, Default)]
pub struct Metrics {
    /// LLM 相关指标
    pub llm: LlmMetrics,
    /// 工具相关指标
    pub tools: ToolMetrics,
    /// 会话相关指标
    pub session: SessionMetrics,
    /// AI 行为质量指标
    pub behavior: BehaviorMetrics,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取全局指标实例
    pub fn global() -> &'static Metrics {
        static INSTANCE: std::sync::OnceLock<Metrics> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(Metrics::new)
    }

    /// 导出为 JSON 格式
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "llm": {
                "total_calls": self.llm.total_calls.load(Ordering::Relaxed),
                "successful_calls": self.llm.successful_calls.load(Ordering::Relaxed),
                "failed_calls": self.llm.failed_calls.load(Ordering::Relaxed),
                "total_prompt_tokens": self.llm.total_prompt_tokens.load(Ordering::Relaxed),
                "total_completion_tokens": self.llm.total_completion_tokens.load(Ordering::Relaxed),
                "total_latency_ms": self.llm.total_latency_ms.load(Ordering::Relaxed),
                "average_latency_ms": self.llm.average_latency_ms(),
                "error_rate": self.llm.error_rate(),
            },
            "tools": {
                "total_executions": self.tools.total_executions.load(Ordering::Relaxed),
                "successful_executions": self.tools.successful_executions.load(Ordering::Relaxed),
                "failed_executions": self.tools.failed_executions.load(Ordering::Relaxed),
                "total_execution_time_ms": self.tools.total_execution_time_ms.load(Ordering::Relaxed),
                "average_execution_time_ms": self.tools.average_execution_time_ms(),
            },
            "session": {
                "total_requests": self.session.total_requests.load(Ordering::Relaxed),
                "active_sessions": self.session.active_sessions.load(Ordering::Relaxed),
            },
            "behavior": {
                "intent_misunderstandings": self.behavior.intent_misunderstandings.load(Ordering::Relaxed),
                "tool_misuses": self.behavior.tool_misuses.load(Ordering::Relaxed),
                "path_errors": self.behavior.path_errors.load(Ordering::Relaxed),
                "output_issues": self.behavior.output_issues.load(Ordering::Relaxed),
                "user_corrections": self.behavior.user_corrections.load(Ordering::Relaxed),
                "total_errors": self.behavior.total_errors(),
                "tasks_completed_first_try": self.behavior.tasks_completed_first_try.load(Ordering::Relaxed),
                "tasks_total": self.behavior.tasks_total.load(Ordering::Relaxed),
                "completion_rate": self.behavior.completion_rate(),
                "error_rate": self.behavior.error_rate(),
            }
        })
    }

    /// 导出为 Prometheus 格式
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();
        
        // LLM metrics
        output.push_str(&format!(
            "# TYPE bee_llm_calls_total counter\nbee_llm_calls_total {}\n",
            self.llm.total_calls.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_llm_calls_success counter\nbee_llm_calls_success {}\n",
            self.llm.successful_calls.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_llm_calls_failure counter\nbee_llm_calls_failure {}\n",
            self.llm.failed_calls.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_llm_prompt_tokens_total counter\nbee_llm_prompt_tokens_total {}\n",
            self.llm.total_prompt_tokens.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_llm_completion_tokens_total counter\nbee_llm_completion_tokens_total {}\n",
            self.llm.total_completion_tokens.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_llm_latency_ms_total counter\nbee_llm_latency_ms_total {}\n",
            self.llm.total_latency_ms.load(Ordering::Relaxed)
        ));
        
        // Tool metrics
        output.push_str(&format!(
            "# TYPE bee_tool_executions_total counter\nbee_tool_executions_total {}\n",
            self.tools.total_executions.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_tool_executions_success counter\nbee_tool_executions_success {}\n",
            self.tools.successful_executions.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_tool_executions_failure counter\nbee_tool_executions_failure {}\n",
            self.tools.failed_executions.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_tool_execution_time_ms_total counter\nbee_tool_execution_time_ms_total {}\n",
            self.tools.total_execution_time_ms.load(Ordering::Relaxed)
        ));
        
        // Session metrics
        output.push_str(&format!(
            "# TYPE bee_session_requests_total counter\nbee_session_requests_total {}\n",
            self.session.total_requests.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_session_active_sessions gauge\nbee_session_active_sessions {}\n",
            self.session.active_sessions.load(Ordering::Relaxed)
        ));

        // Behavior metrics
        output.push_str(&format!(
            "# TYPE bee_behavior_intent_misunderstandings counter\nbee_behavior_intent_misunderstandings {}\n",
            self.behavior.intent_misunderstandings.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_behavior_tool_misuses counter\nbee_behavior_tool_misuses {}\n",
            self.behavior.tool_misuses.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_behavior_path_errors counter\nbee_behavior_path_errors {}\n",
            self.behavior.path_errors.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_behavior_output_issues counter\nbee_behavior_output_issues {}\n",
            self.behavior.output_issues.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_behavior_user_corrections counter\nbee_behavior_user_corrections {}\n",
            self.behavior.user_corrections.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_behavior_tasks_total counter\nbee_behavior_tasks_total {}\n",
            self.behavior.tasks_total.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_behavior_tasks_completed_first_try counter\nbee_behavior_tasks_completed_first_try {}\n",
            self.behavior.tasks_completed_first_try.load(Ordering::Relaxed)
        ));
        output.push_str(&format!(
            "# TYPE bee_behavior_completion_rate gauge\nbee_behavior_completion_rate {}\n",
            self.behavior.completion_rate()
        ));
        output.push_str(&format!(
            "# TYPE bee_behavior_error_rate gauge\nbee_behavior_error_rate {}\n",
            self.behavior.error_rate()
        ));
        
        output
    }
}

/// LLM 相关指标
#[derive(Debug, Default)]
pub struct LlmMetrics {
    pub total_calls: AtomicU64,
    pub successful_calls: AtomicU64,
    pub failed_calls: AtomicU64,
    pub total_prompt_tokens: AtomicU64,
    pub total_completion_tokens: AtomicU64,
    pub total_latency_ms: AtomicU64,
}

impl LlmMetrics {
    pub fn record_call(&self, success: bool, latency: Duration, prompt_tokens: u64, completion_tokens: u64) {
        self.total_calls.fetch_add(1, Ordering::Relaxed);
        if success {
            self.successful_calls.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_calls.fetch_add(1, Ordering::Relaxed);
        }
        self.total_latency_ms.fetch_add(latency.as_millis() as u64, Ordering::Relaxed);
        self.total_prompt_tokens.fetch_add(prompt_tokens, Ordering::Relaxed);
        self.total_completion_tokens.fetch_add(completion_tokens, Ordering::Relaxed);
    }

    pub fn average_latency_ms(&self) -> f64 {
        let total = self.total_latency_ms.load(Ordering::Relaxed);
        let count = self.total_calls.load(Ordering::Relaxed);
        if count == 0 {
            0.0
        } else {
            total as f64 / count as f64
        }
    }

    pub fn error_rate(&self) -> f64 {
        let total = self.total_calls.load(Ordering::Relaxed);
        let failed = self.failed_calls.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            failed as f64 / total as f64
        }
    }
}

/// 工具相关指标
#[derive(Debug, Default)]
pub struct ToolMetrics {
    pub total_executions: AtomicU64,
    pub successful_executions: AtomicU64,
    pub failed_executions: AtomicU64,
    pub total_execution_time_ms: AtomicU64,
}

impl ToolMetrics {
    pub fn record_execution(&self, success: bool, duration: Duration) {
        self.total_executions.fetch_add(1, Ordering::Relaxed);
        if success {
            self.successful_executions.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_executions.fetch_add(1, Ordering::Relaxed);
        }
        self.total_execution_time_ms.fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }

    pub fn average_execution_time_ms(&self) -> f64 {
        let total = self.total_execution_time_ms.load(Ordering::Relaxed);
        let count = self.total_executions.load(Ordering::Relaxed);
        if count == 0 {
            0.0
        } else {
            total as f64 / count as f64
        }
    }
}

/// 会话相关指标
#[derive(Debug, Default)]
pub struct SessionMetrics {
    pub total_requests: AtomicU64,
    pub active_sessions: AtomicU64,
}

impl SessionMetrics {
    pub fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_active_sessions(&self) {
        self.active_sessions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn decrement_active_sessions(&self) {
        self.active_sessions.fetch_sub(1, Ordering::Relaxed);
    }
}

/// AI 行为质量指标（从 Python ai_monitor.py 迁移）
#[derive(Debug, Default)]
pub struct BehaviorMetrics {
    /// 意图误解次数
    pub intent_misunderstandings: AtomicU64,
    /// 工具误用次数
    pub tool_misuses: AtomicU64,
    /// 路径错误次数
    pub path_errors: AtomicU64,
    /// 输出不当次数
    pub output_issues: AtomicU64,
    /// 用户纠正次数
    pub user_corrections: AtomicU64,
    /// 一次完成的任务数
    pub tasks_completed_first_try: AtomicU64,
    /// 总任务数
    pub tasks_total: AtomicU64,
}

impl BehaviorMetrics {
    /// 记录意图误解
    pub fn record_intent_misunderstanding(&self) {
        self.intent_misunderstandings.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录工具误用
    pub fn record_tool_misuse(&self) {
        self.tool_misuses.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录路径错误
    pub fn record_path_error(&self) {
        self.path_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录输出不当
    pub fn record_output_issue(&self) {
        self.output_issues.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录用户纠正
    pub fn record_user_correction(&self) {
        self.user_corrections.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录任务完成情况
    pub fn record_task(&self, completed_first_try: bool) {
        self.tasks_total.fetch_add(1, Ordering::Relaxed);
        if completed_first_try {
            self.tasks_completed_first_try.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 获取总错误数
    pub fn total_errors(&self) -> u64 {
        self.intent_misunderstandings.load(Ordering::Relaxed)
            + self.tool_misuses.load(Ordering::Relaxed)
            + self.path_errors.load(Ordering::Relaxed)
            + self.output_issues.load(Ordering::Relaxed)
    }

    /// 计算任务完成率
    pub fn completion_rate(&self) -> f64 {
        let total = self.tasks_total.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            self.tasks_completed_first_try.load(Ordering::Relaxed) as f64 / total as f64
        }
    }

    /// 计算错误率（相对于总任务数）
    pub fn error_rate(&self) -> f64 {
        let total = self.tasks_total.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            self.total_errors() as f64 / total as f64
        }
    }
}

/// Span 计时器（RAII 风格）
pub struct SpanTimer {
    name: &'static str,
    start: Instant,
    span: tracing::Span,
}

impl SpanTimer {
    /// 创建新的 span 计时器
    pub fn new(name: &'static str) -> Self {
        let span = tracing::info_span!(target: "bee::timing", "operation", name = name);
        {
            let _guard = span.enter();
            tracing::debug!(target: "bee::timing", "Starting {}", name);
        }
        Self {
            name,
            start: Instant::now(),
            span,
        }
    }

    /// 获取已经过的时间
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for SpanTimer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        let _guard = self.span.enter();
        tracing::debug!(
            target: "bee::timing",
            name = self.name,
            elapsed_ms = elapsed.as_millis() as u64,
            "Completed"
        );
    }
}

/// 用于记录 LLM 调用的宏
#[macro_export]
macro_rules! record_llm_call {
    ($metrics:expr, $success:expr, $latency:expr, $prompt:expr, $completion:expr) => {
        $metrics.llm.record_call($success, $latency, $prompt, $completion);
    };
}

/// 用于记录工具执行的宏
#[macro_export]
macro_rules! record_tool_execution {
    ($metrics:expr, $success:expr, $duration:expr) => {
        $metrics.tools.record_execution($success, $duration);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_metrics() {
        let metrics = LlmMetrics::default();
        metrics.record_call(true, Duration::from_millis(100), 50, 25);
        metrics.record_call(false, Duration::from_millis(200), 30, 0);

        assert_eq!(metrics.total_calls.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.successful_calls.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.failed_calls.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.error_rate(), 0.5);
    }

    #[test]
    fn test_tool_metrics() {
        let metrics = ToolMetrics::default();
        metrics.record_execution(true, Duration::from_millis(50));
        metrics.record_execution(true, Duration::from_millis(100));

        assert_eq!(metrics.total_executions.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.average_execution_time_ms(), 75.0);
    }

    #[test]
    fn test_session_metrics() {
        let metrics = SessionMetrics::default();
        metrics.record_request();
        metrics.increment_active_sessions();

        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.active_sessions.load(Ordering::Relaxed), 1);

        metrics.decrement_active_sessions();
        assert_eq!(metrics.active_sessions.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_metrics_to_json() {
        let metrics = Metrics::new();
        metrics.llm.record_call(true, Duration::from_millis(100), 50, 25);
        
        let json = metrics.to_json();
        assert!(json["llm"]["total_calls"].as_u64().unwrap() == 1);
    }

    #[test]
    fn test_span_timer() {
        let timer = SpanTimer::new("test_operation");
        std::thread::sleep(Duration::from_millis(10));
        assert!(timer.elapsed() >= Duration::from_millis(10));
    }

    #[test]
    fn test_behavior_metrics() {
        let metrics = BehaviorMetrics::default();
        
        metrics.record_intent_misunderstanding();
        metrics.record_tool_misuse();
        metrics.record_path_error();
        metrics.record_output_issue();
        metrics.record_user_correction();

        assert_eq!(metrics.intent_misunderstandings.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.tool_misuses.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.path_errors.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.output_issues.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.user_corrections.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.total_errors(), 4);
    }

    #[test]
    fn test_behavior_metrics_completion_rate() {
        let metrics = BehaviorMetrics::default();
        
        metrics.record_task(true);
        metrics.record_task(true);
        metrics.record_task(false);
        metrics.record_task(true);

        assert_eq!(metrics.tasks_total.load(Ordering::Relaxed), 4);
        assert_eq!(metrics.tasks_completed_first_try.load(Ordering::Relaxed), 3);
        assert!((metrics.completion_rate() - 0.75).abs() < 0.001);
    }
}
