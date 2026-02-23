pub mod executor;
pub mod filesystem;
pub mod echo;
pub mod plugin;
pub mod registry;
pub mod schema;
pub mod shell;
pub mod search;
pub mod code_read;
pub mod code_grep;
pub mod code_edit;
pub mod code_write;
pub mod code_review;
pub mod test_run;
pub mod test_check;
pub mod git_commit;
pub mod git_diff;
pub mod deep_search;
pub mod source_validator;
pub mod report_generator;
pub mod knowledge_graph;

#[cfg(feature = "web")]
pub mod create;
#[cfg(feature = "web")]
pub mod create_group;
#[cfg(feature = "web")]
pub mod list_agents;
#[cfg(feature = "web")]
pub mod send;

#[cfg(feature = "browser")]
pub mod browser;

pub use executor::ToolExecutor;
pub use echo::EchoTool;
pub use filesystem::{CatTool, LsTool, SafeFs};
pub use plugin::PluginTool;
pub use registry::{Tool, ToolRegistry};
pub use schema::tool_call_schema_json;
pub use shell::ShellTool;
pub use search::SearchTool;
pub use code_read::CodeReadTool;
pub use code_grep::CodeGrepTool;
pub use code_edit::CodeEditTool;
pub use code_write::CodeWriteTool;
pub use code_review::CodeReviewTool;
pub use test_run::TestRunTool;
pub use test_check::TestCheckTool;
pub use git_commit::GitCommitTool;
pub use git_diff::GitDiffTool;
pub use deep_search::DeepSearchTool;
pub use source_validator::SourceValidatorTool;
pub use report_generator::ReportGeneratorTool;
pub use knowledge_graph::KnowledgeGraphBuilder;

#[cfg(feature = "web")]
pub use create::{CreateTool, DynamicAgent};
#[cfg(feature = "web")]
pub use create_group::CreateGroupTool;
#[cfg(feature = "web")]
pub use list_agents::ListAgentsTool;
#[cfg(feature = "web")]
pub use send::{SendTool, CURRENT_ASSISTANT_ID};

#[cfg(feature = "browser")]
pub use browser::BrowserTool;
