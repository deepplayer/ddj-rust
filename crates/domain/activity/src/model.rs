//! 活动聚合
//!
//! 对应 Java：`bf.game.server.domain.activity.*`（签到 / 七日 / 限时 / 兑换）。

use ddj_core::{PlayerId, TemplateId};

/// 活动类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    /// 签到。
    SignIn = 1,
    /// 七日目标。
    SevenDay = 2,
    /// 限时冲榜。
    Timed = 3,
    /// 兑换商店。
    Exchange = 4,
}

/// 活动状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActivityState {
    /// 未开始。
    Pending = 0,
    /// 进行中。
    Running = 1,
    /// 已结束待结算。
    Settling = 2,
    /// 已关闭。
    Closed = 3,
}

/// 一个活动的运行期配置。
#[derive(Debug, Clone)]
pub struct Activity {
    /// 模板 ID。
    pub id: TemplateId,
    /// 类型。
    pub kind: ActivityKind,
    /// 开始时间。
    pub start_ms: i64,
    /// 结束时间。
    pub end_ms: i64,
}

impl Activity {
    /// 在给定时刻的状态。
    #[must_use]
    pub const fn state_at(&self, now_ms: i64) -> ActivityState {
        if now_ms < self.start_ms {
            ActivityState::Pending
        } else if now_ms < self.end_ms {
            ActivityState::Running
        } else {
            ActivityState::Settling
        }
    }

    /// 是否可参与。
    #[must_use]
    pub const fn open(&self, now_ms: i64) -> bool {
        matches!(self.state_at(now_ms), ActivityState::Running)
    }
}

/// 单个玩家的活动进度。
#[derive(Debug, Clone)]
pub struct Progress {
    /// 玩家。
    pub player: PlayerId,
    /// 活动。
    pub activity: TemplateId,
    /// 已达成值。
    pub value: i64,
    /// 已领取档位。
    pub claimed: Vec<i32>,
}
