//! Hub（轮毂/中枢）- 核心运行时
//!
//! Hub 是整个系统的大脑，包含：
//! - **LLM 路由网关**：模型选择、负载均衡、fallback
//! - **记忆系统**：短期对话日志 + 长期文件索引
//! - **意图识别**：理解用户意图，路由到合适的能力
//! - **决策引擎**：ReAct 循环、规划、执行

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::intent::IntentRecognizer;
use super::message::{ClientInfo, GatewayMessage, HistoryMessage, MessageType};
use super::runtime::{AgentRuntime, RuntimeConfig};
use super::session::SessionManager;
use super::spoke::SpokeAdapter;

/// Hub 配置
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// WebSocket 监听地址
    pub bind_addr: String,
    /// 最大并发连接数
    pub max_connections: usize,
    /// 心跳间隔（秒）
    pub heartbeat_interval: u64,
    /// 会话超时（秒）
    pub session_timeout: u64,
    /// 最大上下文轮数
    pub max_context_turns: usize,
    /// Runtime 配置
    pub runtime: RuntimeConfig,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:9000".to_string(),
            max_connections: 1000,
            heartbeat_interval: 30,
            session_timeout: 3600,
            max_context_turns: 20,
            runtime: RuntimeConfig::default(),
        }
    }
}

/// WebSocket 连接
#[allow(dead_code)]
struct Connection {
    client_info: ClientInfo,
    session_id: String,
    tx: mpsc::UnboundedSender<String>,
}

/// Hub（轮毂/中枢）- 核心运行时
pub struct Hub {
    config: HubConfig,
    session_manager: Arc<SessionManager>,
    runtime: Arc<AgentRuntime>,
    #[allow(dead_code)]
    intent_recognizer: Arc<IntentRecognizer>,
    connections: Arc<RwLock<HashMap<String, Connection>>>,
    spokes: Arc<RwLock<Vec<Arc<dyn SpokeAdapter>>>>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Hub {
    pub fn new(config: HubConfig) -> Self {
        let session_manager = Arc::new(SessionManager::new(
            config.max_context_turns,
            config.session_timeout,
        ));
        let runtime = Arc::new(AgentRuntime::new(
            config.runtime.clone(),
            Arc::clone(&session_manager),
        ));
        let intent_recognizer = Arc::new(IntentRecognizer::new(
            Arc::clone(&runtime.components().llm),
        ));
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);

        Self {
            config,
            session_manager,
            runtime,
            intent_recognizer,
            connections: Arc::new(RwLock::new(HashMap::new())),
            spokes: Arc::new(RwLock::new(Vec::new())),
            shutdown: shutdown_tx,
        }
    }

    /// 注册 Spoke 适配器
    pub async fn register_spoke(&self, spoke: Arc<dyn SpokeAdapter>) {
        self.spokes.write().await.push(spoke);
    }

    /// 启动网关
    pub async fn start(&self) -> Result<(), String> {
        let addr: SocketAddr = self
            .config
            .bind_addr
            .parse()
            .map_err(|e| format!("Invalid bind address: {}", e))?;

        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Failed to bind: {}", e))?;

        tracing::info!("Gateway listening on ws://{}", addr);

        let mut shutdown_rx = self.shutdown.subscribe();
        let connections = Arc::clone(&self.connections);
        let session_manager = Arc::clone(&self.session_manager);
        let runtime = Arc::clone(&self.runtime);
        let heartbeat_interval = self.config.heartbeat_interval;

        tokio::spawn(async move {
            let cleanup_interval = tokio::time::Duration::from_secs(60);
            let mut cleanup_timer = tokio::time::interval(cleanup_interval);

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = cleanup_timer.tick() => {
                        let expired = session_manager.cleanup_expired().await;
                        if expired > 0 {
                            tracing::info!("Cleaned up {} expired sessions", expired);
                        }
                    }
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                let connections = Arc::clone(&connections);
                                let session_manager = Arc::clone(&session_manager);
                                let runtime = Arc::clone(&runtime);

                                tokio::spawn(async move {
                                    if let Err(e) = handle_connection(
                                        stream,
                                        addr,
                                        connections,
                                        session_manager,
                                        runtime,
                                        heartbeat_interval,
                                    ).await {
                                        tracing::error!("Connection error from {}: {}", addr, e);
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::error!("Accept error: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// 停止网关
    pub async fn stop(&self) {
        let _ = self.shutdown.send(true);

        for spoke in self.spokes.read().await.iter() {
            spoke.stop().await;
        }

        self.connections.write().await.clear();
    }

    /// 向指定客户端发送消息
    pub async fn send_to_client(&self, client_id: &str, message: GatewayMessage) -> Result<(), String> {
        let connections = self.connections.read().await;
        if let Some(conn) = connections.get(client_id) {
            let json = serde_json::to_string(&message)
                .map_err(|e| format!("Serialize error: {}", e))?;
            conn.tx
                .send(json)
                .map_err(|e| format!("Send error: {}", e))?;
            Ok(())
        } else {
            Err("Client not found".to_string())
        }
    }

    /// 向会话的所有客户端广播消息
    pub async fn broadcast_to_session(&self, session_id: &str, message: GatewayMessage) {
        let connections = self.connections.read().await;
        let json = match serde_json::to_string(&message) {
            Ok(j) => j,
            Err(_) => return,
        };

        for conn in connections.values() {
            if conn.session_id == session_id {
                let _ = conn.tx.send(json.clone());
            }
        }
    }

    /// 获取活跃连接数
    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    /// 获取活跃会话数
    pub async fn session_count(&self) -> usize {
        self.session_manager.active_count().await
    }
}

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    connections: Arc<RwLock<HashMap<String, Connection>>>,
    session_manager: Arc<SessionManager>,
    runtime: Arc<AgentRuntime>,
    _heartbeat_interval: u64,
) -> Result<(), String> {
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| format!("WebSocket handshake failed: {}", e))?;

    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let client_id = format!("ws_{}_{}", addr, uuid::Uuid::new_v4());
    let mut session_id: Option<String> = None;
    let mut client_info: Option<ClientInfo> = None;

    tracing::info!("New WebSocket connection from {}", addr);

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(WsMessage::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("WebSocket receive error: {}", e);
                break;
            }
        };

        match msg {
            WsMessage::Text(text) => {
                let gateway_msg: GatewayMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        let error = GatewayMessage::error("parse_error", &e.to_string());
                        let _ = tx.send(serde_json::to_string(&error).unwrap_or_default());
                        continue;
                    }
                };

                match gateway_msg.message {
                    MessageType::Auth { token: _, client_info: info } => {
                        let sid = session_manager
                            .get_or_create(&info.client_id, info.clone())
                            .await;

                        session_id = Some(sid.clone());
                        client_info = Some(info.clone());

                        connections.write().await.insert(
                            client_id.clone(),
                            Connection {
                                client_info: info,
                                session_id: sid.clone(),
                                tx: tx.clone(),
                            },
                        );

                        let response = GatewayMessage::new(
                            Some(sid.clone()),
                            MessageType::AuthResult {
                                success: true,
                                session_id: Some(sid),
                                message: None,
                            },
                        );
                        let _ = tx.send(serde_json::to_string(&response).unwrap_or_default());
                    }

                    MessageType::UserMessage {
                        content,
                        assistant_id,
                        model,
                    } => {
                        let sid = match &session_id {
                            Some(s) => s.clone(),
                            None => {
                                let error = GatewayMessage::error("not_authenticated", "Please authenticate first");
                                let _ = tx.send(serde_json::to_string(&error).unwrap_or_default());
                                continue;
                            }
                        };

                        let (response_tx, mut response_rx) = mpsc::unbounded_channel();
                        let tx_for_response = tx.clone();

                        tokio::spawn(async move {
                            while let Some(msg) = response_rx.recv().await {
                                let json = serde_json::to_string(&msg).unwrap_or_default();
                                if tx_for_response.send(json).is_err() {
                                    break;
                                }
                            }
                        });

                        let runtime_clone = Arc::clone(&runtime);
                        tokio::spawn(async move {
                            let _ = runtime_clone
                                .process_message(
                                    &sid,
                                    &content,
                                    assistant_id.as_deref(),
                                    model.as_deref(),
                                    response_tx,
                                )
                                .await;
                        });
                    }

                    MessageType::Cancel { request_id: _ } => {
                        if let Some(sid) = &session_id {
                            runtime.cancel(sid).await;
                        }
                    }

                    MessageType::GetHistory { limit } => {
                        if let Some(sid) = &session_id {
                            let history = runtime.get_history(sid, limit).await;
                            let messages: Vec<HistoryMessage> = history
                                .into_iter()
                                .map(|(role, content)| HistoryMessage {
                                    role,
                                    content,
                                    timestamp: 0,
                                })
                                .collect();

                            let response = GatewayMessage::new(
                                Some(sid.clone()),
                                MessageType::History { messages },
                            );
                            let _ = tx.send(serde_json::to_string(&response).unwrap_or_default());
                        }
                    }

                    MessageType::Ping { timestamp } => {
                        let pong = GatewayMessage::pong(timestamp);
                        let _ = tx.send(serde_json::to_string(&pong).unwrap_or_default());
                    }

                    _ => {}
                }
            }

            WsMessage::Close(_) => {
                break;
            }

            _ => {}
        }
    }

    connections.write().await.remove(&client_id);

    if let (Some(sid), Some(info)) = (&session_id, &client_info) {
        session_manager.remove_client(sid, info.platform).await;
    }

    tracing::info!("WebSocket connection closed: {}", addr);
    Ok(())
}
