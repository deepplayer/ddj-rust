//! 邮件聚合
//!
//! 对应 Java：`bf.game.server.domain.mail.Mail` + `mail` 表 + `mail_attach` 表。

use ddj_core::{PlayerId, TemplateId};

/// 邮件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailKind {
    /// 系统公告。
    System = 1,
    /// 玩家私信。
    Personal = 2,
    /// 公会邮件。
    Guild = 3,
    /// 奖励邮件（带附件）。
    Reward = 4,
}

/// 附件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// 物品类（`TypeModule` 编号）。
    pub kind: i32,
    /// 模板 ID。
    pub template: TemplateId,
    /// 数量。
    pub num: i64,
}

/// 一封邮件。
#[derive(Debug, Clone)]
pub struct Mail {
    /// 主键。
    pub id: i64,
    /// 收件人。
    pub to: PlayerId,
    /// 类型。
    pub kind: MailKind,
    /// 标题。
    pub title: String,
    /// 正文。
    pub body: String,
    /// 附件。
    pub attachments: Vec<Attachment>,
    /// 是否已读。
    pub read: bool,
    /// 是否已领奖。
    pub claimed: bool,
    /// 发送时间。
    pub sent_ms: i64,
    /// 过期时间（0 = 永不过期）。
    pub expire_ms: i64,
}

/// 邮件上限（原系统单角色 100 封）。
pub const MAIL_LIMIT: usize = 100;

impl Mail {
    /// 是否已过期。
    #[must_use]
    pub const fn expired(&self, now_ms: i64) -> bool {
        self.expire_ms > 0 && now_ms >= self.expire_ms
    }

    /// 是否可领奖。
    #[must_use]
    pub const fn claimable(&self, now_ms: i64) -> bool {
        !self.claimed && !self.attachments.is_empty() && !self.expired(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(expire: i64, claimed: bool, att: bool) -> Mail {
        Mail {
            id: 1,
            to: PlayerId::new(1),
            kind: MailKind::Reward,
            title: "t".into(),
            body: "b".into(),
            attachments: if att {
                vec![Attachment {
                    kind: 501,
                    template: TemplateId::new(1),
                    num: 100,
                }]
            } else {
                vec![]
            },
            read: false,
            claimed,
            sent_ms: 0,
            expire_ms: expire,
        }
    }

    #[test]
    fn expired_blocks_claim() {
        assert!(!m(1000, false, true).claimable(2000));
    }

    #[test]
    fn claimed_blocks_claim() {
        assert!(!m(0, true, true).claimable(0));
    }

    #[test]
    fn no_attachment_blocks_claim() {
        assert!(!m(0, false, false).claimable(0));
    }

    #[test]
    fn normal_is_claimable() {
        assert!(m(0, false, true).claimable(0));
    }
}
