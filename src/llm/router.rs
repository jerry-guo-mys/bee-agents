//! 多模型路由器（Phase 4 长期演进）
//!
//! 根据任务类型自动选择最合适的模型：
//! - 简单问答：使用快速轻量模型
//! - 代码生成：使用专门的代码模型
//! - 复杂推理：使用高能力模型
//! - 成本优化：根据预算选择模型

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::{LlmClient, LlmError};
use crate::memory::Message;

/// 任务类型（用于路由决策）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskType {
    /// 简单问答/闲聊
    SimpleChat,
    /// 代码生成/编辑
    CodeGeneration,
    /// 复杂推理/分析
    ComplexReasoning,
    /// 工具调用决策
    ToolDecision,
    /// 摘要/压缩
    Summarization,
    /// 默认/未知
    Default,
}

/// 模型能力评级
#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    /// 模型名称
    pub name: String,
    /// 代码能力（0-100）
    pub code_score: u8,
    /// 推理能力（0-100）
    pub reasoning_score: u8,
    /// 速度评分（0-100，越高越快）
    pub speed_score: u8,
    /// 成本评分（0-100，越高越便宜）
    pub cost_score: u8,
    /// 是否支持流式输出
    pub supports_streaming: bool,
}

impl ModelCapabilities {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            code_score: 50,
            reasoning_score: 50,
            speed_score: 50,
            cost_score: 50,
            supports_streaming: true,
        }
    }

    pub fn with_code(mut self, score: u8) -> Self {
        self.code_score = score;
        self
    }

    pub fn with_reasoning(mut self, score: u8) -> Self {
        self.reasoning_score = score;
        self
    }

    pub fn with_speed(mut self, score: u8) -> Self {
        self.speed_score = score;
        self
    }

    pub fn with_cost(mut self, score: u8) -> Self {
        self.cost_score = score;
        self
    }
}

/// 路由策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// 最佳质量（选择能力最强的模型）
    BestQuality,
    /// 最快速度（选择最快的模型）
    Fastest,
    /// 最低成本（选择最便宜的模型）
    LowestCost,
    /// 平衡（综合考虑各因素）
    Balanced,
    /// 指定模型（不进行路由）
    Fixed(usize),
}

/// 任务类型检测器
pub struct TaskClassifier;

impl TaskClassifier {
    /// 根据消息内容推断任务类型
    pub fn classify(messages: &[Message]) -> TaskType {
        let last_user_msg = messages
            .iter()
            .rev()
            .find(|m| m.role == crate::memory::Role::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let content_lower = last_user_msg.to_lowercase();

        // 代码相关关键词
        if Self::contains_code_keywords(&content_lower) {
            return TaskType::CodeGeneration;
        }

        // 推理/分析关键词
        if Self::contains_reasoning_keywords(&content_lower) {
            return TaskType::ComplexReasoning;
        }

        // 摘要关键词
        if Self::contains_summary_keywords(&content_lower) {
            return TaskType::Summarization;
        }

        // 工具相关（消息历史中有工具调用）
        if messages.iter().any(|m| m.role == crate::memory::Role::Tool) {
            return TaskType::ToolDecision;
        }

        // 短消息倾向于简单问答
        if last_user_msg.len() < 100 {
            return TaskType::SimpleChat;
        }

        TaskType::Default
    }

    fn contains_code_keywords(content: &str) -> bool {
        let keywords = [
            "代码", "编程", "函数", "代码", "bug", "error", "compile",
            "rust", "python", "javascript", "typescript", "java", "go",
            "implement", "fix", "refactor", "debug", "写个", "写一个",
            "function", "class", "struct", "enum", "trait", "impl",
        ];
        keywords.iter().any(|k| content.contains(k))
    }

    fn contains_reasoning_keywords(content: &str) -> bool {
        let keywords = [
            "分析", "解释", "为什么", "怎么", "如何", "推理",
            "analyze", "explain", "why", "how", "reason", "think",
            "compare", "evaluate", "assess", "比较", "评估",
        ];
        keywords.iter().any(|k| content.contains(k))
    }

    fn contains_summary_keywords(content: &str) -> bool {
        let keywords = [
            "总结", "摘要", "概括", "简述",
            "summarize", "summary", "tldr", "brief",
        ];
        keywords.iter().any(|k| content.contains(k))
    }
}

/// 多模型路由器
pub struct ModelRouter {
    /// 可用模型及其客户端
    models: Vec<(ModelCapabilities, Arc<dyn LlmClient>)>,
    /// 任务类型到模型索引的映射
    task_routes: HashMap<TaskType, usize>,
    /// 默认路由策略
    default_strategy: RoutingStrategy,
    /// 调用统计（模型索引 -> 调用次数）
    call_counts: std::sync::atomic::AtomicUsize,
}

impl ModelRouter {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            task_routes: HashMap::new(),
            default_strategy: RoutingStrategy::Balanced,
            call_counts: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// 添加模型
    pub fn add_model(&mut self, capabilities: ModelCapabilities, client: Arc<dyn LlmClient>) {
        self.models.push((capabilities, client));
    }

    /// 设置任务类型的固定路由
    pub fn set_task_route(&mut self, task: TaskType, model_index: usize) {
        self.task_routes.insert(task, model_index);
    }

    /// 设置默认路由策略
    pub fn set_default_strategy(&mut self, strategy: RoutingStrategy) {
        self.default_strategy = strategy;
    }

    /// 根据任务类型选择模型
    pub fn select_model(&self, task_type: TaskType) -> Option<&Arc<dyn LlmClient>> {
        // 检查是否有固定路由
        if let Some(&index) = self.task_routes.get(&task_type) {
            return self.models.get(index).map(|(_, client)| client);
        }

        // 根据策略选择
        let index = match self.default_strategy {
            RoutingStrategy::BestQuality => self.select_best_quality(task_type),
            RoutingStrategy::Fastest => self.select_fastest(),
            RoutingStrategy::LowestCost => self.select_lowest_cost(),
            RoutingStrategy::Balanced => self.select_balanced(task_type),
            RoutingStrategy::Fixed(idx) => Some(idx),
        };

        index.and_then(|i| self.models.get(i).map(|(_, client)| client))
    }

    fn select_best_quality(&self, task_type: TaskType) -> Option<usize> {
        self.models
            .iter()
            .enumerate()
            .max_by_key(|(_, (cap, _))| {
                match task_type {
                    TaskType::CodeGeneration => cap.code_score,
                    TaskType::ComplexReasoning => cap.reasoning_score,
                    _ => (cap.code_score + cap.reasoning_score) / 2,
                }
            })
            .map(|(i, _)| i)
    }

    fn select_fastest(&self) -> Option<usize> {
        self.models
            .iter()
            .enumerate()
            .max_by_key(|(_, (cap, _))| cap.speed_score)
            .map(|(i, _)| i)
    }

    fn select_lowest_cost(&self) -> Option<usize> {
        self.models
            .iter()
            .enumerate()
            .max_by_key(|(_, (cap, _))| cap.cost_score)
            .map(|(i, _)| i)
    }

    fn select_balanced(&self, task_type: TaskType) -> Option<usize> {
        self.models
            .iter()
            .enumerate()
            .max_by_key(|(_, (cap, _))| {
                let quality = match task_type {
                    TaskType::CodeGeneration => cap.code_score as u16,
                    TaskType::ComplexReasoning => cap.reasoning_score as u16,
                    _ => ((cap.code_score as u16) + (cap.reasoning_score as u16)) / 2,
                };
                // 平衡质量、速度和成本
                quality + (cap.speed_score as u16) / 2 + (cap.cost_score as u16) / 2
            })
            .map(|(i, _)| i)
    }

    /// 获取模型数量
    pub fn model_count(&self) -> usize {
        self.models.len()
    }

    /// 获取调用统计
    pub fn call_count(&self) -> usize {
        self.call_counts.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// 路由型 LLM 客户端
pub struct RoutingLlmClient {
    router: ModelRouter,
}

impl RoutingLlmClient {
    pub fn new(router: ModelRouter) -> Self {
        Self { router }
    }

    pub fn router(&self) -> &ModelRouter {
        &self.router
    }
}

#[async_trait]
impl LlmClient for RoutingLlmClient {
    async fn complete(&self, messages: &[Message]) -> Result<String, LlmError> {
        let task_type = TaskClassifier::classify(messages);
        
        let client = self
            .router
            .select_model(task_type)
            .ok_or_else(|| LlmError::ApiError("No model available".to_string()))?;
        
        self.router
            .call_counts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        
        client.complete(messages).await
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
    ) -> Result<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<String, LlmError>> + Send>>,
        LlmError,
    > {
        let task_type = TaskClassifier::classify(messages);
        
        let client = self
            .router
            .select_model(task_type)
            .ok_or_else(|| LlmError::ApiError("No model available".to_string()))?;
        
        self.router
            .call_counts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        
        client.complete_stream(messages).await
    }

    fn token_usage(&self) -> (u64, u64, u64) {
        // 聚合所有模型的 token 使用
        self.router
            .models
            .iter()
            .map(|(_, client)| client.token_usage())
            .fold((0, 0, 0), |acc, (a, b, c)| (acc.0 + a, acc.1 + b, acc.2 + c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmClient, MockLlmClient};
    use crate::memory::Role;

    #[test]
    fn test_task_classifier_code() {
        let messages = vec![Message::user("请帮我写一个 Rust 函数来排序数组")];
        let task_type = TaskClassifier::classify(&messages);
        assert_eq!(task_type, TaskType::CodeGeneration);
    }

    #[test]
    fn test_task_classifier_reasoning() {
        let messages = vec![Message::user("分析一下这个算法的时间复杂度")];
        let task_type = TaskClassifier::classify(&messages);
        assert_eq!(task_type, TaskType::ComplexReasoning);
    }

    #[test]
    fn test_task_classifier_summary() {
        let messages = vec![Message::user("总结一下这篇文章的要点")];
        let task_type = TaskClassifier::classify(&messages);
        assert_eq!(task_type, TaskType::Summarization);
    }

    #[test]
    fn test_task_classifier_simple() {
        let messages = vec![Message::user("你好")];
        let task_type = TaskClassifier::classify(&messages);
        assert_eq!(task_type, TaskType::SimpleChat);
    }

    #[test]
    fn test_model_router_selection() {
        let mut router = ModelRouter::new();
        
        let fast_model: Arc<dyn LlmClient> = Arc::new(MockLlmClient);
        let smart_model: Arc<dyn LlmClient> = Arc::new(MockLlmClient);
        
        router.add_model(
            ModelCapabilities::new("fast")
                .with_speed(90)
                .with_cost(80)
                .with_code(50)
                .with_reasoning(50),
            fast_model,
        );
        
        router.add_model(
            ModelCapabilities::new("smart")
                .with_speed(40)
                .with_cost(30)
                .with_code(90)
                .with_reasoning(95),
            smart_model,
        );
        
        // 测试不同策略
        router.set_default_strategy(RoutingStrategy::Fastest);
        assert!(router.select_model(TaskType::Default).is_some());
        
        router.set_default_strategy(RoutingStrategy::BestQuality);
        assert!(router.select_model(TaskType::CodeGeneration).is_some());
    }

    #[test]
    fn test_model_capabilities_builder() {
        let cap = ModelCapabilities::new("test")
            .with_code(80)
            .with_reasoning(90)
            .with_speed(70)
            .with_cost(60);
        
        assert_eq!(cap.name, "test");
        assert_eq!(cap.code_score, 80);
        assert_eq!(cap.reasoning_score, 90);
        assert_eq!(cap.speed_score, 70);
        assert_eq!(cap.cost_score, 60);
    }

    #[test]
    fn test_task_with_tool_history() {
        let messages = vec![
            Message::user("执行命令"),
            Message {
                role: Role::Tool,
                content: "执行结果".to_string(),
            },
            Message::user("继续"),
        ];
        let task_type = TaskClassifier::classify(&messages);
        assert_eq!(task_type, TaskType::ToolDecision);
    }
}
