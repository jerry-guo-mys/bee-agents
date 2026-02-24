//! Bee - Rust 个人智能体系统
//!
//! 模块划分：
//! - **agent**: 无头 Agent 运行时（供 WhatsApp / HTTP 等调用）
//! - **config**: 应用配置加载（TOML + 环境变量）
//! - **core**: 编排、状态、恢复、会话监管、任务调度
//! - **gateway**: 轮毂式网关架构（WebSocket 服务器 + Agent Runtime）
//! - **llm**: LLM 客户端抽象与实现（OpenAI 兼容 / DeepSeek / Mock）
//! - **memory**: 短期 / 中期 / 长期记忆与持久化
//! - **react**: Planner、Critic、ReAct 主循环
//! - **skills**: 技能系统（能力描述、模板、脚本）
//! - **tools**: 工具箱（cat、ls、shell、search、echo）与执行器
//! - **ui**: Ratatui TUI 界面

pub mod agent;
pub mod config;
pub mod core;
pub mod evolution;
#[cfg(feature = "gateway")]
pub mod gateway;
pub mod integrations;
pub mod llm;
pub mod memory;
pub mod observability;
pub mod plugins;
pub mod react;
pub mod skills;
pub mod tools;
pub mod workflow;
pub mod ui;

pub use evolution::{EvolutionLoop, EvolutionConfig};

#[cfg(test)]
mod integration_tests {
    //! 集成测试：完整 submit→react→response 流程（解决问题 8.1）

    use std::sync::Arc;

    use crate::core::RecoveryEngine;
    use crate::llm::MockLlmClient;
    use crate::memory::Message;
    use crate::react::{react_loop, ContextManager, Planner, ReactSession};
    use crate::tools::{EchoTool, ToolExecutor, ToolRegistry};

    /// 创建测试用的最小组件
    fn create_test_components() -> (Planner, ToolExecutor, RecoveryEngine) {
        let llm = Arc::new(MockLlmClient);
        let planner = Planner::new(llm, "You are a test assistant.".to_string());

        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);

        let executor = ToolExecutor::new(registry, 30);
        let recovery = RecoveryEngine::new();

        (planner, executor, recovery)
    }

    #[test]
    fn test_full_react_loop_with_tool_call() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (planner, executor, recovery) = create_test_components();
            let mut context = ContextManager::new(10);
            let cancel_token = tokio_util::sync::CancellationToken::new();

            let result = react_loop(
                &planner,
                &executor,
                &recovery,
                &mut context,
                "Hello, test",
                None,
                None,
                cancel_token,
                None,
                None,
                None,
                None,
            ).await;

            assert!(result.is_ok(), "React loop should complete successfully");
            let react_result = result.unwrap();

            // 验证消息历史非空
            assert!(!react_result.messages.is_empty(), "Should have at least one message");

            // 验证存在用户消息
            let has_user_msg = react_result.messages.iter().any(|m| m.role == crate::memory::Role::User);
            assert!(has_user_msg, "Should contain user message");

            // 验证存在助手回复
            let has_assistant_msg = react_result.messages.iter().any(|m| m.role == crate::memory::Role::Assistant);
            assert!(has_assistant_msg, "Should contain assistant message");
        });
    }

    #[test]
    fn test_react_session_v2() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (planner, executor, recovery) = create_test_components();
            let mut context = ContextManager::new(10);
            let cancel_token = tokio_util::sync::CancellationToken::new();

            let session = ReactSession::new(&planner, &executor, &recovery, cancel_token);

            let result = crate::react::react_loop_v2(&session, &mut context, "Test input").await;

            assert!(result.is_ok(), "React loop v2 should complete successfully");
        });
    }

    #[test]
    fn test_context_manager_message_flow() {
        let mut context = ContextManager::new(10);

        // 添加用户消息
        context.push_message(Message::user("What is 2+2?"));
        assert_eq!(context.messages().len(), 1);

        // 添加助手消息
        context.push_message(Message::assistant("2+2 equals 4."));
        assert_eq!(context.messages().len(), 2);

        // 验证消息内容
        let messages = context.to_llm_messages();
        assert_eq!(messages[0].content, "What is 2+2?");
        assert_eq!(messages[1].content, "2+2 equals 4.");
    }

    #[test]
    fn test_tool_execution() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut registry = ToolRegistry::new();
            registry.register(EchoTool);

            let executor = ToolExecutor::new(registry, 30);

            let result = executor.execute("echo", serde_json::json!({"text": "Hello"})).await;
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "Hello");
        });
    }

    #[test]
    fn test_tool_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let registry = ToolRegistry::new();
            let executor = ToolExecutor::new(registry, 30);

            let result = executor.execute("nonexistent", serde_json::json!({})).await;
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_working_memory_integration() {
        let mut context = ContextManager::new(10);

        // 设置工作记忆
        context.working.set_goal("Find and read a file");
        context.working.add_attempt("cat -> file content");
        context.working.add_failure("Permission denied");

        // 验证工作记忆段落生成
        let section = context.working_memory_section();
        assert!(section.contains("Find and read a file"));
        assert!(section.contains("cat -> file content"));
        assert!(section.contains("Permission denied"));
    }

    #[test]
    fn test_cancel_token_integration() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (planner, executor, recovery) = create_test_components();
            let mut context = ContextManager::new(10);
            let cancel_token = tokio_util::sync::CancellationToken::new();

            // 在调用前取消
            cancel_token.cancel();

            let result = react_loop(
                &planner,
                &executor,
                &recovery,
                &mut context,
                "This should be cancelled",
                None,
                None,
                cancel_token,
                None,
                None,
                None,
                None,
            ).await;

            // 应该返回取消错误
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(matches!(err, crate::core::AgentError::Cancelled));
        });
    }
}
