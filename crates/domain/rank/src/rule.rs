//! 排行规则

/// 排行错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RankError {
    /// 分数为负。
    #[error("分数不能为负")]
    NegativeScore,
    /// 榜不存在。
    #[error("榜单不存在")]
    NotFound,
}
