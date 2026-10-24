//! 角色规则

use crate::model::Currency;

/// 角色领域错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlayerError {
    /// 数量非法。
    #[error("数量非法")]
    BadAmount,
    /// 货币不足。
    #[error("货币不足：{currency:?} 需要 {need}，实际 {have}")]
    NotEnough {
        /// 货币。
        currency: Currency,
        /// 需要。
        need: i64,
        /// 实际。
        have: i64,
    },
    /// 已达上限。
    #[error("已达上限：{0}")]
    Maxed(&'static str),
}
