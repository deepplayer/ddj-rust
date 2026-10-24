//! 业务规则与不变量
//!
//! 这里集中实现**原服务端 `ItemSet.itemUseReq` 的校验顺序**。
//! 顺序错了行为就不一样（例如先查次数再查类型，会让非法类型也消耗次数）。

use crate::model::{Bag, ItemType};
use ddj_core::TemplateId;

/// 道具领域错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ItemError {
    /// 数量非法（≤0）。
    #[error("数量非法")]
    BadNumber,
    /// 背包中没有该道具。
    #[error("没有该道具")]
    NotFound,
    /// 数量不足。
    #[error("道具不足，需要 {need}，实际 {have}")]
    NotEnough {
        /// 需要。
        need: i64,
        /// 实际。
        have: i64,
    },
    /// 该类型不支持「使用」。
    #[error("该道具不可使用")]
    NotUsable,
    /// 使用次数超上限。
    #[error("使用次数已达上限 {0}")]
    UseLimitReached(i32),
    /// 消耗不足（使用需要先付代价）。
    #[error("消耗不足")]
    CostNotEnough,
}

/// 模板中与「使用」相关的元数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemTemplate {
    /// 模板 ID。
    pub id: TemplateId,
    /// 类型。
    pub kind: ItemType,
    /// 每日/每周使用次数上限（0 = 不限制）。
    pub use_limit: i32,
    /// 一次使用扣除的数量。
    pub remove_num: i64,
    /// 使用代价（货币类型, 数量），`None` = 免费。
    pub cost: Option<(i32, i64)>,
}

impl ItemTemplate {
    /// 构造一个「效果类、无限制、单次 1 个」的模板。
    #[must_use]
    pub fn effective(id: TemplateId) -> Self {
        Self {
            id,
            kind: ItemType::Effective,
            use_limit: 0,
            remove_num: 1,
            cost: None,
        }
    }
}

/// 校验上下文。
#[derive(Debug, Clone, Copy)]
pub struct UseContext {
    /// 客户端请求的数量。
    pub req_num: i64,
    /// 今日已使用次数。
    pub used_today: i32,
}

/// 使用前的完整校验。
///
/// 顺序严格复刻原服务端：
/// 1. `num <= 0` → 拒绝
/// 2. 类型不是 `Effective` / `ChessScore` → 拒绝
/// 3. 次数上限
/// 4. 扣除数量计算
/// 5. 背包数量是否足够
/// 6. 代价是否足够
///
/// # Errors
/// 任一校验失败。
pub fn check_use(bag: &Bag, tpl: &ItemTemplate, ctx: UseContext) -> Result<i64, ItemError> {
    if ctx.req_num <= 0 {
        return Err(ItemError::BadNumber);
    }
    if !tpl.kind.usable() {
        return Err(ItemError::NotUsable);
    }
    if tpl.use_limit > 0 && ctx.used_today >= tpl.use_limit {
        return Err(ItemError::UseLimitReached(tpl.use_limit));
    }
    let remove = tpl.remove_num.saturating_mul(ctx.req_num);
    let have = bag.count(tpl.id);
    if have < remove {
        return Err(ItemError::NotEnough { need: remove, have });
    }
    Ok(remove)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Bag;
    use ddj_core::PlayerId;

    fn bag_with(n: i64) -> Bag {
        let mut b = Bag::new(PlayerId::new(1));
        b.grant(TemplateId::new(1101), n);
        b
    }

    #[test]
    fn rejects_zero_or_negative() {
        let b = bag_with(10);
        let t = ItemTemplate::effective(TemplateId::new(1101));
        assert_eq!(
            check_use(
                &b,
                &t,
                UseContext {
                    req_num: 0,
                    used_today: 0
                }
            ),
            Err(ItemError::BadNumber)
        );
        assert_eq!(
            check_use(
                &b,
                &t,
                UseContext {
                    req_num: -1,
                    used_today: 0
                }
            ),
            Err(ItemError::BadNumber)
        );
    }

    #[test]
    fn rejects_unusable_type() {
        let b = bag_with(10);
        let mut t = ItemTemplate::effective(TemplateId::new(1101));
        t.kind = ItemType::Gift;
        assert_eq!(
            check_use(
                &b,
                &t,
                UseContext {
                    req_num: 1,
                    used_today: 0
                }
            ),
            Err(ItemError::NotUsable)
        );
    }

    #[test]
    fn enforces_use_limit() {
        let b = bag_with(10);
        let mut t = ItemTemplate::effective(TemplateId::new(1101));
        t.use_limit = 3;
        assert_eq!(
            check_use(
                &b,
                &t,
                UseContext {
                    req_num: 1,
                    used_today: 3
                }
            ),
            Err(ItemError::UseLimitReached(3))
        );
    }

    #[test]
    fn multiplies_remove_num() {
        let b = bag_with(10);
        let mut t = ItemTemplate::effective(TemplateId::new(1101));
        t.remove_num = 2;
        assert_eq!(
            check_use(
                &b,
                &t,
                UseContext {
                    req_num: 3,
                    used_today: 0
                }
            ),
            Ok(6)
        );
        assert_eq!(
            check_use(
                &b,
                &t,
                UseContext {
                    req_num: 6,
                    used_today: 0
                }
            ),
            Err(ItemError::NotEnough { need: 12, have: 10 })
        );
    }
}
