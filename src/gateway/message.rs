//! 网关消息协议定义
//!
//! 统一的消息格式，用于 Gateway 与各 Spoke 之间的通信

use serde::{Deserialize, Serialize};

/// 客户端信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    /// 客户端唯一标识（如 user_id + platform）
    pub client_id: String,
    /// 来源平台
    pub platform: SpokeType,
    /// 用户显示名称
    pub display_name: Option<String>,
    /// 额外元数据
    pub metadata: Option<serde_json::Value>,
}

/// Spoke 类型（平台来源）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpokeType {
    /// Web 浏览器
    Web,
    /// 终端 TUI
    Tui,
    /// WhatsApp
    WhatsApp,
    /// 飞书
    Lark,
    /// HTTP API
    Api,
    /// 其他
    Other,
}

impl std::fmt::Display for SpokeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpokeType::Web => write!(f, "web"),
            SpokeType::Tui => write!(f, "tui"),
            SpokeType::WhatsApp => write!(f, "whatsapp"),
            SpokeType::Lark => write!(f, "lark"),
            SpokeType::Api => write!(f, "api"),
            SpokeType::Other => write!(f, "other"),
        }
    }
}

/// 消息类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageType {
    /// 用户输入消息
    UserMessage {
        content: String,
        /// 可选：指定助手 ID
        assistant_id: Option<String>,
        /// 可选：指定模型
        model: Option<String>,
    },

    /// AI 响应（流式开始）
    ResponseStart {
        request_id: String,
    },

    /// AI 响应（流式 chunk）
    ResponseChunk {
        request_id: String,
        content: String,
    },

    /// AI 响应（流式结束）
    ResponseEnd {
        request_id: String,
        full_content: String,
    },

    /// 工具调用通知
    ToolCall {
        request_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },

    /// 工具执行结果
    ToolResult {
        request_id: String,
        tool_name: String,
        result: String,
        success: bool,
    },

    /// 思考过程
    Thinking {
        request_id: String,
        content: String,
    },

    /// 错误
    Error {
        request_id: Option<String>,
        code: String,
        message: String,
    },

    /// 会话状态更新
    SessionUpdate {
        session_id: String,
        status: SessionStatus,
    },

    /// 心跳 ping
    Ping {
        timestamp: u64,
    },

    /// 心跳 pong
    Pong {
        timestamp: u64,
    },

    /// 客户端认证
    Auth {
        token: Option<String>,
        client_info: ClientInfo,
    },

    /// 认证结果
    AuthResult {
        success: bool,
        session_id: Option<String>,
        message: Option<String>,
    },

    /// 取消当前请求
    Cancel {
        request_id: String,
    },

    /// 请求会话历史
    GetHistory {
        limit: Option<usize>,
    },

    /// 会话历史响应
    History {
        messages: Vec<HistoryMessage>,
    },

    /// 后台任务完成通知
    TaskComplete {
        task_id: String,
        user_id: String,
        success: bool,
        result: Option<String>,
        error: Option<String>,
    },

    /// 提交后台任务
    SubmitTask {
        instruction: String,
        priority: Option<String>,
    },

    /// 任务提交结果
    TaskSubmitted {
        task_id: String,
    },

    /// 查询任务状态
    GetTaskStatus {
        task_id: String,
    },

    /// 任务状态响应
    TaskStatus {
        task_id: String,
        status: String,
        progress: u8,
        result: Option<String>,
        error: Option<String>,
    },
}

/// 会话状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// 空闲
    Idle,
    /// 处理中
    Processing,
    /// 等待用户输入
    WaitingInput,
    /// 已断开
    Disconnected,
}

/// 历史消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
    pub timestamp: u64,
}

/// 网关消息（带元信息的完整消息）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    /// 消息 ID
    pub id: String,
    /// 会话 ID
    pub session_id: Option<String>,
    /// 消息内容
    pub message: MessageType,
    /// 时间戳（毫秒）
    pub timestamp: u64,
}

impl GatewayMessage {
    pub fn new(session_id: Option<String>, message: MessageType) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id,
            message,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }

    pub fn error(code: &str, message: &str) -> Self {
        Self::new(
            None,
            MessageType::Error {
                request_id: None,
                code: code.to_string(),
                message: message.to_string(),
            },
        )
    }

    pub fn pong(timestamp: u64) -> Self {
        Self::new(None, MessageType::Pong { timestamp })
    }
}
