//! Bee Web UI
//!
//! 启动: cargo run --bin bee-web --features web
//! 浏览器访问 http://127.0.0.1:8080

#![cfg(feature = "web")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, Response},
    routing::{get, post},
    Json, Router,
};
use bee::memory::{Message, Role};
use bytes::Bytes;
use futures_util::stream::{self, TryStreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use bee::agent::{
    consolidate_memory_with_llm, create_agent_components, create_context_with_long_term,
    create_shared_vector_long_term, process_message, process_message_stream,
};
use bee::core::AgentComponents;
use bee::skills::{Skill, SkillLoader};
use bee::tools::tool_call_schema_json;
use bee::memory::InMemoryVectorLongTerm;
use bee::config::{load_config, AppConfig};
use bee::memory::{
    append_daily_log, append_heartbeat_log, consolidate_memory, lessons_path, preferences_path,
    procedural_path, record_error as learnings_record_error, record_learning as learnings_record_learning,
    ConversationMemory, memory_root,
};
use bee::react::{compact_context, ContextManager, Planner, ReactEvent};

/// 会话快照：仅持久化对话消息，重启后恢复
#[derive(serde::Serialize, serde::Deserialize)]
struct SessionSnapshot {
    messages: Vec<Message>,
    max_turns: usize,
}

const DEFAULT_MAX_TURNS: usize = 20;

/// 心跳时发给 Agent 的提示：根据长期记忆与当前状态检查待办或需跟进事项
const HEARTBEAT_PROMPT: &str = "Heartbeat: 你正在后台自主运行。请根据长期记忆与当前状态，检查是否有待办或需跟进的事项；若有则输出一条简短建议，若无则仅回复 OK。可使用 cat/ls 查看 workspace 下 memory 或任务文件。";

struct AppState {
    /// 应用配置（解决问题 1.2）
    config: AppConfig,
    /// 可运行时替换，以支持「多 LLM 后端切换」与配置热更新（白皮书 Phase 5）
    components: Arc<RwLock<Arc<AgentComponents>>>,
    sessions: Arc<RwLock<HashMap<String, ContextManager>>>,
    sessions_dir: PathBuf,
    /// 记忆根目录（workspace/memory），用于短期日志与长期 Markdown
    memory_root: PathBuf,
    workspace: PathBuf,
    /// 向量长期记忆共享实例（启用时带快照路径，定期保存避免重启丢失）
    shared_vector_long_term: Option<Arc<InMemoryVectorLongTerm>>,
    /// 多助手：列表与 id -> 完整 system prompt（含 tool schema）
    assistants: Vec<AssistantInfo>,
    assistant_prompts: Arc<RwLock<HashMap<String, String>>>,
    /// 每个智能体可用的技能（工具名列表），空表示全部可用
    assistant_skills: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// 工具列表（id, name, description），用于技能配置
    tool_descriptions: Vec<(String, String)>,
    /// 助手元数据（prompt 路径等），用于重建 prompt
    assistant_entries: HashMap<String, AssistantEntry>,
    config_base: PathBuf,
    /// 可切换模型：列表与 id -> 模型配置
    models: Vec<ModelInfo>,
    model_configs: HashMap<String, ModelEntry>,
    /// 技能加载器
    skill_loader: Arc<SkillLoader>,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
    /// 多助手：选用的助手 id，缺省为 "default"
    #[serde(default)]
    assistant_id: Option<String>,
    /// 可切换模型：选用的模型 id，缺省为 "default"（使用配置）
    #[serde(default)]
    model_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatResponse {
    reply: String,
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    session_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct HistoryMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct HistoryResponse {
    session_id: String,
    messages: Vec<HistoryMessage>,
}

#[derive(Debug, Deserialize)]
struct ConsolidateQuery {
    #[serde(default)]
    since_days: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ConsolidateResponse {
    dates_processed: Vec<String>,
    blocks_added: usize,
}

#[derive(Debug, Deserialize)]
struct ClearSessionRequest {
    #[serde(default)]
    session_id: Option<String>,
}

/// 会话列表项
#[derive(Debug, Serialize)]
struct SessionListItem {
    id: String,
    title: String,
    message_count: usize,
    updated_at: String,
    /// 日期 YYYY-MM-DD，用于前端分组（今天/昨天/上周/更早）
    date: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RenameSessionRequest {
    session_id: String,
    title: String,
}

/// 多助手：前端展示用
#[derive(Debug, Clone, Serialize)]
struct AssistantInfo {
    id: String,
    name: String,
    description: String,
    /// 该智能体可用的技能（工具名列表）
    #[serde(skip_serializing_if = "Option::is_none")]
    skills: Option<Vec<String>>,
}

/// 工具信息：供前端技能配置使用
#[derive(Debug, Clone, Serialize)]
struct ToolInfo {
    id: String,
    name: String,
    description: String,
}

/// 可切换模型：前端展示用
#[derive(Debug, Clone, Serialize)]
struct ModelInfo {
    id: String,
    name: String,
}

/// 技能信息：前端展示用
#[derive(Debug, Clone, Serialize)]
struct SkillInfo {
    id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    capability: String,
    template: Option<String>,
    has_script: bool,
}

impl From<&Skill> for SkillInfo {
    fn from(s: &Skill) -> Self {
        Self {
            id: s.meta.id.clone(),
            name: s.meta.name.clone(),
            description: s.meta.description.clone(),
            tags: s.meta.tags.clone(),
            capability: s.capability.clone(),
            template: s.template.clone(),
            has_script: s.script_path.is_some(),
        }
    }
}

/// models.toml 中单条配置
#[derive(Debug, Clone, Deserialize)]
struct ModelEntry {
    id: String,
    name: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    api_key_env: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelsConfig {
    models: Vec<ModelEntry>,
}

/// assistants.toml 中单条配置
#[derive(Debug, Clone, Deserialize)]
struct AssistantEntry {
    id: String,
    name: String,
    description: String,
    prompt: String,
    /// 该智能体可用的技能（工具名列表），缺省则使用全部
    #[serde(default)]
    skills: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct AssistantsConfig {
    assistants: Vec<AssistantEntry>,
}

/// 单文件技能：config/skills/xxx.toml 内为 [assistant] 表
#[derive(Debug, Deserialize)]
struct SingleSkillConfig {
    assistant: AssistantEntry,
}

/// 自动分派：根据用户提问调用 LLM 选择最合适的助手，返回 assistant_id
async fn dispatch_assistant(
    state: &AppState,
    message: &str,
) -> Result<String, String> {
    let candidates: Vec<&AssistantInfo> = state.assistants.iter().filter(|a| a.id != "auto").collect();
    if candidates.is_empty() {
        return Ok("default".to_string());
    }
    let list_text = candidates
        .iter()
        .map(|a| format!("- {} (id={}): {}", a.name, a.id, a.description))
        .collect::<Vec<_>>()
        .join("\n");
    let system = format!(
        "You are a router. Given the user's question and the list of assistants below, choose the most suitable one.\n\
         Reply with ONLY the assistant id (e.g. default, media, student, money, viral). No explanation, no punctuation.\n\n\
         Available assistants:\n{}",
        list_text
    );
    let user_msg = format!("User question:\n{}", message);
    let messages = vec![Message::user(user_msg)];
    let components = state.components.read().await;
    let output = components
        .planner
        .plan_with_system(&messages, &system)
        .await
        .map_err(|e| e.to_string())?;
    let id = output
        .trim()
        .split(|c: char| c.is_whitespace() || c == '.' || c == '。')
        .next()
        .unwrap_or("default")
        .to_lowercase();
    let valid = candidates.iter().any(|a| a.id == id);
    Ok(if valid { id } else { "default".to_string() })
}

/// 从 config/assistant_skills.json 加载页面配置的技能覆盖（可选）
fn load_skills_overrides(config_base: &std::path::Path) -> HashMap<String, Vec<String>> {
    let paths = [
        config_base.join("assistant_skills.json"),
        std::path::Path::new("config/assistant_skills.json").to_path_buf(),
        std::path::Path::new("../config/assistant_skills.json").to_path_buf(),
    ];
    for p in &paths {
        if let Ok(s) = std::fs::read_to_string(p) {
            if let Ok(m) = serde_json::from_str(&s) {
                return m;
            }
        }
    }
    HashMap::new()
}

/// 保存技能覆盖到 config/assistant_skills.json
fn save_skills_overrides(config_base: &std::path::Path, overrides: &HashMap<String, Vec<String>>) -> std::io::Result<()> {
    let path = config_base.join("assistant_skills.json");
    std::fs::create_dir_all(config_base).ok();
    let s = serde_json::to_string_pretty(overrides).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, s)
}

/// 从 config/assistants.toml 与 config/skills/*.toml 加载助手；后者与前者 id 冲突时以 skills 为准。
/// tool_descriptions: (name, description) 列表，用于按 skills 过滤后注入 prompt
fn load_assistants(
    config_base: &std::path::Path,
    tool_descriptions: &[(String, String)],
) -> (
    Vec<AssistantInfo>,
    HashMap<String, String>,
    HashMap<String, Vec<String>>,
    HashMap<String, AssistantEntry>,
) {
    let toml_path = [
        config_base.join("assistants.toml"),
        std::path::Path::new("config/assistants.toml").to_path_buf(),
        std::path::Path::new("../config/assistants.toml").to_path_buf(),
    ]
    .into_iter()
    .find(|p| p.exists());

    let mut entries: Vec<AssistantEntry> = match toml_path.and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(s) => toml::from_str::<AssistantsConfig>(&s)
            .map(|c| c.assistants)
            .unwrap_or_default(),
        None => vec![
            AssistantEntry {
                id: "default".to_string(),
                name: "通用助手".to_string(),
                description: "全能型个人助手".to_string(),
                prompt: "prompts/system.md".to_string(),
                skills: None,
            },
        ],
    };

    // 从 config/skills/*.toml 合并：每个文件一个 [assistant]，同 id 覆盖
    let skills_dirs = [
        config_base.join("skills"),
        std::path::Path::new("config/skills").to_path_buf(),
        std::path::Path::new("../config/skills").to_path_buf(),
    ];
    for skills_dir in skills_dirs {
        if let Ok(rd) = std::fs::read_dir(&skills_dir) {
            let mut skill_entries: Vec<AssistantEntry> = Vec::new();
            for entry in rd.flatten() {
                let path = entry.path();
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if stem.starts_with('_') || stem.starts_with('.') {
                    continue;
                }
                if path.extension().map_or(true, |e| e != "toml") {
                    continue;
                }
                if let Ok(s) = std::fs::read_to_string(&path) {
                    if let Ok(parsed) = toml::from_str::<SingleSkillConfig>(&s) {
                        skill_entries.push(parsed.assistant);
                    }
                }
            }
            for e in skill_entries {
                if let Some(old) = entries.iter_mut().find(|x| x.id == e.id) {
                    *old = e;
                } else {
                    entries.push(e);
                }
            }
            break;
        }
    }

    let overrides = load_skills_overrides(config_base);
    let tool_schema = tool_call_schema_json();
    let base = if config_base.is_absolute() {
        config_base.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(config_base)
    };
    let all_names: std::collections::HashSet<_> =
        tool_descriptions.iter().map(|(n, _)| n.as_str()).collect();
    let mut prompts = HashMap::new();
    let mut skills_map = HashMap::new();
    let mut entries_map = HashMap::new();
    for e in &entries {
        let allowed: Vec<String> = overrides.get(&e.id)
            .cloned()
            .or_else(|| match &e.skills {
                Some(s) if !s.is_empty() => Some(s
                    .iter()
                    .filter(|n| all_names.contains(n.as_str()))
                    .cloned()
                    .collect()),
                _ => Some(tool_descriptions.iter().map(|(n, _)| n.clone()).collect()),
            })
            .unwrap_or_else(|| tool_descriptions.iter().map(|(n, _)| n.clone()).collect());
        skills_map.insert(e.id.clone(), allowed.clone());
        entries_map.insert(e.id.clone(), e.clone());

        let tool_list: String = tool_descriptions
            .iter()
            .filter(|(name, _)| allowed.contains(name))
            .map(|(name, desc)| format!("- {}: {}", name, desc))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt_path = [
            base.join(&e.prompt),
            std::path::Path::new("config").join(&e.prompt),
            std::path::Path::new("../config").join(&e.prompt),
        ]
        .into_iter()
        .find(|p| p.exists());

        let content = prompt_path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_else(|| format!("You are {}, a helpful assistant.", e.name));

        let tools_section = if tool_list.is_empty() {
            String::new()
        } else {
            format!("\n\nAvailable tools:\n{}\n", tool_list)
        };
        let full = if tool_schema.is_empty() {
            format!("{}{}", content, tools_section)
        } else {
            format!(
                "{}{}\n\n## Tool call JSON Schema (you must output valid JSON matching this)\n```json\n{}\n```",
                content, tools_section, tool_schema
            )
        };
        prompts.insert(e.id.clone(), full);
    }
    let list: Vec<AssistantInfo> = entries
        .iter()
        .map(|e| AssistantInfo {
            id: e.id.clone(),
            name: e.name.clone(),
            description: e.description.clone(),
            skills: Some(skills_map.get(&e.id).cloned().unwrap_or_default()),
        })
        .collect();
    (list, prompts, skills_map, entries_map)
}

/// 从 config/models.toml 加载可切换模型
fn load_models(config_base: &std::path::Path) -> (Vec<ModelInfo>, HashMap<String, ModelEntry>) {
    let toml_path = [
        config_base.join("models.toml"),
        std::path::Path::new("config/models.toml").to_path_buf(),
        std::path::Path::new("../config/models.toml").to_path_buf(),
    ]
    .into_iter()
    .find(|p| p.exists());

    let entries: Vec<ModelEntry> = match toml_path.and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(s) => toml::from_str::<ModelsConfig>(&s)
            .map(|c| c.models)
            .unwrap_or_default(),
        None => vec![ModelEntry {
            id: "default".to_string(),
            name: "默认（配置）".to_string(),
            base_url: None,
            model: None,
            api_key_env: None,
        }],
    };

    let list: Vec<ModelInfo> = entries
        .iter()
        .map(|e| ModelInfo {
            id: e.id.clone(),
            name: e.name.clone(),
        })
        .collect();

    let mut configs = HashMap::new();
    for e in entries {
        configs.insert(e.id.clone(), e);
    }
    (list, configs)
}

/// 根据模型配置创建 LlmClient（OpenAI 兼容）
fn create_llm_for_model(entry: &ModelEntry) -> Arc<dyn bee::llm::LlmClient> {
    let base_url = entry.base_url.as_deref();
    let model = entry
        .model
        .as_deref()
        .unwrap_or("gpt-4o-mini");
    let api_key = entry
        .api_key_env
        .as_deref()
        .and_then(|k| std::env::var(k).ok())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());
    Arc::new(bee::llm::OpenAiClient::new(
        base_url,
        model,
        api_key.as_deref(),
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();

    let cfg = load_config(None).unwrap_or_default();
    let workspace = cfg
        .app
        .workspace_root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap().join("workspace"));
    let workspace = workspace.canonicalize().unwrap_or(workspace);
    std::fs::create_dir_all(&workspace).ok();

    let config_base = std::path::Path::new("config");
    let system_prompt = [
        config_base.join("prompts/system.md"),
        std::path::Path::new("../config/prompts/system.md").to_path_buf(),
    ]
    .into_iter()
    .find_map(|p| std::fs::read_to_string(&p).ok())
    .unwrap_or_else(|| "You are Bee, a helpful AI assistant. Use tools: cat, ls, echo, shell, search.".to_string());

    let (models, model_configs) = load_models(config_base);

    let config_base = config_base.to_path_buf();
    let components_inner = create_agent_components(&cfg, &workspace);
    let tool_descriptions = components_inner.executor.tool_descriptions();
    let (mut assistants, mut prompts_map, skills_map, assistant_entries) =
        load_assistants(&config_base, &tool_descriptions);
    if !prompts_map.contains_key("default") {
        let fallback = assistants
            .iter()
            .find(|a| a.id != "auto")
            .and_then(|a| prompts_map.get(&a.id).cloned())
            .unwrap_or_else(|| system_prompt.clone());
        prompts_map.insert("default".to_string(), fallback);
    }
    let assistant_prompts = Arc::new(RwLock::new(prompts_map));
    let assistant_skills = Arc::new(RwLock::new(skills_map));
    let components = Arc::new(RwLock::new(Arc::new(components_inner)));
    assistants.insert(
        0,
        AssistantInfo {
            id: "auto".to_string(),
            name: "自动分派助手".to_string(),
            description: "根据提问自动选择最合适的助手".to_string(),
            skills: None,
        },
    );

    let sessions_dir = workspace.join("sessions");
    let memory_root = memory_root(&workspace);
    std::fs::create_dir_all(&sessions_dir).ok();
    std::fs::create_dir_all(&memory_root).ok();

    let shared_vector_long_term = create_shared_vector_long_term(&workspace, &cfg);

    let skill_loader = Arc::new(SkillLoader::from_default());
    if let Err(e) = skill_loader.load_all().await {
        tracing::warn!("Failed to load skills: {}", e);
    }

    let state = Arc::new(AppState {
        config: cfg.clone(),
        components,
        sessions: Arc::new(RwLock::new(HashMap::new())),
        sessions_dir,
        memory_root: memory_root.clone(),
        workspace: workspace.clone(),
        shared_vector_long_term,
        assistants,
        assistant_prompts,
        assistant_skills,
        tool_descriptions,
        assistant_entries,
        config_base,
        models,
        model_configs,
        skill_loader,
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/js/marked.min.js", get(serve_marked_js))
        .route("/js/highlight.min.js", get(serve_highlight_js))
        .route("/css/github-dark.min.css", get(serve_highlight_css))
        .route("/api/chat", post(api_chat))
        .route("/api/chat/stream", post(api_chat_stream))
        .route("/api/history", get(api_history))
        .route("/api/sessions", get(api_sessions_list))
        .route("/api/session/clear", post(api_session_clear))
        .route("/api/compact", post(api_compact))
        .route("/api/session/rename", post(api_session_rename))
        .route("/api/assistants", get(api_assistants_list))
        .route("/api/tools", get(api_tools_list))
        .route("/api/assistant/:id/skills", axum::routing::put(api_assistant_skills_put))
        .route("/api/models", get(api_models_list))
        .route("/api/skills", get(api_skills_list))
        .route("/api/skills/:id", get(api_skill_get))
        .route("/api/skills/:id", axum::routing::put(api_skill_update))
        .route("/api/skills/import-openclaw", post(api_skill_import_openclaw))
        .route("/api/memory/consolidate", post(api_memory_consolidate))
        .route("/api/memory/consolidate-llm", post(api_memory_consolidate_llm))
        .route("/api/config/reload", post(api_config_reload))
        .route("/api/health", get(|| async { "OK" }))
        .with_state(Arc::clone(&state));

    // 定期整理记忆：每 24 小时将近期短期日志归纳写入长期记忆
    let memory_root_periodic = state.memory_root.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Ok(r) = consolidate_memory(&memory_root_periodic, 7) {
                if !r.dates_processed.is_empty() {
                    tracing::info!("memory consolidated: {} days, {} blocks", r.dates_processed.len(), r.blocks_added);
                }
            }
        }
    });

    // 向量快照定期保存（每 5 分钟）
    if state.shared_vector_long_term.is_some() {
        let vec_ref = state.shared_vector_long_term.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Some(v) = vec_ref.as_ref() {
                    v.save_snapshot();
                }
            }
        });
    }

    // 心跳：若配置启用了 heartbeat，后台定期让 Agent 自主检查待办与反思
    if cfg.heartbeat.enabled {
        let heartbeat_state = Arc::clone(&state);
        let interval_secs = cfg.heartbeat.interval_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            interval.tick().await; // 跳过启动后立即执行
            loop {
                interval.tick().await;
                let shared_vec = heartbeat_state.shared_vector_long_term.clone();
                let mut context = create_context_with_long_term(
                    &heartbeat_state.config,
                    DEFAULT_MAX_TURNS,
                    Some(&heartbeat_state.workspace),
                    shared_vec,
                );
                let guard = heartbeat_state.components.read().await;
                match process_message(&**guard, &mut context, HEARTBEAT_PROMPT, None).await {
                    Ok(reply) => {
                        tracing::info!("heartbeat ok: {}", reply.trim());
                        append_heartbeat_log(&heartbeat_state.memory_root, &reply);
                    }
                    Err(e) => {
                        tracing::warn!("heartbeat error: {:?}", e);
                        append_heartbeat_log(
                            &heartbeat_state.memory_root,
                            &format!("[heartbeat error] {:?}", e),
                        );
                    }
                }
            }
        });
        tracing::info!("heartbeat enabled, interval {}s", interval_secs);
    }

    let port = std::env::var("BEE_WEB_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(cfg.web.port);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Bee Web UI: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}

async fn serve_marked_js() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/javascript; charset=utf-8")
        .body(Body::from(include_str!("../../static/js/marked.min.js")))
        .unwrap()
}

async fn serve_highlight_js() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/javascript; charset=utf-8")
        .body(Body::from(include_str!("../../static/js/highlight.min.js")))
        .unwrap()
}

async fn serve_highlight_css() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(Body::from(include_str!("../../static/css/github-dark.min.css")))
        .unwrap()
}

/// 会话在磁盘上的路径：workspace/sessions/{session_id}.json（路径非法字符替换为 _）
fn session_path(sessions_dir: &std::path::Path, session_id: &str) -> PathBuf {
    let name = session_id.replace('/', "_").replace('\\', "_");
    sessions_dir.join(format!("{}.json", name))
}

/// 从磁盘加载会话：反序列化 SessionSnapshot，重建 ConversationMemory 与 FileLongTerm
fn load_session_from_disk(
    sessions_dir: &std::path::Path,
    session_id: &str,
    memory_root: &std::path::Path,
) -> Option<ContextManager> {
    let path = session_path(sessions_dir, session_id);
    let data = std::fs::read_to_string(&path).ok()?;
    let snap: SessionSnapshot = serde_json::from_str(&data).ok()?;
    let conversation = ConversationMemory::from_messages(snap.messages, snap.max_turns);
    let long_term = Arc::new(bee::memory::FileLongTerm::new(
        bee::memory::long_term_path(memory_root),
        2000,
    ));
    let cfg = load_config(None).unwrap_or_else(|_| AppConfig::default());
    let mut ctx = ContextManager::new(snap.max_turns)
        .with_long_term(long_term)
        .with_lessons_path(lessons_path(memory_root))
        .with_procedural_path(procedural_path(memory_root))
        .with_preferences_path(preferences_path(memory_root))
        .with_auto_lesson_on_hallucination(cfg.evolution.auto_lesson_on_hallucination)
        .with_record_tool_success(cfg.evolution.record_tool_success);
    ctx.conversation = conversation;
    Some(ctx)
}

/// 将会话写回磁盘（JSON 快照），并追加本轮对话到当日短期日志 memory/logs/YYYY-MM-DD.md
fn save_session_to_disk(
    sessions_dir: &std::path::Path,
    memory_root: &std::path::Path,
    session_id: &str,
    context: &ContextManager,
) {
    let path = session_path(sessions_dir, session_id);
    let snap = SessionSnapshot {
        messages: context.messages().to_vec(),
        max_turns: context.conversation.max_turns(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&snap) {
        let _ = std::fs::write(path, json);
    }
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let _ = append_daily_log(memory_root, &date, session_id, context.messages());
}

/// POST /api/memory/consolidate?since_days=7：手动触发记忆整理（截断式），将近期短期日志归纳写入长期记忆
async fn api_memory_consolidate(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ConsolidateQuery>,
) -> Result<Json<ConsolidateResponse>, (StatusCode, String)> {
    let since_days = q.since_days.unwrap_or(7);
    let r = consolidate_memory(&state.memory_root, since_days)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ConsolidateResponse {
        dates_processed: r.dates_processed,
        blocks_added: r.blocks_added,
    }))
}

/// POST /api/memory/consolidate-llm?since_days=7：用 LLM 对近期每日日志做摘要后写入长期记忆（EVOLUTION §3.3）
async fn api_memory_consolidate_llm(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ConsolidateQuery>,
) -> Result<Json<ConsolidateResponse>, (StatusCode, String)> {
    let since_days = q.since_days.unwrap_or(7);
    let components = state.components.read().await;
    let r = consolidate_memory_with_llm(&components.planner, &state.workspace, since_days)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ConsolidateResponse {
        dates_processed: r.dates_processed,
        blocks_added: r.blocks_added,
    }))
}

/// POST /api/config/reload：重新加载配置并重建 Agent 组件（LLM/Planner/Recovery/Critic 等），实现运行时多 LLM 后端切换（白皮书 Phase 5）
async fn api_config_reload(
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, (StatusCode, String)> {
    let _ = bee::config::reload_config();
    let cfg = load_config(None).unwrap_or_default();
    let new_components = Arc::new(create_agent_components(&cfg, &state.workspace));
    let mut guard = state.components.write().await;
    *guard = new_components;
    Ok(StatusCode::OK)
}

/// POST /api/compact：对指定会话执行 Context Compaction（摘要写入长期记忆并替换为摘要消息），请求体 { "session_id": "..." }
async fn api_compact(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClearSessionRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let session_id = match req.session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Err((StatusCode::BAD_REQUEST, "session_id is required".to_string())),
    };
    let mut context = state
        .sessions
        .write()
        .await
        .remove(&session_id)
        .unwrap_or_else(|| {
            load_session_from_disk(&state.sessions_dir, &session_id, &state.memory_root).unwrap_or_else(|| {
                create_context_with_long_term(
                    &state.config,
                    DEFAULT_MAX_TURNS,
                    Some(&state.workspace),
                    state.shared_vector_long_term.clone(),
                )
            })
        });
    let components = state.components.read().await;
    match compact_context(&components.planner, &mut context).await {
        Ok(()) => {
            save_session_to_disk(&state.sessions_dir, &state.memory_root, &session_id, &context);
            state.sessions.write().await.insert(session_id, context);
            Ok(StatusCode::OK)
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Compaction failed: {}", e),
        )),
    }
}

/// POST /api/session/clear：清除指定会话（从内存移除并删除磁盘文件），请求体可选 { "session_id": "..." }
async fn api_session_clear(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClearSessionRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let session_id = match req.session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Ok(StatusCode::OK),
    };
    {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id);
    }
    let path = session_path(&state.sessions_dir, &session_id);
    let _ = std::fs::remove_file(&path);
    Ok(StatusCode::OK)
}

/// GET /api/sessions：列出所有会话（从磁盘读取），按更新时间倒序
async fn api_sessions_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SessionListItem>>, (StatusCode, String)> {
    let mut items = Vec::new();
    let entries = std::fs::read_dir(&state.sessions_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "json") {
            continue;
        }
        let id = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        
        // 读取会话快照获取消息
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let snap: SessionSnapshot = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => continue,
        };
        
        // 提取标题：第一条用户消息的前 50 字符
        let title = snap.messages.iter()
            .find(|m| matches!(m.role, Role::User) && !m.content.trim().starts_with("Observation from "))
            .map(|m| {
                let t = m.content.trim();
                if t.chars().count() > 50 {
                    format!("{}...", t.chars().take(50).collect::<String>())
                } else {
                    t.to_string()
                }
            })
            .unwrap_or_else(|| "新对话".to_string());
        
        // 获取文件修改时间
        let (updated_at, date) = entry.metadata()
            .and_then(|m| m.modified())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Local> = t.into();
                (
                    dt.format("%m-%d %H:%M").to_string(),
                    dt.format("%Y-%m-%d").to_string(),
                )
            })
            .unwrap_or_else(|_| (String::new(), String::new()));
        
        items.push(SessionListItem {
            id,
            title,
            message_count: snap.messages.len(),
            updated_at,
            date,
        });
    }
    
    // 按更新时间倒序（最新在前）
    items.sort_by(|a, b| b.date.cmp(&a.date).then(b.updated_at.cmp(&a.updated_at)));
    
    Ok(Json(items))
}

/// POST /api/session/rename：重命名会话（更新标题，存储在元数据中）
async fn api_session_rename(
    State(_state): State<Arc<AppState>>,
    Json(_req): Json<RenameSessionRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // TODO: 实现会话元数据存储以支持自定义标题
    Ok(StatusCode::OK)
}

/// GET /api/assistants：返回多助手列表（含 skills），供前端选择与配置
async fn api_assistants_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<AssistantInfo>>, (StatusCode, String)> {
    let skills = state.assistant_skills.read().await;
    let list: Vec<AssistantInfo> = state
        .assistants
        .iter()
        .map(|a| {
            let skills_val = skills.get(&a.id).cloned();
            AssistantInfo {
                id: a.id.clone(),
                name: a.name.clone(),
                description: a.description.clone(),
                skills: skills_val.or(a.skills.clone()),
            }
        })
        .collect();
    Ok(Json(list))
}

/// GET /api/tools：返回可用工具列表，供技能配置使用
async fn api_tools_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ToolInfo>>, (StatusCode, String)> {
    let list: Vec<ToolInfo> = state
        .tool_descriptions
        .iter()
        .map(|(id, desc)| ToolInfo {
            id: id.clone(),
            name: id.clone(),
            description: desc.clone(),
        })
        .collect();
    Ok(Json(list))
}

#[derive(Debug, Deserialize)]
struct UpdateSkillsRequest {
    skills: Vec<String>,
}

/// PUT /api/assistant/:id/skills：更新该智能体的技能配置，持久化到 config/assistant_skills.json
async fn api_assistant_skills_put(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<UpdateSkillsRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if id == "auto" {
        return Err((StatusCode::BAD_REQUEST, "无法配置自动分派助手的技能".to_string()));
    }
    let all_tools: std::collections::HashSet<_> =
        state.tool_descriptions.iter().map(|(n, _)| n.as_str()).collect();
    let skills: Vec<String> = req
        .skills
        .into_iter()
        .filter(|n| all_tools.contains(n.as_str()))
        .collect();

    let tool_schema = tool_call_schema_json();
    let base = &state.config_base;
    let tool_descriptions = &state.tool_descriptions;
    let entry = state
        .assistant_entries
        .get(&id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "智能体不存在".to_string()))?;

    let tool_list: String = tool_descriptions
        .iter()
        .filter(|(name, _)| skills.contains(name))
        .map(|(name, desc)| format!("- {}: {}", name, desc))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt_path = [
        base.join(&entry.prompt),
        std::path::Path::new("config").join(&entry.prompt),
        std::path::Path::new("../config").join(&entry.prompt),
    ]
    .into_iter()
    .find(|p| p.exists());

    let content = prompt_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| format!("You are {}, a helpful assistant.", entry.name));

    let tools_section = if tool_list.is_empty() {
        String::new()
    } else {
        format!("\n\nAvailable tools:\n{}\n", tool_list)
    };
    let full = if tool_schema.is_empty() {
        format!("{}{}", content, tools_section)
    } else {
        format!(
            "{}{}\n\n## Tool call JSON Schema (you must output valid JSON matching this)\n```json\n{}\n```",
            content, tools_section, tool_schema
        )
    };

    {
        let mut prompts = state.assistant_prompts.write().await;
        prompts.insert(id.clone(), full);
    }
    {
        let mut skills_map = state.assistant_skills.write().await;
        skills_map.insert(id.clone(), skills.clone());
    }

    let mut overrides = load_skills_overrides(base);
    overrides.insert(id, skills);
    save_skills_overrides(base, &overrides).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("保存配置失败: {}", e),
        )
    })?;
    Ok(StatusCode::OK)
}

/// GET /api/models：返回可切换模型列表（id、name）
async fn api_models_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ModelInfo>>, (StatusCode, String)> {
    Ok(Json(state.models.clone()))
}

/// GET /api/skills：返回所有技能列表
async fn api_skills_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SkillInfo>>, (StatusCode, String)> {
    let cache = state.skill_loader.cache();
    let skills = cache.read().await;
    let list: Vec<SkillInfo> = skills.values().map(SkillInfo::from).collect();
    Ok(Json(list))
}

/// GET /api/skills/:id：获取单个技能详情
async fn api_skill_get(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<SkillInfo>, (StatusCode, String)> {
    let skill = state
        .skill_loader
        .get(&id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("技能 {} 不存在", id)))?;
    Ok(Json(SkillInfo::from(&skill)))
}

#[derive(Debug, Deserialize)]
struct UpdateSkillRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    capability: Option<String>,
    #[serde(default)]
    template: Option<String>,
}

/// PUT /api/skills/:id：更新技能（保存到文件）
async fn api_skill_update(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<UpdateSkillRequest>,
) -> Result<Json<SkillInfo>, (StatusCode, String)> {
    let skill = state
        .skill_loader
        .get(&id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("技能 {} 不存在", id)))?;

    let skill_dir = &skill.dir;

    if req.name.is_some() || req.description.is_some() || req.tags.is_some() {
        let mut meta = skill.meta.clone();
        if let Some(name) = req.name {
            meta.name = name;
        }
        if let Some(description) = req.description {
            meta.description = description;
        }
        if let Some(tags) = req.tags {
            meta.tags = tags;
        }

        let toml_content = format!(
            "[skill]\nid = \"{}\"\nname = \"{}\"\ndescription = \"{}\"\ntags = {:?}\n",
            meta.id, meta.name, meta.description, meta.tags
        );
        if let Some(script) = &meta.script {
            let toml_content = format!("{}script = \"{}\"\n", toml_content, script);
            std::fs::write(skill_dir.join("skill.toml"), toml_content)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        } else {
            std::fs::write(skill_dir.join("skill.toml"), toml_content)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
    }

    if let Some(capability) = &req.capability {
        std::fs::write(skill_dir.join("capability.md"), capability)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    if let Some(template) = &req.template {
        if template.is_empty() {
            let _ = std::fs::remove_file(skill_dir.join("template.md"));
        } else {
            std::fs::write(skill_dir.join("template.md"), template)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
    }

    state
        .skill_loader
        .load_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let updated = state
        .skill_loader
        .get(&id)
        .await
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "重新加载失败".to_string()))?;
    Ok(Json(SkillInfo::from(&updated)))
}

/// OpenClaw skill.json 格式
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenClawSkillJson {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

/// 导入 OpenClaw 技能请求
#[derive(Debug, Deserialize)]
struct ImportOpenClawRequest {
    /// OpenClaw skill.json 内容 (JSON 字符串)
    skill_json: String,
    /// SKILL.md 内容
    #[serde(default)]
    skill_md: Option<String>,
    /// 可选：覆盖已有的同名技能
    #[serde(default)]
    overwrite: bool,
}

/// 导入 OpenClaw 技能：将 OpenClaw 格式转换为 Bee 格式并保存
async fn api_skill_import_openclaw(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ImportOpenClawRequest>,
) -> Result<Json<SkillInfo>, (StatusCode, String)> {
    let openclaw: OpenClawSkillJson = serde_json::from_str(&req.skill_json)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("无效的 skill.json: {}", e)))?;
    
    let skill_id = openclaw.name
        .to_lowercase()
        .replace(' ', "-")
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "");
    
    if skill_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "技能名称无效".to_string()));
    }

    let existing = state.skill_loader.get(&skill_id).await;
    if existing.is_some() && !req.overwrite {
        return Err((StatusCode::CONFLICT, format!("技能 '{}' 已存在，使用 overwrite=true 覆盖", skill_id)));
    }

    let skill_dir = state.skill_loader.skills_dir().join(&skill_id);
    std::fs::create_dir_all(&skill_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("创建目录失败: {}", e)))?;

    let description = openclaw.description.as_deref().unwrap_or("从 OpenClaw 导入的技能");
    let tags = openclaw.tags.unwrap_or_default();
    let toml_content = format!(
        "[skill]\nid = \"{}\"\nname = \"{}\"\ndescription = \"{}\"\ntags = {:?}\n",
        skill_id, openclaw.name, description, tags
    );
    std::fs::write(skill_dir.join("skill.toml"), toml_content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("写入 skill.toml 失败: {}", e)))?;

    let capability = req.skill_md.unwrap_or_else(|| {
        format!(
            "# {}\n\n{}\n\n- Author: {}\n- License: {}",
            openclaw.name,
            description,
            openclaw.author.as_deref().unwrap_or("unknown"),
            openclaw.license.as_deref().unwrap_or("MIT")
        )
    });
    std::fs::write(skill_dir.join("capability.md"), capability)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("写入 capability.md 失败: {}", e)))?;

    state
        .skill_loader
        .load_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let imported = state
        .skill_loader
        .get(&skill_id)
        .await
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "导入后无法加载技能".to_string()))?;
    
    tracing::info!("Imported OpenClaw skill: {} ({})", openclaw.name, skill_id);
    Ok(Json(SkillInfo::from(&imported)))
}

/// GET /api/history?session_id=...：返回该会话的对话列表，过滤掉 Tool call / Observation 等内部消息
async fn api_history(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, (StatusCode, String)> {
    let session_id = match q.session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Err((StatusCode::BAD_REQUEST, "session_id is required".to_string())),
    };
    let context_opt = {
        let sessions = state.sessions.read().await;
        sessions.get(&session_id).cloned()
    };
    let context = match context_opt {
        Some(c) => c,
        None => {
            if let Some(loaded) = load_session_from_disk(&state.sessions_dir, &session_id, &state.memory_root) {
                loaded
            } else {
                return Ok(Json(HistoryResponse {
                    session_id: session_id.clone(),
                    messages: vec![],
                }));
            }
        }
    };
    // 主聊天区不展示内部消息：User 的 "Observation from ..."、Assistant 的 "Tool call: ..."
    let messages: Vec<HistoryMessage> = context
        .messages()
        .iter()
        .filter(|m| !matches!(m.role, Role::System))
        .filter(|m| {
            let c = m.content.trim();
            if matches!(m.role, Role::User) {
                !c.starts_with("Observation from ") && !c.starts_with("Critic 建议：")
            } else {
                !c.starts_with("Tool call:")  // 任意 "Tool call:..." 均过滤，不依赖 " | Result: "
            }
        })
        .map(|m: &Message| HistoryMessage {
            role: match m.role {
                Role::User => "user".to_string(),
                Role::Assistant => "assistant".to_string(),
                Role::System => "system".to_string(),
                Role::Tool => "tool".to_string(),
            },
            content: m.content.clone(),
        })
        .collect();
    Ok(Json(HistoryResponse {
        session_id: session_id.clone(),
        messages,
    }))
}

async fn api_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    let message = req.message.trim();
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message is required".to_string()));
    }

    let session_id = req
        .session_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut context = {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id).unwrap_or_else(|| {
            load_session_from_disk(&state.sessions_dir, &session_id, &state.memory_root).unwrap_or_else(|| {
                create_context_with_long_term(
                    &state.config,
                    DEFAULT_MAX_TURNS,
                    Some(&state.workspace),
                    state.shared_vector_long_term.clone(),
                )
            })
        })
    };

    let components = state.components.read().await.clone();
    let assistant_id = req.assistant_id.as_deref().unwrap_or("default");
    let allowed = state.assistant_skills.read().await.get(assistant_id).cloned();
    let reply = process_message(components.as_ref(), &mut context, message, allowed.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(session_id.clone(), context.clone());
        save_session_to_disk(&state.sessions_dir, &state.memory_root, &session_id, &context);
    }

    Ok(Json(ChatResponse {
        reply,
        session_id,
    }))
}

/// 流式聊天：NDJSON 流，首行 session_id，后续为 ReactEvent
async fn api_chat_stream(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<impl axum::response::IntoResponse, (StatusCode, String)> {
    let message = req.message.trim().to_string();
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message is required".to_string()));
    }

    let session_id = req
        .session_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let model_id = req.model_id.as_deref().unwrap_or("default").to_string();
    let mut assistant_id = req.assistant_id.as_deref().unwrap_or("default").to_string();
    let mut dispatched_name: Option<String> = None;
    if assistant_id == "auto" {
        match dispatch_assistant(&state, &message).await {
            Ok(id) => {
                assistant_id = id.clone();
                dispatched_name = state.assistants.iter().find(|a| a.id == id).map(|a| a.name.clone());
            }
            Err(e) => {
                tracing::warn!("Auto dispatch failed: {}, using default", e);
                assistant_id = "default".to_string();
            }
        }
    }
    let system_prompt_override = state.assistant_prompts.read().await.get(&assistant_id).cloned();

    let context = {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id).unwrap_or_else(|| {
            load_session_from_disk(&state.sessions_dir, &session_id, &state.memory_root).unwrap_or_else(|| {
                create_context_with_long_term(
                    &state.config,
                    DEFAULT_MAX_TURNS,
                    Some(&state.workspace),
                    state.shared_vector_long_term.clone(),
                )
            })
        })
    };

    let (event_tx, event_rx) = mpsc::unbounded_channel::<ReactEvent>();
    let (context_tx, context_rx) = tokio::sync::oneshot::channel();

    let allowed_for_spawn = state.assistant_skills.read().await.get(&assistant_id).cloned();
    let components = state.components.read().await.clone();
    let session_id_clone = session_id.clone();
    let state_spawn = Arc::clone(&state);
    let model_configs = state.model_configs.clone();
    tokio::spawn(async move {
        let mut ctx = context;
        let prompt_ref = system_prompt_override.as_deref();
        let planner_override: Option<Arc<Planner>> = if model_id != "default" {
            model_configs.get(&model_id).map(|entry| {
                let llm = create_llm_for_model(entry);
                let sys = prompt_ref
                    .unwrap_or_else(|| components.planner.base_system_prompt())
                    .to_string();
                Arc::new(Planner::new(llm, sys))
            })
        } else {
            None
        };
        let planner_ref = planner_override.as_deref();
        let allowed = allowed_for_spawn.as_deref();
        let _ = process_message_stream(
            components.as_ref(),
            &mut ctx,
            &message,
            event_tx,
            prompt_ref,
            planner_ref,
            allowed,
        )
        .await;
        // 无论流是否被客户端断开（超时/刷新），都持久化当前会话（含用户刚发的提问），刷新后历史不丢
        save_session_to_disk(
            &state_spawn.sessions_dir,
            &state_spawn.memory_root,
            &session_id_clone,
            &ctx,
        );
        let mut sessions = state_spawn.sessions.write().await;
        sessions.insert(session_id_clone.clone(), ctx);
        let _ = context_tx.send(());
    });

    let mut first_line = format!(
        "{}\n",
        serde_json::to_string(&serde_json::json!({
            "type": "session_id",
            "session_id": session_id
        }))
        .unwrap()
    );
    if let Some(ref name) = dispatched_name {
        first_line.push_str(
            &format!(
                "{}\n",
                serde_json::to_string(&serde_json::json!({
                    "type": "assistant_dispatched",
                    "assistant_id": assistant_id,
                    "assistant_name": name
                }))
                .unwrap()
            ),
        );
    }

    let state_reinsert = Arc::clone(&state);
    let session_id_reinsert = session_id.clone();
    let stream = stream::try_unfold(
        (
            state_reinsert,
            session_id_reinsert,
            context_rx,
            event_rx,
            Some(first_line),
        ),
        move |(state_reinsert, session_id_reinsert, context_rx, mut event_rx, first_line_opt)| async move {
            if let Some(line) = first_line_opt {
                return Ok(Some((
                    Bytes::from(line),
                    (state_reinsert, session_id_reinsert, context_rx, event_rx, None),
                )));
            }
            match event_rx.recv().await {
                Some(ev) => {
                    // 自我改进：工具失败 → ERRORS.md；Critic 纠正 → LEARNINGS.md (correction)
                    match &ev {
                        ReactEvent::ToolFailure { tool, reason } => {
                            learnings_record_error(&state_reinsert.workspace, tool, reason);
                        }
                        ReactEvent::Recovery { action, detail } if action == "Critic" => {
                            learnings_record_learning(
                                &state_reinsert.workspace,
                                "correction",
                                detail,
                                None,
                            );
                        }
                        _ => {}
                    }
                    let line = format!("{}\n", serde_json::to_string(&ev).unwrap());
                    Ok(Some((
                        Bytes::from(line),
                        (state_reinsert, session_id_reinsert, context_rx, event_rx, None),
                    )))
                }
                None => {
                    let _ = context_rx.await;
                    Ok(None)
                }
            }
        },
    );

    type BoxErr = Box<dyn std::error::Error + Send + Sync>;
    let stream = stream.map_err(|e: tokio::sync::oneshot::error::RecvError| Box::new(e) as BoxErr);

    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "application/x-ndjson; charset=utf-8",
        )],
        Body::from_stream(stream),
    ))
}
