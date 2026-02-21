//! 会话存储抽象层
//!
//! 定义统一的会话管理接口，支持内存和持久化两种实现

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::message::{ClientInfo, SessionStatus, SpokeType};
use super::session::SessionId;
use crate::memory::Message;
use crate::react::ContextManager;

#[cfg(feature = "async-sqlite")]
use super::persistent_session::PersistentSessionManager;

/// 会话存储接口
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// 获取或创建用户的会话
    async fn get_or_create(&self, user_id: &str, client: ClientInfo) -> SessionId;

    /// 添加消息到会话
    async fn add_message(&self, session_id: &str, message: Message);

    /// 获取会话上下文
    async fn get_context(&self, session_id: &str) -> Option<ContextManager>;

    /// 更新会话上下文
    async fn set_context(&self, session_id: &str, context: ContextManager);

    /// 设置会话状态
    async fn set_status(&self, session_id: &str, status: SessionStatus);

    /// 取消会话的当前请求
    async fn cancel(&self, session_id: &str);

    /// 创建新的取消令牌
    async fn new_cancel_token(&self, session_id: &str) -> Option<CancellationToken>;

    /// 移除客户端连接
    async fn remove_client(&self, session_id: &str, platform: SpokeType);

    /// 清理过期会话
    async fn cleanup_expired(&self) -> usize;

    /// 获取活跃会话数
    async fn active_count(&self) -> usize;

    /// 获取用户的会话 ID
    async fn get_user_session(&self, user_id: &str) -> Option<SessionId>;

    /// 获取会话历史
    async fn get_history(&self, session_id: &str, limit: Option<usize>) -> Vec<(String, String)>;
}

/// 内存会话存储（包装 SessionManager）
pub struct MemorySessionStore {
    inner: super::session::SessionManager,
}

impl MemorySessionStore {
    pub fn new(max_context_turns: usize, session_timeout_secs: u64) -> Self {
        Self {
            inner: super::session::SessionManager::new(max_context_turns, session_timeout_secs),
        }
    }
}

#[async_trait]
impl SessionStore for MemorySessionStore {
    async fn get_or_create(&self, user_id: &str, client: ClientInfo) -> SessionId {
        self.inner.get_or_create(user_id, client).await
    }

    async fn add_message(&self, session_id: &str, message: Message) {
        self.inner.with_session(session_id, |s| {
            s.context.push_message(message);
        }).await;
    }

    async fn get_context(&self, session_id: &str) -> Option<ContextManager> {
        self.inner.with_session(session_id, |s| s.context.clone()).await
    }

    async fn set_context(&self, session_id: &str, context: ContextManager) {
        self.inner.with_session(session_id, |s| {
            s.context = context;
        }).await;
    }

    async fn set_status(&self, session_id: &str, status: SessionStatus) {
        self.inner.with_session(session_id, |s| {
            s.set_status(status);
        }).await;
    }

    async fn cancel(&self, session_id: &str) {
        self.inner.with_session(session_id, |s| {
            s.cancel();
        }).await;
    }

    async fn new_cancel_token(&self, session_id: &str) -> Option<CancellationToken> {
        self.inner.with_session(session_id, |s| s.new_cancel_token()).await
    }

    async fn remove_client(&self, session_id: &str, platform: SpokeType) {
        self.inner.remove_client(session_id, platform).await;
    }

    async fn cleanup_expired(&self) -> usize {
        self.inner.cleanup_expired().await
    }

    async fn active_count(&self) -> usize {
        self.inner.active_count().await
    }

    async fn get_user_session(&self, user_id: &str) -> Option<SessionId> {
        self.inner.get_user_session(user_id).await
    }

    async fn get_history(&self, session_id: &str, limit: Option<usize>) -> Vec<(String, String)> {
        self.inner.with_session(session_id, |s| {
            let messages = s.context.messages();
            let limited = if let Some(l) = limit {
                &messages[messages.len().saturating_sub(l)..]
            } else {
                messages
            };
            limited.iter().map(|m| (format!("{:?}", m.role), m.content.clone())).collect()
        }).await.unwrap_or_default()
    }
}

/// 持久化会话存储（包装 PersistentSessionManager）
#[cfg(feature = "async-sqlite")]
pub struct PersistentSessionStore {
    inner: PersistentSessionManager,
}

#[cfg(feature = "async-sqlite")]
impl PersistentSessionStore {
    pub async fn new(
        db_path: impl AsRef<std::path::Path>,
        max_context_turns: usize,
        session_timeout_secs: u64,
    ) -> Result<Self, sqlx::Error> {
        let inner = PersistentSessionManager::new(
            db_path,
            max_context_turns,
            session_timeout_secs,
        ).await?;
        Ok(Self { inner })
    }
}

#[cfg(feature = "async-sqlite")]
#[async_trait]
impl SessionStore for PersistentSessionStore {
    async fn get_or_create(&self, user_id: &str, client: ClientInfo) -> SessionId {
        self.inner.get_or_create(user_id, client).await
    }

    async fn add_message(&self, session_id: &str, message: Message) {
        self.inner.add_message(session_id, message).await;
    }

    async fn get_context(&self, session_id: &str) -> Option<ContextManager> {
        self.inner.get_context(session_id).await
    }

    async fn set_context(&self, session_id: &str, context: ContextManager) {
        self.inner.with_session(session_id, |s| {
            s.context = context;
        }).await;
    }

    async fn set_status(&self, session_id: &str, status: SessionStatus) {
        self.inner.set_status(session_id, status).await;
    }

    async fn cancel(&self, session_id: &str) {
        self.inner.cancel(session_id).await;
    }

    async fn new_cancel_token(&self, session_id: &str) -> Option<CancellationToken> {
        self.inner.new_cancel_token(session_id).await
    }

    async fn remove_client(&self, session_id: &str, platform: SpokeType) {
        self.inner.remove_client(session_id, platform).await;
    }

    async fn cleanup_expired(&self) -> usize {
        self.inner.cleanup_expired().await
    }

    async fn active_count(&self) -> usize {
        self.inner.active_count().await
    }

    async fn get_user_session(&self, user_id: &str) -> Option<SessionId> {
        self.inner.get_user_session(user_id).await
    }

    async fn get_history(&self, session_id: &str, limit: Option<usize>) -> Vec<(String, String)> {
        self.inner.with_session(session_id, |s| {
            let messages = s.context.messages();
            let limited = if let Some(l) = limit {
                &messages[messages.len().saturating_sub(l)..]
            } else {
                messages
            };
            limited.iter().map(|m| (format!("{:?}", m.role), m.content.clone())).collect()
        }).await.unwrap_or_default()
    }
}

/// 创建会话存储
/// 
/// 如果提供了 db_path 且启用了 async-sqlite feature，则使用持久化存储；否则使用内存存储
pub async fn create_session_store(
    db_path: Option<&std::path::Path>,
    max_context_turns: usize,
    session_timeout_secs: u64,
) -> Arc<dyn SessionStore> {
    #[cfg(feature = "async-sqlite")]
    if let Some(path) = db_path {
        match PersistentSessionStore::new(path, max_context_turns, session_timeout_secs).await {
            Ok(store) => {
                tracing::info!("Using persistent session store: {:?}", path);
                return Arc::new(store);
            }
            Err(e) => {
                tracing::warn!("Failed to create persistent store, falling back to memory: {}", e);
            }
        }
    }
    
    #[cfg(not(feature = "async-sqlite"))]
    if db_path.is_some() {
        tracing::warn!("Persistent session store requested but async-sqlite feature not enabled, using memory store");
    }
    
    tracing::info!("Using in-memory session store");
    Arc::new(MemorySessionStore::new(max_context_turns, session_timeout_secs))
}
