# Agent Guidelines for Bee

This file provides guidance for AI coding agents working in this repository.

## Project Overview

Bee is a Rust personal AI agent system with ReAct architecture, supporting TUI (default), Web, and WhatsApp interfaces.

## Build Commands

```bash
# Run TUI (default binary)
cargo run

# Run specific binary
cargo run --bin bee-web --features web
cargo run --bin bee-whatsapp --features whatsapp
cargo run --bin bee-evolution

# Build for release
cargo run --release
cargo build --release

# Check only (fast)
cargo check
```

## Test Commands

```bash
# Run all tests
cargo test

# Run specific test by name
cargo test test_name

# Run tests matching pattern
cargo test prefix_

# Run tests in specific module
cargo test module_name::

# Run with output
cargo test -- --nocapture

# Run single test with output
cargo test test_name -- --nocapture
```

## Lint/Format Commands

```bash
# Run clippy
cargo clippy
cargo clippy -- -D warnings  # fail on warnings

# Format code
cargo fmt

# Check formatting
cargo fmt -- --check
```

## Code Style Guidelines

### Imports (use statements)

Order: std → external crates → internal modules (separated by blank line)

```rust
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc};
use anyhow::Context;

use crate::config::AppConfig;
use crate::core::AgentError;
```

### Naming Conventions

- **Types/Structs/Enums/Traits**: `PascalCase` (e.g., `AgentError`, `ToolExecutor`)
- **Functions/Methods/Variables**: `snake_case` (e.g., `create_agent`, `tool_name`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `DEEPSEEK_BASE_URL`)
- **Modules**: `snake_case` (e.g., `memory`, `code_edit`)
- **Test functions**: `snake_case` prefixed with `test_` (e.g., `test_exact_match`)

### Documentation

- Use `//!` for module-level documentation (in Chinese for this project)
- Use `///` for item-level documentation
- Document public APIs thoroughly

### Error Handling

- Use `thiserror` for custom error enums
- Use `anyhow` for application-level error handling
- Define specific error types in `core/error.rs`

```rust
#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Tool execution failed: {0}")]
    ToolExecutionFailed(String),
    #[error("Path escape attempt: {0}")]
    PathEscape(String),
}
```

### Async Patterns

- Use `tokio` for async runtime
- `#[tokio::main]` for main functions
- Use `async-trait` for trait-based async methods
- Prefer `tokio::sync` primitives (mpsc, broadcast, watch, RwLock)

### Testing

- Tests are inline in `#[cfg(test)]` modules at file bottom
- No separate `tests/` directory for integration tests
- Use `super::*` to import parent module
- For async tests, create a runtime explicitly:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // async test code
        });
    }
}
```

### Features

The project uses Cargo features:
- `web` - Web server binary
- `whatsapp` - WhatsApp integration
- `browser` - Browser automation

Use feature gates when needed:
```rust
#[cfg(feature = "browser")]
use crate::tools::BrowserTool;
```

### Logging

Use `tracing` for logging:
```rust
tracing::info!("Message: {}", value);
tracing::warn!("Warning condition");
tracing::error!("Error occurred: {:?}", err);
```

## File Organization

- `src/main.rs` - TUI entry point
- `src/lib.rs` - Library exports
- `src/bin/` - Additional binaries
- `src/core/` - Orchestrator, state, recovery
- `src/llm/` - LLM clients (DeepSeek, OpenAI, Mock)
- `src/memory/` - Short/mid/long-term memory
- `src/react/` - Planner, Critic, ReAct loop
- `src/tools/` - Tool implementations
- `src/ui/` - TUI components
- `config/` - Configuration files

## Key Dependencies

- `tokio` - Async runtime
- `anyhow`/`thiserror` - Error handling
- `serde` - Serialization
- `async-openai` - OpenAI API
- `ratatui`/`crossterm` - TUI
- `axum` - Web server (optional)
- `rusqlite` - SQLite persistence
