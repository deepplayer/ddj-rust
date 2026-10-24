//! 会话状态机
//!
//! 对应 Java 的隐式状态：`ChannelPlayer` 是否绑定、`Player` 是否已加载。
//! 原系统靠 `if (channelPlayer == null || channelPlayer.getPlayer() == null)` 判断，
//! 分散在 103 个 Handler 里；这里收敛为显式枚举 + 迁移表。

/// 会话状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SessionState {
    /// 已建 TCP 连接，未登录。
    Connected = 0,
    /// 已通过登录校验，已加载 `Player`。
    Authenticated = 1,
    /// 已进入场景，可收发游戏消息。
    Playing = 2,
    /// 已断开，等待清理。
    Closed = 3,
}

impl SessionState {
    /// 是否允许收发需要登录的消息。
    ///
    /// 等价于原 `FREE_LOGIN_CMDS` 之外的所有消息的前置条件。
    #[must_use]
    pub const fn can_play(self) -> bool {
        matches!(self, Self::Authenticated | Self::Playing)
    }

    /// 迁移是否合法。
    ///
    /// ```text
    /// Connected ──▶ Authenticated ──▶ Playing
    ///     │              │              │
    ///     └──────────────┴──────────────┴──▶ Closed
    /// ```
    /// 不允许回退（`Playing` → `Authenticated`），掉线一律走 `Closed` 后重建。
    #[must_use]
    pub const fn can_transit_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Connected, Self::Authenticated | Self::Closed)
                | (Self::Authenticated, Self::Playing | Self::Closed)
                | (Self::Playing, Self::Closed)
        )
    }

    /// 中文名，用于日志。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "已连接",
            Self::Authenticated => "已登录",
            Self::Playing => "游戏中",
            Self::Closed => "已断开",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionState::{Authenticated, Closed, Connected, Playing};

    #[test]
    fn happy_path() {
        assert!(Connected.can_transit_to(Authenticated));
        assert!(Authenticated.can_transit_to(Playing));
        assert!(Playing.can_transit_to(Closed));
    }

    #[test]
    fn no_regress() {
        assert!(!Playing.can_transit_to(Authenticated));
        assert!(!Authenticated.can_transit_to(Connected));
        assert!(!Closed.can_transit_to(Connected));
    }

    #[test]
    fn skip_is_rejected() {
        assert!(!Connected.can_transit_to(Playing));
    }
}
