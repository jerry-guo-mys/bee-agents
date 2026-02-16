//! Agent 编排器：主控循环
//!
//! 负责：加载配置、创建 LLM/工具/Planner/Recovery、建立 cmd/state/stream 三通道，
//! 并在后台任务中消费用户命令（Submit/Cancel/Clear/Quit），驱动 ReAct 循环并更新 UI 状态。

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, watch};

use crate::config::{load_config, AppConfig};
use crate::core::{AgentPhase, RecoveryEngine, SessionSupervisor, TaskScheduler, UiState};
use crate::llm::{create_deepseek_client, LlmClient, OpenAiClient};
use crate::memory::{InMemoryLongTerm, SqlitePersistence};
use std::sync::Mutex;
use crate::react::{react_loop, ContextManager, Critic, Planner};
use crate::tools::{
    tool_call_schema_json, CatTool, EchoTool, LsTool, PluginTool, SearchTool, ShellTool,
    ToolExecutor, ToolRegistry,
};
#[cfg(feature = "browser")]
use crate::tools::BrowserTool;

/// 从 UI 发往编排器的用户命令
#[derive(Debug, Clone)]
pub enum Command {
    /// 提交用户输入，触发 ReAct 循环
    Submit(String),
    /// 取消当前生成（Stop generating）
    Cancel,
    /// 清空对话与 Working Memory
    Clear,
    /// 退出应用
    Quit,
}

/// 根据配置与环境变量选择 LLM 后端（DeepSeek / OpenAI 兼容 / Mock）
pub fn create_llm_from_config(cfg: &AppConfig) -> Arc<dyn LlmClient> {
    let provider = cfg.llm.provider.to_lowercase();
    // 有 DeepSeek Key 或（配置为 deepseek 且仅有 OpenAI Key 时也走 DeepSeek 兼容端点）
    let use_deepseek = std::env::var("DEEPSEEK_API_KEY").is_ok()
        || (provider == "deepseek" && std::env::var("OPENAI_API_KEY").is_ok());
    let use_openai = std::env::var("OPENAI_API_KEY").is_ok() && provider != "deepseek";

    if use_deepseek {
        let model = cfg
            .llm
            .deepseek
            .model
            .clone()
            .or_else(|| Some(cfg.llm.model.clone()))
            .unwrap_or_else(|| "deepseek-chat".to_string());
        tracing::info!("Using DeepSeek LLM ({})", model);
        Arc::new(create_deepseek_client(Some(&model)))
    } else if use_openai {
        let model = cfg
            .llm
            .openai
            .model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());
        let base = cfg.llm.base_url.as_deref();
        tracing::info!("Using OpenAI LLM ({})", model);
        Arc::new(OpenAiClient::new(
            base,
            &model,
            std::env::var("OPENAI_API_KEY").ok().as_deref(),
        ))
    } else {
        tracing::warn!("No API key set or provider unknown, using Mock LLM");
        Arc::new(crate::llm::MockLlmClient)
    }
}

/// 创建 Agent 运行时：返回命令发送端、状态接收端、流接收端；后台任务消费命令并更新 state/stream。
pub async fn create_agent(
    config_path: Option<PathBuf>,
) -> anyhow::Result<(
    mpsc::UnboundedSender<Command>,
    watch::Receiver<UiState>,
    broadcast::Receiver<String>,
)> {
    let cfg = load_config(config_path.clone()).unwrap_or_else(|e| {
        tracing::warn!("Config load failed ({}), using defaults", e);
        AppConfig::default()
    });

    // 工作目录：配置 > 当前目录下的 workspace
    let workspace = cfg
        .app
        .workspace_root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap().join("workspace"));
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.clone());
    std::fs::create_dir_all(&workspace).ok();

    let system_prompt = [
        "config/prompts/system.txt",
        "../config/prompts/system.txt",
    ]
    .into_iter()
    .find_map(|p| std::fs::read_to_string(p).ok())
    .unwrap_or_else(|| {
        "You are Bee, a helpful AI assistant. Use tools: cat, ls, echo, shell, search.".to_string()
    });

    let llm = create_llm_from_config(&cfg);

    let mut tools = ToolRegistry::new();
    tools.register(CatTool::new(&workspace));
    tools.register(LsTool::new(&workspace));
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
        tools.register(PluginTool::new(entry, &workspace, cfg.tools.tool_timeout_secs));
    }

    let executor = ToolExecutor::new(tools, cfg.tools.tool_timeout_secs);
    let tool_schema = tool_call_schema_json();
    let full_system_prompt = if tool_schema.is_empty() {
        system_prompt.clone()
    } else {
        format!(
            "{}\n\n## Tool call JSON Schema (you must output valid JSON matching this)\n```json\n{}\n```",
            system_prompt, tool_schema
        )
    };
    let planner = Planner::new(llm.clone(), full_system_prompt);
    let critic_prompt = [
        "config/prompts/critic.txt",
        "../config/prompts/critic.txt",
    ]
    .into_iter()
    .find_map(|p| std::fs::read_to_string(p).ok())
    .unwrap_or_else(|| {
        "The user wanted: {goal}\nYou executed tool: {tool} with result: {observation}\nIs this result reasonable? If yes, respond with exactly: OK\nIf not, provide a brief correction (one sentence).".to_string()
    });
    let critic = Critic::new(llm.clone(), critic_prompt);
    let recovery = RecoveryEngine::new();
    let task_scheduler = TaskScheduler::default();
    let supervisor = SessionSupervisor::new();

    // 三通道：UI -> Core 命令；Core -> UI 状态快照；Core -> UI Token 流
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Command>();
    let (state_tx, state_rx) = watch::channel(UiState::default());
    let (stream_tx, stream_rx) = broadcast::channel::<String>(16);

    let long_term = Arc::new(InMemoryLongTerm::default());
    let mut context = ContextManager::new(cfg.app.max_context_turns).with_long_term(long_term);

    // 初始化 SQLite 持久化
    let sqlite_db_path = workspace.join(".bee/conversations.db");
    if let Some(parent) = sqlite_db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let sqlite_persistence = Arc::new(Mutex::new(
        SqlitePersistence::new(&sqlite_db_path).ok()
    ));

    // 生成 session_id
    let session_id = uuid::Uuid::new_v4().to_string();
    if let Ok(persistence) = sqlite_persistence.lock() {
        if let Some(ref p) = *persistence {
            let _ = p.create_session(&session_id, Some("New Conversation"));
            // 尝试加载之前的消息
            if let Ok(messages) = p.load_messages(&session_id) {
                for msg in messages {
                    context.conversation.push(msg);
                }
            }
        }
    }

    let sqlite_persistence_clone = sqlite_persistence.clone();
    let session_id_clone = session_id.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        Command::Submit(input) => {
                            // 保存用户消息到 SQLite
                            if let Ok(persistence) = sqlite_persistence_clone.lock() {
                                if let Some(ref p) = *persistence {
                                    let _ = p.save_message(&session_id_clone, &crate::memory::Message {
                                        role: crate::memory::Role::User,
                                        content: input.clone(),
                                    });
                                }
                            }

                            // 先更新为 Thinking，再跑 ReAct
                            let _ = state_tx.send(UiState {
                                phase: AgentPhase::Thinking,
                                history: context.conversation.messages().to_vec(),
                                active_tool: None,
                                input_locked: true,
                                error_message: None,
                            });

                            let result = react_loop(
                                &planner,
                                &executor,
                                &recovery,
                                &mut context,
                                &input,
                                Some(&stream_tx),
                                None,
                                supervisor.cancel_token(),
                                Some(&critic),
                                Some(&task_scheduler),
                            ).await;

                            match result {
                                Ok(react_result) => {
                                    // 保存助手消息到 SQLite
                                    if let Some(last_msg) = react_result.messages.last() {
                                        if last_msg.role == crate::memory::Role::Assistant {
                                            if let Ok(persistence) = sqlite_persistence_clone.lock() {
                                                if let Some(ref p) = *persistence {
                                                    let _ = p.save_message(&session_id_clone, last_msg);
                                                }
                                            }
                                        }
                                    }

                                    let _ = state_tx.send(UiState {
                                        phase: AgentPhase::Idle,
                                        history: react_result.messages,
                                        active_tool: None,
                                        input_locked: false,
                                        error_message: None,
                                    });
                                }
                                Err(e) => {
                                    let _ = state_tx.send(UiState {
                                        phase: AgentPhase::Error,
                                        history: context.conversation.messages().to_vec(),
                                        active_tool: None,
                                        input_locked: false,
                                        error_message: Some(e.to_string()),
                                    });
                                }
                            }
                        }
                        Command::Cancel => {
                            supervisor.cancel(); // 触发 ReAct 中的 cancel_token
                        }
                        Command::Clear => {
                            // 清空对话与 Working Memory，长期记忆保留
                            context.conversation.clear();
                            context.working.clear();
                            let _ = state_tx.send(UiState {
                                phase: AgentPhase::Idle,
                                history: vec![],
                                active_tool: None,
                                input_locked: false,
                                error_message: None,
                            });
                        }
                        Command::Quit => break,
                    }
                }
                else => break,  // cmd_tx 已关闭，退出循环
            }
        }
    });

    Ok((cmd_tx, state_rx, stream_rx))
}
