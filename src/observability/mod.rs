//! 可观测性
//!
//! 初始化 tracing（默认 info，可通过 RUST_LOG 覆盖）。

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// 初始化 tracing 订阅器（main 中也可直接用 tracing_subscriber，此处预留统一入口）
pub fn init() {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();
}
