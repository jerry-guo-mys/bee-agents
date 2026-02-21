//! 飞书（Lark）集成
//!
//! 通过事件订阅 Webhook 接收消息，调用 Agent 处理后回复。
//! 支持单聊和群聊。
//!
//! 重要：飞书要求 Webhook 在 **3 秒内** 返回 200，否则判失败并重试。
//! 本模块在解析事件后立即返回，耗时处理在后台异步执行。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::agent::{create_context_default, process_message};
use crate::core::AgentComponents;
use crate::react::ContextManager;

/// 会话存储：chat_id -> ContextManager
pub type SessionStore = Arc<RwLock<HashMap<String, ContextManager>>>;

/// 已处理事件 ID 缓存（用于去重，防止飞书重试时重复处理）
pub type ProcessedEvents = Arc<RwLock<HashSet<String>>>;

/// 飞书服务状态
pub struct LarkState {
    pub components: AgentComponents,
    pub sessions: SessionStore,
    pub processed_events: ProcessedEvents,
    pub app_id: String,
    pub app_secret: String,
    pub base_url: String,
}

/// URL 校验请求（未配置 Encrypt Key 时）
#[derive(Debug, Deserialize)]
pub struct UrlVerification {
    pub challenge: Option<String>,
    pub token: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

/// 事件回调顶层（兼容 v1 与 v2.0 格式）
#[derive(Debug, Deserialize)]
pub struct EventPayload {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub challenge: Option<String>,
    pub token: Option<String>,
    /// v1 格式：event 内包含 type、event_id
    pub event: Option<EventData>,
    /// v2.0 格式：header 内含 event_type、event_id
    pub header: Option<EventHeader>,
    /// 若配置了 Encrypt Key，飞书会发送加密体，需解密后解析
    pub encrypt: Option<String>,
}

/// v2.0 事件头
#[derive(Debug, Deserialize)]
pub struct EventHeader {
    pub event_id: Option<String>,
    pub event_type: Option<String>,
}

/// 事件数据（v1 的 event 或 v2 的 event）
#[derive(Debug, Deserialize)]
pub struct EventData {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub event_id: Option<String>,
    pub message: Option<MessageData>,
}

/// 消息数据
#[derive(Debug, Deserialize)]
pub struct MessageData {
    pub chat_id: Option<String>,
    pub chat_type: Option<String>,
    pub content: Option<String>,
    pub message_id: Option<String>,
    #[serde(rename = "message_type")]
    pub message_type: Option<String>,
}

/// 文本 content JSON
#[derive(Debug, Deserialize)]
struct ContentText {
    pub text: Option<String>,
}

/// 发送消息请求
#[derive(Debug, Serialize)]
struct SendMessageRequest {
    pub receive_id: String,
    pub msg_type: String,
    pub content: String,
}

/// 创建飞书路由
pub fn create_router(state: Arc<LarkState>) -> Router {
    Router::new()
        .route("/webhook", post(webhook_handler))
        .route("/health", axum::routing::get(|| async { "OK" }))
        .with_state(state)
}

/// POST /webhook - 接收飞书事件（URL 校验 + 消息回调）
async fn webhook_handler(
    State(state): State<Arc<LarkState>>,
    Json(payload): Json<EventPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    tracing::info!(
        "Lark webhook received: type={:?}",
        payload.type_.as_deref().unwrap_or("(none)")
    );

    if payload.type_.as_deref() == Some("url_verification") {
        if let Some(challenge) = payload.challenge {
            return Ok(Json(serde_json::json!({ "challenge": challenge })));
        }
        return Err(StatusCode::BAD_REQUEST);
    }

    if payload.encrypt.is_some() {
        tracing::warn!(
            "Lark webhook: payload is encrypted (Encrypt Key enabled). \
             Remove Encrypt Key in Lark console (事件订阅 -> 加密配置) or implement decryption."
        );
        return Ok(Json(serde_json::json!({})));
    }

    let Some(event) = payload.event else {
        tracing::warn!("Lark webhook: no event in payload, ignoring");
        return Ok(Json(serde_json::json!({})));
    };

    let event_type = payload
        .header
        .as_ref()
        .and_then(|h| h.event_type.as_deref())
        .or(event.type_.as_deref());
    if event_type != Some("im.message.receive_v1") {
        tracing::info!(
            "Lark webhook: event type {:?} not im.message.receive_v1, ignoring",
            event_type
        );
        return Ok(Json(serde_json::json!({})));
    }

    let Some(msg) = event.message else {
        tracing::warn!("Lark webhook: no message in event, ignoring");
        return Ok(Json(serde_json::json!({})));
    };

    let Some(chat_id) = msg.chat_id.clone() else {
        tracing::warn!("Lark webhook: no chat_id, ignoring");
        return Ok(Json(serde_json::json!({})));
    };

    if msg.message_type.as_deref() != Some("text") {
        tracing::info!(
            "Lark webhook: message_type {:?} not text, ignoring",
            msg.message_type.as_deref()
        );
        return Ok(Json(serde_json::json!({})));
    }

    let content_str = msg.content.as_deref().unwrap_or("{}");
    let content: ContentText = serde_json::from_str(content_str).unwrap_or(ContentText { text: None });
    let Some(body) = content.text else {
        tracing::warn!("Lark webhook: no text in content, ignoring. raw content: {}", content_str);
        return Ok(Json(serde_json::json!({})));
    };

    let mut body = strip_at_mentions(body.trim());
    if body.is_empty() {
        body = "你好".to_string();
    }

    let event_id = payload
        .header
        .as_ref()
        .and_then(|h| h.event_id.clone())
        .or(event.event_id.clone())
        .unwrap_or_default();
    {
        let mut processed = state.processed_events.write().await;
        if !event_id.is_empty() && processed.contains(&event_id) {
            tracing::debug!("Duplicate event ignored: {}", event_id);
            return Ok(Json(serde_json::json!({})));
        }
        if !event_id.is_empty() {
            processed.insert(event_id.clone());
            if processed.len() > 10_000 {
                processed.clear();
            }
        }
    }

    let state_clone = Arc::clone(&state);
    let chat_id_clone = chat_id.clone();
    let body_clone = body.clone();

    tracing::info!(
        "Lark webhook: accepted message chat_id={} body_len={}, spawning background task",
        chat_id,
        body.len()
    );

    tokio::spawn(async move {
        if let Err(e) = process_and_reply(state_clone, &chat_id_clone, &body_clone).await {
            tracing::error!("Lark background process error: {}", e);
        } else {
            tracing::info!("Lark webhook: reply sent for chat_id={}", chat_id_clone);
        }
    });

    Ok(Json(serde_json::json!({})))
}

static AT_MENTION_RE: OnceLock<Regex> = OnceLock::new();

/// 去掉飞书 @ 提及标签，如 <at user_id="ou_xxx">@名字</at>
fn strip_at_mentions(s: &str) -> String {
    let re = AT_MENTION_RE.get_or_init(|| Regex::new(r#"<at[^>]*>.*?</at>\s*"#).unwrap());
    re.replace_all(s, "").trim().to_string()
}

/// 后台执行：获取/创建 context，调用 Agent，发送回复
async fn process_and_reply(state: Arc<LarkState>, chat_id: &str, body: &str) -> anyhow::Result<()> {
    let mut context = {
        let mut sessions = state.sessions.write().await;
        sessions
            .remove(chat_id)
            .unwrap_or_else(|| create_context_default(20, None, None))
    };

    let result = process_message(&state.components, &mut context, body, None).await;

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(chat_id.to_string(), context);
    }

    match result {
        Ok(response) => {
            send_lark_message(&state, chat_id, &response).await?;
        }
        Err(e) => {
            tracing::error!("Agent error: {}", e);
            send_lark_message(&state, chat_id, &format!("抱歉，处理时出错: {}", e)).await?;
        }
    }
    Ok(())
}

/// 获取 tenant_access_token（带缓存）
async fn get_tenant_token(state: &LarkState) -> anyhow::Result<String> {
    let url = format!("{}/open-apis/auth/v3/tenant_access_token/internal", state.base_url);

    let body = serde_json::json!({
        "app_id": state.app_id,
        "app_secret": state.app_secret
    });

    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post(&url)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;

    let token = resp["tenant_access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No tenant_access_token in response"))?;
    Ok(token.to_string())
}

/// 发送飞书消息
async fn send_lark_message(state: &LarkState, chat_id: &str, body: &str) -> anyhow::Result<()> {
    let token = get_tenant_token(state).await?;

    let max_len = 4000usize;
    let chunks: Vec<String> = if body.chars().count() <= max_len {
        vec![body.to_string()]
    } else {
        body.chars()
            .collect::<Vec<_>>()
            .chunks(max_len)
            .map(|c| c.iter().collect())
            .collect()
    };

    let url = format!(
        "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
        state.base_url
    );

    for chunk in chunks {
        let content = serde_json::json!({ "text": chunk }).to_string();
        let req = SendMessageRequest {
            receive_id: chat_id.to_string(),
            msg_type: "text".to_string(),
            content,
        };

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .bearer_auth(&token)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await?;
            anyhow::bail!("Lark API error: {}", text);
        }
    }

    Ok(())
}
