//! TUI 应用主循环
//!
//! 进入全屏/原始模式，轮询 state_rx 与键盘事件，将用户输入与快捷键转为 Command 发送给编排器，
//! 每帧用 draw 渲染 UiState 与输入缓冲。

use std::io::{self, Stdout};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use crossterm::event::KeyCode;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::watch;

use crate::core::UiState;
use crate::ui::render::{draw, InputFocus, InputState};

/// 默认智能体列表（TUI 用，与 config/assistants.toml 可扩展）
const DEFAULT_AGENTS: &[&str] = &["默认", "自动分派"];
/// 默认模型列表（TUI 用，与 config/models.toml 可扩展）
const DEFAULT_MODELS: &[&str] = &["默认", "DeepSeek", "GPT-4o", "Claude"];

/// 运行 TUI：启用原始模式与全屏，循环 poll 事件 + 渲染，退出时恢复终端
pub async fn run_app(
    state_rx: watch::Receiver<UiState>,
    _stream_rx: tokio::sync::broadcast::Receiver<String>,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<crate::core::Command>,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let event_handler = super::event::EventHandler::new(cmd_tx);
    let mut input_buffer = String::new();
    let mut conversation_scroll = 0usize;
    let mut last_history_len = 0usize;
    let mut input_state = InputState::default();
    let agents: Vec<&str> = DEFAULT_AGENTS.to_vec();
    let models: Vec<&str> = DEFAULT_MODELS.to_vec();

    loop {
        let state = state_rx.borrow().clone();

        if state.history.len() != last_history_len {
            last_history_len = state.history.len();
            conversation_scroll = usize::MAX;
        }

        if let Ok(Some(ev)) = event_handler.poll() {
            match ev {
                super::event::AppEvent::Command(cmd) => {
                    if matches!(cmd, crate::core::Command::Quit) {
                        break;
                    }
                }
                super::event::AppEvent::Key(key) if !state.input_locked => {
                    match key.code {
                        KeyCode::Enter => {
                            if input_state.focus == InputFocus::Input
                                || input_state.focus == InputFocus::Send
                            {
                                let input = input_buffer.trim().to_string();
                                input_buffer.clear();
                                if !input.is_empty() {
                                    if matches!(input.to_lowercase().as_str(), "/exit" | "exit" | "/quit" | "quit") {
                                        break;
                                    }
                                    event_handler.send_submit(input);
                                }
                            }
                        }
                        KeyCode::Tab => {
                            input_state.focus = match input_state.focus {
                                InputFocus::Input => InputFocus::Agent,
                                InputFocus::Agent => InputFocus::Model,
                                InputFocus::Model => InputFocus::Send,
                                InputFocus::Send | InputFocus::Mode | InputFocus::Image => InputFocus::Input,
                            };
                        }
                        KeyCode::BackTab => {
                            input_state.focus = match input_state.focus {
                                InputFocus::Input => InputFocus::Send,
                                InputFocus::Agent => InputFocus::Input,
                                InputFocus::Model => InputFocus::Agent,
                                InputFocus::Send | InputFocus::Mode | InputFocus::Image => InputFocus::Model,
                            };
                        }
                        KeyCode::Backspace => {
                            if input_state.focus == InputFocus::Input {
                                input_buffer.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if input_state.focus == InputFocus::Input {
                                input_buffer.push(c);
                            }
                        }
                        KeyCode::Up => {
                            if input_state.focus == InputFocus::Agent {
                                input_state.agent_index = input_state.agent_index.saturating_sub(1);
                            } else if input_state.focus == InputFocus::Model {
                                input_state.model_index = input_state.model_index.saturating_sub(1);
                            } else {
                                conversation_scroll = conversation_scroll.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if input_state.focus == InputFocus::Agent {
                                input_state.agent_index = (input_state.agent_index + 1).min(agents.len().saturating_sub(1));
                            } else if input_state.focus == InputFocus::Model {
                                input_state.model_index = (input_state.model_index + 1).min(models.len().saturating_sub(1));
                            } else {
                                conversation_scroll = conversation_scroll.saturating_add(1);
                            }
                        }
                        KeyCode::PageUp => {
                            conversation_scroll = conversation_scroll.saturating_sub(10);
                        }
                        KeyCode::PageDown => {
                            conversation_scroll = conversation_scroll.saturating_add(10);
                        }
                        KeyCode::Home => {
                            conversation_scroll = 0;
                        }
                        KeyCode::End => {
                            conversation_scroll = usize::MAX;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        let mut scroll_info = (0usize, 0usize);
        terminal.draw(|f| {
            draw(
                f,
                &state,
                &input_buffer,
                conversation_scroll,
                &mut scroll_info,
                &input_state,
                &agents,
                &models,
            );
        })?;
        let (total_lines, viewport_height) = scroll_info;
        let max_scroll = total_lines.saturating_sub(viewport_height);
        conversation_scroll = conversation_scroll.min(max_scroll);

        tokio::task::yield_now().await;
    }

    restore_terminal(&mut terminal)?;
    Ok(())
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}
