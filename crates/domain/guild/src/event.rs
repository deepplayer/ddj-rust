//! 公会事件

use ddj_core::{GuildId, PlayerId};

/// 公会事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuildEvent {
    /// 创建。
    Created {
        /// 公会。
        guild: GuildId,
        /// 会长。
        master: PlayerId,
    },
    /// 加入。
    Joined {
        /// 公会。
        guild: GuildId,
        /// 玩家。
        player: PlayerId,
    },
    /// 退出。
    Left {
        /// 公会。
        guild: GuildId,
        /// 玩家。
        player: PlayerId,
    },
    /// 踢出。
    Kicked {
        /// 公会。
        guild: GuildId,
        /// 玩家。
        player: PlayerId,
        /// 操作者。
        by: PlayerId,
    },
    /// 解散。
    Dissolved {
        /// 公会。
        guild: GuildId,
    },
}
