//! 聚合根与值对象
//!
//! 对应 Java：`bf.game.server.domain.item.Item` + `ItemSet`（道具集合）
//! 以及模板表 `t_item`（6500 行 CSV）。
//!
//! ## 与 Java 版的差异
//! - Java 的 `Item` 是可变 POJO，任何 Handler 都能直接改 `num`
//! - 这里是聚合根：`num` 私有，只能通过 `add` / `consume` 变更，自动维护不变量

use ddj_core::{PlayerId, TemplateId};

/// 道具类型（对应模板表 `type` 字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemType {
    /// 普通道具（可丢弃）。
    Normal = 1,
    /// 效果类（使用后立即生效，本次逆向的对象）。
    Effective = 2,
    /// 礼包（开出东西）。
    Gift = 3,
    /// 棋子积分（活动道具）。
    ChessScore = 4,
    /// 装备。
    Equip = 5,
    /// 货币类。
    Currency = 6,
}

impl ItemType {
    /// 从模板值还原。
    #[must_use]
    pub const fn from_i32(v: i32) -> Option<Self> {
        Some(match v {
            1 => Self::Normal,
            2 => Self::Effective,
            3 => Self::Gift,
            4 => Self::ChessScore,
            5 => Self::Equip,
            6 => Self::Currency,
            _ => return None,
        })
    }

    /// 是否允许「使用」。
    ///
    /// 复刻原服务端校验：只有 `EFFECTIVE` / `CHESS_SCORE` 能走 `ItemUseReq`。
    #[must_use]
    pub const fn usable(self) -> bool {
        matches!(self, Self::Effective | Self::ChessScore)
    }
}

/// 背包里的一格。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 模板 ID。
    pub template_id: TemplateId,
    /// 数量。
    num: i64,
    /// 是否绑定。
    pub bound: bool,
}

/// 数量上限（原表 `max_stack`，绝大多数是 999999999）。
pub const MAX_STACK: i64 = 999_999_999;

impl Item {
    /// 新建。
    #[must_use]
    pub fn new(template_id: TemplateId, num: i64) -> Self {
        Self {
            template_id,
            num: num.clamp(0, MAX_STACK),
            bound: false,
        }
    }

    /// 当前数量。
    #[must_use]
    pub const fn num(&self) -> i64 {
        self.num
    }

    /// 增加。
    pub fn add(&mut self, n: i64) {
        self.num = (self.num + n).clamp(0, MAX_STACK);
    }

    /// 消耗。
    ///
    /// # Errors
    /// 数量不足。
    pub fn consume(&mut self, n: i64) -> Result<(), crate::rule::ItemError> {
        if n <= 0 {
            return Err(crate::rule::ItemError::BadNumber);
        }
        if self.num < n {
            return Err(crate::rule::ItemError::NotEnough {
                need: n,
                have: self.num,
            });
        }
        self.num -= n;
        Ok(())
    }

    /// 是否为空格。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.num <= 0
    }
}

/// 一个玩家的背包。
#[derive(Debug, Clone)]
pub struct Bag {
    /// 归属。
    pub owner: PlayerId,
    /// 道具列表。
    items: std::collections::HashMap<TemplateId, Item>,
}

impl Bag {
    /// 新建空背包。
    #[must_use]
    pub fn new(owner: PlayerId) -> Self {
        Self {
            owner,
            items: std::collections::HashMap::new(),
        }
    }

    /// 查询数量。
    #[must_use]
    pub fn count(&self, id: TemplateId) -> i64 {
        self.items.get(&id).map_or(0, Item::num)
    }

    /// 取（只读）。
    #[must_use]
    pub fn get(&self, id: TemplateId) -> Option<&Item> {
        self.items.get(&id)
    }

    /// 发放。
    pub fn grant(&mut self, id: TemplateId, n: i64) {
        self.items
            .entry(id)
            .and_modify(|i| i.add(n))
            .or_insert_with(|| Item::new(id, n));
        self.trim();
    }

    /// 扣除。
    ///
    /// # Errors
    /// 数量不足。
    pub fn take(&mut self, id: TemplateId, n: i64) -> Result<(), crate::rule::ItemError> {
        let item = self
            .items
            .get_mut(&id)
            .ok_or(crate::rule::ItemError::NotFound)?;
        item.consume(n)?;
        self.trim();
        Ok(())
    }

    /// 移除空格。
    fn trim(&mut self) {
        self.items.retain(|_, i| !i.is_empty());
    }

    /// 遍历。
    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        self.items.values()
    }

    /// 格数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_and_take() {
        let mut b = Bag::new(PlayerId::new(1));
        b.grant(TemplateId::new(1001), 5);
        assert_eq!(b.count(TemplateId::new(1001)), 5);
        b.take(TemplateId::new(1001), 2).expect("足够");
        assert_eq!(b.count(TemplateId::new(1001)), 3);
    }

    #[test]
    fn take_more_than_have() {
        let mut b = Bag::new(PlayerId::new(1));
        b.grant(TemplateId::new(1001), 1);
        assert!(b.take(TemplateId::new(1001), 2).is_err());
    }

    #[test]
    fn empty_slot_removed() {
        let mut b = Bag::new(PlayerId::new(1));
        b.grant(TemplateId::new(1001), 1);
        b.take(TemplateId::new(1001), 1).expect("足够");
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn usable_only_effective_and_chess() {
        assert!(ItemType::Effective.usable());
        assert!(ItemType::ChessScore.usable());
        assert!(!ItemType::Gift.usable());
        assert!(!ItemType::Normal.usable());
    }
}
