//! 模块间共享上下文
//!
//! 原系统到处 `static` 单例 + `getInstance()`。这里改为显式传递的
//! `ServiceContext`，测试时可构造替身。

use ddj_core::Clock;
use std::sync::Arc;

/// 服务上下文。
#[derive(Clone)]
pub struct ServiceContext {
    /// 服务名。
    pub service: &'static str,
    /// 区服 ID。
    pub server_id: i32,
    /// 时钟。
    pub clock: Arc<dyn Clock>,
    /// 停机信号。
    pub shutdown: tokio::sync::watch::Receiver<bool>,
}

impl std::fmt::Debug for ServiceContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceContext")
            .field("service", &self.service)
            .field("server_id", &self.server_id)
            .finish_non_exhaustive()
    }
}
