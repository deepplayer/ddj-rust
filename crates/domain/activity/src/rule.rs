//! 活动规则

/// 活动错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ActivityError {
    /// 未开启。
    #[error("活动未开启")]
    NotOpen,
    /// 已结束。
    #[error("活动已结束")]
    Closed,
    /// 条件未达成。
    #[error("条件未达成：需要 {need}，当前 {have}")]
    NotReached {
        /// 需要。
        need: i64,
        /// 当前。
        have: i64,
    },
    /// 已领取。
    #[error("该档位已领取")]
    AlreadyClaimed,
}
