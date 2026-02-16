//! 可观测性

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init() {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();
}

pub fn init_metrics() {
    tracing::info!("Metrics initialized (placeholder)");
}
