//! 经济领域服务

use crate::event::EconomyEvent;
use crate::model::{Order, OrderState};
use crate::rule::EconomyError;

/// 事件发布。
pub trait EventSink: Send + Sync {
    /// 发布。
    fn publish(&self, ev: EconomyEvent);
}

/// 发货：`Paid` → `Delivered`，幂等。
///
/// # Errors
/// 状态非法、重复发货。
pub fn deliver(order: &mut Order, sink: &dyn EventSink) -> Result<(), EconomyError> {
    match order.state {
        OrderState::Delivered => return Err(EconomyError::DuplicateDeliver),
        state if !order.can_transit(OrderState::Delivered) => {
            tracing::warn!(?state, order = %order.id, "非法发货请求");
            return Err(EconomyError::IllegalTransition);
        }
        _ => {}
    }
    order.state = OrderState::Delivered;
    sink.publish(EconomyEvent::Delivered {
        order: order.id.clone(),
        player: order.player,
    });
    Ok(())
}

/// 静默 sink。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, ev: EconomyEvent) {
        tracing::debug!(?ev, "事件（无订阅者）");
    }
}
