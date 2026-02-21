//! ReAct 主循环
//!
//! Plan -> Act (Tool) -> Observe -> 可选 Critic -> 下一轮 Plan；支持 RetryWithPrompt、Cancel、最大步数限制。
//! 可选 event_tx：向 Web 等前端推送 Thinking / ToolCall / Observation / MessageChunk / MessageDone。

use tokio::sync::broadcast;

use crate::core::{AgentError, RecoveryAction, RecoveryEngine, TaskScheduler};
use crate::memory::Message;
use crate::react::{parse_llm_output, ContextManager, Critic, CriticResult, Planner, ReactEvent};
use crate::tools::ToolExecutor;

/// 单次对话内最大 ReAct 步数，防止死循环
const MAX_REACT_STEPS: usize = 20;
/// 对话条数超过此值时在规划前执行一次 Context Compaction（摘要写入长期记忆并替换为摘要消息）
const COMPACT_THRESHOLD: usize = 24;

/// 从用户输入中提取「记住：xxx」类内容，用于写入 preferences
fn extract_remember_content(input: &str) -> Option<String> {
    let input = input.trim();
    let idx = input.find("记住")?;
    let after = input.get(idx + "记住".len()..)?;
    let sep = after.find('：').or_else(|| after.find(':'))?;
    let content = after.get(sep + 1..)?.trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}
/// 流式回复时每段字符数（模拟打字效果）
const CHUNK_CHARS: usize = 6;
/// Observation 预览最大字符数
const OBSERVATION_PREVIEW_CHARS: usize = 200;
/// 思考内容展示最大字符数
const THINKING_PREVIEW_CHARS: usize = 800;
/// 记忆相关展示最大字符数
const MEMORY_PREVIEW_CHARS: usize = 300;

/// ReAct 循环执行结果：最终回复与当前对话历史
#[derive(Debug)]
pub struct ReactResult {
    pub response: String,
    pub messages: Vec<Message>,
}

/// ReAct 会话配置（解决问题 1.3：将 12 个参数封装为结构体）
pub struct ReactSession<'a> {
    /// Planner（必需）
    pub planner: &'a Planner,
    /// 工具执行器（必需）
    pub executor: &'a ToolExecutor,
    /// 恢复引擎（必需）
    pub recovery: &'a RecoveryEngine,
    /// 取消令牌（必需）
    pub cancel_token: tokio_util::sync::CancellationToken,
    /// 可选：工具结果校验
    pub critic: Option<&'a Critic>,
    /// 可选：任务调度器
    pub task_scheduler: Option<&'a TaskScheduler>,
    /// 可选：流式输出通道
    pub stream_tx: Option<&'a broadcast::Sender<String>>,
    /// 可选：事件推送通道
    pub event_tx: Option<&'a tokio::sync::mpsc::UnboundedSender<ReactEvent>>,
    /// 可选：覆盖系统提示词（多助手场景）
    pub system_prompt_override: Option<&'a str>,
    /// 可选：限制可用工具列表
    pub allowed_tools: Option<&'a [String]>,
}

impl<'a> ReactSession<'a> {
    /// 创建最小配置的 ReactSession
    pub fn new(
        planner: &'a Planner,
        executor: &'a ToolExecutor,
        recovery: &'a RecoveryEngine,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        Self {
            planner,
            executor,
            recovery,
            cancel_token,
            critic: None,
            task_scheduler: None,
            stream_tx: None,
            event_tx: None,
            system_prompt_override: None,
            allowed_tools: None,
        }
    }

    /// 设置 Critic
    pub fn with_critic(mut self, critic: &'a Critic) -> Self {
        self.critic = Some(critic);
        self
    }

    /// 设置任务调度器
    pub fn with_task_scheduler(mut self, scheduler: &'a TaskScheduler) -> Self {
        self.task_scheduler = Some(scheduler);
        self
    }

    /// 设置流式输出通道
    pub fn with_stream_tx(mut self, tx: &'a broadcast::Sender<String>) -> Self {
        self.stream_tx = Some(tx);
        self
    }

    /// 设置事件推送通道
    pub fn with_event_tx(mut self, tx: &'a tokio::sync::mpsc::UnboundedSender<ReactEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// 设置系统提示词覆盖
    pub fn with_system_prompt(mut self, prompt: &'a str) -> Self {
        self.system_prompt_override = Some(prompt);
        self
    }

    /// 设置允许的工具列表
    pub fn with_allowed_tools(mut self, tools: &'a [String]) -> Self {
        self.allowed_tools = Some(tools);
        self
    }
}

fn send_event(tx: &Option<&tokio::sync::mpsc::UnboundedSender<ReactEvent>>, ev: ReactEvent) {
    if let Some(t) = tx {
        let _ = t.send(ev);
    }
}

/// Context Compaction：将当前对话摘要写入长期记忆，并替换为一条摘要型 system 消息，避免 token 溢出。
/// 可由 ReAct 循环在消息数超过阈值时自动调用，或由 Web API 手动触发。
pub async fn compact_context(
    planner: &Planner,
    context: &mut ContextManager,
) -> Result<(), AgentError> {
    let messages = context.messages().to_vec();
    if messages.len() < 2 {
        return Ok(());
    }
    let summary = planner.summarize(&messages).await?;
    if summary.is_empty() {
        return Ok(());
    }
    context.push_to_long_term(&format!("Conversation summary: {}", summary));
    context.set_messages(vec![Message::system(format!(
        "Previous conversation summary:\n\n{}",
        summary
    ))]);
    Ok(())
}

/// 执行 ReAct 循环（新版本，使用 ReactSession 结构体）
///
/// 用户输入 -> 拼 system(working+long_term) -> plan -> 解析输出 ->
/// 若 ToolCall 则执行并写回 Observation（可选 Critic 校验）->
/// 若 Response 则返回并写入长期记忆
pub async fn react_loop_v2(
    session: &ReactSession<'_>,
    context: &mut ContextManager,
    user_input: &str,
) -> Result<ReactResult, AgentError> {
    let planner = session.planner;
    let executor = session.executor;
    let recovery = session.recovery;
    let cancel_token = session.cancel_token.clone();
    let critic = session.critic;
    let task_scheduler = session.task_scheduler;
    let stream_tx = session.stream_tx;
    let event_tx = session.event_tx;
    let system_prompt_override = session.system_prompt_override;
    let allowed_tools = session.allowed_tools;

    react_loop_impl(
        planner, executor, recovery, context, user_input,
        stream_tx, event_tx, cancel_token, critic, task_scheduler,
        system_prompt_override, allowed_tools,
    ).await
}

/// 执行 ReAct 循环（兼容版本，保留原有 12 参数签名）
///
/// 若提供 system_prompt_override，则用其替代 planner 的 base_system_prompt（用于多助手场景）。
/// allowed_tools: 该智能体可用的工具名列表；为 None 或空时使用 executor 全部工具。
#[allow(clippy::too_many_arguments)]
pub async fn react_loop(
    planner: &Planner,
    executor: &ToolExecutor,
    recovery: &RecoveryEngine,
    context: &mut ContextManager,
    user_input: &str,
    stream_tx: Option<&broadcast::Sender<String>>,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<ReactEvent>>,
    cancel_token: tokio_util::sync::CancellationToken,
    critic: Option<&Critic>,
    task_scheduler: Option<&TaskScheduler>,
    system_prompt_override: Option<&str>,
    allowed_tools: Option<&[String]>,
) -> Result<ReactResult, AgentError> {
    react_loop_impl(
        planner, executor, recovery, context, user_input,
        stream_tx, event_tx, cancel_token, critic, task_scheduler,
        system_prompt_override, allowed_tools,
    ).await
}

/// ReAct 循环内部实现
#[allow(clippy::too_many_arguments)]
async fn react_loop_impl(
    planner: &Planner,
    executor: &ToolExecutor,
    recovery: &RecoveryEngine,
    context: &mut ContextManager,
    user_input: &str,
    stream_tx: Option<&broadcast::Sender<String>>,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<ReactEvent>>,
    cancel_token: tokio_util::sync::CancellationToken,
    critic: Option<&Critic>,
    task_scheduler: Option<&TaskScheduler>,
    system_prompt_override: Option<&str>,
    allowed_tools: Option<&[String]>,
) -> Result<ReactResult, AgentError> {
    context.push_message(Message::user(user_input.to_string()));
    context.working.set_goal(user_input);

    // 显式用户偏好：若用户说「记住：xxx」，写入 preferences 并同步到长期记忆
    if let Some(pref) = extract_remember_content(user_input) {
        context.append_preference(&pref);
        context.push_to_long_term(&format!("User preference: {}", pref));
    }

    // 记录初始 token 数，用于计算本次增量
    let (init_prompt, init_completion, _) = planner.token_usage();

    let mut step = 0;
    let mut last_llm_output = String::new();

    loop {
        send_event(&event_tx, ReactEvent::StepUpdate { step, max_steps: MAX_REACT_STEPS });

        if cancel_token.is_cancelled() {
            send_event(&event_tx, ReactEvent::Error { text: "Cancelled by user".to_string() });
            return Err(AgentError::Cancelled);
        }

        if step >= MAX_REACT_STEPS {
            return Ok(ReactResult {
                response: format!(
                    "达到最大步数限制 ({})，最后输出：\n{}",
                    MAX_REACT_STEPS, last_llm_output
                ),
                messages: context.messages().to_vec(),
            });
        }

        // 若当前对话条数过多，先压缩：摘要写入长期记忆并替换为一条摘要消息
        if context.messages().len() > COMPACT_THRESHOLD {
            if let Err(e) = compact_context(planner, context).await {
                send_event(&event_tx, ReactEvent::Error {
                    text: format!("Compaction failed: {}", e),
                });
                // 不中止，继续用当前消息规划
            }
        }

        let messages = context.to_llm_messages();
        let working_section = context.working_memory_section();
        let long_term_block = context.long_term_section(user_input);
        if !long_term_block.is_empty() {
            let preview: String = long_term_block.chars().take(MEMORY_PREVIEW_CHARS).collect();
            let preview = if long_term_block.len() > MEMORY_PREVIEW_CHARS {
                format!("{}...", preview)
            } else {
                preview
            };
            send_event(&event_tx, ReactEvent::MemoryRecovery { preview });
        }
        // 动态 system：基础 prompt（或 override）+ Working Memory + 长期记忆检索 + 行为约束/教训 + 程序记忆 + 用户偏好（自我进化）
        let lessons_block = context.lessons_section();
        let procedural_block = context.procedural_section();
        let preferences_block = context.preferences_section();
        let base_prompt = system_prompt_override.unwrap_or_else(|| planner.base_system_prompt());
        let system = format!(
            "{}\n\n{}\n\n{}{}{}{}",
            base_prompt,
            working_section,
            long_term_block,
            lessons_block,
            procedural_block,
            preferences_block
        );
        send_event(&event_tx, ReactEvent::Thinking);
        let output = match planner.plan_with_system(&messages, &system).await {
            Ok(o) => o,
            Err(e) => {
                let mut hist = context.conversation.messages().to_vec();
                let action = recovery.handle(&e, &mut hist);
                match action {
                    RecoveryAction::RetryWithPrompt(prompt) => {
                        send_event(&event_tx, ReactEvent::Recovery {
                            action: "RetryWithPrompt".to_string(),
                            detail: prompt.clone(),
                        });
                        context.push_message(Message::user(prompt));
                        step += 1;
                        continue;
                    }
                    RecoveryAction::AskUser(msg) => {
                        send_event(&event_tx, ReactEvent::Recovery {
                            action: "AskUser".to_string(),
                            detail: msg.clone(),
                        });
                        send_event(&event_tx, ReactEvent::Error { text: e.to_string() });
                        return Err(e);
                    }
                    RecoveryAction::SummarizeAndPrune => {
                        send_event(&event_tx, ReactEvent::Recovery {
                            action: "SummarizeAndPrune".to_string(),
                            detail: "Compacting context and retrying".to_string(),
                        });
                        if let Err(compact_err) = compact_context(planner, context).await {
                            send_event(&event_tx, ReactEvent::Error {
                                text: format!("Compaction failed: {}", compact_err),
                            });
                            return Err(compact_err);
                        }
                        step += 1;
                        continue;
                    }
                    RecoveryAction::DowngradeModel => {
                        send_event(&event_tx, ReactEvent::Recovery {
                            action: "DowngradeModel".to_string(),
                            detail: "建议切换至轻量模型".to_string(),
                        });
                        return Err(AgentError::SuggestDowngradeModel(
                            "LLM 调用失败，建议切换至轻量模型或检查网络与 API Key。".to_string(),
                        ));
                    }
                    _ => {
                        send_event(&event_tx, ReactEvent::Recovery {
                            action: "Abort".to_string(),
                            detail: e.to_string(),
                        });
                        send_event(&event_tx, ReactEvent::Error { text: e.to_string() });
                        return Err(e);
                    }
                }
            }
        };

        last_llm_output = output.clone();

        let thinking_preview: String = output.chars().take(THINKING_PREVIEW_CHARS).collect();
        let thinking_preview = if output.len() > THINKING_PREVIEW_CHARS {
            format!("{}...", thinking_preview)
        } else {
            thinking_preview
        };
        send_event(&event_tx, ReactEvent::ThinkingContent { text: thinking_preview });

        if let Some(tx) = stream_tx {
            let _ = tx.send(output.clone());
        }

        match parse_llm_output(&output) {
            Ok(crate::react::planner::PlannerOutput::Response(resp)) => {
                let chars: Vec<char> = resp.chars().collect();
                for chunk in chars.chunks(CHUNK_CHARS) {
                    send_event(&event_tx, ReactEvent::MessageChunk {
                        text: chunk.iter().collect(),
                    });
                }
                send_event(&event_tx, ReactEvent::MessageDone);
                context.push_message(Message::assistant(resp.clone()));
                let cons_preview: String = resp.chars().take(MEMORY_PREVIEW_CHARS).collect();
                let cons_preview = if resp.len() > MEMORY_PREVIEW_CHARS {
                    format!("{}...", cons_preview)
                } else {
                    cons_preview
                };
                send_event(&event_tx, ReactEvent::MemoryConsolidation { preview: cons_preview });
                context.push_to_long_term(&resp); // 最终回复写入长期记忆

                // 发送 token 统计
                let (cur_prompt, cur_completion, cur_total) = planner.token_usage();
                send_event(&event_tx, ReactEvent::TokenUsage {
                    prompt_tokens: cur_prompt.saturating_sub(init_prompt),
                    completion_tokens: cur_completion.saturating_sub(init_completion),
                    total_tokens: cur_prompt.saturating_sub(init_prompt) + cur_completion.saturating_sub(init_completion),
                    cumulative_prompt: cur_prompt,
                    cumulative_completion: cur_completion,
                    cumulative_total: cur_total,
                });

                // 策略沉淀：将本轮目标与使用的工具写入长期记忆，供后续检索（EVOLUTION §3.5）
                let tools_used = context.working.tool_names_used();
                context.push_session_strategy_to_long_term(user_input, &tools_used);

                return Ok(ReactResult {
                    response: resp,
                    messages: context.messages().to_vec(),
                });
            }
            Ok(crate::react::planner::PlannerOutput::ToolCall(tc)) => {
                send_event(&event_tx, ReactEvent::ToolCall {
                    tool: tc.tool.clone(),
                    args: tc.args.clone(),
                });
                let valid_names: &[String] = match allowed_tools {
                    Some(a) if !a.is_empty() => a,
                    _ => &[], // 空 slice 表示用 executor 全部工具
                };
                let is_allowed = if valid_names.is_empty() {
                    executor.tool_names().iter().any(|n| n == &tc.tool)
                } else {
                    valid_names.iter().any(|n| n == &tc.tool)
                };
                if !is_allowed {
                    let ref_names: Vec<String> = if valid_names.is_empty() {
                        executor.tool_names()
                    } else {
                        valid_names.to_vec()
                    };
                    send_event(&event_tx, ReactEvent::Error { text: format!("工具 {} 不在该智能体技能范围内", tc.tool) });
                    context.append_hallucination_lesson(&tc.tool, &ref_names);
                    return Err(AgentError::HallucinatedTool(tc.tool.clone()));
                }
                // 工具并发限制：从 TaskScheduler 获取许可后再执行
                let _permit = if let Some(sched) = task_scheduler {
                    Some(sched.acquire_tool().await)
                } else {
                    None
                };
                let result = executor.execute(&tc.tool, tc.args).await;
                let observation = match result {
                    Ok(r) => {
                        if context.record_tool_success {
                            context.append_procedural_record(&tc.tool, true, "ok");
                        }
                        r
                    }
                    Err(e) => {
                        let failure_msg = format!("{}: {}", tc.tool, e);
                        context.working.add_failure(failure_msg.clone());
                        context.append_procedural_record(&tc.tool, false, &e.to_string());
                        send_event(&event_tx, ReactEvent::ToolFailure {
                            tool: tc.tool.clone(),
                            reason: e.to_string(),
                        });
                        format!("Error: {}", e)
                    }
                };
                let preview: String = observation.chars().take(OBSERVATION_PREVIEW_CHARS).collect();
                if observation.len() > OBSERVATION_PREVIEW_CHARS {
                    send_event(&event_tx, ReactEvent::Observation {
                        tool: tc.tool.clone(),
                        preview: preview + "...",
                    });
                } else {
                    send_event(&event_tx, ReactEvent::Observation {
                        tool: tc.tool.clone(),
                        preview,
                    });
                }
                context.working.add_attempt(format!("{} -> {}", tc.tool, observation));
                // 可选 Critic：校验工具结果是否符合目标；若 observation 已明确表示工具失败，则跳过 Critic，避免重复的“修正建议”
                let obs_upper = observation.to_uppercase();
                let is_tool_failure = obs_upper.contains("FAILED")
                    || obs_upper.contains("ERROR")
                    || obs_upper.contains("ACCESS RESTRICTED")
                    || obs_upper.contains("TIMEOUT");
                if !is_tool_failure {
                    if let Some(c) = critic {
                        if let Ok(CriticResult::Correction(suggestion)) =
                            c.evaluate(user_input, &tc.tool, &observation).await
                        {
                            send_event(&event_tx, ReactEvent::Recovery {
                                action: "Critic".to_string(),
                                detail: suggestion.clone(),
                            });
                            context.append_critic_lesson(&suggestion);
                            context.push_message(Message::user(format!(
                                "Critic 建议：{}",
                                suggestion
                            )));
                        }
                    }
                }
                // 将工具调用与结果写回对话，供下一轮 Plan 使用
                context.push_message(Message::assistant(format!(
                    "Tool call: {} | Result: {}",
                    tc.tool, observation
                )));
                context.push_message(Message::user(format!(
                    "Observation from {}: {}",
                    tc.tool, observation
                )));
            }
            Err(e) => {
                // 解析失败（如 JSON 错误），交给 Recovery 决定是否 RetryWithPrompt
                let mut hist = context.conversation.messages().to_vec();
                let action = recovery.handle(&e, &mut hist);
                match action {
                    RecoveryAction::RetryWithPrompt(prompt) => {
                        send_event(&event_tx, ReactEvent::Recovery {
                            action: "RetryWithPrompt".to_string(),
                            detail: prompt.clone(),
                        });
                        context.push_message(Message::user(prompt));
                    }
                    _ => {
                        send_event(&event_tx, ReactEvent::Recovery {
                            action: "Abort".to_string(),
                            detail: e.to_string(),
                        });
                        send_event(&event_tx, ReactEvent::Error { text: e.to_string() });
                        return Err(e);
                    }
                }
            }
        }

        step += 1;
    }
}
