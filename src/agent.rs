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
use crate::llm::create_embedder_from_config;
use crate::memory::{
    ConsolidateResult, FileLongTerm, InMemoryLongTerm, InMemoryVectorLongTerm, list_daily_logs_for_llm,
    lessons_path, long_term_path, memory_root, preferences_path, procedural_path,
    vector_snapshot_path, LongTermMemory, Message,
};
use crate::core::TaskScheduler;
use crate::react::{react_loop, ContextManager, Critic, Planner, ReactEvent};
use tokio::sync::mpsc;
use crate::tools::{
    tool_call_schema_json, CatTool, EchoTool, LsTool, PluginTool, SearchTool, ShellTool,
    ToolExecutor, ToolRegistry,
};
#[cfg(feature = "browser")]
use crate::tools::BrowserTool;

/// 预构建的 Agent 组件：Planner、ToolExecutor、Recovery、Critic、TaskScheduler，可多会话共享
pub struct AgentComponents {
    pub planner: Planner,
    pub executor: ToolExecutor,
    pub recovery: RecoveryEngine,
    /// 可选：工具结果反思与校验，接入 ReAct 循环
    pub critic: Option<Critic>,
    /// 工具并发限制（如最多 3 个），接入 ReAct 循环
    pub task_scheduler: TaskScheduler,
}

/// 创建 Agent 组件：从配置加载 LLM、工具（cat/ls/shell/search/echo）、超时，与 TUI 侧逻辑一致
pub fn create_agent_components(
    workspace: &PathBuf,
    system_prompt: &str,
) -> AgentComponents {
    let cfg = load_config(None).unwrap_or_else(|_| AppConfig::default());

    let llm = crate::core::orchestrator::create_llm_from_config(&cfg);

    let critic_prompt = [
        "config/prompts/critic.txt",
        "../config/prompts/critic.txt",
    ]
    .into_iter()
    .find_map(|p| std::fs::read_to_string(p).ok())
    .unwrap_or_else(|| {
        "The user wanted: {goal}\nYou executed tool: {tool} with result: {observation}\nIs this result reasonable? If yes, respond with exactly: OK\nIf not, provide a brief correction (one sentence).".to_string()
    });
    let critic = Some(Critic::new(llm.clone(), critic_prompt));

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

    #[cfg(feature = "browser")]
    tools.register(BrowserTool::new(
        cfg.tools.search.allowed_domains.clone(),
        cfg.tools.search.max_result_chars,
    ));

    for entry in &cfg.tools.plugins {
        tools.register(PluginTool::new(entry, workspace, cfg.tools.tool_timeout_secs));
    }

    let tool_schema = tool_call_schema_json();
    let full_system_prompt = if tool_schema.is_empty() {
        system_prompt.to_string()
    } else {
        format!(
            "{}\n\n## Tool call JSON Schema (you must output valid JSON matching this)\n```json\n{}\n```",
            system_prompt, tool_schema
        )
    };
    AgentComponents {
        planner: Planner::new(llm.clone(), full_system_prompt),
        executor: ToolExecutor::new(tools, cfg.tools.tool_timeout_secs),
        recovery: RecoveryEngine::new(),
        critic,
        task_scheduler: TaskScheduler::default(),
    }
}

/// 创建可共享的向量长期记忆（带快照路径，启动时加载、可定期 save_snapshot）；未启用或无法创建 embedder 时返回 None
pub fn create_shared_vector_long_term(
    workspace: &Path,
    cfg: &AppConfig,
) -> Option<Arc<InMemoryVectorLongTerm>> {
    if !cfg.memory.vector_enabled {
        return None;
    }
    let api_key = cfg
        .memory
        .embedding_api_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());
    let embedder = create_embedder_from_config(
        cfg.memory.embedding_base_url.as_deref().or(cfg.llm.base_url.as_deref()),
        &cfg.memory.embedding_model,
        api_key.as_deref(),
    )?;
    let root = memory_root(workspace);
    let snapshot_path = vector_snapshot_path(&root);
    Some(Arc::new(InMemoryVectorLongTerm::new_with_persistence(
        embedder,
        2000,
        Some(snapshot_path),
    )))
}

/// 创建带长期记忆的 ContextManager。
/// 若 workspace 提供，则使用 Markdown 文件长期记忆（memory/long-term.md + BM25 检索），
/// 或当 [memory].vector_enabled 时使用嵌入 API + 内存向量检索（可传入 shared_vector 以共享并持久化）；否则使用 InMemoryLongTerm。
pub fn create_context_with_long_term(
    max_turns: usize,
    workspace: Option<&Path>,
    shared_vector_long_term: Option<Arc<InMemoryVectorLongTerm>>,
) -> ContextManager {
    let cfg = load_config(None).unwrap_or_else(|_| AppConfig::default());
    let (long_term, lessons_path_opt, procedural_path_opt, preferences_path_opt): (
        Arc<dyn crate::memory::LongTermMemory>,
        Option<std::path::PathBuf>,
        Option<std::path::PathBuf>,
        Option<std::path::PathBuf>,
    ) = match workspace {
        Some(w) => {
            let root = memory_root(w);
            let lessons = Some(lessons_path(&root));
            let procedural = Some(procedural_path(&root));
            let preferences = Some(preferences_path(&root));
            let lt: Arc<dyn crate::memory::LongTermMemory> = if cfg.memory.vector_enabled {
                if let Some(shared) = shared_vector_long_term {
                    tracing::info!("long-term memory: vector (shared with snapshot)");
                    shared
                } else {
                    let api_key = cfg
                        .memory
                        .embedding_api_key
                        .clone()
                        .or_else(|| std::env::var("OPENAI_API_KEY").ok());
                    if let Some(embedder) = create_embedder_from_config(
                        cfg.memory.embedding_base_url.as_deref().or(cfg.llm.base_url.as_deref()),
                        &cfg.memory.embedding_model,
                        api_key.as_deref(),
                    ) {
                        tracing::info!("long-term memory: vector (embedding model {})", cfg.memory.embedding_model);
                        let snapshot = vector_snapshot_path(&root);
                        Arc::new(InMemoryVectorLongTerm::new_with_persistence(
                            embedder,
                            2000,
                            Some(snapshot),
                        ))
                    } else {
                        let path = long_term_path(&root);
                        Arc::new(FileLongTerm::new(path, 2000))
                    }
                }
            } else {
                let path = long_term_path(&root);
                Arc::new(FileLongTerm::new(path, 2000))
            };
            (lt, lessons, procedural, preferences)
        }
        None => (Arc::new(InMemoryLongTerm::default()), None, None, None),
    };
    let mut ctx = ContextManager::new(max_turns)
        .with_long_term(long_term)
        .with_auto_lesson_on_hallucination(cfg.evolution.auto_lesson_on_hallucination)
        .with_record_tool_success(cfg.evolution.record_tool_success);
    if let Some(p) = lessons_path_opt {
        ctx = ctx.with_lessons_path(p);
    }
    if let Some(p) = procedural_path_opt {
        ctx = ctx.with_procedural_path(p);
    }
    if let Some(p) = preferences_path_opt {
        ctx = ctx.with_preferences_path(p);
    }
    ctx
}

/// 用 LLM 对近期每日日志做摘要后写入长期记忆（EVOLUTION §3.3 整理与摘要的智能化）
pub async fn consolidate_memory_with_llm(
    planner: &Planner,
    workspace: &Path,
    since_days: u32,
) -> Result<ConsolidateResult, AgentError> {
    let root = memory_root(workspace);
    let list = list_daily_logs_for_llm(&root, since_days)
        .map_err(|e| AgentError::ConfigError(e.to_string()))?;
    if list.is_empty() {
        return Ok(ConsolidateResult::default());
    }
    let path = long_term_path(&root);
    let lt = FileLongTerm::new(path, 2000);
    let mut dates_processed = Vec::new();
    for (date, content) in list {
        let prompt = format!(
            "Summarize the following daily log in one short paragraph: key facts, decisions, user preferences. Use the same language as the log. Output only the summary, no preamble.\n\n{}",
            content
        );
        let summary = planner
            .summarize(&[Message::user(prompt)])
            .await
            .unwrap_or_else(|_| content.chars().take(500).collect::<String>());
        if summary.is_empty() {
            continue;
        }
        lt.add(&format!("整理 {}（LLM 摘要）：\n\n{}", date, summary));
        dates_processed.push(date);
    }
    let blocks_added = dates_processed.len();
    Ok(ConsolidateResult {
        dates_processed,
        blocks_added,
    })
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
        components.critic.as_ref(),
        Some(&components.task_scheduler),
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
        components.critic.as_ref(),
        Some(&components.task_scheduler),
    )
    .await?;
    Ok(result.response)
}
