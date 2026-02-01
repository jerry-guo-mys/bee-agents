//! Bee Web UI
//!
//! 启动: cargo run --bin bee-web --features web
//! 浏览器访问 http://127.0.0.1:8080

#![cfg(feature = "web")]

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Query, State},
    http::StatusCode,
    response::Html,
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
    create_agent_components, create_context_with_long_term, process_message,
    process_message_stream, AgentComponents,
};
use bee::react::{ContextManager, ReactEvent};

struct AppState {
    components: Arc<AgentComponents>,
    sessions: Arc<RwLock<HashMap<String, ContextManager>>>,
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

    let components = Arc::new(create_agent_components(&workspace, &system_prompt));
    let state = Arc::new(AppState {
        components,
        sessions: Arc::new(RwLock::new(HashMap::new())),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/chat", post(api_chat))
        .route("/api/chat/stream", post(api_chat_stream))
        .route("/api/history", get(api_history))
        .route("/api/health", get(|| async { "OK" }))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("Bee Web UI: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}

async fn api_history(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, (StatusCode, String)> {
    let session_id = match q.session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Err((StatusCode::BAD_REQUEST, "session_id is required".to_string())),
    };
    let sessions = state.sessions.read().await;
    let context = match sessions.get(&session_id) {
        Some(c) => c,
        None => {
            return Ok(Json(HistoryResponse {
                session_id: session_id.clone(),
                messages: vec![],
            }))
        }
    };
    let messages: Vec<HistoryMessage> = context
        .messages()
        .iter()
        .filter(|m| !matches!(m.role, Role::System))
        .filter(|m| {
            let c = m.content.trim();
            if matches!(m.role, Role::User) {
                !c.starts_with("Observation from ")
            } else {
                !(c.starts_with("Tool call: ") && c.contains(" | Result: "))
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
        sessions
            .remove(&session_id)
            .unwrap_or_else(|| create_context_with_long_term(20))
    };

    let reply = process_message(state.components.as_ref(), &mut context, message)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(session_id.clone(), context);
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
        sessions
            .remove(&session_id)
            .unwrap_or_else(|| create_context_with_long_term(20))
    };

    let (event_tx, event_rx) = mpsc::unbounded_channel::<ReactEvent>();
    let (context_tx, context_rx) = tokio::sync::oneshot::channel();

    let components = Arc::clone(&state.components);
    let session_id_clone = session_id.clone();
    tokio::spawn(async move {
        let mut ctx = context;
        let _ = process_message_stream(components.as_ref(), &mut ctx, &message, event_tx).await;
        let _ = context_tx.send(ctx);
    });

    let first_line = serde_json::json!({
        "type": "session_id",
        "session_id": session_id_clone
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
                    let line = format!("{}\n", serde_json::to_string(&ev).unwrap());
                    Ok(Some((
                        Bytes::from(line),
                        (state_reinsert, session_id_reinsert, context_rx, event_rx, None),
                    )))
                }
                None => {
                    if let Ok(ctx) = context_rx.await {
                        let mut sessions = state_reinsert.sessions.write().await;
                        sessions.insert(session_id_reinsert, ctx);
                    }
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
