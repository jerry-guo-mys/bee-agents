//! 应用配置：从 config/default.toml 与环境变量加载
//!
//! 加载顺序：先读 TOML 文件，再用环境变量 `BEE__*` 覆盖（双下划线表示嵌套，如 `BEE__LLM__PROVIDER=openai`）。

use std::path::PathBuf;

use serde::Deserialize;

/// 应用配置根（对应 config/default.toml 的顶层）
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct AppConfig {
    #[serde(default)]
    pub app: AppSection,
    #[serde(default)]
    pub llm: LlmSection,
    #[serde(default)]
    pub tools: ToolsSection,
    #[serde(default)]
    pub memory: MemorySection,
    #[serde(default)]
    pub evolution: EvolutionSection,
    #[serde(default)]
    pub heartbeat: HeartbeatSection,
    #[serde(default)]
    pub web: WebSection,
    /// Critic 配置（解决问题 4.3：配置化与模型分离）
    #[serde(default)]
    pub critic: CriticSection,
}

/// [web] 段：bee-web 服务端口等（可被环境变量 BEE__WEB__PORT 覆盖）
#[derive(Debug, Clone, Deserialize)]
pub struct WebSection {
    #[serde(default = "default_web_port")]
    pub port: u16,
}

fn default_web_port() -> u16 {
    8080
}

impl Default for WebSection {
    fn default() -> Self {
        Self {
            port: default_web_port(),
        }
    }
}

/// [app] 段：应用名、工作目录、对话轮数上限
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AppSection {
    pub name: Option<String>,
    /// 沙箱根目录，未设置时用 ./workspace
    pub workspace_root: Option<PathBuf>,
    /// 对话历史保留轮数（短期记忆）
    #[serde(default = "default_max_context_turns")]
    pub max_context_turns: usize,
}

fn default_max_context_turns() -> usize {
    20
}

/// 进化调度类型
#[derive(Debug, Clone, Deserialize)]
#[derive(Default)]
pub enum ScheduleType {
    #[serde(rename = "manual")]
    #[default]
    Manual,
    #[serde(rename = "interval")]
    Interval,
    #[serde(rename = "daily")]
    Daily,
    #[serde(rename = "weekly")]
    Weekly,
}

/// 审批模式
#[derive(Debug, Clone, Deserialize)]
#[derive(Default)]
pub enum ApprovalMode {
    #[serde(rename = "none")]
    #[default]
    None,
    #[serde(rename = "console")]
    Console,
    #[serde(rename = "prompt")]
    Prompt,
    #[serde(rename = "webhook")]
    Webhook,
}

/// 安全级别
#[derive(Debug, Clone, Deserialize)]
#[derive(Default)]
pub enum SafeMode {
    #[serde(rename = "strict")]
    #[default]
    Strict,
    #[serde(rename = "balanced")]
    Balanced,
    #[serde(rename = "permissive")]
    Permissive,
}

/// [critic] 段：Critic 配置（解决问题 4.3）
#[derive(Debug, Clone, Deserialize)]
pub struct CriticSection {
    /// 是否启用 Critic
    #[serde(default = "default_critic_enabled")]
    pub enabled: bool,
    /// Critic 使用的模型（为空时使用与 Planner 相同的模型）
    #[serde(default)]
    pub model: Option<String>,
    /// Critic 使用的 API 提供商（为空时使用与 Planner 相同的提供商）
    #[serde(default)]
    pub provider: Option<String>,
    /// 自定义 Critic prompt 模板
    #[serde(default = "default_critic_prompt")]
    pub prompt_template: String,
    /// 是否对每次工具调用都进行评估（false 时仅评估关键工具）
    #[serde(default)]
    pub evaluate_all_tools: bool,
    /// 仅评估的工具列表（为空时评估所有，evaluate_all_tools=false 时生效）
    #[serde(default)]
    pub evaluate_tools: Vec<String>,
}

fn default_critic_enabled() -> bool {
    false
}

fn default_critic_prompt() -> String {
    r#"You are a Critic evaluating tool execution results.

Goal: {goal}
Tool used: {tool}
Observation: {observation}

If the result looks correct and helpful for achieving the goal, respond with "OK".
If there's an issue or better approach, briefly explain the problem.

Response:"#.to_string()
}

impl Default for CriticSection {
    fn default() -> Self {
        Self {
            enabled: default_critic_enabled(),
            model: None,
            provider: None,
            prompt_template: default_critic_prompt(),
            evaluate_all_tools: false,
            evaluate_tools: vec![],
        }
    }
}

/// [evolution] 段：自我进化相关（参见 docs/EVOLUTION.md）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct EvolutionSection {
    /// HallucinatedTool 时是否自动向 lessons.md 追加教训
    #[serde(default = "default_auto_lesson_on_hallucination")]
    pub auto_lesson_on_hallucination: bool,
    /// 是否将工具调用成功也写入 procedural.md（EVOLUTION §3.5 工具统计；默认 false 减少噪音）
    #[serde(default)]
    pub record_tool_success: bool,
    /// 是否启用自主迭代功能
    #[serde(default = "default_evolution_enabled")]
    pub enabled: bool,
    /// 单次运行最大迭代次数
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// 目标质量分数阈值
    #[serde(default = "default_target_score_threshold")]
    pub target_score_threshold: f64,
    /// 是否自动提交 Git
    #[serde(default = "default_auto_commit")]
    pub auto_commit: bool,
    /// 是否需要人工确认（向后兼容）
    #[serde(default = "default_require_approval")]
    pub require_approval: bool,
    /// 重点改进领域
    #[serde(default = "default_focus_areas")]
    pub focus_areas: Vec<String>,
    /// 调度类型
    #[serde(default = "default_schedule_type")]
    pub schedule_type: ScheduleType,
    /// 间隔调度时的秒数
    #[serde(default = "default_schedule_interval_seconds")]
    pub schedule_interval_seconds: u64,
    /// 每日/每周调度的具体时间 (HH:MM)
    #[serde(default = "default_schedule_time")]
    pub schedule_time: String,
    /// 每个周期最大迭代次数
    #[serde(default = "default_max_iterations_per_period")]
    pub max_iterations_per_period: usize,
    /// 失败后的冷却时间（秒）
    #[serde(default = "default_cooldown_seconds")]
    pub cooldown_seconds: u64,
    /// 审批模式
    #[serde(default = "default_approval_mode")]
    pub approval_mode: ApprovalMode,
    /// 等待审批的超时时间（秒）
    #[serde(default = "default_approval_timeout_seconds")]
    pub approval_timeout_seconds: u64,
    /// Webhook URL（用于外部审批系统）
    pub approval_webhook_url: Option<String>,
    /// 需要审批的操作类型
    #[serde(default = "default_require_approval_for")]
    pub require_approval_for: Vec<String>,
    /// 安全级别
    #[serde(default = "default_safe_mode")]
    pub safe_mode: SafeMode,
    /// 允许修改的目录白名单
    #[serde(default = "default_allowed_directories")]
    pub allowed_directories: Vec<String>,
    /// 禁止修改的关键文件
    #[serde(default = "default_restricted_files")]
    pub restricted_files: Vec<String>,
    /// 单次修改最大文件大小（KB）
    #[serde(default = "default_max_file_size_kb")]
    pub max_file_size_kb: usize,
    /// 允许的操作类型
    #[serde(default = "default_allowed_operation_types")]
    pub allowed_operation_types: Vec<String>,
    /// 失败时自动回滚
    #[serde(default = "default_rollback_enabled")]
    pub rollback_enabled: bool,
    /// 编辑前创建备份
    #[serde(default = "default_backup_before_edit")]
    pub backup_before_edit: bool,
}

fn default_auto_lesson_on_hallucination() -> bool {
    true
}

fn default_evolution_enabled() -> bool {
    true
}

fn default_max_iterations() -> usize {
    10
}

fn default_target_score_threshold() -> f64 {
    0.8
}

fn default_auto_commit() -> bool {
    true
}

fn default_require_approval() -> bool {
    false
}

fn default_focus_areas() -> Vec<String> {
    vec![
        "performance".to_string(),
        "readability".to_string(),
        "documentation".to_string(),
        "testing".to_string(),
    ]
}

fn default_schedule_type() -> ScheduleType {
    ScheduleType::Manual
}

fn default_schedule_interval_seconds() -> u64 {
    86400 // 24 hours
}

fn default_schedule_time() -> String {
    "02:00".to_string() // 2 AM
}

fn default_max_iterations_per_period() -> usize {
    3
}

fn default_cooldown_seconds() -> u64 {
    300 // 5 minutes
}

fn default_approval_mode() -> ApprovalMode {
    ApprovalMode::None
}

fn default_approval_timeout_seconds() -> u64 {
    3600 // 1 hour
}

fn default_require_approval_for() -> Vec<String> {
    vec!["critical".to_string()]
}

fn default_safe_mode() -> SafeMode {
    SafeMode::Strict
}

fn default_allowed_directories() -> Vec<String> {
    vec!["./src".to_string()]
}

fn default_restricted_files() -> Vec<String> {
    vec!["Cargo.toml".to_string(), "src/main.rs".to_string()]
}

fn default_max_file_size_kb() -> usize {
    1024 // 1 MB
}

fn default_allowed_operation_types() -> Vec<String> {
    vec!["add".to_string(), "replace".to_string()]
}

fn default_rollback_enabled() -> bool {
    true
}

fn default_backup_before_edit() -> bool {
    true
}

/// [heartbeat] 段：后台自主循环（OpenClaw 风格：无人时定期「思考现状 → 检查待办 → 反思」）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct HeartbeatSection {
    /// 是否启用心跳（仅 bee-web 生效，定时向 Agent 发送一次 tick 提示）
    #[serde(default)]
    pub enabled: bool,
    /// 心跳间隔秒数
    #[serde(default = "default_heartbeat_interval_secs")]
    pub interval_secs: u64,
}

fn default_heartbeat_interval_secs() -> u64 {
    300
}

/// [memory] 段：长期记忆后端（向量检索：嵌入 API + 内存向量存储）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MemorySection {
    /// 是否启用向量长期记忆（嵌入 API 写入/检索，与 FileLongTerm 二选一）
    #[serde(default)]
    pub vector_enabled: bool,
    /// 嵌入模型名（如 text-embedding-3-small）
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    /// 嵌入 API base_url（未设置时使用 [llm].base_url，便于嵌入服务独立部署）
    pub embedding_base_url: Option<String>,
    /// 嵌入 API Key（未设置时使用 OPENAI_API_KEY）
    pub embedding_api_key: Option<String>,
    /// 向量库 URL（如 http://localhost:6333），预留供 qdrant 扩展
    pub qdrant_url: Option<String>,
}

fn default_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}

/// [llm] 段：后端选择与超时
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmSection {
    /// 后端：deepseek / openai；优先级由 API Key 与 provider 共同决定
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    pub base_url: Option<String>,
    #[serde(default)]
    pub deepseek: LlmDeepSeekSection,
    #[serde(default)]
    pub openai: LlmOpenAiSection,
    #[serde(default)]
    pub timeouts: LlmTimeoutsSection,
}

fn default_provider() -> String {
    "deepseek".to_string()
}

fn default_model() -> String {
    "deepseek-reasoner".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmDeepSeekSection {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmOpenAiSection {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmTimeoutsSection {
    #[serde(default = "default_request_timeout")]
    pub request: u64,
    #[serde(default = "default_stream_timeout")]
    pub stream: u64,
}

fn default_request_timeout() -> u64 {
    60
}

fn default_stream_timeout() -> u64 {
    120
}

/// [tools] 段：文件系统根、工具超时、Shell 白名单、Search 域名、技能插件
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolsSection {
    pub filesystem_root: Option<PathBuf>,
    /// 单次工具调用超时（秒）
    #[serde(default = "default_tool_timeout_secs")]
    pub tool_timeout_secs: u64,
    #[serde(default)]
    pub shell: ShellSection,
    #[serde(default)]
    pub search: SearchSection,
    /// 技能插件：从配置注册，每项对应一个「程序 + 参数模板」工具（白皮书：Agent 动态注册新工具）
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,
}

/// 单条技能插件配置：[[tools.plugins]]
#[derive(Debug, Clone, Deserialize)]
pub struct PluginEntry {
    /// 工具名（LLM 可见）
    pub name: String,
    /// 工具描述（供 LLM 选择）
    pub description: String,
    /// 可执行程序（如 python、node、/path/to/script）
    pub program: String,
    /// 参数模板列表；{{workspace}} 替换为沙箱根路径，{{key}} 从 LLM 传入的 args 中取 key
    #[serde(default)]
    pub args: Vec<String>,
    /// 本插件超时秒数（未设置时使用全局 tool_timeout_secs）
    pub timeout_secs: Option<u64>,
    /// 工作目录（未设置时使用 workspace 根）
    pub working_dir: Option<PathBuf>,
}

fn default_tool_timeout_secs() -> u64 {
    30
}

/// [tools.shell] 段：允许执行的命令名（仅首词，如 ls、grep、cargo）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ShellSection {
    #[serde(default = "default_allowed_commands")]
    pub allowed_commands: Vec<String>,
}

fn default_allowed_commands() -> Vec<String> {
    vec![
        "ls".into(),
        "grep".into(),
        "cat".into(),
        "head".into(),
        "tail".into(),
        "wc".into(),
        "find".into(),
        "cargo".into(),
        "rustc".into(),
    ]
}

/// [tools.search] 段：抓取 URL 的超时、最大字符数、允许的域名白名单
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SearchSection {
    #[serde(default = "default_search_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_result_chars")]
    pub max_result_chars: usize,
    #[serde(default = "default_allowed_domains")]
    pub allowed_domains: Vec<String>,
}

fn default_search_timeout_secs() -> u64 {
    15
}

fn default_max_result_chars() -> usize {
    8000
}

fn default_allowed_domains() -> Vec<String> {
    vec![
        // 维基百科
        "en.wikipedia.org".into(),
        "zh.wikipedia.org".into(),
        "ja.wikipedia.org".into(),
        // 中文常用
        "www.baidu.com".into(),
        "baike.baidu.com".into(),      // 百度百科
        "www.jd.com".into(),
        "item.jd.com".into(),          // 京东商品页
        "www.taobao.com".into(),
        "www.zhihu.com".into(),
        "zhuanlan.zhihu.com".into(),   // 知乎专栏
        "www.bilibili.com".into(),
        "www.douban.com".into(),
        "movie.douban.com".into(),
        "book.douban.com".into(),
        // 开发者资源
        "github.com".into(),
        "raw.githubusercontent.com".into(),
        "gist.github.com".into(),
        "stackoverflow.com".into(),
        "docs.rs".into(),
        "crates.io".into(),
        "doc.rust-lang.org".into(),
        "www.rust-lang.org".into(),
        "docs.python.org".into(),
        "pypi.org".into(),
        "www.npmjs.com".into(),
        "nodejs.org".into(),
        "developer.mozilla.org".into(), // MDN
        "devdocs.io".into(),
        "dev.to".into(),
        "medium.com".into(),
        // 学术 / 新闻
        "arxiv.org".into(),
        "news.google.com".into(),      // Google 新闻
        "news.ycombinator.com".into(), // Hacker News
        "www.reddit.com".into(),
        // 工具类
        "www.wolframalpha.com".into(),
        "www.weather.com".into(),
        "openweathermap.org".into(),
    ]
}


/// 从 config 目录加载配置，环境变量 BEE__* 可覆盖
///
/// 1. 按顺序查找 config/default.toml、../config/default.toml、default.toml，找到则作为第一源
/// 2. 若传入 config_path 且文件存在，则追加该文件（可覆盖前面的键）
/// 3. 最后叠加环境变量 BEE__*（双下划线表示嵌套键）
pub fn load_config(config_path: Option<PathBuf>) -> Result<AppConfig, config::ConfigError> {
    let mut builder = config::Config::builder();

    let default_names = ["config/default", "../config/default", "default"];
    for name in default_names {
        let path = format!("{}.toml", name);
        if std::path::Path::new(&path).exists() {
            builder = builder.add_source(
                config::File::with_name(name).required(false),
            );
            break;
        }
    }

    if let Some(ref path) = config_path {
        if path.exists() {
            builder = builder.add_source(config::File::from(path.clone()).required(false));
        }
    }

    builder = builder.add_source(
        config::Environment::with_prefix("BEE")
            .separator("__")
            .try_parsing(true),
    );

    let c = builder.build()?;
    c.try_deserialize()
}

/// 重新从磁盘与环境变量加载配置（用于「配置热更新」：调用方可在运行时调用此函数并决定是否用新配置重建 LLM 等组件）
pub fn reload_config() -> Result<AppConfig, config::ConfigError> {
    load_config(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_app_config() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.web.port, 8080);
        assert!(!cfg.memory.vector_enabled);
    }
}
