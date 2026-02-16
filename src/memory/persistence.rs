use std::path::Path;

use crate::memory::{Message, Role};

#[derive(Debug)]
pub struct ConversationPersistence {
    path: std::path::PathBuf,
}

impl ConversationPersistence {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self) -> anyhow::Result<Vec<Message>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let data = std::fs::read_to_string(&self.path)?;
        let messages: Vec<SerMessage> = serde_json::from_str(&data)?;
        Ok(messages
            .into_iter()
            .map(|m| Message {
                role: match m.role.as_str() {
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    _ => Role::System,
                },
                content: m.content,
            })
            .collect())
    }

    pub fn save(&self, messages: &[Message]) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let ser: Vec<SerMessage> = messages
            .iter()
            .map(|m| SerMessage {
                role: match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => "system",
                }
                .to_string(),
                content: m.content.clone(),
            })
            .collect();
        std::fs::write(&self.path, serde_json::to_string_pretty(&ser)?)?;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SerMessage {
    role: String,
    content: String,
}

use rusqlite::{params, Connection, Result as SqliteResult};
use chrono::Utc;

pub struct SqlitePersistence {
    conn: Connection,
}

impl SqlitePersistence {
    pub fn new(db_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(db_path)?;
        let persistence = Self { conn };
        persistence.init_tables()?;
        Ok(persistence)
    }

    fn init_tables(&self) -> SqliteResult<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                title TEXT
            )",
            [],
        )?;
        
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )",
            [],
        )?;
        
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                step INTEGER NOT NULL,
                state TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )",
            [],
        )?;
        
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id)",
            [],
        )?;
        
        Ok(())
    }

    pub fn create_session(&self, session_id: &str, title: Option<&str>) -> SqliteResult<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (id, created_at, updated_at, title) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, now, now, title],
        )?;
        Ok(())
    }

    pub fn save_message(&self, session_id: &str, message: &Message) -> SqliteResult<()> {
        let role_str = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
        };
        let now = Utc::now().to_rfc3339();
        
        self.conn.execute(
            "INSERT INTO messages (session_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, role_str, message.content, now],
        )?;
        
        self.conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        
        Ok(())
    }

    pub fn load_messages(&self, session_id: &str) -> SqliteResult<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT role, content FROM messages WHERE session_id = ?1 ORDER BY id ASC"
        )?;
        
        let messages = stmt.query_map([session_id], |row| {
            let role_str: String = row.get(0)?;
            let content: String = row.get(1)?;
            let role = match role_str.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                _ => Role::System,
            };
            Ok(Message { role, content })
        })?.collect::<SqliteResult<Vec<_>>>()?;
        
        Ok(messages)
    }

    pub fn save_checkpoint(&self, session_id: &str, step: i32, state: &str) -> SqliteResult<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO checkpoints (session_id, step, state, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, step, state, now],
        )?;
        Ok(())
    }

    pub fn load_latest_checkpoint(&self, session_id: &str) -> SqliteResult<Option<(i32, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT step, state FROM checkpoints WHERE session_id = ?1 ORDER BY id DESC LIMIT 1"
        )?;
        
        let result = stmt.query_row([session_id], |row| {
            let step: i32 = row.get(0)?;
            let state: String = row.get(1)?;
            Ok((step, state))
        });
        
        match result {
            Ok(checkpoint) => Ok(Some(checkpoint)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn list_sessions(&self, limit: i32) -> SqliteResult<Vec<(String, String, Option<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, updated_at, title FROM sessions ORDER BY updated_at DESC LIMIT ?1"
        )?;
        
        let sessions = stmt.query_map([limit], |row| {
            let id: String = row.get(0)?;
            let updated: String = row.get(1)?;
            let title: Option<String> = row.get(2)?;
            Ok((id, updated, title))
        })?.collect::<SqliteResult<Vec<_>>>()?;
        
        Ok(sessions)
    }

    pub fn delete_session(&self, session_id: &str) -> SqliteResult<()> {
        self.conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            [session_id],
        )?;
        Ok(())
    }
}
