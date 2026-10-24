//! 战斗事件

use crate::model::Outcome;
use ddj_core::PlayerId;

/// 战斗事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BattleEvent {
    /// 创建。
    Created {
        /// 战斗。
        battle: u64,
        /// 攻击方。
        attacker: PlayerId,
        /// 关卡。
        stage: i32,
    },
    /// 结算完成。
    Settled {
        /// 战斗。
        battle: u64,
        /// 结果。
        outcome: Outcome,
        /// 经验。
        exp: i64,
    },
    /// 战报校验失败（可疑行为）。
    VerifyFailed {
        /// 战斗。
        battle: u64,
        /// 玩家。
        player: PlayerId,
        /// 原因。
        reason: String,
    },
}
