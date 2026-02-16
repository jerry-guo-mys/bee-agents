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
    create_shared_vector_long_term, process_message, process_message_stream, AgentComponents,
};
use bee::memory::InMemoryVectorLongTerm;
use bee::config::{load_config, AppConfig};
use bee::memory::{
    append_daily_log, append_heartbeat_log, consolidate_memory, lessons_path, preferences_path,
    procedural_path, record_error as learnings_record_error, record_learning as learnings_record_learning,
    ConversationMemory, memory_root,
};
use bee::react::{compact_context, ContextManager, ReactEvent};

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
    /// 可运行时替换，以支持「多 LLM 后端切换」与配置热更新（白皮书 Phase 5）
    components: Arc<RwLock<Arc<AgentComponents>>>,
    sessions: Arc<RwLock<HashMap<String, ContextManager>>>,
    sessions_dir: PathBuf,
    /// 记忆根目录（workspace/memory），用于短期日志与长期 Markdown
    memory_root: PathBuf,
    workspace: PathBuf,
    /// 启动时加载的 system prompt，重载组件时复用
    system_prompt: String,
    /// 向量长期记忆共享实例（启用时带快照路径，定期保存避免重启丢失）
    shared_vector_long_term: Option<Arc<InMemoryVectorLongTerm>>,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with(fmt::layer())
        .init();

    let workspace = std::env::current_dir()?
        .join("workspace")
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap().join("workspace"));
    std::fs::create_dir_all(&workspace).ok();

    let system_prompt = [
        "config/prompts/system.txt",
        "../config/prompts/system.txt",
    ]
    .into_iter()
    .find_map(|p| std::fs::read_to_string(p).ok())
    .unwrap_or_else(|| "You are Bee, a helpful AI assistant. Use tools: cat, ls, echo, shell, search.".to_string());

    let sessions_dir = workspace.join("sessions");
    let memory_root = memory_root(&workspace);
    std::fs::create_dir_all(&sessions_dir).ok();
    std::fs::create_dir_all(&memory_root).ok();

    let app_config = load_config(None).unwrap_or_else(|_| AppConfig::default());
    let shared_vector_long_term =
        create_shared_vector_long_term(&workspace, &app_config);

    let components = Arc::new(create_agent_components(&workspace, &system_prompt));
    let state = Arc::new(AppState {
        components: Arc::new(RwLock::new(components)),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        sessions_dir,
        memory_root: memory_root.clone(),
        workspace: workspace.clone(),
        system_prompt: system_prompt.clone(),
        shared_vector_long_term,
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
    if app_config.heartbeat.enabled {
        let heartbeat_state = Arc::clone(&state);
        let interval_secs = app_config.heartbeat.interval_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            interval.tick().await; // 跳过启动后立即执行
            loop {
                interval.tick().await;
                let shared_vec = heartbeat_state.shared_vector_long_term.clone();
                let mut context = create_context_with_long_term(
                    DEFAULT_MAX_TURNS,
                    Some(&heartbeat_state.workspace),
                    shared_vec,
                );
                let guard = heartbeat_state.components.read().await;
                match process_message(&**guard, &mut context, HEARTBEAT_PROMPT).await {
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
        .unwrap_or(app_config.web.port);
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
    let _ = bee::config::reload_config(); // 使后续 load_config 读到最新配置
    let new_components = Arc::new(create_agent_components(&state.workspace, &state.system_prompt));
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
                !c.starts_with("Observation from ")
            } else {
                !c.starts_with("Tool call:")  // 任意 "Tool call:..." 均过滤，不依赖 " | Result: "
            }
        })
        .map(|m: &Message| HistoryMessage {
            role: match m.role {
                Role::User => "user".to_string(),
                Role::Assistant => "assistant".to_string(),
                Role::System => "system".to_string(),
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
                    DEFAULT_MAX_TURNS,
                    Some(&state.workspace),
                    state.shared_vector_long_term.clone(),
                )
            })
        })
    };

    let components = state.components.read().await.clone();
    let reply = process_message(components.as_ref(), &mut context, message)
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

    let context = {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id).unwrap_or_else(|| {
            load_session_from_disk(&state.sessions_dir, &session_id, &state.memory_root).unwrap_or_else(|| {
                create_context_with_long_term(
                    DEFAULT_MAX_TURNS,
                    Some(&state.workspace),
                    state.shared_vector_long_term.clone(),
                )
            })
        })
    };

    let (event_tx, event_rx) = mpsc::unbounded_channel::<ReactEvent>();
    let (context_tx, context_rx) = tokio::sync::oneshot::channel();

    let components = state.components.read().await.clone();
    let session_id_clone = session_id.clone();
    let state_spawn = Arc::clone(&state);
    tokio::spawn(async move {
        let mut ctx = context;
        let _ = process_message_stream(components.as_ref(), &mut ctx, &message, event_tx).await;
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

    let first_line = serde_json::json!({
        "type": "session_id",
        "session_id": session_id
    });
    let first_line = format!("{}\n", serde_json::to_string(&first_line).unwrap());

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
