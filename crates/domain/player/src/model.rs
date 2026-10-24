//! 角色聚合根
//!
//! 对应 Java：`bf.game.server.domain.player.Player`（一个 2000+ 行的巨型 POJO）
//! + `player` 表（173 个字段）。
//!
//! ## 拆分原则
//! Java 的 `Player` 同时装着货币、等级、背包、任务、公会、活动进度。
//! 这里拆成多个聚合：`Player`（身份 + 等级 + 货币）、`Bag`（在 `ddj-item`）、
//! `Guild`（在 `ddj-guild`）、进度类在 `ddj-activity`。

use ddj_core::{PlayerId, ServerId};

/// 货币种类（对应 `t_currency`，原 `TypeModule.TYPE_5xx`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum Currency {
    /// 铜钱（TYPE_501）。
    Copper = 501,
    /// 元宝（TYPE_502）。
    Ingot = 502,
    /// 体力（TYPE_503）。
    Energy = 503,
    /// 声望（TYPE_504）。
    Fame = 504,
    /// 贡献（TYPE_505）。
    Contribution = 505,
    /// 帮派资金（TYPE_506）。
    GuildFund = 506,
    /// 活动积分（TYPE_507）。
    ActivityScore = 507,
}

impl Currency {
    /// 从模板值还原。
    #[must_use]
    pub const fn from_i32(v: i32) -> Option<Self> {
        Some(match v {
            501 => Self::Copper,
            502 => Self::Ingot,
            503 => Self::Energy,
            504 => Self::Fame,
            505 => Self::Contribution,
            506 => Self::GuildFund,
            507 => Self::ActivityScore,
            _ => return None,
        })
    }
}

/// 角色身份（不可变部分）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// 角色 ID。
    pub id: PlayerId,
    /// 区服。
    pub server: ServerId,
    /// 账号 ID（`platform.user.userid`）。
    pub account_id: i64,
    /// 昵称。
    pub name: String,
}

/// 角色聚合根。
#[derive(Debug, Clone)]
pub struct Player {
    /// 身份。
    pub identity: Identity,
    /// 等级。
    level: u32,
    /// 经验。
    exp: i64,
    /// VIP 等级。
    vip: u32,
    /// 货币余额。
    wallet: std::collections::BTreeMap<Currency, i64>,
    /// 最后登录（毫秒时间戳）。
    pub last_login_ms: i64,
}

/// 等级上限（原表 `t_player_level` 最大行数）。
pub const MAX_LEVEL: u32 = 300;

impl Player {
    /// 新建 1 级角色。
    #[must_use]
    pub fn new(identity: Identity) -> Self {
        Self {
            identity,
            level: 1,
            exp: 0,
            vip: 0,
            wallet: std::collections::BTreeMap::new(),
            last_login_ms: 0,
        }
    }

    /// 等级。
    #[must_use]
    pub const fn level(&self) -> u32 {
        self.level
    }

    /// 经验。
    #[must_use]
    pub const fn exp(&self) -> i64 {
        self.exp
    }

    /// VIP。
    #[must_use]
    pub const fn vip(&self) -> u32 {
        self.vip
    }

    /// 余额。
    #[must_use]
    pub fn balance(&self, c: Currency) -> i64 {
        self.wallet.get(&c).copied().unwrap_or(0)
    }

    /// 加货币。
    pub fn credit(&mut self, c: Currency, n: i64) {
        let e = self.wallet.entry(c).or_insert(0);
        *e = e.saturating_add(n);
    }

    /// 扣货币。
    ///
    /// # Errors
    /// 余额不足。
    pub fn debit(&mut self, c: Currency, n: i64) -> Result<(), crate::rule::PlayerError> {
        if n <= 0 {
            return Err(crate::rule::PlayerError::BadAmount);
        }
        let cur = self.balance(c);
        if cur < n {
            return Err(crate::rule::PlayerError::NotEnough {
                currency: c,
                need: n,
                have: cur,
            });
        }
        self.credit(c, -n);
        Ok(())
    }

    /// 加经验，自动处理升级。
    ///
    /// # Errors
    /// 已达最高等级时返回 `Ok(0)`（不报错，经验照收）。
    pub fn gain_exp(
        &mut self,
        n: i64,
        need_fn: impl Fn(u32) -> i64,
    ) -> Result<u32, crate::rule::PlayerError> {
        if n < 0 {
            return Err(crate::rule::PlayerError::BadAmount);
        }
        self.exp = self.exp.saturating_add(n);
        let mut ups = 0;
        while self.level < MAX_LEVEL {
            let need = need_fn(self.level);
            if need <= 0 || self.exp < need {
                break;
            }
            self.exp -= need;
            self.level += 1;
            ups += 1;
        }
        Ok(ups)
    }

    /// 提升 VIP。
    pub fn set_vip(&mut self, v: u32) {
        self.vip = v.min(20);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Player {
        Player::new(Identity {
            id: PlayerId::new(1),
            server: ServerId::new(7010544),
            account_id: 42,
            name: "测试".into(),
        })
    }

    #[test]
    fn currency_roundtrip() {
        let mut pl = p();
        pl.credit(Currency::Ingot, 100);
        assert_eq!(pl.balance(Currency::Ingot), 100);
        pl.debit(Currency::Ingot, 30).expect("足够");
        assert_eq!(pl.balance(Currency::Ingot), 70);
        assert!(pl.debit(Currency::Ingot, 1000).is_err());
    }

    #[test]
    fn level_up() {
        let mut pl = p();
        // 每级固定 100 经验
        let ups = pl.gain_exp(250, |_| 100).expect("合法");
        assert_eq!(ups, 2);
        assert_eq!(pl.level(), 3);
        assert_eq!(pl.exp(), 50);
    }
}
