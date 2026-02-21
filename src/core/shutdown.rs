//! 优雅关闭处理（解决问题 7.2）
//!
//! 提供统一的关闭信号监听和清理逻辑，确保：
//! - 向量快照在退出时保存
//! - SQLite 连接正确关闭
//! - 日志 flush
//! - 正在进行的任务有机会完成或取消

use std::future::Future;
use std::sync::Arc;

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// 关闭信号管理器
#[derive(Clone)]
pub struct ShutdownManager {
    /// 关闭信号 token
    shutdown_token: CancellationToken,
    /// 关闭原因广播
    reason_tx: broadcast::Sender<ShutdownReason>,
}

/// 关闭原因
#[derive(Debug, Clone)]
pub enum ShutdownReason {
    /// 用户发起的退出 (Ctrl+C 或 quit 命令)
    UserInitiated,
    /// SIGTERM 信号
    Signal,
    /// 致命错误
    FatalError(String),
}

impl ShutdownManager {
    /// 创建新的关闭管理器
    pub fn new() -> Self {
        let (reason_tx, _) = broadcast::channel(1);
        Self {
            shutdown_token: CancellationToken::new(),
            reason_tx,
        }
    }

    /// 获取关闭 token（用于取消正在进行的任务）
    pub fn token(&self) -> CancellationToken {
        self.shutdown_token.clone()
    }

    /// 触发关闭
    pub fn shutdown(&self, reason: ShutdownReason) {
        let _ = self.reason_tx.send(reason);
        self.shutdown_token.cancel();
    }

    /// 是否已触发关闭
    pub fn is_shutdown(&self) -> bool {
        self.shutdown_token.is_cancelled()
    }

    /// 订阅关闭原因
    pub fn subscribe(&self) -> broadcast::Receiver<ShutdownReason> {
        self.reason_tx.subscribe()
    }

    /// 等待关闭信号
    pub async fn wait_for_shutdown(&self) {
        self.shutdown_token.cancelled().await;
    }

    /// 安装系统信号处理器 (Ctrl+C, SIGTERM)
    pub fn install_signal_handlers(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            if let Ok(()) = tokio::signal::ctrl_c().await {
                tracing::info!("Received Ctrl+C, initiating graceful shutdown...");
                manager.shutdown(ShutdownReason::UserInitiated);
            }
        });

        #[cfg(unix)]
        {
            let manager = Arc::clone(self);
            tokio::spawn(async move {
                use tokio::signal::unix::{signal, SignalKind};
                if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
                    sigterm.recv().await;
                    tracing::info!("Received SIGTERM, initiating graceful shutdown...");
                    manager.shutdown(ShutdownReason::Signal);
                }
            });
        }
    }
}

impl Default for ShutdownManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 关闭时需要执行的清理任务
#[async_trait::async_trait]
pub trait ShutdownCleanup: Send + Sync {
    /// 执行清理，返回清理是否成功
    async fn cleanup(&self) -> anyhow::Result<()>;

    /// 清理任务名称（用于日志）
    fn name(&self) -> &'static str;
}

/// 关闭协调器：管理多个清理任务
pub struct ShutdownCoordinator {
    manager: Arc<ShutdownManager>,
    cleanup_tasks: Vec<Arc<dyn ShutdownCleanup>>,
    /// 等待清理完成的超时时间（秒）
    timeout_secs: u64,
}

impl ShutdownCoordinator {
    /// 创建新的关闭协调器
    pub fn new(manager: Arc<ShutdownManager>) -> Self {
        Self {
            manager,
            cleanup_tasks: Vec::new(),
            timeout_secs: 5,
        }
    }

    /// 设置清理超时时间
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// 注册清理任务
    pub fn register<T: ShutdownCleanup + 'static>(&mut self, task: T) {
        self.cleanup_tasks.push(Arc::new(task));
    }

    /// 执行所有清理任务
    pub async fn run_cleanup(&self) {
        tracing::info!("Running {} cleanup tasks...", self.cleanup_tasks.len());

        let timeout = tokio::time::Duration::from_secs(self.timeout_secs);

        for task in &self.cleanup_tasks {
            let name = task.name();
            match tokio::time::timeout(timeout, task.cleanup()).await {
                Ok(Ok(())) => {
                    tracing::info!("Cleanup task '{}' completed successfully", name);
                }
                Ok(Err(e)) => {
                    tracing::warn!("Cleanup task '{}' failed: {}", name, e);
                }
                Err(_) => {
                    tracing::warn!("Cleanup task '{}' timed out after {}s", name, self.timeout_secs);
                }
            }
        }

        tracing::info!("All cleanup tasks finished");
    }

    /// 获取关闭管理器
    pub fn manager(&self) -> &Arc<ShutdownManager> {
        &self.manager
    }
}

/// 向量存储清理任务
pub struct VectorStoreCleanup {
    store: Arc<dyn crate::memory::LongTermMemory>,
}

impl VectorStoreCleanup {
    pub fn new(store: Arc<dyn crate::memory::LongTermMemory>) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl ShutdownCleanup for VectorStoreCleanup {
    async fn cleanup(&self) -> anyhow::Result<()> {
        self.store.flush();
        Ok(())
    }

    fn name(&self) -> &'static str {
        "VectorStore"
    }
}

/// SQLite 清理任务
pub struct SqliteCleanup<F>
where
    F: Fn() + Send + Sync,
{
    cleanup_fn: F,
}

impl<F> SqliteCleanup<F>
where
    F: Fn() + Send + Sync,
{
    pub fn new(cleanup_fn: F) -> Self {
        Self { cleanup_fn }
    }
}

#[async_trait::async_trait]
impl<F> ShutdownCleanup for SqliteCleanup<F>
where
    F: Fn() + Send + Sync,
{
    async fn cleanup(&self) -> anyhow::Result<()> {
        (self.cleanup_fn)();
        Ok(())
    }

    fn name(&self) -> &'static str {
        "SQLite"
    }
}

/// 运行主应用直到收到关闭信号，然后执行清理
pub async fn run_with_graceful_shutdown<F, Fut>(
    shutdown_manager: Arc<ShutdownManager>,
    app: F,
    cleanup: impl FnOnce() -> Fut,
) where
    F: Future<Output = ()>,
    Fut: Future<Output = ()>,
{
    shutdown_manager.install_signal_handlers();

    tokio::select! {
        _ = app => {
            tracing::info!("Application finished normally");
        }
        _ = shutdown_manager.wait_for_shutdown() => {
            tracing::info!("Shutdown signal received");
        }
    }

    cleanup().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_manager_new() {
        let manager = ShutdownManager::new();
        assert!(!manager.is_shutdown());
    }

    #[test]
    fn test_shutdown_manager_shutdown() {
        let manager = ShutdownManager::new();
        manager.shutdown(ShutdownReason::UserInitiated);
        assert!(manager.is_shutdown());
    }

    #[test]
    fn test_shutdown_manager_token() {
        let manager = ShutdownManager::new();
        let token = manager.token();
        assert!(!token.is_cancelled());
        manager.shutdown(ShutdownReason::UserInitiated);
        assert!(token.is_cancelled());
    }

    struct MockCleanup {
        called: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait::async_trait]
    impl ShutdownCleanup for MockCleanup {
        async fn cleanup(&self) -> anyhow::Result<()> {
            self.called.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        fn name(&self) -> &'static str {
            "MockCleanup"
        }
    }

    #[test]
    fn test_shutdown_coordinator() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let manager = Arc::new(ShutdownManager::new());
            let mut coordinator = ShutdownCoordinator::new(manager);

            let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            coordinator.register(MockCleanup { called: called.clone() });

            coordinator.run_cleanup().await;
            assert!(called.load(std::sync::atomic::Ordering::SeqCst));
        });
    }
}
