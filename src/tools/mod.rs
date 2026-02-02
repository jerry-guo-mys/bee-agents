//! 工具箱：cat、ls、shell、search、echo 与注册表、执行器
//!
//! 所有工具实现 Tool trait；ToolRegistry 注册，ToolExecutor 带超时执行。
//! 可选 browser 工具（feature "browser"）：Headless Chrome 控制浏览器提取内容。

pub mod executor;
pub mod filesystem;
pub mod echo;
pub mod registry;
pub mod shell;
pub mod search;

#[cfg(feature = "browser")]
pub mod browser;

pub use executor::ToolExecutor;
pub use echo::EchoTool;
pub use filesystem::{CatTool, LsTool, SafeFs};
pub use registry::{Tool, ToolRegistry};
pub use shell::ShellTool;
pub use search::SearchTool;

#[cfg(feature = "browser")]
pub use browser::BrowserTool;
