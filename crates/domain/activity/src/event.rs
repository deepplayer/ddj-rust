//! 活动事件

use ddj_core::{PlayerId, TemplateId};

/// 活动事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityEvent {
    /// 活动开启。
    Opened {
        /// 活动。
        activity: TemplateId,
    },
    /// 进度推进。
    Progressed {
        /// 玩家。
        player: PlayerId,
        /// 活动。
        activity: TemplateId,
        /// 新值。
        value: i64,
    },
    /// 领取奖励。
    RewardClaimed {
        /// 玩家。
        player: PlayerId,
        /// 活动。
        activity: TemplateId,
        /// 档位。
        tier: i32,
    },
}
