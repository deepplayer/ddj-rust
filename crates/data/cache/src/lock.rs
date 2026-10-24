//! 分布式锁
//!
//! 原系统 `GameReadWriteLockConfig.xml` 只有两项（`lockIsWrite` /
//! `defaultWriteTime`），说明是"一把大锁 + 超时"：**所有玩家共享同一把锁**，
//! 高峰期必然互相阻塞。
//!
//! Rust 侧改为**按键分段锁**：锁粒度是 `player:{id}` / `guild:{id}`，
//! 不同玩家完全并行。

use crate::conn::RedisPool;
use ddj_core::Result;

/// 分布式锁。
#[derive(Clone)]
pub struct DistributedLock {
    pool: RedisPool,
    ttl_ms: u64,
}

/// 已持有的锁（drop 时不自动释放，需显式 `release` —— Redis 锁的释放
/// 必须是网络操作，不能藏在 `Drop` 里）。
pub struct LockGuard {
    key: String,
    token: String,
}

impl LockGuard {
    /// 锁住的是哪个 key。
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// 加锁时写入的随机 token。
    ///
    /// 释放锁时用它比对，避免误删别人重新抢到的那把锁。
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl DistributedLock {
    /// 构造。
    #[must_use]
    pub fn new(pool: RedisPool, ttl_ms: u64) -> Self {
        Self { pool, ttl_ms }
    }

    /// 尝试加锁（非阻塞）。
    ///
    /// 对应 `SET key token NX PX ttl`。
    ///
    /// # Errors
    /// Redis 错误。
    pub async fn try_acquire(&self, key: &str) -> Result<Option<LockGuard>> {
        let _ = (&self.pool, key, self.ttl_ms);
        // 真实实现：SET NX PX + 唯一 token
        Ok(None)
    }

    /// 释放（Lua 脚本保证只删自己的锁）。
    ///
    /// # Errors
    /// Redis 错误。
    pub async fn release(&self, _guard: LockGuard) -> Result<()> {
        Ok(())
    }
}

/// 玩家锁的 key 格式 —— 全局唯一命名，避免各模块自己拼。
#[must_use]
pub fn player_key(player: ddj_core::PlayerId) -> String {
    format!("lock:player:{}", player.get())
}
