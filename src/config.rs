//! 应用配置：从 config/default.toml 与环境变量加载
//!
//! 加载顺序：先读 TOML 文件，再用环境变量 `BEE__*` 覆盖（双下划线表示嵌套，如 `BEE__LLM__PROVIDER=openai`）。

use std::path::PathBuf;

use serde::Deserialize;

/// 应用配置根（对应 config/default.toml 的顶层）
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(default)]
    pub app: AppSection,
    #[serde(default)]
    pub llm: LlmSection,
    #[serde(default)]
    pub tools: ToolsSection,
    #[serde(default)]
    pub evolution: EvolutionSection,
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

/// [evolution] 段：自我进化相关（参见 docs/EVOLUTION.md）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct EvolutionSection {
    /// HallucinatedTool 时是否自动向 lessons.md 追加教训
    #[serde(default = "default_auto_lesson_on_hallucination")]
    pub auto_lesson_on_hallucination: bool,
    /// 是否将工具调用成功也写入 procedural.md（EVOLUTION §3.5 工具统计；默认 false 减少噪音）
    #[serde(default)]
    pub record_tool_success: bool,
}

fn default_auto_lesson_on_hallucination() -> bool {
    true
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

/// [tools] 段：文件系统根、工具超时、Shell 白名单、Search 域名
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app: AppSection::default(),
            llm: LlmSection::default(),
            tools: ToolsSection::default(),
            evolution: EvolutionSection::default(),
        }
    }
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
