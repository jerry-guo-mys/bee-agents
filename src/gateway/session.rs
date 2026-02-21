//! 会话管理
//!
//! 统一管理所有平台的会话状态，支持跨平台上下文连贯

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::message::{ClientInfo, SessionStatus, SpokeType};
use crate::react::ContextManager;

/// 会话 ID（用户维度，跨平台共享）
pub type SessionId = String;

/// 单个会话
pub struct Session {
    /// 会话 ID
    pub id: SessionId,
    /// 关联的用户 ID（跨平台统一）
    pub user_id: String,
    /// 当前连接的客户端信息
    pub clients: HashMap<SpokeType, ClientInfo>,
    /// 对话上下文（跨平台共享）
    pub context: ContextManager,
    /// 会话状态
    pub status: SessionStatus,
    /// 当前请求的取消令牌
    pub cancel_token: Option<CancellationToken>,
    /// 最后活跃时间
    pub last_active: Instant,
    /// 创建时间
    pub created_at: Instant,
    /// 助手 ID（可选）
    pub assistant_id: Option<String>,
    /// 模型 ID（可选）
    pub model_id: Option<String>,
}

impl Session {
    pub fn new(user_id: String, max_context_turns: usize) -> Self {
        let id = format!("session_{}", uuid::Uuid::new_v4());
        Self {
            id,
            user_id,
            clients: HashMap::new(),
            context: ContextManager::new(max_context_turns),
            status: SessionStatus::Idle,
            cancel_token: None,
            last_active: Instant::now(),
            created_at: Instant::now(),
            assistant_id: None,
            model_id: None,
        }
    }

    /// 添加客户端连接
    pub fn add_client(&mut self, client: ClientInfo) {
        self.clients.insert(client.platform, client);
        self.last_active = Instant::now();
    }

    /// 移除客户端连接
    pub fn remove_client(&mut self, platform: SpokeType) {
        self.clients.remove(&platform);
    }

    /// 检查会话是否还有活跃连接
    pub fn has_active_clients(&self) -> bool {
        !self.clients.is_empty()
    }

    /// 更新状态
    pub fn set_status(&mut self, status: SessionStatus) {
        self.status = status;
        self.last_active = Instant::now();
    }

    /// 取消当前请求
    pub fn cancel(&mut self) {
        if let Some(token) = self.cancel_token.take() {
            token.cancel();
        }
        self.status = SessionStatus::Idle;
    }

    /// 创建新的取消令牌
    pub fn new_cancel_token(&mut self) -> CancellationToken {
        self.cancel();
        let token = CancellationToken::new();
        self.cancel_token = Some(token.clone());
        token
    }

    /// 会话是否过期
    pub fn is_expired(&self, timeout: Duration) -> bool {
        self.last_active.elapsed() > timeout && !self.has_active_clients()
    }
}

/// 会话管理器
pub struct SessionManager {
    /// 所有会话（session_id -> Session）
    sessions: RwLock<HashMap<SessionId, Session>>,
    /// 用户到会话的映射（user_id -> session_id）
    user_sessions: RwLock<HashMap<String, SessionId>>,
    /// 最大上下文轮数
    max_context_turns: usize,
    /// 会话过期时间
    session_timeout: Duration,
}

impl SessionManager {
    pub fn new(max_context_turns: usize, session_timeout_secs: u64) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            user_sessions: RwLock::new(HashMap::new()),
            max_context_turns,
            session_timeout: Duration::from_secs(session_timeout_secs),
        }
    }

    /// 获取或创建用户的会话
    pub async fn get_or_create(&self, user_id: &str, client: ClientInfo) -> SessionId {
        let user_sessions = self.user_sessions.read().await;
        
        if let Some(session_id) = user_sessions.get(user_id) {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(session_id) {
                session.add_client(client);
                return session_id.clone();
            }
        }
        drop(user_sessions);

        let mut session = Session::new(user_id.to_string(), self.max_context_turns);
        session.add_client(client);
        let session_id = session.id.clone();

        self.sessions.write().await.insert(session_id.clone(), session);
        self.user_sessions.write().await.insert(user_id.to_string(), session_id.clone());

        session_id
    }

    /// 获取会话
    pub async fn get(&self, session_id: &str) -> Option<Arc<RwLock<Session>>> {
        let sessions = self.sessions.read().await;
        if sessions.contains_key(session_id) {
            drop(sessions);
            Some(Arc::new(RwLock::new(
                self.sessions.write().await.remove(session_id).unwrap()
            )))
        } else {
            None
        }
    }

    /// 获取会话（可变引用）
    pub async fn with_session<F, R>(&self, session_id: &str, f: F) -> Option<R>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut sessions = self.sessions.write().await;
        sessions.get_mut(session_id).map(f)
    }

    /// 移除客户端连接
    pub async fn remove_client(&self, session_id: &str, platform: SpokeType) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.remove_client(platform);
        }
    }

    /// 清理过期会话
    pub async fn cleanup_expired(&self) -> usize {
        let mut sessions = self.sessions.write().await;
        let mut user_sessions = self.user_sessions.write().await;
        
        let expired: Vec<_> = sessions
            .iter()
            .filter(|(_, s)| s.is_expired(self.session_timeout))
            .map(|(id, s)| (id.clone(), s.user_id.clone()))
            .collect();

        for (session_id, user_id) in &expired {
            sessions.remove(session_id);
            user_sessions.remove(user_id);
        }

        expired.len()
    }

    /// 获取活跃会话数
    pub async fn active_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// 获取用户的会话 ID
    pub async fn get_user_session(&self, user_id: &str) -> Option<SessionId> {
        self.user_sessions.read().await.get(user_id).cloned()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(20, 3600)
    }
}
