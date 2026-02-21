//! 会话监管：生命周期、中断管理
//!
//! 持有 CancellationToken，用户 Ctrl+C 时取消当前 ReAct 步；支持暂停与子 token（单任务取消）。

use std::sync::{Arc, RwLock};

use tokio_util::sync::CancellationToken;

/// 会话级生命周期管理：取消令牌与暂停状态
///
/// 解决问题 1.4：每次 Submit 重建 CancellationToken
#[derive(Debug)]
pub struct SessionSupervisor {
    /// 用户 Cancel 时触发，使用 RwLock 支持重建（解决问题 1.4）
    cancel_token: Arc<RwLock<CancellationToken>>,
    /// 是否已暂停
    paused: Arc<RwLock<bool>>,
}

impl SessionSupervisor {
    pub fn new() -> Self {
        Self {
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            paused: Arc::new(RwLock::new(false)),
        }
    }

    /// 获取当前 cancel token 的克隆
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.read().unwrap().clone()
    }

    /// 触发取消（用户 Ctrl+C）
    pub fn cancel(&self) {
        self.cancel_token.read().unwrap().cancel();
    }

    /// 重建 cancel token（每次 Submit 前调用，解决问题 1.4）
    ///
    /// 当前 token 取消后，新请求需要新的 token
    pub fn reset_cancel_token(&self) -> CancellationToken {
        let mut guard = self.cancel_token.write().unwrap();
        *guard = CancellationToken::new();
        guard.clone()
    }

    /// 检查是否已取消
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.read().unwrap().is_cancelled()
    }

    pub fn is_paused(&self) -> bool {
        *self.paused.read().unwrap()
    }

    pub fn set_paused(&self, paused: bool) {
        *self.paused.write().unwrap() = paused;
    }

    /// 创建子 token（用于单个任务）
    pub fn child_token(&self) -> CancellationToken {
        self.cancel_token.read().unwrap().child_token()
    }
}

impl Default for SessionSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
