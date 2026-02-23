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
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, Response,
    },
    routing::{get, post},
    Json, Router,
};
use bee::memory::{Message, Role};
use bytes::Bytes;
use futures_util::stream::{self, TryStreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use bee::agent::{
    consolidate_memory_with_llm, create_agent_components, create_context_with_long_term_for_assistant,
    create_vector_long_term_for_assistant, process_message, process_message_stream,
};
use bee::core::AgentComponents;
use bee::skills::{Skill, SkillLoader};
use bee::tools::{tool_call_schema_json, CreateTool, DynamicAgent};
use bee::memory::InMemoryVectorLongTerm;
use bee::config::{load_config, AppConfig};
use bee::memory::{
    append_daily_log, append_heartbeat_log, assistant_memory_root, consolidate_memory,
    lessons_path, preferences_path, procedural_path,
    record_error as learnings_record_error, record_learning as learnings_record_learning,
    ConversationMemory, memory_root,
};
use bee::react::{compact_context, ContextManager, Planner, ReactEvent};

/// 会话快照：仅持久化对话消息，重启后恢复
#[derive(serde::Serialize, serde::Deserialize)]
struct SessionSnapshot {
    messages: Vec<Message>,
    max_turns: usize,
}

/// 群聊消息（含发送者标识）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct GroupChatMessage {
    role: String, // "user" | "assistant"
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    assistant_id: Option<String>,
}

/// 群聊会话快照
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct GroupChatSnapshot {
    messages: Vec<GroupChatMessage>,
    max_turns: usize,
}

/// 群组定义
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct GroupInfo {
    id: String,
    name: Option<String>,
    member_ids: Vec<String>,
    created_at: String,
}

const DEFAULT_MAX_TURNS: usize = 20;

/// 拓扑事件（Phase 4）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WorkspaceEvent {
    GroupCreated {
        id: String,
        name: Option<String>,
        member_ids: Vec<String>,
    },
    MessageCreated {
        group_id: String,
        from: Option<String>,
        to: Option<String>,
        content_preview: String,
    },
    AgentCreated {
        id: String,
        role: String,
        parent_id: Option<String>,
    },
    TaskCreated { id: String, title: String },
    TaskUpdated { id: String, status: String },
}

struct CreateObservationParsed {
    id: String,
    role: String,
    parent_id: Option<String>,
}

/// 从 create 工具 Observation preview 解析 id、role
fn parse_create_observation(preview: &str) -> Option<CreateObservationParsed> {
    let re = regex::Regex::new(r"id=([a-zA-Z0-9_-]+),\s*role=([^.]+)").ok()?;
    let cap = re.captures(preview)?;
    let id = cap.get(1)?.as_str().to_string();
    let role = cap.get(2)?.as_str().trim().to_string();
    Some(CreateObservationParsed { id, role, parent_id: None })
}

fn emit_event(bus: &broadcast::Sender<String>, ev: WorkspaceEvent) {
    if let Ok(json) = serde_json::to_string(&ev) {
        let _ = bus.send(json);
    }
}

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
    /// 每个助手的向量长期记忆（assistant_id -> Arc），启用时按需创建
    shared_vector_by_assistant: Arc<RwLock<HashMap<String, Arc<InMemoryVectorLongTerm>>>>,
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
    /// 群组：id -> GroupInfo
    groups: Arc<RwLock<HashMap<String, GroupInfo>>>,
    /// 群组持久化路径
    groups_path: PathBuf,
    /// 拓扑事件广播（SSE /api/events）
    event_bus: broadcast::Sender<String>,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
    /// 多助手：选用的助手 id，缺省为 "default"
    #[serde(default)]
    assistant_id: Option<String>,
    /// 群聊：group_id 与 assistant_id 互斥，有 group_id 时为群聊模式
    #[serde(default)]
    group_id: Option<String>,
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
struct CreateGroupRequest {
    name: Option<String>,
    member_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CreateAgentRequest {
    role: String,
    #[serde(default)]
    guidance: Option<String>,
}

/// 任务状态：看板列
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Todo,
    InProgress,
    Done,
}

/// 任务
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    id: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    status: TaskStatus,
    #[serde(default)]
    assignee_ids: Vec<String>,
    #[serde(default)]
    group_id: Option<String>,
    /// 统筹负责人 agent id，负责拆分任务、创建子 agent、组队、分配职责
    #[serde(default)]
    coordinator_id: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct CreateTaskRequest {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    assignee_ids: Vec<String>,
    #[serde(default)]
    coordinator_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateTaskRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<TaskStatus>,
    #[serde(default)]
    assignee_ids: Option<Vec<String>>,
    #[serde(default)]
    coordinator_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InboxProcessRequest {
    assistant_id: String,
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    session_id: Option<String>,
    #[serde(default)]
    assistant_id: Option<String>,
    /// 群聊：有 group_id 时按群加载历史，返回消息含 assistant_id
    #[serde(default)]
    group_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct HistoryMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    assistant_id: Option<String>,
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
    #[serde(default)]
    assistant_id: Option<String>,
}

/// 会话列表项
#[derive(Debug, Serialize)]
struct SessionListItem {
    /// 复合 key：{session_id}::{assistant_id}，用于 API 调用
    id: String,
    /// 会话 id
    session_id: String,
    /// 助手 id，该会话属于该助手的独立记忆
    assistant_id: String,
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

/// 从 workspace/agents.json 加载动态创建的 sub-agent（Phase 3）
fn load_dynamic_agents(workspace: &std::path::Path) -> Vec<DynamicAgent> {
    let path = workspace.join("agents.json");
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str(&data).unwrap_or_default()
}

const TASKS_FILE: &str = "tasks.json";

fn load_tasks(workspace: &std::path::Path) -> Vec<Task> {
    let path = workspace.join(TASKS_FILE);
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str(&data).unwrap_or_default()
}

fn save_tasks(workspace: &std::path::Path, tasks: &[Task]) {
    std::fs::create_dir_all(workspace).ok();
    let path = workspace.join(TASKS_FILE);
    if let Ok(json) = serde_json::to_string_pretty(tasks) {
        let _ = std::fs::write(path, json);
    }
}

/// 热更新：将 agents.json 中新 agent 并入 assistant_prompts / assistant_skills
async fn reload_dynamic_agents_into_state(state: &AppState) {
    let dynamic = load_dynamic_agents(&state.workspace);
    if dynamic.is_empty() {
        return;
    }
    let all_tool_list: String = state
        .tool_descriptions
        .iter()
        .map(|(n, d)| format!("- {}: {}", n, d))
        .collect::<Vec<_>>()
        .join("\n");
    let tool_schema = tool_call_schema_json();
    let mut prompts = state.assistant_prompts.write().await;
    let mut skills = state.assistant_skills.write().await;
    for da in &dynamic {
        if !prompts.contains_key(&da.id) {
            let prompt = dynamic_agent_prompt(da, &all_tool_list, &tool_schema);
            prompts.insert(da.id.clone(), prompt);
        }
        if !skills.contains_key(&da.id) {
            skills.insert(
                da.id.clone(),
                state.tool_descriptions.iter().map(|(n, _)| n.clone()).collect(),
            );
        }
    }
}

/// 为动态 agent 生成 system prompt
fn dynamic_agent_prompt(agent: &DynamicAgent, tool_list: &str, tool_schema: &str) -> String {
    let guidance = agent
        .guidance
        .as_deref()
        .unwrap_or("Follow your role and assist the user.");
    format!(
        "You are a sub-agent with role: {}. Guidance: {}\n\nAvailable tools:\n{}",
        agent.role,
        guidance,
        if tool_list.is_empty() {
            "".to_string()
        } else {
            format!(
                "{}\n\n## Tool call JSON Schema (you must output valid JSON matching this)\n```json\n{}\n```",
                tool_list, tool_schema
            )
        }
    )
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
    let skill_loader = components_inner.skill_loader.clone();
    let (mut assistants, mut prompts_map, mut skills_map, assistant_entries) =
        load_assistants(&config_base, &tool_descriptions);

    let dynamic = load_dynamic_agents(&workspace);
    let all_tool_list: String = tool_descriptions
        .iter()
        .map(|(n, d)| format!("- {}: {}", n, d))
        .collect::<Vec<_>>()
        .join("\n");
    let tool_schema = tool_call_schema_json();
    for da in &dynamic {
        if !prompts_map.contains_key(&da.id) {
            let prompt = dynamic_agent_prompt(da, &all_tool_list, &tool_schema);
            prompts_map.insert(da.id.clone(), prompt);
        }
        if assistants.iter().all(|a| a.id != da.id) {
            assistants.push(AssistantInfo {
                id: da.id.clone(),
                name: da.role.clone(),
                description: da.guidance.clone().unwrap_or_else(|| da.role.clone()),
                skills: Some(tool_descriptions.iter().map(|(n, _)| n.clone()).collect()),
            });
        }
        if !skills_map.contains_key(&da.id) {
            skills_map.insert(
                da.id.clone(),
                tool_descriptions.iter().map(|(n, _)| n.clone()).collect(),
            );
        }
    }

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

    let shared_vector_by_assistant = Arc::new(RwLock::new(HashMap::new()));

    let groups_path = workspace.join("groups.json");
    let groups = load_groups_from_disk(&groups_path);
    let (event_bus, _) = broadcast::channel::<String>(64);

    let state = Arc::new(AppState {
        config: cfg.clone(),
        components,
        sessions: Arc::new(RwLock::new(HashMap::new())),
        sessions_dir,
        memory_root: memory_root.clone(),
        workspace: workspace.clone(),
        shared_vector_by_assistant,
        assistants,
        assistant_prompts,
        assistant_skills,
        tool_descriptions,
        assistant_entries,
        config_base,
        models,
        model_configs,
        skill_loader,
        groups,
        groups_path,
        event_bus,
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/metrics", get(serve_metrics_dashboard))
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
        .route("/api/agents", get(api_agents_list).post(api_agents_create))
        .route("/api/groups", get(api_groups_list).post(api_groups_create))
        .route("/api/tasks", get(api_tasks_list).post(api_tasks_create))
        .route("/api/tasks/:id", axum::routing::patch(api_tasks_update))
        .route("/api/tasks/:id/start", post(api_tasks_start))
        .route("/api/inbox/process", post(api_inbox_process))
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
        .route("/api/metrics", get(api_metrics))
        .route("/api/metrics/prometheus", get(api_metrics_prometheus))
        .route("/api/events", get(api_events_sse))
        .route("/swarm", get(serve_swarm_page))
        .route("/tasks", get(serve_tasks_page))
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
    let vec_by_assistant_ref = state.shared_vector_by_assistant.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        interval.tick().await;
        loop {
            interval.tick().await;
            let map = vec_by_assistant_ref.read().await;
            for v in map.values() {
                v.save_snapshot();
            }
        }
    });

    // 心跳：若配置启用了 heartbeat，后台定期让 Agent 自主检查待办与反思
    if cfg.heartbeat.enabled {
        let heartbeat_state = Arc::clone(&state);
        let interval_secs = cfg.heartbeat.interval_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            interval.tick().await; // 跳过启动后立即执行
            loop {
                interval.tick().await;
                let shared_vec = {
                    let map = heartbeat_state.shared_vector_by_assistant.read().await;
                    map.get("default").cloned()
                };
                let mut context = create_context_with_long_term_for_assistant(
                    &heartbeat_state.config,
                    DEFAULT_MAX_TURNS,
                    Some(&heartbeat_state.workspace),
                    shared_vec,
                    Some("default"),
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

async fn serve_metrics_dashboard() -> Html<&'static str> {
    Html(include_str!("../../static/metrics.html"))
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

/// 会话的复合 key：{session_id}::{assistant_id}
fn session_key(session_id: &str, assistant_id: &str) -> String {
    format!("{}::{}", session_id, assistant_id)
}

/// 群聊会话路径：workspace/sessions/group_{group_id}.json
fn group_session_path(sessions_dir: &std::path::Path, group_id: &str) -> PathBuf {
    let safe_id: String = group_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    sessions_dir.join(format!("group_{}.json", safe_id))
}

/// 会话在磁盘上的路径：workspace/sessions/{session_id}---{assistant_id}.json（--- 为分隔符）
fn session_path(sessions_dir: &std::path::Path, session_id: &str, assistant_id: &str) -> PathBuf {
    let safe_sid: String = session_id
        .chars()
        .map(|c| if c == '/' || c == '\\' || c == '-' { '_' } else { c })
        .collect();
    let safe_aid: String = assistant_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let aid = if safe_aid.is_empty() { "default" } else { safe_aid.as_str() };
    sessions_dir.join(format!("{}---{}.json", safe_sid, aid))
}

fn load_groups_from_disk(path: &std::path::Path) -> Arc<RwLock<HashMap<String, GroupInfo>>> {
    let map: HashMap<String, GroupInfo> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Arc::new(RwLock::new(map))
}

fn save_groups_to_disk(path: &std::path::Path, groups: &HashMap<String, GroupInfo>) {
    if let Ok(json) = serde_json::to_string_pretty(groups) {
        let _ = std::fs::write(path, json);
    }
}

/// 加载群聊会话
fn load_group_session(
    sessions_dir: &std::path::Path,
    group_id: &str,
) -> Vec<GroupChatMessage> {
    let path = group_session_path(sessions_dir, group_id);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<GroupChatSnapshot>(&s).ok())
        .map(|snap| snap.messages)
        .unwrap_or_default()
}

/// 保存群聊会话
fn save_group_session(
    sessions_dir: &std::path::Path,
    group_id: &str,
    messages: &[GroupChatMessage],
    max_turns: usize,
) {
    let path = group_session_path(sessions_dir, group_id);
    let snap = GroupChatSnapshot {
        messages: messages.to_vec(),
        max_turns,
    };
    if let Ok(json) = serde_json::to_string_pretty(&snap) {
        let _ = std::fs::write(path, json);
    }
}

/// 将群聊消息转换为 LLM 上下文（带助手名）
fn group_messages_to_llm_messages(
    messages: &[GroupChatMessage],
    assistants: &[AssistantInfo],
) -> Vec<Message> {
    messages
        .iter()
        .map(|m| match m.role.as_str() {
            "user" => Message::user(&m.content),
            "assistant" => {
                let label = m
                    .assistant_id
                    .as_ref()
                    .and_then(|id| assistants.iter().find(|a| a.id == *id).map(|a| a.name.as_str()))
                    .unwrap_or("Assistant");
                Message::assistant(format!("{}: {}", label, m.content))
            }
            _ => Message::assistant(&m.content),
        })
        .collect()
}

/// 获取或创建指定助手的向量长期记忆
async fn get_or_create_vector_for_assistant(
    state: &AppState,
    assistant_id: &str,
) -> Option<Arc<InMemoryVectorLongTerm>> {
    let aid = if assistant_id.is_empty() { "default" } else { assistant_id };
    {
        let map = state.shared_vector_by_assistant.read().await;
        if let Some(v) = map.get(aid) {
            return Some(Arc::clone(v));
        }
    }
    if let Some(vec) = create_vector_long_term_for_assistant(&state.workspace, &state.config, Some(aid)) {
        let mut map = state.shared_vector_by_assistant.write().await;
        map.insert(aid.to_string(), Arc::clone(&vec));
        Some(vec)
    } else {
        None
    }
}

/// 从磁盘加载会话：反序列化 SessionSnapshot，重建 ConversationMemory 与 per-assistant 长期记忆
fn load_session_from_disk(
    sessions_dir: &std::path::Path,
    session_id: &str,
    assistant_id: &str,
    workspace: &std::path::Path,
    cfg: &AppConfig,
    vector_for_assistant: Option<Arc<InMemoryVectorLongTerm>>,
) -> Option<ContextManager> {
    // 尝试新格式 {session_id}_{assistant_id}.json
    let path = session_path(sessions_dir, session_id, assistant_id);
    let data = std::fs::read_to_string(&path).ok().or_else(|| {
        // 兼容旧格式：仅 session_id.json（视为 default 助手）
        if assistant_id == "default" {
            let legacy_path = sessions_dir.join(format!("{}.json", session_id.replace('/', "_").replace('\\', "_")));
            std::fs::read_to_string(&legacy_path).ok()
        } else {
            None
        }
    })?;
    let snap: SessionSnapshot = serde_json::from_str(&data).ok()?;
    let conversation = ConversationMemory::from_messages(snap.messages, snap.max_turns);
    let assistant_root = assistant_memory_root(workspace, assistant_id);
    std::fs::create_dir_all(&assistant_root).ok();
    let long_term: Arc<dyn bee::memory::LongTermMemory> = if let Some(vec) = vector_for_assistant {
        vec
    } else {
        Arc::new(bee::memory::FileLongTerm::new(
            bee::memory::long_term_path(&assistant_root),
            2000,
        ))
    };
    let mut ctx = ContextManager::new(snap.max_turns)
        .with_long_term(long_term)
        .with_lessons_path(lessons_path(&assistant_root))
        .with_procedural_path(procedural_path(&assistant_root))
        .with_preferences_path(preferences_path(&assistant_root))
        .with_auto_lesson_on_hallucination(cfg.evolution.auto_lesson_on_hallucination)
        .with_record_tool_success(cfg.evolution.record_tool_success);
    ctx.conversation = conversation;
    Some(ctx)
}

/// 将会话写回磁盘（JSON 快照），并追加本轮对话到当日短期日志 memory/{assistant_id}/logs/YYYY-MM-DD.md
fn save_session_to_disk(
    sessions_dir: &std::path::Path,
    workspace: &std::path::Path,
    session_id: &str,
    assistant_id: &str,
    context: &ContextManager,
) {
    let path = session_path(sessions_dir, session_id, assistant_id);
    let snap = SessionSnapshot {
        messages: context.messages().to_vec(),
        max_turns: context.conversation.max_turns(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&snap) {
        let _ = std::fs::write(path, json);
    }
    let assistant_root = assistant_memory_root(workspace, assistant_id);
    std::fs::create_dir_all(assistant_root.join("logs")).ok();
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let _ = append_daily_log(&assistant_root, &date, &format!("{}:{}", session_id, assistant_id), context.messages());
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

/// POST /api/compact：对指定会话执行 Context Compaction（摘要写入长期记忆并替换为摘要消息），请求体 { "session_id": "...", "assistant_id": "..." }
async fn api_compact(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClearSessionRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let session_id = match req.session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Err((StatusCode::BAD_REQUEST, "session_id is required".to_string())),
    };
    let assistant_id = req.assistant_id.as_deref().unwrap_or("default");
    let key = session_key(&session_id, assistant_id);
    let vector = get_or_create_vector_for_assistant(&state, assistant_id).await;
    let mut context = state
        .sessions
        .write()
        .await
        .remove(&key)
        .unwrap_or_else(|| {
            load_session_from_disk(
                &state.sessions_dir,
                &session_id,
                assistant_id,
                &state.workspace,
                &state.config,
                vector.clone(),
            )
            .unwrap_or_else(|| {
                create_context_with_long_term_for_assistant(
                    &state.config,
                    DEFAULT_MAX_TURNS,
                    Some(&state.workspace),
                    vector,
                    Some(assistant_id),
                )
            })
        });
    let components = state.components.read().await;
    match compact_context(&components.planner, &mut context).await {
        Ok(()) => {
            save_session_to_disk(
                &state.sessions_dir,
                &state.workspace,
                &session_id,
                assistant_id,
                &context,
            );
            state.sessions.write().await.insert(key, context);
            Ok(StatusCode::OK)
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Compaction failed: {}", e),
        )),
    }
}

/// POST /api/session/clear：清除指定会话（从内存移除并删除磁盘文件），请求体可选 { "session_id": "...", "assistant_id": "..." }
async fn api_session_clear(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClearSessionRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let session_id = match req.session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Ok(StatusCode::OK),
    };
    let assistant_id = req.assistant_id.as_deref().unwrap_or("default");
    let key = session_key(&session_id, assistant_id);
    {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&key);
    }
    let path = session_path(&state.sessions_dir, &session_id, assistant_id);
    let _ = std::fs::remove_file(&path);
    // 兼容旧格式：若存在 session_id.json 也删除
    if assistant_id == "default" {
        let legacy = state.sessions_dir.join(format!("{}.json", session_id.replace('/', "_").replace('\\', "_")));
        let _ = std::fs::remove_file(legacy);
    }
    Ok(StatusCode::OK)
}

/// GET /api/sessions：列出所有会话（从磁盘读取），按更新时间倒序。每个 (session_id, assistant_id) 为独立会话
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
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if stem.is_empty() {
            continue;
        }
        let (session_id, assistant_id) = if let Some(idx) = stem.find("---") {
            let (sid, aid) = stem.split_at(idx);
            (sid.to_string(), aid.trim_start_matches("---").to_string())
        } else {
            (stem.to_string(), "default".to_string())
        };
        let id = session_key(&session_id, &assistant_id);

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let snap: SessionSnapshot = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let title = snap
            .messages
            .iter()
            .find(|m| {
                matches!(m.role, Role::User)
                    && !m.content.trim().starts_with("Observation from ")
            })
            .map(|m| {
                let t = m.content.trim();
                if t.chars().count() > 50 {
                    format!("{}...", t.chars().take(50).collect::<String>())
                } else {
                    t.to_string()
                }
            })
            .unwrap_or_else(|| "新对话".to_string());

        let (updated_at, date) = entry
            .metadata()
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
            id: id.clone(),
            session_id,
            assistant_id,
            title,
            message_count: snap.messages.len(),
            updated_at,
            date,
        });
    }

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

/// GET /api/agents：返回动态创建的 sub-agent 列表（Phase 3，含 parent_id 用于树状展示）
async fn api_agents_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DynamicAgent>>, (StatusCode, String)> {
    let list = load_dynamic_agents(&state.workspace);
    Ok(Json(list))
}

/// POST /api/agents：前端创建 agent，body: { role, guidance? }，parent_id 为 human
async fn api_agents_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateAgentRequest>,
) -> Result<(StatusCode, Json<DynamicAgent>), (StatusCode, String)> {
    let role = req.role.trim().to_string();
    if role.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "role is required".to_string()));
    }
    let create_tool = CreateTool::new(&state.workspace);
    let guidance = req.guidance.as_deref().and_then(|s| {
        let t = s.trim();
        if t.is_empty() { None } else { Some(t.to_string()) }
    });
    let agent = create_tool
        .create_agent_direct(&role, guidance.as_deref(), "human")
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    reload_dynamic_agents_into_state(&state).await;
    emit_event(&state.event_bus, WorkspaceEvent::AgentCreated {
        id: agent.id.clone(),
        role: agent.role.clone(),
        parent_id: agent.parent_id.clone(),
    });
    Ok((StatusCode::CREATED, Json(agent)))
}

/// GET /api/assistants：返回多助手列表（含 skills），供前端选择与配置；动态 agent 从 agents.json 合并
async fn api_assistants_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<AssistantInfo>>, (StatusCode, String)> {
    reload_dynamic_agents_into_state(&state).await;
    let skills = state.assistant_skills.read().await;
    let mut list: Vec<AssistantInfo> = state
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
    let dynamic = load_dynamic_agents(&state.workspace);
    let existing_ids: std::collections::HashSet<String> = list.iter().map(|a| a.id.clone()).collect();
    for da in &dynamic {
        if !existing_ids.contains(&da.id) {
            list.push(AssistantInfo {
                id: da.id.clone(),
                name: da.role.clone(),
                description: da.guidance.clone().unwrap_or_else(|| da.role.clone()),
                skills: skills.get(&da.id).cloned(),
            });
        }
    }
    Ok(Json(list))
}

/// GET /api/groups：列出所有群组
async fn api_groups_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<GroupInfo>>, (StatusCode, String)> {
    let groups = state.groups.read().await;
    let list: Vec<GroupInfo> = groups.values().cloned().collect();
    Ok(Json(list))
}

/// POST /api/groups：创建群组，body: { name?, member_ids }，返回创建的群组
async fn api_groups_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<(StatusCode, Json<GroupInfo>), (StatusCode, String)> {
    if req.member_ids.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "member_ids cannot be empty".into()));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let name = req.name.unwrap_or_else(|| format!("群聊 {}", &id[..8]));
    let group = GroupInfo {
        id: id.clone(),
        name: Some(name),
        member_ids: req.member_ids,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    {
        let mut groups = state.groups.write().await;
        groups.insert(id.clone(), group.clone());
        save_groups_to_disk(&state.groups_path, &*groups);
    }
    emit_event(&state.event_bus, WorkspaceEvent::GroupCreated {
        id: group.id.clone(),
        name: group.name.clone(),
        member_ids: group.member_ids.clone(),
    });
    Ok((StatusCode::CREATED, Json(group)))
}

/// GET /api/tasks：列出所有任务（可选 status 过滤）
async fn api_tasks_list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<Task>>, (StatusCode, String)> {
    let tasks = load_tasks(&state.workspace);
    let status_filter = query.get("status").and_then(|s| {
        match s.as_str() {
            "todo" => Some(TaskStatus::Todo),
            "in_progress" => Some(TaskStatus::InProgress),
            "done" => Some(TaskStatus::Done),
            _ => None,
        }
    });
    let list: Vec<Task> = if let Some(st) = status_filter {
        tasks.into_iter().filter(|t| t.status == st).collect()
    } else {
        tasks
    };
    Ok(Json(list))
}

/// POST /api/tasks：创建任务，可选 assignee_ids 自动建群
async fn api_tasks_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<Task>), (StatusCode, String)> {
    let title = req.title.trim().to_string();
    if title.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "title is required".to_string()));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let assignee_ids: Vec<String> = req.assignee_ids.iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let group_id = if assignee_ids.len() >= 2 {
        let gid = uuid::Uuid::new_v4().to_string();
        let group = GroupInfo {
            id: gid.clone(),
            name: Some(format!("任务: {}", title.chars().take(20).collect::<String>())),
            member_ids: assignee_ids.clone(),
            created_at: now.clone(),
        };
        {
            let mut groups = state.groups.write().await;
            groups.insert(gid.clone(), group);
            save_groups_to_disk(&state.groups_path, &*groups);
        }
        emit_event(&state.event_bus, WorkspaceEvent::GroupCreated {
            id: gid.clone(),
            name: Some(format!("任务: {}", title.chars().take(20).collect::<String>())),
            member_ids: assignee_ids.clone(),
        });
        Some(gid)
    } else {
        None
    };
    let task = Task {
        id: id.clone(),
        title: title.clone(),
        description: req.description.as_ref().and_then(|s| {
            let t = s.trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
        }),
        status: TaskStatus::Todo,
        assignee_ids,
        group_id,
        coordinator_id: req.coordinator_id.as_ref().and_then(|s| {
            let t = s.trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
        }),
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let mut tasks = load_tasks(&state.workspace);
    tasks.push(task.clone());
    save_tasks(&state.workspace, &tasks);
    emit_event(&state.event_bus, WorkspaceEvent::TaskCreated {
        id: task.id.clone(),
        title: task.title.clone(),
    });
    Ok((StatusCode::CREATED, Json(task)))
}

/// PATCH /api/tasks/:id：更新任务
async fn api_tasks_update(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<Json<Task>, (StatusCode, String)> {
    let mut tasks = load_tasks(&state.workspace);
    let pos = tasks.iter().position(|t| t.id == task_id);
    let task = match pos {
        Some(i) => &mut tasks[i],
        None => return Err((StatusCode::NOT_FOUND, "task not found".to_string())),
    };
    if let Some(t) = req.title {
        let t = t.trim();
        if !t.is_empty() {
            task.title = t.to_string();
        }
    }
    if let Some(d) = req.description {
        task.description = if d.trim().is_empty() { None } else { Some(d.trim().to_string()) };
    }
    if let Some(s) = req.status {
        task.status = s;
    }
    if let Some(a) = req.assignee_ids {
        task.assignee_ids = a.into_iter().filter(|s| !s.trim().is_empty()).map(|s| s.trim().to_string()).collect();
    }
    if let Some(c) = req.coordinator_id {
        task.coordinator_id = if c.trim().is_empty() { None } else { Some(c.trim().to_string()) };
    }
    task.updated_at = chrono::Utc::now().to_rfc3339();
    let task = task.clone();
    save_tasks(&state.workspace, &tasks);
    let status_str = match task.status {
        TaskStatus::Todo => "todo",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Done => "done",
    };
    emit_event(&state.event_bus, WorkspaceEvent::TaskUpdated {
        id: task.id.clone(),
        status: status_str.to_string(),
    });
    Ok(Json(task))
}

/// 统筹 agent 收到的系统级提示（追加到其 system prompt）
const COORDINATOR_INSTRUCTION: &str = "\n\n你是指定任务的统筹负责人。请使用 list_agents 查看可用 agent，使用 create 创建 specialized 子 agent，使用 create_group 组建团队，使用 send 分配职责和发起协作。完成后简要总结。";

/// POST /api/tasks/:id/start：启动任务统筹，由 coordinator agent 执行规划与组队
async fn api_tasks_start(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    reload_dynamic_agents_into_state(&state).await;
    let tasks = load_tasks(&state.workspace);
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "task not found".to_string()))?;
    let coordinator_id = task
        .coordinator_id
        .as_ref()
        .filter(|s| !s.is_empty())
        .cloned()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "task has no coordinator_id, please assign one first".to_string()))?;
    let prompt = state.assistant_prompts.read().await.get(&coordinator_id).cloned();
    let base_prompt = prompt.as_deref().unwrap_or("");
    let system_prompt = format!("{}{}", base_prompt, COORDINATOR_INSTRUCTION);
    let desc = task.description.as_deref().unwrap_or("无");
    let user_message = format!(
        "请统筹以下任务：\n\n【任务标题】{}\n【任务描述】{}\n\n请分析任务、创建或调用 agent、组队、分配职责、建立协作流程。",
        task.title,
        desc
    );
    let key = format!("task_coord_{}", task_id);
    let vector = get_or_create_vector_for_assistant(&state, &coordinator_id).await;
    let mut context = {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&key).unwrap_or_else(|| {
            create_context_with_long_term_for_assistant(
                &state.config,
                DEFAULT_MAX_TURNS,
                Some(&state.workspace),
                vector,
                Some(&coordinator_id),
            )
        })
    };
    let system_prompt_override = Some(system_prompt);
    let allowed = state.assistant_skills.read().await.get(&coordinator_id).cloned();
    let components = state.components.read().await.clone();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<ReactEvent>();
    let state_spawn = Arc::clone(&state);
    let task_id_clone = task_id.clone();
    let coordinator_id_clone = coordinator_id.clone();
    tokio::spawn(async move {
        let _ = process_message_stream(
            components.as_ref(),
            &mut context,
            &user_message,
            event_tx,
            system_prompt_override.as_deref(),
            None,
            allowed.as_deref(),
            Some(&coordinator_id_clone),
        )
        .await;
        save_session_to_disk(
            &state_spawn.sessions_dir,
            &state_spawn.workspace,
            &format!("task_coord_{}", task_id_clone),
            &coordinator_id_clone,
            &context,
        );
        let mut tasks = load_tasks(&state_spawn.workspace);
        let task_updated = tasks.iter_mut().find(|x| x.id == task_id_clone).map(|t| {
            t.status = TaskStatus::InProgress;
            t.updated_at = chrono::Utc::now().to_rfc3339();
            t.id.clone()
        });
        if let Some(id) = task_updated {
            save_tasks(&state_spawn.workspace, &tasks);
            emit_event(&state_spawn.event_bus, WorkspaceEvent::TaskUpdated {
                id,
                status: "in_progress".to_string(),
            });
        }
    });
    let first_line = format!(
        "{}\n",
        serde_json::to_string(&serde_json::json!({
            "type": "session_id",
            "session_id": format!("task_{}", task_id)
        }))
        .unwrap()
    );
    let first_line2 = format!(
        "{}\n",
        serde_json::to_string(&serde_json::json!({
            "type": "coordinator_start",
            "task_id": task_id,
            "coordinator_id": coordinator_id
        }))
        .unwrap()
    );
    let pending = vec![first_line, first_line2];
    let stream = stream::unfold(
        (state, event_rx, pending),
        move |(state, mut event_rx, mut pending)| async move {
            if !pending.is_empty() {
                let line = pending.remove(0);
                return Some((
                    Ok::<_, std::convert::Infallible>(Bytes::from(line)),
                    (state, event_rx, pending),
                ));
            }
            match event_rx.recv().await {
                Some(ev) => {
                    let line = format!("{}\n", serde_json::to_string(&ev).unwrap());
                    Some((Ok::<_, std::convert::Infallible>(Bytes::from(line)), (state, event_rx, vec![])))
                }
                None => None,
            }
        },
    );
    let mut res = Response::new(Body::from_stream(stream));
    res.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        "application/x-ndjson; charset=utf-8".parse().unwrap(),
    );
    Ok(res)
}

/// POST /api/inbox/process：处理指定 assistant 的收件箱（P2P 未读消息触发 ReAct）
async fn api_inbox_process(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InboxProcessRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    reload_dynamic_agents_into_state(&state).await;
    let assistant_id = req.assistant_id.trim();
    if assistant_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "assistant_id is required".to_string()));
    }
    let groups = state.groups.read().await;
    let p2p_groups: Vec<_> = groups
        .values()
        .filter(|g| g.id.starts_with("p2p_") && g.member_ids.contains(&assistant_id.to_string()))
        .cloned()
        .collect();
    drop(groups);

    let mut processed = 0;
    for g in p2p_groups {
        let msgs = load_group_session(&state.sessions_dir, &g.id);
        let last = match msgs.last() {
            Some(m) => m,
            None => continue,
        };
        if last.role != "assistant" {
            continue;
        }
        let from = last.assistant_id.as_deref().unwrap_or("");
        if from == assistant_id {
            continue;
        }
        let from_name = state
            .assistants
            .iter()
            .find(|a| a.id == from)
            .map(|a| a.name.as_str())
            .unwrap_or(from);
        let user_input = format!("[来自 {}] {}", from_name, last.content);

        let vector = get_or_create_vector_for_assistant(&state, assistant_id).await;
        let mut context = create_context_with_long_term_for_assistant(
            &state.config,
            DEFAULT_MAX_TURNS,
            Some(&state.workspace),
            vector,
            Some(assistant_id),
        );
        let llm_history = group_messages_to_llm_messages(&msgs[..msgs.len() - 1], &state.assistants);
        context.set_messages(llm_history);

        let (tx, _rx) = mpsc::unbounded_channel();
        let components = state.components.read().await.clone();
        let prompt = state.assistant_prompts.read().await.get(assistant_id).cloned();
        let allowed = state.assistant_skills.read().await.get(assistant_id).cloned();
        let reply = process_message_stream(
            components.as_ref(),
            &mut context,
            &user_input,
            tx,
            prompt.as_deref(),
            None,
            allowed.as_deref(),
            Some(assistant_id),
        )
        .await
        .unwrap_or_else(|e| format!("Error: {}", e));

        let mut all_msgs = msgs.clone();
        all_msgs.push(GroupChatMessage {
            role: "assistant".to_string(),
            content: reply.clone(),
            assistant_id: Some(assistant_id.to_string()),
        });
        save_group_session(
            &state.sessions_dir,
            &g.id,
            &all_msgs,
            DEFAULT_MAX_TURNS,
        );
        let preview: String = reply.chars().take(80).collect::<String>()
            + if reply.len() > 80 { "…" } else { "" };
        emit_event(&state.event_bus, WorkspaceEvent::MessageCreated {
            group_id: g.id.clone(),
            from: Some(assistant_id.to_string()),
            to: Some(from.to_string()),
            content_preview: preview,
        });
        processed += 1;
    }

    Ok(Json(serde_json::json!({
        "processed": processed,
        "assistant_id": assistant_id
    })))
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

/// GET /api/history?session_id=...&assistant_id=... 或 ?group_id=...：返回该会话的对话列表，过滤掉 Tool call / Observation 等内部消息
async fn api_history(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, (StatusCode, String)> {
    if let Some(ref gid) = q.group_id.filter(|s| !s.is_empty()) {
        let group_msgs = load_group_session(&state.sessions_dir, gid);
        let messages: Vec<HistoryMessage> = group_msgs
            .into_iter()
            .map(|m| HistoryMessage {
                role: m.role,
                content: m.content,
                assistant_id: m.assistant_id,
            })
            .collect();
        return Ok(Json(HistoryResponse {
            session_id: gid.clone(),
            messages,
        }));
    }
    let session_id = match q.session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Err((StatusCode::BAD_REQUEST, "session_id or group_id is required".to_string())),
    };
    let assistant_id = q.assistant_id.as_deref().unwrap_or("default");
    let key = session_key(&session_id, assistant_id);
    let vector = get_or_create_vector_for_assistant(&state, assistant_id).await;
    let context_opt = {
        let sessions = state.sessions.read().await;
        sessions.get(&key).cloned()
    };
    let context = match context_opt {
        Some(c) => c,
        None => {
            if let Some(loaded) = load_session_from_disk(
                &state.sessions_dir,
                &session_id,
                assistant_id,
                &state.workspace,
                &state.config,
                vector,
            ) {
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
            assistant_id: None,
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
    let assistant_id = req.assistant_id.as_deref().unwrap_or("default");
    let key = session_key(&session_id, assistant_id);
    let vector = get_or_create_vector_for_assistant(&state, assistant_id).await;
    let mut context = {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&key).unwrap_or_else(|| {
            load_session_from_disk(
                &state.sessions_dir,
                &session_id,
                assistant_id,
                &state.workspace,
                &state.config,
                vector.clone(),
            )
            .unwrap_or_else(|| {
                create_context_with_long_term_for_assistant(
                    &state.config,
                    DEFAULT_MAX_TURNS,
                    Some(&state.workspace),
                    vector,
                    Some(assistant_id),
                )
            })
        })
    };

    let components = state.components.read().await.clone();
    let allowed = state.assistant_skills.read().await.get(assistant_id).cloned();
    let reply = process_message(components.as_ref(), &mut context, message, allowed.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(key, context.clone());
        save_session_to_disk(
            &state.sessions_dir,
            &state.workspace,
            &session_id,
            assistant_id,
            &context,
        );
    }

    Ok(Json(ChatResponse {
        reply,
        session_id,
    }))
}

/// 群聊流式：多助手串行回复，共享群历史，各自长期记忆
async fn api_chat_stream_group(
    state: Arc<AppState>,
    group_id: String,
    message: String,
) -> Result<Response, (StatusCode, String)> {
    let group = {
        let groups = state.groups.read().await;
        groups
            .get(&group_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "group not found".to_string()))?
    };
    let member_ids = group.member_ids.clone();
    if member_ids.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "group has no members".to_string()));
    }

    let mut group_msgs = load_group_session(&state.sessions_dir, &group_id);
    group_msgs.push(GroupChatMessage {
        role: "user".to_string(),
        content: message.clone(),
        assistant_id: None,
    });
    let preview: String = message.chars().take(80).collect::<String>()
        + if message.len() > 80 { "…" } else { "" };
    emit_event(&state.event_bus, WorkspaceEvent::MessageCreated {
        group_id: group_id.clone(),
        from: None,
        to: None,
        content_preview: preview,
    });
    let mut llm_history = group_messages_to_llm_messages(&group_msgs[..group_msgs.len() - 1], &state.assistants);

    let (line_tx, line_rx) = mpsc::unbounded_channel::<String>();
    let components = state.components.read().await.clone();
    let state_spawn = Arc::clone(&state);
    let group_id_spawn = group_id.clone();
    tokio::spawn(async move {
        let _ = line_tx.send(format!(
            "{}\n",
            serde_json::to_string(&serde_json::json!({
                "type": "session_id",
                "session_id": group_id_spawn
            }))
            .unwrap()
        ));

        for assistant_id in &member_ids {
            let _ = line_tx.send(format!(
                "{}\n",
                serde_json::to_string(&serde_json::json!({
                    "type": "group_assistant_start",
                    "assistant_id": assistant_id
                }))
                .unwrap()
            ));

            let vector = get_or_create_vector_for_assistant(&state_spawn, assistant_id).await;
            let mut context = create_context_with_long_term_for_assistant(
                &state_spawn.config,
                DEFAULT_MAX_TURNS,
                Some(&state_spawn.workspace),
                vector,
                Some(assistant_id),
            );
            context.set_messages(llm_history.clone());

            let system_prompt_override = state_spawn.assistant_prompts.read().await.get(assistant_id).cloned();
            let allowed_for_spawn = state_spawn.assistant_skills.read().await.get(assistant_id).cloned();
            let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ReactEvent>();
            let line_tx_fwd = line_tx.clone();
            let event_bus_fwd = state_spawn.event_bus.clone();
            let forward_handle = tokio::spawn(async move {
                while let Some(ev) = event_rx.recv().await {
                    if let ReactEvent::Observation { tool, preview } = &ev {
                        if tool == "create" {
                            if let Some(agent) = parse_create_observation(preview) {
                                emit_event(&event_bus_fwd, WorkspaceEvent::AgentCreated {
                                    id: agent.id,
                                    role: agent.role,
                                    parent_id: agent.parent_id,
                                });
                            }
                        }
                    }
                    let _ = line_tx_fwd.send(format!("{}\n", serde_json::to_string(&ev).unwrap()));
                }
            });

            let prompt_ref = system_prompt_override.as_deref();
            let planner_override: Option<Arc<Planner>> = None;
            let allowed = allowed_for_spawn.as_deref();
            let reply = process_message_stream(
                components.as_ref(),
                &mut context,
                &message,
                event_tx,
                prompt_ref,
                planner_override.as_deref(),
                allowed,
                Some(assistant_id.as_str()),
            )
            .await
            .unwrap_or_else(|e| format!("Error: {}", e));

            let _ = forward_handle.await;
            let _ = line_tx.send(format!(
                "{}\n",
                serde_json::to_string(&serde_json::json!({
                    "type": "group_assistant_done",
                    "assistant_id": assistant_id
                }))
                .unwrap()
            ));

            group_msgs.push(GroupChatMessage {
                role: "assistant".to_string(),
                content: reply.clone(),
                assistant_id: Some(assistant_id.clone()),
            });
            let preview: String = reply.chars().take(80).collect::<String>()
                + if reply.len() > 80 { "…" } else { "" };
            emit_event(&state_spawn.event_bus, WorkspaceEvent::MessageCreated {
                group_id: group_id_spawn.clone(),
                from: Some(assistant_id.clone()),
                to: None,
                content_preview: preview,
            });
            llm_history = group_messages_to_llm_messages(&group_msgs, &state_spawn.assistants);
        }

        save_group_session(
            &state_spawn.sessions_dir,
            &group_id_spawn,
            &group_msgs,
            DEFAULT_MAX_TURNS,
        );
    });

    type BoxErr = Box<dyn std::error::Error + Send + Sync>;
    let stream = stream::unfold(line_rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|line| (Ok::<Bytes, BoxErr>(Bytes::from(line)), rx))
    });
    let mut res = Response::new(Body::from_stream(stream));
    res.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        "application/x-ndjson; charset=utf-8".parse().unwrap(),
    );
    Ok(res)
}

/// 流式聊天：NDJSON 流，首行 session_id，后续为 ReactEvent；group_id 时走群聊模式
async fn api_chat_stream(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Response, (StatusCode, String)> {
    let message = req.message.trim().to_string();
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message is required".to_string()));
    }

    if let Some(ref gid) = req.group_id.filter(|s| !s.is_empty()) {
        return api_chat_stream_group(Arc::clone(&state), gid.clone(), message).await;
    }

    reload_dynamic_agents_into_state(&state).await;

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

    let key = session_key(&session_id, &assistant_id);
    let vector = get_or_create_vector_for_assistant(&state, &assistant_id).await;
    let context = {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&key).unwrap_or_else(|| {
            load_session_from_disk(
                &state.sessions_dir,
                &session_id,
                &assistant_id,
                &state.workspace,
                &state.config,
                vector.clone(),
            )
            .unwrap_or_else(|| {
                create_context_with_long_term_for_assistant(
                    &state.config,
                    DEFAULT_MAX_TURNS,
                    Some(&state.workspace),
                    vector,
                    Some(&assistant_id),
                )
            })
        })
    };

    let (event_tx, event_rx) = mpsc::unbounded_channel::<ReactEvent>();
    let (context_tx, context_rx) = tokio::sync::oneshot::channel();

    let allowed_for_spawn = state.assistant_skills.read().await.get(&assistant_id).cloned();
    let components = state.components.read().await.clone();
    let session_id_clone = session_id.clone();
    let assistant_id_clone = assistant_id.clone();
    let session_key_clone = key.clone();
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
            Some(assistant_id_clone.as_str()),
        )
        .await;
        // 无论流是否被客户端断开（超时/刷新），都持久化当前会话（含用户刚发的提问），刷新后历史不丢
        save_session_to_disk(
            &state_spawn.sessions_dir,
            &state_spawn.workspace,
            &session_id_clone,
            &assistant_id_clone,
            &ctx,
        );
        let mut sessions = state_spawn.sessions.write().await;
        sessions.insert(session_key_clone.clone(), ctx);
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
                        ReactEvent::Observation { tool, preview } if tool == "create" => {
                            if let Some(agent) = parse_create_observation(preview) {
                                emit_event(&state_reinsert.event_bus, WorkspaceEvent::AgentCreated {
                                    id: agent.id,
                                    role: agent.role,
                                    parent_id: agent.parent_id,
                                });
                            }
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

    let mut res = Response::new(Body::from_stream(stream));
    res.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        "application/x-ndjson; charset=utf-8".parse().unwrap(),
    );
    Ok(res)
}

/// GET /api/events：SSE 流，推送 group.created / message.created
async fn api_events_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.event_bus.subscribe();
    let event_stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(msg) => return Some((Ok(Event::default().data(msg)), rx)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    )
}

/// GET /swarm：蜂群拓扑 Graph 页
async fn serve_swarm_page() -> Html<&'static str> {
    Html(include_str!("../../static/swarm.html"))
}

/// GET /tasks：任务看板页
async fn serve_tasks_page() -> Html<&'static str> {
    Html(include_str!("../../static/tasks.html"))
}

/// GET /api/metrics：返回 JSON 格式的 metrics
async fn api_metrics() -> Json<serde_json::Value> {
    let metrics = bee::observability::Metrics::global();
    Json(metrics.to_json())
}

/// GET /api/metrics/prometheus：返回 Prometheus 格式的 metrics
async fn api_metrics_prometheus() -> (axum::http::StatusCode, String) {
    let metrics = bee::observability::Metrics::global();
    (axum::http::StatusCode::OK, metrics.to_prometheus())
}
