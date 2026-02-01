//! TUI 层：Ratatui + crossterm，主循环（app）、事件（event）、渲染（render）

pub mod app;
pub mod event;
pub mod render;

pub use app::run_app;
pub use event::EventHandler;
pub use render::draw;
