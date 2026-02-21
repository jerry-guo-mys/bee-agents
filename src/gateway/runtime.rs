//! Agent Runtime（代理运行时）
//!
//! 实际的 AI 处理逻辑，与 Gateway 解耦

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

use super::message::{GatewayMessage, MessageType, SessionStatus};
use super::session::SessionManager;
use crate::agent::create_agent_components;
use crate::config::AppConfig;
use crate::core::{AgentComponents, AgentError};
use crate::react::{react_loop, ReactEvent};
use crate::skills::SkillSelector;

/// Runtime 配置
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// 应用配置
    pub app_config: AppConfig,
    /// 工作目录
    pub workspace: PathBuf,
    /// 系统提示词
    pub system_prompt: String,
    /// 最大并发请求数
    pub max_concurrent: usize,
    /// 启用技能选择
    pub enable_skills: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            app_config: AppConfig::default(),
            workspace: PathBuf::from("."),
            system_prompt: "You are a helpful AI assistant.".to_string(),
            max_concurrent: 10,
            enable_skills: true,
        }
    }
}

/// Agent Runtime - AI 处理核心
pub struct AgentRuntime {
    config: RuntimeConfig,
    components: AgentComponents,
    session_manager: Arc<SessionManager>,
}

impl AgentRuntime {
    pub fn new(config: RuntimeConfig, session_manager: Arc<SessionManager>) -> Self {
        let components = create_agent_components(&config.app_config, &config.workspace);
        Self {
            config,
            components,
            session_manager,
        }
    }

    /// 获取 Agent 组件（用于共享 LLM 等）
    pub fn components(&self) -> &AgentComponents {
        &self.components
    }

    /// 处理用户消息
    pub async fn process_message(
        &self,
        session_id: &str,
        user_input: &str,
        assistant_id: Option<&str>,
        model: Option<&str>,
        response_tx: mpsc::UnboundedSender<GatewayMessage>,
    ) -> Result<String, AgentError> {
        let request_id = uuid::Uuid::new_v4().to_string();

        self.session_manager
            .with_session(session_id, |s| {
                s.set_status(SessionStatus::Processing);
            })
            .await;

        response_tx
            .send(GatewayMessage::new(
                Some(session_id.to_string()),
                MessageType::ResponseStart {
                    request_id: request_id.clone(),
                },
            ))
            .ok();

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ReactEvent>();

        let response_tx_clone = response_tx.clone();
        let request_id_clone = request_id.clone();
        let session_id_owned = session_id.to_string();

        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let msg = match event {
                    ReactEvent::Thinking => continue,
                    ReactEvent::ThinkingContent { text } => GatewayMessage::new(
                        Some(session_id_owned.clone()),
                        MessageType::Thinking {
                            request_id: request_id_clone.clone(),
                            content: text,
                        },
                    ),
                    ReactEvent::ToolCall { tool, args } => GatewayMessage::new(
                        Some(session_id_owned.clone()),
                        MessageType::ToolCall {
                            request_id: request_id_clone.clone(),
                            tool_name: tool,
                            arguments: args,
                        },
                    ),
                    ReactEvent::Observation { tool, preview } => GatewayMessage::new(
                        Some(session_id_owned.clone()),
                        MessageType::ToolResult {
                            request_id: request_id_clone.clone(),
                            tool_name: tool,
                            result: preview,
                            success: true,
                        },
                    ),
                    ReactEvent::MessageChunk { text } => GatewayMessage::new(
                        Some(session_id_owned.clone()),
                        MessageType::ResponseChunk {
                            request_id: request_id_clone.clone(),
                            content: text,
                        },
                    ),
                    ReactEvent::MessageDone => continue,
                    ReactEvent::ToolFailure { tool, reason } => GatewayMessage::new(
                        Some(session_id_owned.clone()),
                        MessageType::ToolResult {
                            request_id: request_id_clone.clone(),
                            tool_name: tool,
                            result: reason,
                            success: false,
                        },
                    ),
                    ReactEvent::Error { text } => GatewayMessage::new(
                        Some(session_id_owned.clone()),
                        MessageType::Error {
                            request_id: Some(request_id_clone.clone()),
                            code: "react_error".to_string(),
                            message: text,
                        },
                    ),
                    _ => continue,
                };
                if response_tx_clone.send(msg).is_err() {
                    break;
                }
            }
        });

        let result = self
            .run_react_loop(session_id, user_input, event_tx, assistant_id, model)
            .await;

        self.session_manager
            .with_session(session_id, |s| {
                s.set_status(SessionStatus::Idle);
            })
            .await;

        match &result {
            Ok(response) => {
                response_tx
                    .send(GatewayMessage::new(
                        Some(session_id.to_string()),
                        MessageType::ResponseEnd {
                            request_id,
                            full_content: response.clone(),
                        },
                    ))
                    .ok();
            }
            Err(e) => {
                response_tx
                    .send(GatewayMessage::new(
                        Some(session_id.to_string()),
                        MessageType::Error {
                            request_id: Some(request_id),
                            code: "runtime_error".to_string(),
                            message: e.to_string(),
                        },
                    ))
                    .ok();
            }
        }

        result
    }

    async fn run_react_loop(
        &self,
        session_id: &str,
        user_input: &str,
        event_tx: mpsc::UnboundedSender<ReactEvent>,
        _assistant_id: Option<&str>,
        _model: Option<&str>,
    ) -> Result<String, AgentError> {
        let cancel_token = self
            .session_manager
            .with_session(session_id, |s| s.new_cancel_token())
            .await
            .unwrap_or_else(tokio_util::sync::CancellationToken::new);

        let mut context = self
            .session_manager
            .with_session(session_id, |s| {
                std::mem::replace(&mut s.context, crate::react::ContextManager::new(20))
            })
            .await
            .unwrap_or_else(|| crate::react::ContextManager::new(20));

        let system_prompt = if self.config.enable_skills {
            let selector = SkillSelector::new(
                Arc::clone(&self.components.skill_cache),
                Arc::clone(&self.components.llm),
            );
            let skills = selector.select(user_input).await;
            if skills.is_empty() {
                None
            } else {
                let skills_prompt = SkillSelector::build_skills_prompt(&skills);
                Some(format!("{}\n\n{}", self.config.system_prompt, skills_prompt))
            }
        } else {
            None
        };

        let result = react_loop(
            &self.components.planner,
            &self.components.executor,
            &self.components.recovery,
            &mut context,
            user_input,
            None,
            Some(&event_tx),
            cancel_token,
            self.components.critic.as_ref(),
            Some(&self.components.task_scheduler),
            system_prompt.as_deref(),
            None,
        )
        .await;

        self.session_manager
            .with_session(session_id, |s| {
                s.context = context;
            })
            .await;

        result.map(|r| r.response)
    }

    /// 取消正在进行的请求
    pub async fn cancel(&self, session_id: &str) {
        self.session_manager
            .with_session(session_id, |s| {
                s.cancel();
            })
            .await;
    }

    /// 获取会话历史
    pub async fn get_history(&self, session_id: &str, limit: Option<usize>) -> Vec<(String, String)> {
        self.session_manager
            .with_session(session_id, |s| {
                let messages = s.context.messages();
                let limited = if let Some(l) = limit {
                    &messages[messages.len().saturating_sub(l)..]
                } else {
                    messages
                };
                limited
                    .iter()
                    .map(|m| (format!("{:?}", m.role), m.content.clone()))
                    .collect()
            })
            .await
            .unwrap_or_default()
    }
}
