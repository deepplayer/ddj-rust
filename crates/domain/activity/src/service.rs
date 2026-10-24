//! 活动领域服务

use crate::event::ActivityEvent;
use crate::model::{Activity, Progress};
use crate::rule::ActivityError;

/// 事件发布。
pub trait EventSink: Send + Sync {
    /// 发布。
    fn publish(&self, ev: ActivityEvent);
}

/// 领取某档位奖励。
///
/// # Errors
/// 活动未开 / 条件不足 / 重复领取。
pub fn claim(
    act: &Activity,
    prog: &mut Progress,
    tier: i32,
    need: i64,
    now_ms: i64,
    sink: &dyn EventSink,
) -> Result<(), ActivityError> {
    match act.state_at(now_ms) {
        crate::model::ActivityState::Running => {}
        crate::model::ActivityState::Pending => return Err(ActivityError::NotOpen),
        crate::model::ActivityState::Settling | crate::model::ActivityState::Closed => {
            return Err(ActivityError::Closed);
        }
    }
    if prog.value < need {
        return Err(ActivityError::NotReached {
            need,
            have: prog.value,
        });
    }
    if prog.claimed.contains(&tier) {
        return Err(ActivityError::AlreadyClaimed);
    }
    prog.claimed.push(tier);
    sink.publish(ActivityEvent::RewardClaimed {
        player: prog.player,
        activity: prog.activity,
        tier,
    });
    Ok(())
}

/// 静默 sink。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, ev: ActivityEvent) {
        tracing::debug!(?ev, "事件（无订阅者）");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Activity, ActivityKind, Progress};
    use ddj_core::{PlayerId, TemplateId};

    fn act() -> Activity {
        Activity {
            id: TemplateId::new(1),
            kind: ActivityKind::SignIn,
            start_ms: 0,
            end_ms: 1000,
        }
    }

    fn prog(v: i64) -> Progress {
        Progress {
            player: PlayerId::new(1),
            activity: TemplateId::new(1),
            value: v,
            claimed: vec![],
        }
    }

    #[test]
    fn claim_requires_threshold() {
        let mut p = prog(3);
        assert_eq!(
            claim(&act(), &mut p, 1, 5, 500, &NoopSink),
            Err(ActivityError::NotReached { need: 5, have: 3 })
        );
    }

    #[test]
    fn claim_twice_fails() {
        let mut p = prog(10);
        claim(&act(), &mut p, 1, 5, 500, &NoopSink).expect("首次");
        assert_eq!(
            claim(&act(), &mut p, 1, 5, 500, &NoopSink),
            Err(ActivityError::AlreadyClaimed)
        );
    }

    #[test]
    fn closed_activity_rejects() {
        let mut p = prog(10);
        assert_eq!(
            claim(&act(), &mut p, 1, 5, 9999, &NoopSink),
            Err(ActivityError::Closed)
        );
        // 未到开始时间
        assert_eq!(
            claim(&act(), &mut p, 1, 5, 0 - 1, &NoopSink),
            Err(ActivityError::NotOpen)
        );
    }
}
