//! 战斗规则

/// 战斗错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BattleError {
    /// 战报步数超限。
    #[error("战报步数超限：{0}")]
    TooManySteps(usize),
    /// 战报与服务器记录不一致。
    #[error("战报校验失败")]
    VerifyFailed,
    /// 战斗不存在。
    #[error("战斗不存在: {0}")]
    NotFound(u64),
    /// 重复结算。
    #[error("战斗已结算: {0}")]
    AlreadySettled(u64),
}
