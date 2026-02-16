//! Bee 进化测试程序 - 测试自主迭代功能

use std::path::PathBuf;
use std::sync::Arc;


use bee::{
    config::load_config,
    core::orchestrator::create_llm_from_config,
    tools::{
        ToolExecutor, ToolRegistry, CatTool, LsTool, EchoTool, ShellTool, SearchTool,
        CodeReadTool, CodeGrepTool, CodeEditTool, CodeWriteTool,
        TestRunTool, TestCheckTool, GitCommitTool,
    },
    evolution::{EvolutionLoop, EvolutionConfig},
};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();

    println!("🚀 Starting Bee Evolution Test");

    let mut cfg = load_config(None).unwrap_or_default();
    // Use mock LLM for safe testing
    cfg.llm.provider = "mock".to_string();

    let llm = create_llm_from_config(&cfg);
    let project_root = PathBuf::from(".");

    let mut tools = ToolRegistry::new();
    tools.register(CatTool::new(&project_root));
    tools.register(LsTool::new(&project_root));
    tools.register(EchoTool);
    tools.register(ShellTool::new(
        cfg.tools.shell.allowed_commands.clone(),
        cfg.tools.tool_timeout_secs,
    ));
    tools.register(SearchTool::new(
        cfg.tools.search.allowed_domains.clone(),
        cfg.tools.search.timeout_secs,
        cfg.tools.search.max_result_chars,
    ));
    tools.register(CodeReadTool::new(&project_root));
    tools.register(CodeGrepTool::new(&project_root));
    tools.register(CodeEditTool::new(&project_root));
    tools.register(CodeWriteTool::new(&project_root));
    tools.register(TestRunTool::new(&project_root));
    tools.register(TestCheckTool::new(&project_root));
    tools.register(GitCommitTool::new(&project_root));

    let executor = ToolExecutor::new(tools, cfg.tools.tool_timeout_secs);
    let executor = Arc::new(executor);

    let mut evolution_config = EvolutionConfig::from(cfg.evolution);
    // Override for safe testing
    evolution_config.require_approval = true;
    evolution_config.auto_commit = false;
    evolution_config.max_iterations = 1;

    if !evolution_config.enabled {
        println!("⚠️ Evolution is disabled in config. Enable it in config/default.toml");
        return Ok(());
    }

    let mut evolution_loop = EvolutionLoop::new(
        llm.clone(),
        executor,
        evolution_config,
        project_root.clone(),
    );

    println!("📊 Starting evolution loop...");

    match evolution_loop.run().await {
        Ok(results) => {
            println!("\n📈 Evolution completed with {} iterations:", results.len());
            for result in results {
                println!(
                    "  Iteration {}: {} (score: {:.2}, tests: {})",
                    result.iteration,
                    if result.success { "✓ SUCCESS" } else { "✗ FAILED" },
                    result.quality_score,
                    if result.tests_passed { "passed" } else { "failed" }
                );
                if !result.changes_made.is_empty() {
                    println!("    Changes made:");
                    for change in &result.changes_made {
                        println!("      - {}", change);
                    }
                }
                if !result.lessons_learned.is_empty() {
                    println!("    Lessons learned:");
                    for lesson in &result.lessons_learned {
                        println!("      - {}", lesson);
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ Evolution failed: {}", e);
            return Err(anyhow::anyhow!(e));
        }
    }

    println!("\n✅ Evolution test completed successfully!");

    Ok(())
}
