//! Headless Agent 运行时
//!
//! 供非 TUI 前端（如 WhatsApp、HTTP API）调用的无界面 Agent 逻辑：
//! create_agent_components 构建 Planner / ToolExecutor / Recovery，
//! create_context_with_long_term 构建带长期记忆的 ContextManager，
//! process_message 对单条用户输入跑 ReAct 并返回最终回复。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{load_config, AppConfig};
use crate::core::{AgentError, RecoveryEngine};
use crate::memory::{FileLongTerm, InMemoryLongTerm, long_term_path, memory_root};
use crate::react::{react_loop, ContextManager, Planner, ReactEvent};
use tokio::sync::mpsc;
use crate::tools::{
    CatTool, EchoTool, LsTool, SearchTool, ShellTool, ToolExecutor, ToolRegistry,
};

/// 预构建的 Agent 组件：Planner、ToolExecutor、Recovery，可多会话共享
pub struct AgentComponents {
    pub planner: Planner,
    pub executor: ToolExecutor,
    pub recovery: RecoveryEngine,
}

/// 创建 Agent 组件：从配置加载 LLM、工具（cat/ls/shell/search/echo）、超时，与 TUI 侧逻辑一致
pub fn create_agent_components(
    workspace: &PathBuf,
    system_prompt: &str,
) -> AgentComponents {
    let cfg = load_config(None).unwrap_or_else(|_| AppConfig::default());

    let llm = crate::core::orchestrator::create_llm_from_config(&cfg);

    let mut tools = ToolRegistry::new();
    tools.register(CatTool::new(workspace));
    tools.register(LsTool::new(workspace));
    tools.register(EchoTool);
    tools.register(ShellTool::new(
        cfg.tools.shell.allowed_commands.clone(),
        cfg.tools.tool_timeout_secs,
    ));
    tools.register(SearchTool::new(
        cfg.tools.search.allowed_domains.clone(),
        cfg.tools.search.timeout_secs,
        cfg.tools.search.max_result_chars,
    ));

    AgentComponents {
        planner: Planner::new(llm, system_prompt.to_string()),
        executor: ToolExecutor::new(tools, cfg.tools.tool_timeout_secs),
        recovery: RecoveryEngine::new(),
    }
}

/// 创建带长期记忆的 ContextManager。
/// 若 workspace 提供，则使用 Markdown 文件长期记忆（memory/long-term.md + BM25 检索）；
/// 否则使用内存实现（与 TUI 一致）。
pub fn create_context_with_long_term(max_turns: usize, workspace: Option<&Path>) -> ContextManager {
    let long_term: Arc<dyn crate::memory::LongTermMemory> = match workspace {
        Some(w) => {
            let root = memory_root(w);
            let path = long_term_path(&root);
            Arc::new(FileLongTerm::new(path, 2000))
        }
        None => Arc::new(InMemoryLongTerm::default()),
    };
    ContextManager::new(max_turns).with_long_term(long_term)
}

/// 处理单条用户消息：跑 ReAct 循环（无 stream），返回最终回复文本
pub async fn process_message(
    components: &AgentComponents,
    context: &mut ContextManager,
    user_input: &str,
) -> Result<String, AgentError> {
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let result = react_loop(
        &components.planner,
        &components.executor,
        &components.recovery,
        context,
        user_input,
        None,
        None,
        cancel_token,
    )
    .await?;
    Ok(result.response)
}

/// 流式处理单条用户消息：通过 event_tx 推送 Thinking / ToolCall / Observation / MessageChunk / MessageDone
pub async fn process_message_stream(
    components: &AgentComponents,
    context: &mut ContextManager,
    user_input: &str,
    event_tx: mpsc::UnboundedSender<ReactEvent>,
) -> Result<String, AgentError> {
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let result = react_loop(
        &components.planner,
        &components.executor,
        &components.recovery,
        context,
        user_input,
        None,
        Some(&event_tx),
        cancel_token,
    )
    .await?;
    Ok(result.response)
}
