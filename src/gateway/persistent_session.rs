//! 持久化会话管理
//!
//! 使用 SQLite 存储会话状态，支持跨重启恢复

#![cfg(feature = "async-sqlite")]

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use sqlx::Row;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::message::{ClientInfo, SessionStatus, SpokeType};
use super::session::{Session, SessionId};
use crate::react::ContextManager;

/// 持久化会话管理器
/// 
/// 与内存版 SessionManager 的区别：
/// - 会话元数据持久化到 SQLite
/// - 消息历史持久化到 SQLite
/// - 服务重启后可恢复会话
pub struct PersistentSessionManager {
    /// 活跃会话（内存缓存）
    sessions: RwLock<HashMap<SessionId, Session>>,
    /// 用户到会话的映射
    user_sessions: RwLock<HashMap<String, SessionId>>,
    /// SQLite 连接池
    pool: sqlx::sqlite::SqlitePool,
    /// 最大上下文轮数
    max_context_turns: usize,
    /// 会话过期时间
    session_timeout: Duration,
}

impl PersistentSessionManager {
    /// 创建新的持久化会话管理器
    pub async fn new(
        db_path: impl AsRef<Path>,
        max_context_turns: usize,
        session_timeout_secs: u64,
    ) -> Result<Self, sqlx::Error> {
        let db_url = format!("sqlite:{}?mode=rwc", db_path.as_ref().display());
        
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await?;
        
        let manager = Self {
            sessions: RwLock::new(HashMap::new()),
            user_sessions: RwLock::new(HashMap::new()),
            pool,
            max_context_turns,
            session_timeout: Duration::from_secs(session_timeout_secs),
        };
        
        manager.init_tables().await?;
        manager.restore_sessions().await?;
        
        Ok(manager)
    }

    /// 初始化数据库表
    async fn init_tables(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS gateway_sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                assistant_id TEXT,
                model_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )"
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS gateway_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES gateway_sessions(id) ON DELETE CASCADE
            )"
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_gateway_sessions_user ON gateway_sessions(user_id)"
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_gateway_messages_session ON gateway_messages(session_id)"
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 从数据库恢复活跃会话
    async fn restore_sessions(&self) -> Result<(), sqlx::Error> {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(self.session_timeout.as_secs() as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let rows = sqlx::query(
            "SELECT id, user_id, assistant_id, model_id, created_at, updated_at 
             FROM gateway_sessions 
             WHERE updated_at > ?"
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await?;

        let mut sessions = self.sessions.write().await;
        let mut user_sessions = self.user_sessions.write().await;

        for row in rows {
            let session_id: String = row.get("id");
            let user_id: String = row.get("user_id");
            let assistant_id: Option<String> = row.get("assistant_id");
            let model_id: Option<String> = row.get("model_id");

            let messages = self.load_messages(&session_id).await?;
            
            let mut session = Session {
                id: session_id.clone(),
                user_id: user_id.clone(),
                clients: HashMap::new(),
                context: ContextManager::new(self.max_context_turns),
                status: SessionStatus::Idle,
                cancel_token: None,
                last_active: Instant::now(),
                created_at: Instant::now(),
                assistant_id,
                model_id,
            };

            for msg in messages {
                session.context.push_message(msg);
            }

            user_sessions.insert(user_id, session_id.clone());
            sessions.insert(session_id, session);
        }

        let count = sessions.len();
        if count > 0 {
            tracing::info!("Restored {} sessions from database", count);
        }

        Ok(())
    }

    /// 加载会话消息
    async fn load_messages(&self, session_id: &str) -> Result<Vec<crate::memory::Message>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT role, content FROM gateway_messages WHERE session_id = ? ORDER BY id ASC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut messages = Vec::new();
        for row in rows {
            let role_str: String = row.get("role");
            let content: String = row.get("content");
            
            let role = match role_str.as_str() {
                "user" => crate::memory::Role::User,
                "assistant" => crate::memory::Role::Assistant,
                "system" => crate::memory::Role::System,
                "tool" => crate::memory::Role::Tool,
                _ => continue,
            };
            
            messages.push(crate::memory::Message { role, content });
        }

        Ok(messages)
    }

    /// 保存消息到数据库
    async fn save_message(&self, session_id: &str, message: &crate::memory::Message) -> Result<(), sqlx::Error> {
        let role_str = match message.role {
            crate::memory::Role::User => "user",
            crate::memory::Role::Assistant => "assistant",
            crate::memory::Role::System => "system",
            crate::memory::Role::Tool => "tool",
        };
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO gateway_messages (session_id, role, content, created_at) VALUES (?, ?, ?, ?)"
        )
        .bind(session_id)
        .bind(role_str)
        .bind(&message.content)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE gateway_sessions SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// 创建会话记录
    async fn create_session_record(&self, session: &Session) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT OR REPLACE INTO gateway_sessions (id, user_id, assistant_id, model_id, created_at, updated_at) 
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&session.id)
        .bind(&session.user_id)
        .bind(&session.assistant_id)
        .bind(&session.model_id)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
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

        if let Err(e) = self.create_session_record(&session).await {
            tracing::error!("Failed to persist session: {}", e);
        }

        self.sessions.write().await.insert(session_id.clone(), session);
        self.user_sessions.write().await.insert(user_id.to_string(), session_id.clone());

        session_id
    }

    /// 添加消息到会话（同时持久化）
    pub async fn add_message(&self, session_id: &str, message: crate::memory::Message) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.context.push_message(message.clone());
            session.last_active = Instant::now();
        }
        drop(sessions);

        if let Err(e) = self.save_message(session_id, &message).await {
            tracing::error!("Failed to persist message: {}", e);
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

    /// 获取会话上下文
    pub async fn get_context(&self, session_id: &str) -> Option<ContextManager> {
        self.sessions.read().await.get(session_id).map(|s| s.context.clone())
    }

    /// 取消会话的当前请求
    pub async fn cancel(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.cancel();
        }
    }

    /// 创建新的取消令牌
    pub async fn new_cancel_token(&self, session_id: &str) -> Option<CancellationToken> {
        let mut sessions = self.sessions.write().await;
        sessions.get_mut(session_id).map(|s| s.new_cancel_token())
    }

    /// 设置会话状态
    pub async fn set_status(&self, session_id: &str, status: SessionStatus) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.set_status(status);
        }
    }

    /// 关闭连接池
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_session_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_sessions.db");

        let manager = PersistentSessionManager::new(&db_path, 20, 3600)
            .await
            .unwrap();

        let client = ClientInfo {
            client_id: "test_client".to_string(),
            platform: SpokeType::Web,
            display_name: None,
            metadata: None,
        };

        let session_id = manager.get_or_create("user_123", client).await;

        let msg = crate::memory::Message::user("Hello, world!");
        manager.add_message(&session_id, msg).await;

        let msg2 = crate::memory::Message::assistant("Hi there!");
        manager.add_message(&session_id, msg2).await;

        assert_eq!(manager.active_count().await, 1);
        
        manager.close().await;

        let manager2 = PersistentSessionManager::new(&db_path, 20, 3600)
            .await
            .unwrap();

        assert_eq!(manager2.active_count().await, 1);

        let session_id2 = manager2.get_user_session("user_123").await;
        assert!(session_id2.is_some());
        assert_eq!(session_id2.unwrap(), session_id);

        let ctx = manager2.get_context(&session_id).await.unwrap();
        let messages = ctx.messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "Hello, world!");
        assert_eq!(messages[1].content, "Hi there!");
    }
}
