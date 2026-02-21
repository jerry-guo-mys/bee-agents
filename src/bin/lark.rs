//! Bee 飞书（Lark）服务
//!
//! 通过飞书事件订阅与 Bee Agent 对话。
//!
//! 环境变量:
//! - LARK_APP_ID: 飞书应用 App ID
//! - LARK_APP_SECRET: 飞书应用 App Secret
//! - LARK_BASE_URL: 飞书 API 基地址（默认 https://open.feishu.cn，国际版用 https://open.larksuite.com）
//! - DEEPSEEK_API_KEY 或 OPENAI_API_KEY: LLM API Key
//!
//! 启动: cargo run --bin bee-lark --features lark

#[cfg(feature = "lark")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use bee::agent::create_agent_components;
    use bee::integrations::lark::{create_router, LarkState};
    use tokio::sync::RwLock;
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();

    let app_id = std::env::var("LARK_APP_ID").expect("LARK_APP_ID must be set");
    let app_secret = std::env::var("LARK_APP_SECRET").expect("LARK_APP_SECRET must be set");
    let base_url = std::env::var("LARK_BASE_URL")
        .unwrap_or_else(|_| "https://open.feishu.cn".to_string());

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

    let state = Arc::new(LarkState {
        components,
        sessions: Arc::new(RwLock::new(HashMap::new())),
        processed_events: Arc::new(RwLock::new(HashSet::new())),
        app_id,
        app_secret,
        base_url,
    });

    let app = create_router(state);

    let port = std::env::var("LARK_PORT").unwrap_or_else(|_| "3001".to_string());
    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    tracing::info!("Bee Lark server listening on http://{}", addr);
    tracing::info!("Webhook URL: http://YOUR_HOST:{}/webhook", port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(not(feature = "lark"))]
fn main() {
    eprintln!("请使用 --features lark 编译: cargo run --bin bee-lark --features lark");
    std::process::exit(1);
}
