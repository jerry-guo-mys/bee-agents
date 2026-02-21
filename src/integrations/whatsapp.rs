//! WhatsApp Cloud API 集成
//!
//! 通过 Webhook 接收消息，调用 Agent 处理后发送回复。

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::agent::{create_context_default, process_message};
use crate::core::AgentComponents;
use crate::react::ContextManager;

/// 会话存储：user_id -> ContextManager
pub type SessionStore = Arc<RwLock<HashMap<String, ContextManager>>>;

/// WhatsApp 服务状态
pub struct WhatsappState {
    pub components: AgentComponents,
    pub sessions: SessionStore,
    pub access_token: String,
    pub phone_number_id: String,
}

/// Webhook 验证参数
#[derive(Debug, Deserialize)]
pub struct WebhookVerifyQuery {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

/// WhatsApp Webhook 请求体
#[derive(Debug, Deserialize)]
pub struct WebhookPayload {
    pub object: Option<String>,
    pub entry: Option<Vec<WebhookEntry>>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEntry {
    pub id: Option<String>,
    pub changes: Option<Vec<WebhookChange>>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookChange {
    pub value: Option<WebhookValue>,
    pub field: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookValue {
    pub messaging_product: Option<String>,
    pub metadata: Option<WebhookMetadata>,
    pub contacts: Option<Vec<WebhookContact>>,
    pub messages: Option<Vec<WebhookMessage>>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookMetadata {
    pub display_phone_number: Option<String>,
    #[serde(rename = "phone_number_id")]
    pub phone_number_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookContact {
    pub profile: Option<WebhookProfile>,
    pub wa_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookProfile {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookMessage {
    pub from: String,
    pub id: Option<String>,
    pub timestamp: Option<String>,
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub text: Option<WebhookText>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookText {
    pub body: String,
}

/// WhatsApp 发送消息 API 请求体
#[derive(Debug, Serialize)]
struct SendMessageRequest {
    messaging_product: String,
    to: String,
    #[serde(rename = "type")]
    msg_type: String,
    text: SendMessageText,
}

#[derive(Debug, Serialize)]
struct SendMessageText {
    body: String,
}

/// 创建 WhatsApp 路由
pub fn create_router(state: Arc<WhatsappState>) -> Router {
    Router::new()
        .route("/webhook", get(webhook_verify).post(webhook_receive))
        .route("/health", get(|| async { "OK" }))
        .with_state(state)
}

/// GET /webhook - Meta 验证 Webhook
async fn webhook_verify(
    State(state): State<Arc<WhatsappState>>,
    Query(query): Query<WebhookVerifyQuery>,
) -> Result<String, StatusCode> {
    let verify_token = std::env::var("WHATSAPP_VERIFY_TOKEN").unwrap_or_else(|_| "bee".to_string());
    if query.mode.as_deref() == Some("subscribe")
        && query.verify_token.as_deref() == Some(&verify_token)
    {
        Ok(query.challenge.unwrap_or_default())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// POST /webhook - 接收 WhatsApp 消息
async fn webhook_receive(
    State(state): State<Arc<WhatsappState>>,
    Json(payload): Json<WebhookPayload>,
) -> StatusCode {
    if payload.object.as_deref() != Some("whatsapp_business_account") {
        return StatusCode::OK;
    }

    let Some(entries) = payload.entry else {
        return StatusCode::OK;
    };

    for entry in entries {
        let Some(changes) = entry.changes else { continue };
        for change in changes {
            let Some(value) = change.value else { continue };
            let Some(messages) = value.messages else { continue };

            for msg in messages {
                if msg.msg_type.as_deref() != Some("text") {
                    continue;
                }
                let Some(text) = msg.text else { continue };
                let user_id = msg.from.clone();
                let body = text.body.clone();

                // 获取或创建会话（取出以释放锁，避免持锁期间调用 LLM）
                let mut context = {
                    let mut sessions: tokio::sync::RwLockWriteGuard<
                        HashMap<String, ContextManager>,
                    > = state.sessions.write().await;
                    sessions.remove(&user_id).unwrap_or_else(|| create_context_default(20, None, None))
                };

                // 处理消息
                let result: Result<String, crate::core::AgentError> =
                    process_message(&state.components, &mut context, &body, None).await;

                match result {
                    Ok(response) => {
                        // 保存会话
                        {
                            let mut sessions = state.sessions.write().await;
                            sessions.insert(user_id.clone(), context);
                        }
                        // 发送回复
                        if let Err(e) = send_whatsapp_message(
                            &state.access_token,
                            &state.phone_number_id,
                            &user_id,
                            &response,
                        )
                        .await
                        {
                            tracing::error!("Failed to send WhatsApp message: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Agent error: {}", e);
                        let _ = send_whatsapp_message(
                            &state.access_token,
                            &state.phone_number_id,
                            &user_id,
                            &format!("抱歉，处理时出错: {}", e),
                        )
                        .await;
                    }
                }
            }
        }
    }

    StatusCode::OK
}

/// 通过 WhatsApp Cloud API 发送消息
async fn send_whatsapp_message(
    access_token: &str,
    phone_number_id: &str,
    to: &str,
    body: &str,
) -> anyhow::Result<()> {
    // WhatsApp 消息有长度限制 (4096 字符)，按字符分段
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
        "https://graph.facebook.com/v18.0/{}/messages",
        phone_number_id
    );

    for chunk in chunks {
        let req = SendMessageRequest {
            messaging_product: "whatsapp".to_string(),
            to: to.replace('+', "").to_string(),
            msg_type: "text".to_string(),
            text: SendMessageText { body: chunk },
        };

        let client = reqwest::Client::new();
        let resp: reqwest::Response = client
            .post(&url)
            .bearer_auth(access_token)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await?;
            anyhow::bail!("WhatsApp API error: {}", text);
        }
    }

    Ok(())
}
