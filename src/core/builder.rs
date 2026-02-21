//! Agent 构建器：统一的 Agent 初始化逻辑
//!
//! 解决问题 1.1：消除 TUI 与 Headless 的工具注册差异

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::AppConfig;
use crate::core::{RecoveryEngine, TaskScheduler};
use crate::llm::LlmClient;
use crate::react::{Critic, Planner};
use crate::skills::{SkillCache, SkillLoader};
use crate::tools::{
    CatTool, CodeEditTool, CodeGrepTool, CodeReadTool, CodeWriteTool,
    DeepSearchTool, EchoTool, GitCommitTool, KnowledgeGraphBuilder, LsTool, PluginTool,
    ReportGeneratorTool, SearchTool, ShellTool, SourceValidatorTool, TestCheckTool, TestRunTool,
    ToolExecutor, ToolRegistry,
};
#[cfg(feature = "browser")]
use crate::tools::BrowserTool;

/// Agent 构建器：统一配置和初始化 Agent 的各个组件
pub struct AgentBuilder {
    config: AppConfig,
    workspace: PathBuf,
    system_prompt: String,
    enable_critic: bool,
    enable_skills: bool,
}

impl AgentBuilder {
    /// 创建新的构建器
    pub fn new(config: AppConfig, workspace: PathBuf) -> Self {
        Self {
            config,
            workspace,
            system_prompt: String::new(),
            enable_critic: true,
            enable_skills: true,
        }
    }

    /// 设置系统提示词
    pub fn with_system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = prompt.to_string();
        self
    }

    /// 从文件加载系统提示词
    pub fn with_system_prompt_from_file(mut self) -> Self {
        self.system_prompt = [
            "config/prompts/system.md",
            "../config/prompts/system.md",
            "config/prompts/default.md",
            "../config/prompts/default.md",
        ]
        .into_iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| {
            "You are Bee, a helpful AI assistant with access to various tools.".to_string()
        });
        self
    }

    /// 是否启用 Critic
    pub fn with_critic(mut self, enable: bool) -> Self {
        self.enable_critic = enable;
        self
    }

    /// 是否启用技能系统
    pub fn with_skills(mut self, enable: bool) -> Self {
        self.enable_skills = enable;
        self
    }

    /// 构建统一的工具注册表（所有接入方式共享同一套工具）
    pub fn build_tool_registry(&self) -> ToolRegistry {
        let mut tools = ToolRegistry::new();

        tools.register(CatTool::new(&self.workspace));
        tools.register(LsTool::new(&self.workspace));
        tools.register(EchoTool);
        tools.register(ShellTool::new(
            self.config.tools.shell.allowed_commands.clone(),
            self.config.tools.tool_timeout_secs,
        ));
        tools.register(SearchTool::new(
            self.config.tools.search.allowed_domains.clone(),
            self.config.tools.search.timeout_secs,
            self.config.tools.search.max_result_chars,
        ));

        #[cfg(feature = "browser")]
        tools.register(BrowserTool::new(
            self.config.tools.search.allowed_domains.clone(),
            self.config.tools.search.max_result_chars,
        ));

        for entry in &self.config.tools.plugins {
            tools.register(PluginTool::new(
                entry,
                &self.workspace,
                self.config.tools.tool_timeout_secs,
            ));
        }

        tools.register(CodeReadTool::new(&self.workspace));
        tools.register(CodeGrepTool::new(&self.workspace));
        tools.register(CodeEditTool::new(&self.workspace));
        tools.register(CodeWriteTool::new(&self.workspace));
        tools.register(TestRunTool::new(&self.workspace));
        tools.register(TestCheckTool::new(&self.workspace));
        tools.register(GitCommitTool::new(&self.workspace));
        tools.register(DeepSearchTool::new(&self.config));
        tools.register(SourceValidatorTool::new(
            self.config.tools.search.allowed_domains.clone(),
        ));
        tools.register(ReportGeneratorTool::new(&self.config));
        tools.register(KnowledgeGraphBuilder::new(&self.config));

        tools
    }

    /// 构建 LLM 客户端
    pub fn build_llm(&self) -> Arc<dyn LlmClient> {
        crate::core::orchestrator::create_llm_from_config(&self.config)
    }

    /// 构建 Critic（可选）
    pub fn build_critic(&self, llm: Arc<dyn LlmClient>) -> Option<Critic> {
        if !self.enable_critic {
            return None;
        }

        let critic_prompt = [
            "config/prompts/critic.md",
            "../config/prompts/critic.md",
        ]
        .into_iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| {
            "The user wanted: {goal}\nYou executed tool: {tool} with result: {observation}\n\
             Is this result reasonable? If yes, respond with exactly: OK\n\
             If not, provide a brief correction (one sentence)."
                .to_string()
        });

        Some(Critic::new(llm, critic_prompt))
    }

    /// 构建技能缓存
    pub fn build_skill_cache(&self) -> SkillCache {
        let skill_loader = SkillLoader::from_default();
        let skill_cache = skill_loader.cache();

        if self.enable_skills {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    if let Err(e) = skill_loader.load_all().await {
                        tracing::warn!("Failed to load skills: {}", e);
                    }
                });
            });
        }

        skill_cache
    }

    /// 构建完整系统提示词（包含工具 schema）
    pub fn build_full_system_prompt(&self, tool_registry: &ToolRegistry) -> String {
        let tool_schema = tool_registry.to_schema_json();
        if tool_schema.is_empty() || tool_schema == "[]" {
            self.system_prompt.clone()
        } else {
            format!(
                "{}\n\n## Tool call JSON Schema (you must output valid JSON matching this)\n```json\n{}\n```",
                self.system_prompt, tool_schema
            )
        }
    }

    /// 构建完整的 AgentComponents（供 Headless/Web/WhatsApp/Gateway 使用）
    pub fn build_components(&self) -> AgentComponents {
        let llm = self.build_llm();
        let critic = self.build_critic(llm.clone());
        let tools = self.build_tool_registry();
        let full_system_prompt = self.build_full_system_prompt(&tools);
        let skill_cache = self.build_skill_cache();

        AgentComponents {
            planner: Planner::new(llm.clone(), full_system_prompt),
            executor: ToolExecutor::new(tools, self.config.tools.tool_timeout_secs),
            recovery: RecoveryEngine::new(),
            critic,
            task_scheduler: TaskScheduler::default(),
            skill_cache,
            llm,
            config: self.config.clone(),
        }
    }

    /// 获取配置
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// 获取工作目录
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

/// 预构建的 Agent 组件：Planner、ToolExecutor、Recovery、Critic、TaskScheduler
/// 可多会话共享
pub struct AgentComponents {
    pub planner: Planner,
    pub executor: ToolExecutor,
    pub recovery: RecoveryEngine,
    pub critic: Option<Critic>,
    pub task_scheduler: TaskScheduler,
    pub skill_cache: SkillCache,
    pub llm: Arc<dyn LlmClient>,
    pub config: AppConfig,
}

impl AgentComponents {
    /// 获取 LLM 客户端引用
    pub fn llm(&self) -> &Arc<dyn LlmClient> {
        &self.llm
    }

    /// 获取配置引用
    pub fn config(&self) -> &AppConfig {
        &self.config
    }
}

/// 便捷函数：从默认路径创建 AgentBuilder
pub fn create_agent_builder(config_path: Option<PathBuf>) -> AgentBuilder {
    let config = crate::config::load_config(config_path).unwrap_or_else(|e| {
        tracing::warn!("Config load failed ({}), using defaults", e);
        AppConfig::default()
    });

    let workspace = config
        .app
        .workspace_root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap().join("workspace"));
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.clone());
    std::fs::create_dir_all(&workspace).ok();

    AgentBuilder::new(config, workspace).with_system_prompt_from_file()
}
