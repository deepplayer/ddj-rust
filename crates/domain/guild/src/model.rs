//! 公会聚合
//!
//! 对应 Java：`bf.game.server.domain.guild.Guild` + `guild` / `guild_member` 表。

use ddj_core::{GuildId, PlayerId};

/// 职位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuildRank {
    /// 成员。
    Member = 0,
    /// 精英。
    Elite = 1,
    /// 副会长。
    ViceMaster = 2,
    /// 会长。
    Master = 3,
}

impl GuildRank {
    /// 是否可以踢人。
    #[must_use]
    pub const fn can_kick(self) -> bool {
        matches!(self, Self::ViceMaster | Self::Master)
    }

    /// 是否可以审批加入。
    #[must_use]
    pub const fn can_approve(self) -> bool {
        self.can_kick()
    }
}

/// 成员。
#[derive(Debug, Clone)]
pub struct Member {
    /// 玩家。
    pub player: PlayerId,
    /// 职位。
    pub rank: GuildRank,
    /// 贡献。
    pub contribution: i64,
    /// 加入时间。
    pub joined_ms: i64,
}

/// 公会。
#[derive(Debug, Clone)]
pub struct Guild {
    /// ID。
    pub id: GuildId,
    /// 名称。
    pub name: String,
    /// 会长。
    pub master: PlayerId,
    /// 等级。
    pub level: u32,
    /// 资金。
    pub fund: i64,
    /// 成员。
    pub members: Vec<Member>,
}

/// 公会人数上限（原表 `t_guild_level.max_member`）。
pub const MAX_MEMBERS: usize = 50;

impl Guild {
    /// 找成员。
    #[must_use]
    pub fn member(&self, p: PlayerId) -> Option<&Member> {
        self.members.iter().find(|m| m.player == p)
    }

    /// 是否满员。
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.members.len() >= MAX_MEMBERS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_can_kick_vice_can_kick_member_cannot() {
        assert!(GuildRank::Master.can_kick());
        assert!(GuildRank::ViceMaster.can_kick());
        assert!(!GuildRank::Elite.can_kick());
        assert!(!GuildRank::Member.can_kick());
    }
}
