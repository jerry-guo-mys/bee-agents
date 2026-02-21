//! Bee Gateway - 轮毂式网关服务器
//!
//! 统一的 WebSocket 中枢，连接所有平台（Web、WhatsApp、Lark、TUI 等）
//!
//! 运行方式：
//! ```bash
//! cargo run --bin bee-gateway --features gateway
//! ```

use std::path::PathBuf;

use bee::config::load_config;
use bee::gateway::{Hub, HubConfig, RuntimeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("bee=info".parse().unwrap()),
        )
        .init();

    let cfg = load_config(None).unwrap_or_default();

    let bind_addr = std::env::var("GATEWAY_BIND")
        .unwrap_or_else(|_| "127.0.0.1:9000".to_string());

    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let system_prompt = std::fs::read_to_string("config/prompts/default.md")
        .unwrap_or_else(|_| "You are a helpful AI assistant.".to_string());

    let hub_config = HubConfig {
        bind_addr: bind_addr.clone(),
        max_connections: 1000,
        heartbeat_interval: 30,
        session_timeout: 3600,
        max_context_turns: cfg.app.max_context_turns,
        runtime: RuntimeConfig {
            app_config: cfg,
            workspace,
            system_prompt,
            max_concurrent: 10,
            enable_skills: true,
        },
    };

    let hub = Hub::new(hub_config);

    tracing::info!("Starting Bee Hub on ws://{}", bind_addr);
    tracing::info!("Press Ctrl+C to stop");

    hub.start().await?;

    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down hub...");
    hub.stop().await;

    Ok(())
}
