//! 邮件事件

use ddj_core::PlayerId;

/// 邮件事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailEvent {
    /// 收到新邮件。
    Received {
        /// 玩家。
        player: PlayerId,
        /// 邮件 ID。
        mail: i64,
    },
    /// 领取附件。
    Claimed {
        /// 玩家。
        player: PlayerId,
        /// 邮件 ID。
        mail: i64,
    },
    /// 过期清理。
    Cleaned {
        /// 玩家。
        player: PlayerId,
        /// 清理数量。
        count: usize,
    },
}
