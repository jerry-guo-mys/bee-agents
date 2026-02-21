//! 任务调度：Foreground / Background / Tool Pool
//!
//! 按任务类型（AgentStep / ToolExecution / Background）分类；工具执行使用 Semaphore 限制并发。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// 任务类型
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TaskKind {
    /// 前台，串行
    AgentStep,
    /// 可并行，受限
    ToolExecution,
    /// 后台，不阻塞 UI
    Background,
}

/// 任务 ID
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TaskId(u64);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(0);

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskId {
    pub fn new() -> Self {
        Self(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// 任务调度器
pub struct TaskScheduler {
    /// 工具并发限制（默认 3）
    tool_semaphore: Arc<Semaphore>,
    /// 活跃任务
    _active_tasks: HashMap<TaskId, TaskKind>,
}

impl TaskScheduler {
    pub fn new(max_concurrent_tools: usize) -> Self {
        Self {
            tool_semaphore: Arc::new(Semaphore::new(max_concurrent_tools.max(1))),
            _active_tasks: HashMap::new(),
        }
    }

    /// 获取工具执行许可
    pub async fn acquire_tool(&self) -> tokio::sync::OwnedSemaphorePermit {
        self.tool_semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed")
    }

    /// 检查是否应取消
    pub fn is_cancelled(token: &CancellationToken) -> bool {
        token.is_cancelled()
    }
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new(3)
    }
}
