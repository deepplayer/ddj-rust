//! 公会规则

/// 公会错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GuildError {
    /// 公会不存在。
    #[error("公会不存在")]
    NotFound,
    /// 名称重复。
    #[error("公会名称已被占用")]
    NameTaken,
    /// 满员。
    #[error("公会已满员")]
    Full,
    /// 已是成员。
    #[error("已在公会中")]
    AlreadyMember,
    /// 权限不足。
    #[error("权限不足")]
    NoPermission,
    /// 会长不能退出。
    #[error("会长不能直接退出，请先转让")]
    MasterCannotLeave,
}
