//! 排行聚合
//!
//! 对应 Java：`bf.game.server.domain.rank.RankList` + `rank` 表。
//! 原实现每次查榜都全表扫 `ORDER BY score DESC`，这里改为内存快照 + Redis ZSET。

use ddj_core::PlayerId;

/// 榜单类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RankKind {
    /// 战力。
    Power = 1,
    /// 等级。
    Level = 2,
    /// 财富。
    Wealth = 3,
    /// 公会。
    Guild = 4,
    /// 活动。
    Activity = 5,
}

impl RankKind {
    /// 对应 Redis key 后缀。
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Power => "power",
            Self::Level => "level",
            Self::Wealth => "wealth",
            Self::Guild => "guild",
            Self::Activity => "activity",
        }
    }
}

/// 榜单条目。
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// 名次（1 起）。
    pub rank: u32,
    /// 玩家。
    pub player: PlayerId,
    /// 分数。
    pub score: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable() {
        assert_eq!(RankKind::Power.key(), "power");
        assert_eq!(RankKind::Guild.key(), "guild");
    }
}
