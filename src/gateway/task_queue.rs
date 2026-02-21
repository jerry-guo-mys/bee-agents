//! 后台任务队列
//!
//! 支持用户离线时 AI 后台完成任务，完成后通知用户
//!
//! 核心功能：
//! - 任务持久化（SQLite）
//! - 后台异步执行
//! - 任务状态追踪
//! - 完成通知

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};

/// 任务 ID
pub type TaskId = String;

/// 任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// 等待执行
    Pending,
    /// 正在执行
    Running,
    /// 已完成
    Completed,
    /// 执行失败
    Failed,
    /// 已取消
    Cancelled,
}

/// 任务优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Urgent = 3,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// 后台任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    /// 任务 ID
    pub id: TaskId,
    /// 关联的用户 ID
    pub user_id: String,
    /// 关联的会话 ID
    pub session_id: Option<String>,
    /// 任务描述/指令
    pub instruction: String,
    /// 任务状态
    pub status: TaskStatus,
    /// 优先级
    pub priority: TaskPriority,
    /// 执行结果
    pub result: Option<String>,
    /// 错误信息
    pub error: Option<String>,
    /// 创建时间（毫秒时间戳）
    pub created_at: i64,
    /// 开始执行时间
    pub started_at: Option<i64>,
    /// 完成时间
    pub completed_at: Option<i64>,
    /// 预估完成时间（秒）
    pub estimated_duration: Option<u64>,
    /// 进度（0-100）
    pub progress: u8,
    /// 元数据
    pub metadata: Option<serde_json::Value>,
}

impl BackgroundTask {
    pub fn new(user_id: String, instruction: String) -> Self {
        Self {
            id: format!("task_{}", uuid::Uuid::new_v4()),
            user_id,
            session_id: None,
            instruction,
            status: TaskStatus::Pending,
            priority: TaskPriority::Normal,
            result: None,
            error: None,
            created_at: chrono::Utc::now().timestamp_millis(),
            started_at: None,
            completed_at: None,
            estimated_duration: None,
            progress: 0,
            metadata: None,
        }
    }

    pub fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.status, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled)
    }
}

/// 任务完成通知
#[derive(Debug, Clone)]
pub struct TaskNotification {
    pub task_id: TaskId,
    pub user_id: String,
    pub status: TaskStatus,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// 任务队列（内存版 + 可选持久化）
pub struct TaskQueue {
    /// 所有任务
    tasks: RwLock<HashMap<TaskId, BackgroundTask>>,
    /// 用户任务索引
    user_tasks: RwLock<HashMap<String, Vec<TaskId>>>,
    /// 待执行队列
    pending_tx: mpsc::UnboundedSender<TaskId>,
    /// 通知发送器
    notification_tx: mpsc::UnboundedSender<TaskNotification>,
    /// SQLite 连接池（可选）
    #[cfg(feature = "async-sqlite")]
    pool: Option<sqlx::sqlite::SqlitePool>,
}

impl TaskQueue {
    /// 创建内存版任务队列
    pub fn new() -> (Self, mpsc::UnboundedReceiver<TaskId>, mpsc::UnboundedReceiver<TaskNotification>) {
        let (pending_tx, pending_rx) = mpsc::unbounded_channel();
        let (notification_tx, notification_rx) = mpsc::unbounded_channel();
        
        (
            Self {
                tasks: RwLock::new(HashMap::new()),
                user_tasks: RwLock::new(HashMap::new()),
                pending_tx,
                notification_tx,
                #[cfg(feature = "async-sqlite")]
                pool: None,
            },
            pending_rx,
            notification_rx,
        )
    }

    /// 创建持久化版任务队列
    #[cfg(feature = "async-sqlite")]
    pub async fn with_persistence(
        db_path: impl AsRef<Path>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<TaskId>, mpsc::UnboundedReceiver<TaskNotification>), sqlx::Error> {
        let db_url = format!("sqlite:{}?mode=rwc", db_path.as_ref().display());
        
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(3)
            .connect(&db_url)
            .await?;
        
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS background_tasks (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                session_id TEXT,
                instruction TEXT NOT NULL,
                status TEXT NOT NULL,
                priority INTEGER NOT NULL,
                result TEXT,
                error TEXT,
                created_at INTEGER NOT NULL,
                started_at INTEGER,
                completed_at INTEGER,
                estimated_duration INTEGER,
                progress INTEGER NOT NULL DEFAULT 0,
                metadata TEXT
            )"
        )
        .execute(&pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_user ON background_tasks(user_id)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_status ON background_tasks(status)")
            .execute(&pool)
            .await?;

        let (pending_tx, pending_rx) = mpsc::unbounded_channel();
        let (notification_tx, notification_rx) = mpsc::unbounded_channel();

        let queue = Self {
            tasks: RwLock::new(HashMap::new()),
            user_tasks: RwLock::new(HashMap::new()),
            pending_tx,
            notification_tx,
            pool: Some(pool),
        };

        queue.restore_pending_tasks().await?;

        Ok((queue, pending_rx, notification_rx))
    }

    /// 从数据库恢复待执行任务
    #[cfg(feature = "async-sqlite")]
    async fn restore_pending_tasks(&self) -> Result<(), sqlx::Error> {
        let pool = match &self.pool {
            Some(p) => p,
            None => return Ok(()),
        };

        let rows = sqlx::query(
            "SELECT id, user_id, session_id, instruction, status, priority, result, error,
                    created_at, started_at, completed_at, estimated_duration, progress, metadata
             FROM background_tasks
             WHERE status IN ('Pending', 'Running')
             ORDER BY priority DESC, created_at ASC"
        )
        .fetch_all(pool)
        .await?;

        let mut tasks = self.tasks.write().await;
        let mut user_tasks = self.user_tasks.write().await;

        for row in rows {
            use sqlx::Row;
            
            let task = BackgroundTask {
                id: row.get("id"),
                user_id: row.get("user_id"),
                session_id: row.get("session_id"),
                instruction: row.get("instruction"),
                status: parse_status(row.get::<String, _>("status").as_str()),
                priority: parse_priority(row.get::<i32, _>("priority")),
                result: row.get("result"),
                error: row.get("error"),
                created_at: row.get("created_at"),
                started_at: row.get("started_at"),
                completed_at: row.get("completed_at"),
                estimated_duration: row.get::<Option<i64>, _>("estimated_duration").map(|v| v as u64),
                progress: row.get::<i32, _>("progress") as u8,
                metadata: row.get::<Option<String>, _>("metadata")
                    .and_then(|s| serde_json::from_str(&s).ok()),
            };

            let task_id = task.id.clone();
            let user_id = task.user_id.clone();
            
            if task.status == TaskStatus::Pending {
                let _ = self.pending_tx.send(task_id.clone());
            }

            user_tasks.entry(user_id).or_default().push(task_id.clone());
            tasks.insert(task_id, task);
        }

        let count = tasks.len();
        if count > 0 {
            tracing::info!("Restored {} background tasks from database", count);
        }

        Ok(())
    }

    /// 提交新任务
    pub async fn submit(&self, task: BackgroundTask) -> TaskId {
        let task_id = task.id.clone();
        let user_id = task.user_id.clone();

        #[cfg(feature = "async-sqlite")]
        if let Some(pool) = &self.pool {
            let _ = self.save_task_to_db(pool, &task).await;
        }

        self.tasks.write().await.insert(task_id.clone(), task);
        self.user_tasks.write().await.entry(user_id).or_default().push(task_id.clone());

        let _ = self.pending_tx.send(task_id.clone());

        task_id
    }

    /// 保存任务到数据库
    #[cfg(feature = "async-sqlite")]
    async fn save_task_to_db(&self, pool: &sqlx::sqlite::SqlitePool, task: &BackgroundTask) -> Result<(), sqlx::Error> {
        let status_str = format!("{:?}", task.status);
        let metadata_str = task.metadata.as_ref().map(|v| v.to_string());

        sqlx::query(
            "INSERT OR REPLACE INTO background_tasks 
             (id, user_id, session_id, instruction, status, priority, result, error,
              created_at, started_at, completed_at, estimated_duration, progress, metadata)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&task.id)
        .bind(&task.user_id)
        .bind(&task.session_id)
        .bind(&task.instruction)
        .bind(&status_str)
        .bind(task.priority as i32)
        .bind(&task.result)
        .bind(&task.error)
        .bind(task.created_at)
        .bind(task.started_at)
        .bind(task.completed_at)
        .bind(task.estimated_duration.map(|v| v as i64))
        .bind(task.progress as i32)
        .bind(&metadata_str)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 更新任务状态
    pub async fn update_status(&self, task_id: &str, status: TaskStatus) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = status;
            
            match status {
                TaskStatus::Running => {
                    task.started_at = Some(chrono::Utc::now().timestamp_millis());
                }
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled => {
                    task.completed_at = Some(chrono::Utc::now().timestamp_millis());
                    task.progress = 100;
                    
                    let notification = TaskNotification {
                        task_id: task.id.clone(),
                        user_id: task.user_id.clone(),
                        status,
                        result: task.result.clone(),
                        error: task.error.clone(),
                    };
                    let _ = self.notification_tx.send(notification);
                }
                _ => {}
            }

            #[cfg(feature = "async-sqlite")]
            if let Some(pool) = &self.pool {
                let task_clone = task.clone();
                let pool_clone = pool.clone();
                tokio::spawn(async move {
                    let status_str = format!("{:?}", task_clone.status);
                    let _ = sqlx::query(
                        "UPDATE background_tasks SET status = ?, started_at = ?, completed_at = ?, progress = ? WHERE id = ?"
                    )
                    .bind(&status_str)
                    .bind(task_clone.started_at)
                    .bind(task_clone.completed_at)
                    .bind(task_clone.progress as i32)
                    .bind(&task_clone.id)
                    .execute(&pool_clone)
                    .await;
                });
            }
        }
    }

    /// 设置任务结果
    pub async fn set_result(&self, task_id: &str, result: String) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.result = Some(result.clone());
            task.status = TaskStatus::Completed;
            task.completed_at = Some(chrono::Utc::now().timestamp_millis());
            task.progress = 100;

            let notification = TaskNotification {
                task_id: task.id.clone(),
                user_id: task.user_id.clone(),
                status: TaskStatus::Completed,
                result: Some(result),
                error: None,
            };
            let _ = self.notification_tx.send(notification);

            #[cfg(feature = "async-sqlite")]
            if let Some(pool) = &self.pool {
                let task_clone = task.clone();
                let pool_clone = pool.clone();
                tokio::spawn(async move {
                    let _ = sqlx::query(
                        "UPDATE background_tasks SET status = 'Completed', result = ?, completed_at = ?, progress = 100 WHERE id = ?"
                    )
                    .bind(&task_clone.result)
                    .bind(task_clone.completed_at)
                    .bind(&task_clone.id)
                    .execute(&pool_clone)
                    .await;
                });
            }
        }
    }

    /// 设置任务错误
    pub async fn set_error(&self, task_id: &str, error: String) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.error = Some(error.clone());
            task.status = TaskStatus::Failed;
            task.completed_at = Some(chrono::Utc::now().timestamp_millis());

            let notification = TaskNotification {
                task_id: task.id.clone(),
                user_id: task.user_id.clone(),
                status: TaskStatus::Failed,
                result: None,
                error: Some(error),
            };
            let _ = self.notification_tx.send(notification);

            #[cfg(feature = "async-sqlite")]
            if let Some(pool) = &self.pool {
                let task_clone = task.clone();
                let pool_clone = pool.clone();
                tokio::spawn(async move {
                    let _ = sqlx::query(
                        "UPDATE background_tasks SET status = 'Failed', error = ?, completed_at = ? WHERE id = ?"
                    )
                    .bind(&task_clone.error)
                    .bind(task_clone.completed_at)
                    .bind(&task_clone.id)
                    .execute(&pool_clone)
                    .await;
                });
            }
        }
    }

    /// 更新进度
    pub async fn update_progress(&self, task_id: &str, progress: u8) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.progress = progress.min(100);
        }
    }

    /// 获取任务
    pub async fn get(&self, task_id: &str) -> Option<BackgroundTask> {
        self.tasks.read().await.get(task_id).cloned()
    }

    /// 获取用户的所有任务
    pub async fn get_user_tasks(&self, user_id: &str) -> Vec<BackgroundTask> {
        let tasks = self.tasks.read().await;
        let user_tasks = self.user_tasks.read().await;

        user_tasks
            .get(user_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| tasks.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 获取用户待处理的任务
    pub async fn get_pending_tasks(&self, user_id: &str) -> Vec<BackgroundTask> {
        self.get_user_tasks(user_id)
            .await
            .into_iter()
            .filter(|t| !t.is_finished())
            .collect()
    }

    /// 取消任务
    pub async fn cancel(&self, task_id: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if !task.is_finished() {
                task.status = TaskStatus::Cancelled;
                task.completed_at = Some(chrono::Utc::now().timestamp_millis());
                return true;
            }
        }
        false
    }

    /// 清理已完成的旧任务
    pub async fn cleanup_old_tasks(&self, max_age_hours: u64) -> usize {
        let cutoff = chrono::Utc::now().timestamp_millis() - (max_age_hours as i64 * 3600 * 1000);
        
        let mut tasks = self.tasks.write().await;
        let mut user_tasks = self.user_tasks.write().await;

        let old_ids: Vec<_> = tasks
            .iter()
            .filter(|(_, t)| t.is_finished() && t.completed_at.map(|c| c < cutoff).unwrap_or(false))
            .map(|(id, _)| id.clone())
            .collect();

        for id in &old_ids {
            if let Some(task) = tasks.remove(id) {
                if let Some(user_ids) = user_tasks.get_mut(&task.user_id) {
                    user_ids.retain(|tid| tid != id);
                }
            }
        }

        #[cfg(feature = "async-sqlite")]
        if let Some(pool) = &self.pool {
            let pool_clone = pool.clone();
            tokio::spawn(async move {
                let _ = sqlx::query(
                    "DELETE FROM background_tasks WHERE status IN ('Completed', 'Failed', 'Cancelled') AND completed_at < ?"
                )
                .bind(cutoff)
                .execute(&pool_clone)
                .await;
            });
        }

        old_ids.len()
    }
}

fn parse_status(s: &str) -> TaskStatus {
    match s {
        "Pending" => TaskStatus::Pending,
        "Running" => TaskStatus::Running,
        "Completed" => TaskStatus::Completed,
        "Failed" => TaskStatus::Failed,
        "Cancelled" => TaskStatus::Cancelled,
        _ => TaskStatus::Pending,
    }
}

fn parse_priority(p: i32) -> TaskPriority {
    match p {
        0 => TaskPriority::Low,
        1 => TaskPriority::Normal,
        2 => TaskPriority::High,
        3 => TaskPriority::Urgent,
        _ => TaskPriority::Normal,
    }
}

/// 后台任务执行器
pub struct TaskExecutor {
    queue: Arc<TaskQueue>,
    max_concurrent: usize,
}

impl TaskExecutor {
    pub fn new(queue: Arc<TaskQueue>, max_concurrent: usize) -> Self {
        Self { queue, max_concurrent }
    }

    /// 启动执行器
    pub async fn start(
        self,
        mut pending_rx: mpsc::UnboundedReceiver<TaskId>,
        process_fn: impl Fn(BackgroundTask) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>> + Send + Sync + 'static,
    ) {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.max_concurrent));
        let process_fn = Arc::new(process_fn);

        while let Some(task_id) = pending_rx.recv().await {
            let permit = semaphore.clone().acquire_owned().await;
            if permit.is_err() {
                continue;
            }
            let permit = permit.unwrap();

            let queue = Arc::clone(&self.queue);
            let process_fn = Arc::clone(&process_fn);

            tokio::spawn(async move {
                let _permit = permit;

                let task = match queue.get(&task_id).await {
                    Some(t) if t.status == TaskStatus::Pending => t,
                    _ => return,
                };

                queue.update_status(&task_id, TaskStatus::Running).await;

                match process_fn(task).await {
                    Ok(result) => {
                        queue.set_result(&task_id, result).await;
                    }
                    Err(error) => {
                        queue.set_error(&task_id, error).await;
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_task_queue_basic() {
        let (queue, mut pending_rx, mut notification_rx) = TaskQueue::new();
        let queue = Arc::new(queue);

        let task = BackgroundTask::new("user_123".to_string(), "Write a report".to_string());
        let task_id = queue.submit(task).await;

        assert!(pending_rx.try_recv().is_ok());

        let task = queue.get(&task_id).await.unwrap();
        assert_eq!(task.status, TaskStatus::Pending);

        queue.set_result(&task_id, "Report completed".to_string()).await;

        let task = queue.get(&task_id).await.unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
        assert_eq!(task.result, Some("Report completed".to_string()));

        let notification = notification_rx.try_recv().unwrap();
        assert_eq!(notification.task_id, task_id);
        assert_eq!(notification.status, TaskStatus::Completed);
    }
}
