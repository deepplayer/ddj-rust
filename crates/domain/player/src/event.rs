//! 角色领域事件

use crate::model::Currency;
use ddj_core::PlayerId;

/// 角色事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerEvent {
    /// 上线。
    Login {
        /// 玩家。
        player: PlayerId,
    },
    /// 下线。
    Logout {
        /// 玩家。
        player: PlayerId,
    },
    /// 升级。
    LevelUp {
        /// 玩家。
        player: PlayerId,
        /// 新等级。
        level: u32,
    },
    /// 货币变动。
    CurrencyChanged {
        /// 玩家。
        player: PlayerId,
        /// 货币。
        currency: Currency,
        /// 变化量（正为增）。
        delta: i64,
        /// 变化后余额。
        balance: i64,
    },
}
