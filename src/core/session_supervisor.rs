//! 会话监管：生命周期、中断管理
//!
//! 持有 CancellationToken，用户 Ctrl+C 时取消当前 ReAct 步；支持暂停与子 token（单任务取消）。

use std::sync::Arc;

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// 会话级生命周期管理：取消令牌与暂停状态
#[derive(Debug)]
pub struct SessionSupervisor {
    /// 用户 Cancel 时触发
    cancel_token: CancellationToken,
    /// 是否已暂停
    paused: Arc<RwLock<bool>>,
}

impl SessionSupervisor {
    pub fn new() -> Self {
        Self {
            cancel_token: CancellationToken::new(),
            paused: Arc::new(RwLock::new(false)),
        }
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    /// 触发取消（用户 Ctrl+C）
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    pub async fn is_paused(&self) -> bool {
        *self.paused.read().await
    }

    pub async fn set_paused(&self, paused: bool) {
        *self.paused.write().await = paused;
    }

    /// 创建子 token（用于单个任务）
    pub fn child_token(&self) -> CancellationToken {
        self.cancel_token.child_token()
    }
}

impl Default for SessionSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
