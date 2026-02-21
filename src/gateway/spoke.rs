//! Spoke（辐条/端点）
//!
//! Spoke 分为两类：
//! - **通讯端点（Communication Spokes）**：Telegram、Slack、WhatsApp、TUI、Web
//! - **能力端点（Capability Spokes）**：Skills、工具、API 插件、自动化脚本

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::message::{ClientInfo, GatewayMessage, SpokeType};

/// Spoke 适配器 trait（通用接口）
#[async_trait]
pub trait SpokeAdapter: Send + Sync {
    /// 适配器类型
    fn spoke_type(&self) -> SpokeType;

    /// 启动适配器
    async fn start(&self, message_tx: mpsc::UnboundedSender<(ClientInfo, GatewayMessage)>) -> Result<(), String>;

    /// 发送消息到客户端
    async fn send(&self, client_id: &str, message: GatewayMessage) -> Result<(), String>;

    /// 停止适配器
    async fn stop(&self);
}

// ============================================================================
// 通讯端点（Communication Spokes）
// ============================================================================

/// 通讯端点类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunicationSpokeType {
    /// Web 浏览器（WebSocket）
    Web,
    /// 终端 TUI
    Tui,
    /// Telegram
    Telegram,
    /// Slack
    Slack,
    /// WhatsApp
    WhatsApp,
    /// Discord
    Discord,
    /// 飞书 Lark
    Lark,
    /// HTTP API
    Api,
}

/// 通讯端点 trait
#[async_trait]
pub trait CommunicationSpoke: SpokeAdapter {
    /// 是否支持流式响应
    fn supports_streaming(&self) -> bool {
        true
    }

    /// 是否支持富文本（Markdown）
    fn supports_rich_text(&self) -> bool {
        true
    }

    /// 消息最大长度（用于自动分割）
    fn max_message_length(&self) -> Option<usize> {
        None
    }
}

// ============================================================================
// 能力端点（Capability Spokes）
// ============================================================================

/// 能力端点类型
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySpokeType {
    /// 技能（Skills）
    Skill { id: String },
    /// 本地工具
    Tool { name: String },
    /// API 插件
    ApiPlugin { name: String },
    /// 自动化脚本
    Script { path: String },
}

/// 能力端点 trait
#[async_trait]
pub trait CapabilitySpoke: Send + Sync {
    /// 能力名称
    fn name(&self) -> &str;

    /// 能力描述
    fn description(&self) -> &str;

    /// 能力类型
    fn capability_type(&self) -> CapabilitySpokeType;

    /// 执行能力
    async fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, String>;

    /// 获取参数 schema
    fn parameters_schema(&self) -> Option<serde_json::Value> {
        None
    }

    /// 是否可用
    fn is_available(&self) -> bool {
        true
    }
}

/// 技能能力端点
#[allow(dead_code)]
pub struct SkillSpoke {
    skill_id: String,
    name: String,
    description: String,
    capability_md: String,
    template_md: Option<String>,
    script_path: Option<String>,
}

#[allow(dead_code)]
impl SkillSpoke {
    pub fn new(
        skill_id: String,
        name: String,
        description: String,
        capability_md: String,
        template_md: Option<String>,
        script_path: Option<String>,
    ) -> Self {
        Self {
            skill_id,
            name,
            description,
            capability_md,
            template_md,
            script_path,
        }
    }

    pub fn capability(&self) -> &str {
        &self.capability_md
    }

    pub fn template(&self) -> Option<&str> {
        self.template_md.as_deref()
    }
}

#[async_trait]
impl CapabilitySpoke for SkillSpoke {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn capability_type(&self) -> CapabilitySpokeType {
        CapabilitySpokeType::Skill {
            id: self.skill_id.clone(),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, String> {
        if let Some(script_path) = &self.script_path {
            let input_str = serde_json::to_string(&input).unwrap_or_default();
            let output = tokio::process::Command::new("python3")
                .arg(script_path)
                .arg(&input_str)
                .output()
                .await
                .map_err(|e| format!("Script execution failed: {}", e))?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let result: serde_json::Value = serde_json::from_str(&stdout)
                .unwrap_or_else(|_| serde_json::Value::String(stdout.to_string()));
            Ok(result)
        } else {
            Ok(serde_json::json!({
                "capability": self.capability_md,
                "template": self.template_md,
            }))
        }
    }
}

/// API 插件能力端点
#[allow(dead_code)]
pub struct ApiPluginSpoke {
    name: String,
    description: String,
    endpoint: String,
    method: String,
    headers: std::collections::HashMap<String, String>,
}

#[allow(dead_code)]
impl ApiPluginSpoke {
    pub fn new(
        name: String,
        description: String,
        endpoint: String,
        method: String,
    ) -> Self {
        Self {
            name,
            description,
            endpoint,
            method,
            headers: std::collections::HashMap::new(),
        }
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }
}

#[async_trait]
impl CapabilitySpoke for ApiPluginSpoke {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn capability_type(&self) -> CapabilitySpokeType {
        CapabilitySpokeType::ApiPlugin {
            name: self.name.clone(),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, String> {
        let client = reqwest::Client::new();
        let mut request = match self.method.to_uppercase().as_str() {
            "GET" => client.get(&self.endpoint),
            "POST" => client.post(&self.endpoint).json(&input),
            "PUT" => client.put(&self.endpoint).json(&input),
            "DELETE" => client.delete(&self.endpoint),
            _ => return Err(format!("Unsupported HTTP method: {}", self.method)),
        };

        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        let response = request
            .send()
            .await
            .map_err(|e| format!("API request failed: {}", e))?;

        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        let result: serde_json::Value = serde_json::from_str(&body)
            .unwrap_or_else(|_| serde_json::Value::String(body));

        Ok(result)
    }
}

// ============================================================================
// 通讯端点实现
// ============================================================================

/// WebSocket Spoke（用于 Web 和通用 WebSocket 客户端）
pub struct WebSocketSpoke {
    bind_addr: String,
    connections: Arc<tokio::sync::RwLock<std::collections::HashMap<String, WebSocketConnection>>>,
}

struct WebSocketConnection {
    tx: mpsc::UnboundedSender<String>,
}

impl WebSocketSpoke {
    pub fn new(bind_addr: &str) -> Self {
        Self {
            bind_addr: bind_addr.to_string(),
            connections: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub fn bind_addr(&self) -> &str {
        &self.bind_addr
    }
}

#[async_trait]
impl SpokeAdapter for WebSocketSpoke {
    fn spoke_type(&self) -> SpokeType {
        SpokeType::Web
    }

    async fn start(&self, _message_tx: mpsc::UnboundedSender<(ClientInfo, GatewayMessage)>) -> Result<(), String> {
        Ok(())
    }

    async fn send(&self, client_id: &str, message: GatewayMessage) -> Result<(), String> {
        let connections = self.connections.read().await;
        if let Some(conn) = connections.get(client_id) {
            let json = serde_json::to_string(&message)
                .map_err(|e| format!("Serialize error: {}", e))?;
            conn.tx
                .send(json)
                .map_err(|e| format!("Send error: {}", e))?;
        }
        Ok(())
    }

    async fn stop(&self) {
        self.connections.write().await.clear();
    }
}

/// HTTP Spoke（用于 Webhook 回调，如 WhatsApp、Lark）
pub struct HttpSpoke {
    spoke_type: SpokeType,
    callback_url: Option<String>,
}

impl HttpSpoke {
    pub fn new(spoke_type: SpokeType, callback_url: Option<String>) -> Self {
        Self {
            spoke_type,
            callback_url,
        }
    }
}

#[async_trait]
impl SpokeAdapter for HttpSpoke {
    fn spoke_type(&self) -> SpokeType {
        self.spoke_type
    }

    async fn start(&self, _message_tx: mpsc::UnboundedSender<(ClientInfo, GatewayMessage)>) -> Result<(), String> {
        Ok(())
    }

    async fn send(&self, _client_id: &str, message: GatewayMessage) -> Result<(), String> {
        if let Some(url) = &self.callback_url {
            let client = reqwest::Client::new();
            client
                .post(url)
                .json(&message)
                .send()
                .await
                .map_err(|e| format!("HTTP callback failed: {}", e))?;
        }
        Ok(())
    }

    async fn stop(&self) {}
}

/// TUI Spoke（用于终端界面）
#[allow(dead_code)]
pub struct TuiSpoke {
    tx: Arc<tokio::sync::RwLock<Option<mpsc::UnboundedSender<GatewayMessage>>>>,
}

impl TuiSpoke {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            tx: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    #[allow(dead_code)]
    pub async fn set_sender(&self, tx: mpsc::UnboundedSender<GatewayMessage>) {
        *self.tx.write().await = Some(tx);
    }
}

impl Default for TuiSpoke {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SpokeAdapter for TuiSpoke {
    fn spoke_type(&self) -> SpokeType {
        SpokeType::Tui
    }

    async fn start(&self, _message_tx: mpsc::UnboundedSender<(ClientInfo, GatewayMessage)>) -> Result<(), String> {
        Ok(())
    }

    async fn send(&self, _client_id: &str, message: GatewayMessage) -> Result<(), String> {
        if let Some(tx) = self.tx.read().await.as_ref() {
            tx.send(message).map_err(|e| format!("TUI send error: {}", e))?;
        }
        Ok(())
    }

    async fn stop(&self) {
        *self.tx.write().await = None;
    }
}
