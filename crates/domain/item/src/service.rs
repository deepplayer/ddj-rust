//! 领域服务
//!
//! 编排「校验 → 扣除 → 记录 → 发布事件」。
//! 不含 IO：仓储与事件总线都以 trait 注入，便于单测。

use crate::event::ItemEvent;
use crate::model::Bag;
use crate::rule::{check_use, ItemError, ItemTemplate, UseContext};
use ddj_core::TemplateId;

/// 事件发布。
pub trait EventSink: Send + Sync {
    /// 发布一个事件。
    fn publish(&self, ev: ItemEvent);
}

/// 空实现（测试 / 无副作用的场景）。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, ev: ItemEvent) {
        tracing::debug!(event = ev.name(), "事件（无订阅者）");
    }
}

/// 使用道具的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseOutcome {
    /// 扣除的道具数量。
    pub removed: i64,
    /// 使用后剩余。
    pub left: i64,
}

/// 使用道具。
///
/// # Errors
/// 见 [`ItemError`]。
pub fn use_item(
    bag: &mut Bag,
    tpl: &ItemTemplate,
    ctx: UseContext,
    sink: &dyn EventSink,
) -> Result<UseOutcome, ItemError> {
    let removed = check_use(bag, tpl, ctx)?;
    bag.take(tpl.id, removed)?;
    let left = bag.count(tpl.id);

    sink.publish(ItemEvent::Used {
        player: bag.owner,
        template: tpl.id,
        num: ctx.req_num,
        removed,
    });
    sink.publish(ItemEvent::Consumed {
        player: bag.owner,
        template: tpl.id,
        num: removed,
        left,
    });

    Ok(UseOutcome { removed, left })
}

/// 发放道具（含事件）。
pub fn grant_item(bag: &mut Bag, tpl: TemplateId, n: i64, sink: &dyn EventSink) {
    bag.grant(tpl, n);
    sink.publish(ItemEvent::Gained {
        player: bag.owner,
        template: tpl,
        num: n,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::ItemTemplate;
    use ddj_core::{PlayerId, TemplateId};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    struct Counter(Arc<AtomicUsize>);

    impl EventSink for Counter {
        fn publish(&self, _ev: ItemEvent) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn use_deducts_and_emits_events() {
        let mut b = Bag::new(PlayerId::new(1));
        b.grant(TemplateId::new(1101), 5);
        let tpl = ItemTemplate::effective(TemplateId::new(1101));
        let c = Counter::default();
        let out = use_item(
            &mut b,
            &tpl,
            UseContext {
                req_num: 2,
                used_today: 0,
            },
            &c,
        )
        .expect("可用");
        assert_eq!(
            out,
            UseOutcome {
                removed: 2,
                left: 3
            }
        );
        assert_eq!(b.count(TemplateId::new(1101)), 3);
        // Used + Consumed
        assert_eq!(c.0.load(Ordering::Relaxed), 2);
    }
}
