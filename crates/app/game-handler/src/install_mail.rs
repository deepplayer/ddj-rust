//! 邮件 4 条消息的接线（10001 / 10002 / 10003 / 10004）。
//!
//! ## 过渡期边界
//! - 邮件模板（`MailTemplateDB`）未接 → `template` 闭包恒 `None`，附件只能来自
//!   邮件实例自带的 `annexs`（与原版回退路径一致）。
//! - `checkAddResourcesJsons`（背包容量校验）未接 → 恒 `true`（装得下）。
//! - `format_send_time` 按东八区手写格式化，不引 `chrono`：原版用
//!   `LogicUtils.getTimeByHour` + `SimpleDateFormat`，只用于事件日志文案。

use std::sync::Arc;

use ddj_core::{Clock, Result};
use ddj_pb::gen::mail_message::{
    MailGetReq10001, MailReceiveReq10003, MailRemoveReq10004, MailReadReq10002,
};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::mail::{self, MailRecord, MailTemplate, Reward};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 东八区偏移（毫秒）。
const TZ_MS: i64 = 8 * 3600 * 1000;

/// `yyyy-MM-dd HH:mm:ss`（东八区）。原版 `SimpleDateFormat` 的等价实现。
#[must_use]
pub fn format_send_time(ms: i64) -> String {
    let total = ms.div_euclid(1000) + TZ_MS / 1000;
    let days = total.div_euclid(86_400);
    let secs = total.rem_euclid(86_400);
    // Howard Hinnant civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// 把 4 条邮件消息挂进表。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 10001 拉取（不加锁、不做过期检查）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(10001, move |ctx, body| {
            let _req = MailGetReq10001::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let mails = w.peek(p, |b| b.mails.clone());
            Ok(Some(
                mail::mail_get_resp(&mails).encode_to_vec().into(),
            ))
        })));
        n += 1;
    }

    // 10002 标记已读
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(10002, move |ctx, body| {
            let req = MailReadReq10002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let ids = parse_ids(&req.id)?;
            let mails = w.peek(p, |b| b.mails.clone());
            let plan = mail::mail_read_plan(&p.get().to_string(), &ids, &mails, format_send_time);
            if !plan.newly_read.is_empty() {
                let touched = plan.newly_read.clone();
                w.with(p, |b| {
                    for m in &mut b.mails {
                        if touched.contains(&m.db_id) {
                            m.read = true;
                        }
                    }
                });
            }
            let ret = mail::mail_read_resp();
            Ok(Some(
                ddj_pb::gen::mail_message::MailReadResp10002 { ret }
                    .encode_to_vec()
                    .into(),
            ))
        })));
        n += 1;
    }

    // 10003 领取附件
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(10003, move |ctx, body| {
            let req = MailReceiveReq10003::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let ids = parse_ids(&req.id)?;
            let mails = w.peek(p, |b| b.mails.clone());
            let plan = mail::mail_receive_plan(
                &p.get().to_string(),
                &ids,
                &mails,
                |_: i32| None::<MailTemplate>,
                |_: &[Reward]| true,
                format_send_time,
            );
            if !plan.claimed.is_empty() {
                let touched = plan.claimed.clone();
                let rewards = plan.rewards.clone();
                w.with(p, |b| {
                    for m in &mut b.mails {
                        if touched.contains(&m.db_id) {
                            m.receive = true;
                        }
                    }
                    for (id, num) in &rewards {
                        *b.bag.entry(*id).or_insert(0) += num;
                    }
                });
            }
            let resp = mail::mail_receive_resp(&plan);
            Ok(Some(resp.encode_to_vec().into()))
        })));
        n += 1;
    }

    // 10004 删除
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(10004, move |ctx, body| {
            let req = MailRemoveReq10004::decode(body).map_err(decode_err)?;
            let _ = c.now_millis();
            let p = ctx.require_player()?;
            let ids = parse_ids(&req.id)?;
            let mails = w.peek(p, |b| b.mails.clone());
            let plan = mail::mail_remove_plan(&ids, &mails);
            let remaining: Vec<MailRecord> = plan.remaining.clone();
            w.with(p, |b| b.mails = remaining);
            let resp = mail::mail_remove_resp(&plan);
            Ok(Some(resp.encode_to_vec().into()))
        })));
        n += 1;
    }

    n
}

/// `repeated string id` → `Vec<i64>`。
///
/// 原版 `getLongs` 用 `Long.parseLong`，非法值抛 `NumberFormatException`，
/// 被 handler 的 `catch` 吞成 `ret = COMMON_SERVER_BUSY`。这里换成等价行为：
/// 一旦有非法值就整体失败（不能只回一半）。
fn parse_ids(raw: &[String]) -> Result<Vec<i64>> {
    raw.iter()
        .map(|s| {
            s.trim()
                .parse::<i64>()
                .map_err(|_| ddj_core::AppError::BadRequest(format!("邮件 id 不是数字: {s}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch_in_shanghai() {
        // 2024-01-01 00:00:00 +08:00 = 1704038400000
        assert_eq!(format_send_time(1_704_038_400_000), "2024-01-01 00:00:00");
    }

    #[test]
    fn rejects_non_numeric_id() {
        assert!(parse_ids(&["12".into(), "x".into()]).is_err());
        assert_eq!(parse_ids(&["12".into()]).unwrap(), vec![12]);
    }
}
