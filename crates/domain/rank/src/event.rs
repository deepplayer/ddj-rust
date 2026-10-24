//! 排行事件

use crate::model::RankKind;
use ddj_core::PlayerId;

/// 排行事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RankEvent {
    /// 分数更新。
    ScoreChanged {
        /// 榜。
        kind: RankKind,
        /// 玩家。
        player: PlayerId,
        /// 新分。
        score: i64,
    },
    /// 榜单结算（发奖）。
    Settled {
        /// 榜。
        kind: RankKind,
        /// 参与人数。
        count: usize,
    },
}
