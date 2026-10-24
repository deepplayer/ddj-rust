//! 邮件规则

/// 邮件错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MailError {
    /// 邮件不存在。
    #[error("邮件不存在")]
    NotFound,
    /// 已领取。
    #[error("已领取过")]
    AlreadyClaimed,
    /// 已过期。
    #[error("邮件已过期")]
    Expired,
    /// 无附件。
    #[error("该邮件没有附件")]
    NoAttachment,
    /// 收件箱已满。
    #[error("收件箱已满（上限 {0}）")]
    BoxFull(usize),
}
