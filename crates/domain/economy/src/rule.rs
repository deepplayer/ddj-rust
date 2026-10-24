//! 经济规则

/// 经济错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EconomyError {
    /// 订单不存在。
    #[error("订单不存在: {0}")]
    OrderNotFound(String),
    /// 状态迁移非法。
    #[error("订单状态非法")]
    IllegalTransition,
    /// 重复发货。
    #[error("订单已发货，拒绝重复处理")]
    DuplicateDeliver,
    /// 金额不符。
    #[error("金额不符：期望 {expect}，实际 {actual}")]
    AmountMismatch {
        /// 期望（分）。
        expect: i64,
        /// 实际（分）。
        actual: i64,
    },
}
