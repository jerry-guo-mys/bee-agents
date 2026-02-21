//! 用户隔离的长期记忆
//!
//! 支持多用户场景下的记忆隔离，每个用户拥有独立的向量存储空间

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::long_term::{InMemoryVectorLongTerm, LongTermMemory, NoopLongTerm};
use crate::llm::EmbeddingProvider;

/// 用户记忆配置
#[derive(Debug, Clone)]
pub struct UserMemoryConfig {
    /// 每用户最大条目数
    pub max_entries_per_user: usize,
    /// 快照存储目录（每用户一个文件）
    pub snapshot_dir: Option<PathBuf>,
    /// 是否启用向量记忆
    pub vector_enabled: bool,
}

impl Default for UserMemoryConfig {
    fn default() -> Self {
        Self {
            max_entries_per_user: 500,
            snapshot_dir: None,
            vector_enabled: true,
        }
    }
}

/// 用户隔离的长期记忆管理器
pub struct UserMemoryManager {
    config: UserMemoryConfig,
    embedder: Arc<dyn EmbeddingProvider>,
    /// user_id -> 向量记忆
    memories: RwLock<HashMap<String, Arc<dyn LongTermMemory>>>,
}

impl UserMemoryManager {
    pub fn new(config: UserMemoryConfig, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            config,
            embedder,
            memories: RwLock::new(HashMap::new()),
        }
    }

    /// 获取或创建用户的记忆存储
    pub async fn get_or_create(&self, user_id: &str) -> Arc<dyn LongTermMemory> {
        {
            let memories = self.memories.read().await;
            if let Some(memory) = memories.get(user_id) {
                return Arc::clone(memory);
            }
        }

        let mut memories = self.memories.write().await;
        if let Some(memory) = memories.get(user_id) {
            return Arc::clone(memory);
        }

        let memory: Arc<dyn LongTermMemory> = if self.config.vector_enabled {
            let snapshot_path = self.config.snapshot_dir.as_ref().map(|dir| {
                dir.join(format!("user_{}_vectors.json", sanitize_user_id(user_id)))
            });
            Arc::new(InMemoryVectorLongTerm::new_with_persistence(
                Arc::clone(&self.embedder),
                self.config.max_entries_per_user,
                snapshot_path,
            ))
        } else {
            Arc::new(NoopLongTerm)
        };

        memories.insert(user_id.to_string(), Arc::clone(&memory));
        memory
    }

    /// 获取用户记忆（不创建）
    pub async fn get(&self, user_id: &str) -> Option<Arc<dyn LongTermMemory>> {
        self.memories.read().await.get(user_id).cloned()
    }

    /// 为用户添加记忆
    pub async fn add(&self, user_id: &str, text: &str) {
        let memory = self.get_or_create(user_id).await;
        memory.add(text);
    }

    /// 为用户搜索记忆
    pub async fn search(&self, user_id: &str, query: &str, k: usize) -> Vec<String> {
        let memory = self.get_or_create(user_id).await;
        memory.search(query, k)
    }

    /// 刷新所有用户的记忆到磁盘
    pub async fn flush_all(&self) {
        let memories = self.memories.read().await;
        for memory in memories.values() {
            memory.flush();
        }
    }

    /// 获取活跃用户数
    pub async fn active_users(&self) -> usize {
        self.memories.read().await.len()
    }

    /// 清理指定用户的记忆
    pub async fn clear_user(&self, user_id: &str) -> bool {
        self.memories.write().await.remove(user_id).is_some()
    }

    /// 列出所有活跃用户
    pub async fn list_users(&self) -> Vec<String> {
        self.memories.read().await.keys().cloned().collect()
    }
}

/// 清理 user_id 中的特殊字符用于文件名
fn sanitize_user_id(user_id: &str) -> String {
    user_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 用户隔离的 LongTermMemory 包装器
/// 
/// 可以替代全局 LongTermMemory，在内部按 user_id 路由
pub struct UserScopedMemory {
    manager: Arc<UserMemoryManager>,
    user_id: String,
}

impl UserScopedMemory {
    pub fn new(manager: Arc<UserMemoryManager>, user_id: String) -> Self {
        Self { manager, user_id }
    }
}

impl LongTermMemory for UserScopedMemory {
    fn add(&self, text: &str) {
        let manager = Arc::clone(&self.manager);
        let user_id = self.user_id.clone();
        let text = text.to_string();
        tokio::spawn(async move {
            manager.add(&user_id, &text).await;
        });
    }

    fn search(&self, query: &str, k: usize) -> Vec<String> {
        let rt = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        };

        let manager = Arc::clone(&self.manager);
        let user_id = self.user_id.clone();
        let query = query.to_string();

        rt.block_on(async move { manager.search(&user_id, &query, k).await })
    }

    fn flush(&self) {
        let manager = Arc::clone(&self.manager);
        let user_id = self.user_id.clone();
        tokio::spawn(async move {
            if let Some(memory) = manager.get(&user_id).await {
                memory.flush();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockEmbedder;

    impl EmbeddingProvider for MockEmbedder {
        fn embed_sync(&self, _text: &str) -> Result<Vec<f32>, String> {
            Ok(vec![0.1, 0.2, 0.3])
        }
    }

    #[tokio::test]
    async fn test_user_isolation() {
        let config = UserMemoryConfig {
            max_entries_per_user: 100,
            snapshot_dir: None,
            vector_enabled: true,
        };
        let embedder = Arc::new(MockEmbedder);
        let manager = UserMemoryManager::new(config, embedder);

        manager.add("user_a", "User A's secret data").await;
        manager.add("user_b", "User B's private info").await;

        assert_eq!(manager.active_users().await, 2);

        let users = manager.list_users().await;
        assert!(users.contains(&"user_a".to_string()));
        assert!(users.contains(&"user_b".to_string()));

        assert!(manager.clear_user("user_a").await);
        assert_eq!(manager.active_users().await, 1);
    }

    #[test]
    fn test_sanitize_user_id() {
        assert_eq!(sanitize_user_id("user@example.com"), "user_example_com");
        assert_eq!(sanitize_user_id("user-123_abc"), "user-123_abc");
        assert_eq!(sanitize_user_id("用户/test"), "用户_test");
    }
}
