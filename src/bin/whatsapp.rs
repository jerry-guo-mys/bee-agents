//! Bee WhatsApp 服务
//!
//! 通过 WhatsApp Cloud API 与 Bee Agent 对话。
//!
//! 环境变量:
//! - WHATSAPP_ACCESS_TOKEN: Meta WhatsApp API 访问令牌
//! - WHATSAPP_PHONE_NUMBER_ID: 企业电话号码 ID
//! - WHATSAPP_VERIFY_TOKEN: Webhook 验证令牌 (默认 "bee")
//! - DEEPSEEK_API_KEY 或 OPENAI_API_KEY: LLM API Key
//!
//! 启动: cargo run --bin bee-whatsapp --features whatsapp

#[cfg(feature = "whatsapp")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use axum::Router;
    use bee::agent::create_agent_components;
    use bee::integrations::whatsapp::{create_router, WhatsappState};
    use tokio::sync::RwLock;
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();

    let access_token = std::env::var("WHATSAPP_ACCESS_TOKEN")
        .expect("WHATSAPP_ACCESS_TOKEN must be set");
    let phone_number_id = std::env::var("WHATSAPP_PHONE_NUMBER_ID")
        .expect("WHATSAPP_PHONE_NUMBER_ID must be set");

    let workspace = std::env::current_dir()?
        .join("workspace")
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap().join("workspace"));
    std::fs::create_dir_all(&workspace).ok();

    let system_prompt = [
        "config/prompts/system.md",
        "../config/prompts/system.md",
    ]
    .into_iter()
    .find_map(|p| std::fs::read_to_string(p).ok())
    .unwrap_or_else(|| "You are Bee, a helpful AI assistant. Use tools: cat, ls, echo.".to_string());

    let components = create_agent_components(&workspace, &system_prompt);

    let state = Arc::new(WhatsappState {
        components,
        sessions: Arc::new(RwLock::new(HashMap::new())),
        access_token,
        phone_number_id,
    });

    let app = create_router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Bee WhatsApp server listening on http://{}", addr);
    tracing::info!("Webhook URL: http://YOUR_HOST:3000/webhook");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(not(feature = "whatsapp"))]
fn main() {
    eprintln!("请使用 --features whatsapp 编译: cargo run --bin bee-whatsapp --features whatsapp");
    std::process::exit(1);
}
