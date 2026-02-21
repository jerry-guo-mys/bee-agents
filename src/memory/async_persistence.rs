//! 异步 SQLite 持久化（Phase 4：sqlx 迁移）
//!
//! 使用 sqlx 提供完全异步的数据库操作，避免在 async 上下文中阻塞。
//! 需要启用 `async-sqlite` feature。

#[cfg(feature = "async-sqlite")]
mod sqlx_impl {
    use std::path::Path;

    use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
    use sqlx::Row;

    use crate::memory::{Message, Role};

    /// 异步 SQLite 持久化
    pub struct AsyncSqlitePersistence {
        pool: SqlitePool,
    }

    impl AsyncSqlitePersistence {
        /// 创建新的异步持久化实例
        pub async fn new(db_path: impl AsRef<Path>) -> Result<Self, sqlx::Error> {
            let db_url = format!("sqlite:{}?mode=rwc", db_path.as_ref().display());
            
            let pool = SqlitePoolOptions::new()
                .max_connections(5)
                .connect(&db_url)
                .await?;
            
            let persistence = Self { pool };
            persistence.init_tables().await?;
            
            Ok(persistence)
        }

        /// 从连接池创建
        pub fn from_pool(pool: SqlitePool) -> Self {
            Self { pool }
        }

        /// 初始化数据库表
        async fn init_tables(&self) -> Result<(), sqlx::Error> {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    title TEXT
                )"
            )
            .execute(&self.pool)
            .await?;

            sqlx::query(
                "CREATE TABLE IF NOT EXISTS messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
                )"
            )
            .execute(&self.pool)
            .await?;

            sqlx::query(
                "CREATE TABLE IF NOT EXISTS checkpoints (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    step INTEGER NOT NULL,
                    state TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
                )"
            )
            .execute(&self.pool)
            .await?;

            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id)"
            )
            .execute(&self.pool)
            .await?;

            Ok(())
        }

        /// 创建会话
        pub async fn create_session(
            &self,
            session_id: &str,
            title: Option<&str>,
        ) -> Result<(), sqlx::Error> {
            let now = chrono::Utc::now().to_rfc3339();
            
            sqlx::query(
                "INSERT OR REPLACE INTO sessions (id, created_at, updated_at, title) VALUES (?, ?, ?, ?)"
            )
            .bind(session_id)
            .bind(&now)
            .bind(&now)
            .bind(title)
            .execute(&self.pool)
            .await?;
            
            Ok(())
        }

        /// 保存消息
        pub async fn save_message(
            &self,
            session_id: &str,
            message: &Message,
        ) -> Result<(), sqlx::Error> {
            let role_str = match message.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => "system",
                Role::Tool => "tool",
            };
            let now = chrono::Utc::now().to_rfc3339();

            sqlx::query(
                "INSERT INTO messages (session_id, role, content, created_at) VALUES (?, ?, ?, ?)"
            )
            .bind(session_id)
            .bind(role_str)
            .bind(&message.content)
            .bind(&now)
            .execute(&self.pool)
            .await?;

            sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
                .bind(&now)
                .bind(session_id)
                .execute(&self.pool)
                .await?;

            Ok(())
        }

        /// 批量保存消息（事务）
        pub async fn save_messages(
            &self,
            session_id: &str,
            messages: &[Message],
        ) -> Result<(), sqlx::Error> {
            let mut tx = self.pool.begin().await?;
            let now = chrono::Utc::now().to_rfc3339();

            for message in messages {
                let role_str = match message.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => "system",
                    Role::Tool => "tool",
                };

                sqlx::query(
                    "INSERT INTO messages (session_id, role, content, created_at) VALUES (?, ?, ?, ?)"
                )
                .bind(session_id)
                .bind(role_str)
                .bind(&message.content)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }

            sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
                .bind(&now)
                .bind(session_id)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            Ok(())
        }

        /// 加载消息
        pub async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>, sqlx::Error> {
            let rows = sqlx::query(
                "SELECT role, content FROM messages WHERE session_id = ? ORDER BY id ASC"
            )
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

            let messages = rows
                .into_iter()
                .map(|row| {
                    let role_str: String = row.get("role");
                    let content: String = row.get("content");
                    let role = match role_str.as_str() {
                        "user" => Role::User,
                        "assistant" => Role::Assistant,
                        "tool" => Role::Tool,
                        _ => Role::System,
                    };
                    Message { role, content }
                })
                .collect();

            Ok(messages)
        }

        /// 保存检查点
        pub async fn save_checkpoint(
            &self,
            session_id: &str,
            step: i32,
            state: &str,
        ) -> Result<(), sqlx::Error> {
            let now = chrono::Utc::now().to_rfc3339();
            
            sqlx::query(
                "INSERT INTO checkpoints (session_id, step, state, created_at) VALUES (?, ?, ?, ?)"
            )
            .bind(session_id)
            .bind(step)
            .bind(state)
            .bind(&now)
            .execute(&self.pool)
            .await?;
            
            Ok(())
        }

        /// 加载最新检查点
        pub async fn load_latest_checkpoint(
            &self,
            session_id: &str,
        ) -> Result<Option<(i32, String)>, sqlx::Error> {
            let result = sqlx::query(
                "SELECT step, state FROM checkpoints WHERE session_id = ? ORDER BY id DESC LIMIT 1"
            )
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;

            Ok(result.map(|row| {
                let step: i32 = row.get("step");
                let state: String = row.get("state");
                (step, state)
            }))
        }

        /// 列出会话
        pub async fn list_sessions(
            &self,
            limit: i32,
        ) -> Result<Vec<(String, String, Option<String>)>, sqlx::Error> {
            let rows = sqlx::query(
                "SELECT id, updated_at, title FROM sessions ORDER BY updated_at DESC LIMIT ?"
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

            let sessions = rows
                .into_iter()
                .map(|row| {
                    let id: String = row.get("id");
                    let updated: String = row.get("updated_at");
                    let title: Option<String> = row.get("title");
                    (id, updated, title)
                })
                .collect();

            Ok(sessions)
        }

        /// 删除会话
        pub async fn delete_session(&self, session_id: &str) -> Result<(), sqlx::Error> {
            sqlx::query("DELETE FROM messages WHERE session_id = ?")
                .bind(session_id)
                .execute(&self.pool)
                .await?;
            
            sqlx::query("DELETE FROM checkpoints WHERE session_id = ?")
                .bind(session_id)
                .execute(&self.pool)
                .await?;
            
            sqlx::query("DELETE FROM sessions WHERE id = ?")
                .bind(session_id)
                .execute(&self.pool)
                .await?;
            
            Ok(())
        }

        /// 清理旧检查点（保留最近 n 个）
        pub async fn cleanup_checkpoints(
            &self,
            session_id: &str,
            keep_count: i32,
        ) -> Result<u64, sqlx::Error> {
            let result = sqlx::query(
                "DELETE FROM checkpoints WHERE session_id = ? AND id NOT IN (
                    SELECT id FROM checkpoints WHERE session_id = ? ORDER BY id DESC LIMIT ?
                )"
            )
            .bind(session_id)
            .bind(session_id)
            .bind(keep_count)
            .execute(&self.pool)
            .await?;
            
            Ok(result.rows_affected())
        }

        /// 获取连接池统计
        pub fn pool_stats(&self) -> (u32, u32) {
            (self.pool.size(), self.pool.num_idle() as u32)
        }

        /// 关闭连接池
        pub async fn close(&self) {
            self.pool.close().await;
        }
    }
}

#[cfg(feature = "async-sqlite")]
pub use sqlx_impl::AsyncSqlitePersistence;

/// 异步持久化 trait（可用于抽象不同的存储后端）
#[cfg(feature = "async-sqlite")]
#[async_trait::async_trait]
pub trait AsyncPersistence: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn create_session(&self, session_id: &str, title: Option<&str>) -> Result<(), Self::Error>;
    async fn save_message(&self, session_id: &str, message: &crate::memory::Message) -> Result<(), Self::Error>;
    async fn load_messages(&self, session_id: &str) -> Result<Vec<crate::memory::Message>, Self::Error>;
    async fn delete_session(&self, session_id: &str) -> Result<(), Self::Error>;
}

#[cfg(feature = "async-sqlite")]
#[async_trait::async_trait]
impl AsyncPersistence for AsyncSqlitePersistence {
    type Error = sqlx::Error;

    async fn create_session(&self, session_id: &str, title: Option<&str>) -> Result<(), Self::Error> {
        AsyncSqlitePersistence::create_session(self, session_id, title).await
    }

    async fn save_message(&self, session_id: &str, message: &crate::memory::Message) -> Result<(), Self::Error> {
        AsyncSqlitePersistence::save_message(self, session_id, message).await
    }

    async fn load_messages(&self, session_id: &str) -> Result<Vec<crate::memory::Message>, Self::Error> {
        AsyncSqlitePersistence::load_messages(self, session_id).await
    }

    async fn delete_session(&self, session_id: &str) -> Result<(), Self::Error> {
        AsyncSqlitePersistence::delete_session(self, session_id).await
    }
}

#[cfg(all(test, feature = "async-sqlite"))]
mod tests {
    use super::*;
    use crate::memory::{Message, Role};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_async_persistence_basic() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        
        let persistence = AsyncSqlitePersistence::new(&db_path).await.unwrap();
        
        // 创建会话
        persistence.create_session("test-session", Some("Test")).await.unwrap();
        
        // 保存消息
        let msg = Message::user("Hello");
        persistence.save_message("test-session", &msg).await.unwrap();
        
        // 加载消息
        let messages = persistence.load_messages("test-session").await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[0].role, Role::User);
    }

    #[tokio::test]
    async fn test_async_persistence_batch() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        
        let persistence = AsyncSqlitePersistence::new(&db_path).await.unwrap();
        persistence.create_session("batch-session", None).await.unwrap();
        
        let messages = vec![
            Message::user("Q1"),
            Message::assistant("A1"),
            Message::user("Q2"),
            Message::assistant("A2"),
        ];
        
        persistence.save_messages("batch-session", &messages).await.unwrap();
        
        let loaded = persistence.load_messages("batch-session").await.unwrap();
        assert_eq!(loaded.len(), 4);
    }

    #[tokio::test]
    async fn test_async_persistence_checkpoints() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        
        let persistence = AsyncSqlitePersistence::new(&db_path).await.unwrap();
        persistence.create_session("checkpoint-session", None).await.unwrap();
        
        // 保存检查点
        persistence.save_checkpoint("checkpoint-session", 1, r#"{"step": 1}"#).await.unwrap();
        persistence.save_checkpoint("checkpoint-session", 2, r#"{"step": 2}"#).await.unwrap();
        
        // 加载最新检查点
        let checkpoint = persistence.load_latest_checkpoint("checkpoint-session").await.unwrap();
        assert!(checkpoint.is_some());
        let (step, state) = checkpoint.unwrap();
        assert_eq!(step, 2);
        assert!(state.contains("step"));
    }
}
