//! 事件处理
//!
//! 轮询 crossterm 键盘事件，将 Ctrl+C/Ctrl+L/Esc/Ctrl+Q 转为 Command（Cancel/Clear/Quit），
//! 其余按键交给 run_app 拼 input_buffer，Enter 时 send_submit。

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use tokio::sync::mpsc;

use crate::core::Command;

/// 应用事件：来自快捷键的 Command 或原始 KeyEvent
#[derive(Debug, Clone)]
pub enum AppEvent {
    Command(Command),
    Key(KeyEvent),
    Tick,
}

/// 事件处理器：持有 cmd_tx，poll 时读键盘并返回 AppEvent，send_submit 发送用户输入
pub struct EventHandler {
    cmd_tx: mpsc::UnboundedSender<Command>,
}

impl EventHandler {
    pub fn new(cmd_tx: mpsc::UnboundedSender<Command>) -> Self {
        Self { cmd_tx }
    }

    pub fn poll(&self) -> anyhow::Result<Option<AppEvent>> {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    return Ok(Some(self.handle_key(key)));
                }
            }
        }
        Ok(None)
    }

    fn handle_key(&self, key: KeyEvent) -> AppEvent {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                let _ = self.cmd_tx.send(Command::Cancel);
                AppEvent::Command(Command::Cancel)
            }
            KeyCode::Char('l') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                let _ = self.cmd_tx.send(Command::Clear);
                AppEvent::Command(Command::Clear)
            }
            KeyCode::Esc => AppEvent::Command(Command::Cancel),
            KeyCode::Char('q') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                AppEvent::Command(Command::Quit)
            }
            _ => AppEvent::Key(key),
        }
    }

    pub fn send_submit(&self, input: String) {
        let _ = self.cmd_tx.send(Command::Submit(input));
    }
}
