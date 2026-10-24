//! 角色领域服务

use crate::event::PlayerEvent;
use crate::model::{Currency, Player};
use crate::rule::PlayerError;

/// 事件发布。
pub trait EventSink: Send + Sync {
    /// 发布。
    fn publish(&self, ev: PlayerEvent);
}

/// 扣费并发布事件。
///
/// # Errors
/// 余额不足。
pub fn spend(
    p: &mut Player,
    c: Currency,
    n: i64,
    sink: &dyn EventSink,
) -> Result<i64, PlayerError> {
    p.debit(c, n)?;
    let bal = p.balance(c);
    sink.publish(PlayerEvent::CurrencyChanged {
        player: p.identity.id,
        currency: c,
        delta: -n,
        balance: bal,
    });
    Ok(bal)
}

/// 发放并发布事件。
pub fn reward(p: &mut Player, c: Currency, n: i64, sink: &dyn EventSink) -> i64 {
    p.credit(c, n);
    let bal = p.balance(c);
    sink.publish(PlayerEvent::CurrencyChanged {
        player: p.identity.id,
        currency: c,
        delta: n,
        balance: bal,
    });
    bal
}

/// 上线。
pub fn login(p: &mut Player, now_ms: i64, sink: &dyn EventSink) {
    p.last_login_ms = now_ms;
    sink.publish(PlayerEvent::Login {
        player: p.identity.id,
    });
}

/// 供测试用的静默 sink。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, ev: PlayerEvent) {
        tracing::debug!(?ev, "事件（无订阅者）");
    }
}
