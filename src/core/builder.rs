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
#[cfg(feature = "web")]
use crate::tools::{CreateTool, SendTool};

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
    /// 
    /// 需要传入共享的 LLM 客户端供深度研究等工具使用
    pub fn build_tool_registry(&self, llm: Arc<dyn LlmClient>) -> ToolRegistry {
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
        tools.register(DeepSearchTool::new(llm.clone()));
        tools.register(SourceValidatorTool::new(
            self.config.tools.search.allowed_domains.clone(),
        ));
        tools.register(ReportGeneratorTool::new(llm.clone()));
        tools.register(KnowledgeGraphBuilder::new(llm));

        #[cfg(feature = "web")]
        tools.register(CreateTool::new(&self.workspace));
        #[cfg(feature = "web")]
        tools.register(SendTool::new(&self.workspace));

        tools
    }

    /// 构建 LLM 客户端
    pub fn build_llm(&self) -> Arc<dyn LlmClient> {
        crate::core::orchestrator::create_llm_from_config(&self.config)
    }

    /// 构建 Critic（可选，解决问题 4.3：配置化与模型分离）
    pub fn build_critic(&self, planner_llm: Arc<dyn LlmClient>) -> Option<Critic> {
        // 检查配置是否启用 Critic
        if !self.config.critic.enabled && !self.enable_critic {
            return None;
        }
        // enable_critic 为 false 时也不创建
        if !self.enable_critic {
            return None;
        }

        // 如果配置了独立的 Critic 模型，使用独立的 LLM 实例
        let critic_llm: Arc<dyn LlmClient> = if let Some(ref model) = self.config.critic.model {
            let provider = self.config.critic.provider.as_deref()
                .unwrap_or(&self.config.llm.provider);
            
            if provider.to_lowercase() == "deepseek" {
                Arc::new(crate::llm::create_deepseek_client(Some(model)))
            } else {
                let base_url = self.config.llm.base_url.as_deref();
                let api_key = std::env::var("OPENAI_API_KEY").ok();
                Arc::new(crate::llm::OpenAiClient::new(base_url, model, api_key.as_deref()))
            }
        } else {
            planner_llm
        };

        // 尝试从文件加载 prompt，否则使用配置中的模板
        let critic_prompt = [
            "config/prompts/critic.md",
            "../config/prompts/critic.md",
        ]
        .into_iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| self.config.critic.prompt_template.clone());

        // 创建修改后的配置副本，使用文件中的 prompt
        let mut critic_config = self.config.critic.clone();
        critic_config.prompt_template = critic_prompt;

        Some(Critic::from_config(critic_llm, &critic_config))
    }

    /// 构建技能加载器（返回 Arc 可共享）
    pub fn build_skill_loader(&self) -> Arc<SkillLoader> {
        let skill_loader = Arc::new(SkillLoader::from_default());

        if self.enable_skills {
            let loader = skill_loader.clone();
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    if let Err(e) = loader.load_all().await {
                        tracing::warn!("Failed to load skills: {}", e);
                    }
                });
            });
        }

        skill_loader
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
        let tools = self.build_tool_registry(llm.clone());
        let full_system_prompt = self.build_full_system_prompt(&tools);
        let skill_loader = self.build_skill_loader();

        AgentComponents {
            planner: Planner::new(llm.clone(), full_system_prompt),
            executor: ToolExecutor::new(tools, self.config.tools.tool_timeout_secs),
            recovery: RecoveryEngine::new(),
            critic,
            task_scheduler: TaskScheduler::default(),
            skill_loader,
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
    pub skill_loader: Arc<SkillLoader>,
    pub llm: Arc<dyn LlmClient>,
    pub config: AppConfig,
}

impl AgentComponents {
    /// 获取技能缓存引用
    pub fn skill_cache(&self) -> SkillCache {
        self.skill_loader.cache()
    }

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
