//! 战斗领域服务

use crate::event::BattleEvent;
use crate::model::{BattleReport, MAX_STEPS};
use crate::rule::BattleError;

/// 事件发布。
pub trait EventSink: Send + Sync {
    /// 发布。
    fn publish(&self, ev: BattleEvent);
}

/// 校验客户端上报的战报。
///
/// 检查项（原系统只查了第一条，其余全信客户端）：
/// 1. 步数不超上限
/// 2. 伤害非负
/// 3. 胜利时伤害必须 > 0（不能零伤害通关）
///
/// # Errors
/// 见 [`BattleError`]。
pub fn verify(report: &BattleReport, sink: &dyn EventSink) -> Result<(), BattleError> {
    if report.suspicious(MAX_STEPS) {
        let reason = format!("步数 {} 超过上限 {}", report.step_count(), MAX_STEPS);
        sink.publish(BattleEvent::VerifyFailed {
            battle: report.battle_id,
            player: report.attacker,
            reason: reason.clone(),
        });
        return Err(BattleError::TooManySteps(report.step_count()));
    }
    if report.claimed_damage < 0
        || (report.outcome == crate::model::Outcome::Win && report.claimed_damage == 0)
    {
        sink.publish(BattleEvent::VerifyFailed {
            battle: report.battle_id,
            player: report.attacker,
            reason: "伤害值异常".into(),
        });
        return Err(BattleError::VerifyFailed);
    }
    Ok(())
}

/// 静默 sink。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, ev: BattleEvent) {
        tracing::debug!(?ev, "事件（无订阅者）");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BattleReport, Outcome};
    use ddj_core::PlayerId;

    fn r(steps: usize, dmg: i64, out: Outcome) -> BattleReport {
        BattleReport {
            battle_id: 1,
            attacker: PlayerId::new(1),
            defender: PlayerId::new(2),
            stage: 1,
            outcome: out,
            steps: vec![(1, 0); steps],
            claimed_damage: dmg,
            ended_ms: 0,
        }
    }

    #[test]
    fn rejects_too_many_steps() {
        assert!(matches!(
            verify(&r(MAX_STEPS + 1, 100, Outcome::Win), &NoopSink),
            Err(BattleError::TooManySteps(_))
        ));
    }

    #[test]
    fn rejects_negative_damage() {
        assert_eq!(
            verify(&r(1, -5, Outcome::Lose), &NoopSink),
            Err(BattleError::VerifyFailed)
        );
    }

    #[test]
    fn rejects_zero_damage_win() {
        assert_eq!(
            verify(&r(1, 0, Outcome::Win), &NoopSink),
            Err(BattleError::VerifyFailed)
        );
    }

    #[test]
    fn accepts_normal() {
        assert!(verify(&r(10, 500, Outcome::Win), &NoopSink).is_ok());
    }
}
