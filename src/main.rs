//! Bee - Rust 个人智能体系统
//!
//! 入口：初始化日志、创建 Agent 编排器与 TUI，并运行主循环。

use anyhow::Context;
use bee::{core::create_agent, ui::run_app};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 日志：默认 info，可通过 RUST_LOG 覆盖
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();

    // 确保工作目录与 Prompt 目录存在
    let _ = std::fs::create_dir_all("workspace");
    let _ = std::fs::create_dir_all("config/prompts");

    // 创建 Agent：返回命令发送端、状态接收端、流接收端
    let (cmd_tx, state_rx, stream_rx) =
        create_agent(None).await.context("Failed to create agent")?;

    // 启动 TUI 主循环（消费 state/stream，向 cmd_tx 发送用户指令）
    run_app(state_rx, stream_rx, cmd_tx)
        .await
        .context("App run failed")?;

    Ok(())
}
