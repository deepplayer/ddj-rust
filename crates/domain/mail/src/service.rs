//! 邮件领域服务

use crate::event::MailEvent;
use crate::model::Mail;
use crate::rule::MailError;

/// 事件发布。
pub trait EventSink: Send + Sync {
    /// 发布。
    fn publish(&self, ev: MailEvent);
}

/// 领取附件，返回被领取的附件列表。
///
/// # Errors
/// 见 [`MailError`]。
pub fn claim(
    mail: &mut Mail,
    now_ms: i64,
    sink: &dyn EventSink,
) -> Result<Vec<crate::model::Attachment>, MailError> {
    if mail.expired(now_ms) {
        return Err(MailError::Expired);
    }
    if mail.claimed {
        return Err(MailError::AlreadyClaimed);
    }
    if mail.attachments.is_empty() {
        return Err(MailError::NoAttachment);
    }
    mail.claimed = true;
    mail.read = true;
    let items = mail.attachments.clone();
    sink.publish(MailEvent::Claimed {
        player: mail.to,
        mail: mail.id,
    });
    Ok(items)
}

/// 静默 sink。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, ev: MailEvent) {
        tracing::debug!(?ev, "事件（无订阅者）");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Attachment, Mail, MailKind};
    use ddj_core::{PlayerId, TemplateId};

    #[test]
    fn claim_twice_fails() {
        let mut m = Mail {
            id: 7,
            to: PlayerId::new(1),
            kind: MailKind::Reward,
            title: "t".into(),
            body: "b".into(),
            attachments: vec![Attachment {
                kind: 501,
                template: TemplateId::new(1),
                num: 9,
            }],
            read: false,
            claimed: false,
            sent_ms: 0,
            expire_ms: 0,
        };
        let s = NoopSink;
        let got = claim(&mut m, 0, &s).expect("首次可领");
        assert_eq!(got.len(), 1);
        assert_eq!(claim(&mut m, 0, &s), Err(MailError::AlreadyClaimed));
    }
}
