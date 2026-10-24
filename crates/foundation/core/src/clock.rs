//! 时钟抽象
//!
//! 原系统到处直接 `System.currentTimeMillis()`，导致：
//! 活动开启时间无法在测试里快进、CD 逻辑只能靠 sleep 测。
//! 这里注入 `Clock`，测试用 `TestClock` 手动拨表。

use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// 时间源。
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// 当前 UTC 时间。
    fn now(&self) -> DateTime<Utc>;
    /// 当前毫秒时间戳。
    fn now_millis(&self) -> i64 {
        self.now().timestamp_millis()
    }
}

/// 系统时钟。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// 测试时钟：可手动前进。
#[derive(Debug, Clone)]
pub struct TestClock {
    millis: Arc<AtomicI64>,
}

impl TestClock {
    /// 从指定毫秒时间戳开始。
    #[must_use]
    pub fn at(millis: i64) -> Self {
        Self {
            millis: Arc::new(AtomicI64::new(millis)),
        }
    }
    /// 前进 `ms` 毫秒。
    pub fn advance(&self, ms: i64) {
        self.millis.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.millis.load(Ordering::SeqCst)).unwrap_or_else(Utc::now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_advances() {
        let c = TestClock::at(1_700_000_000_000);
        let t0 = c.now_millis();
        c.advance(3_600_000);
        assert_eq!(c.now_millis() - t0, 3_600_000);
    }
}
