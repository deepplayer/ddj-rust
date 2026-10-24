//! # 邮件系统（10001 / 10002 / 10003 / 10004）
//!
//! 对应 Java：
//! - `MailMessageHandler`（4 个方法，全部是转发 + `catch`）
//! - `MailBox`（`mailGetReq` 67 B / `mailReadReq` 152 B / `mailReceiveReq` 430 B / `mailRemoveReq` 165 B / `outTimeCheck` 99 B）
//! - `Mail`（`isOutTime` 58 B / `toMsgMailDataBuilder` 235 B）
//!
//! 本模块只做判定与回包构造，不含 IO；读写 `mailMap` 与发放资源由调用方注入。

use ddj_pb::gen::common_msg_message::MsgResources;
use ddj_pb::gen::mail_message::{
    MailGetResp10001, MailReceiveResp10003, MailRemoveResp10004, MsgMailData,
};
use ddj_pb::RetCode;

/// 一份奖励（`ResourcesJson` 的 `(id, num)` 投影）。
pub type Reward = (i32, i64);

/// `MailTemplateDB` 里领奖与日志需要的那几列。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailTemplate {
    /// `rewardJson`：模板自带附件。
    pub reward: Vec<Reward>,
    /// `title`：模板标题（日志用它，不用邮件实例的）。
    pub title: String,
    /// `content`：模板正文。
    pub content: String,
}

/// 邮件状态快照（`bf.game.server.module.mail.Mail` 的可见字段）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailRecord {
    /// `dbId`，客户端用字符串表示。
    pub db_id: i64,
    /// `templateId`（对应 `MailTemplateDB`）。
    pub template_id: i32,
    /// `type`：1 系统 / 2 GM / 3 玩家。
    pub mail_type: i32,
    /// `sendTime`（毫秒）。
    pub send_time: i64,
    /// `outTime`（毫秒）。`<= 0` 表示没有自定义过期时间。
    pub out_time: i64,
    /// 自定义标题。
    pub title: Option<String>,
    /// 自定义正文。
    pub content: Option<String>,
    /// 自定义附件；`None` 与空列表在原版是两种语义（见 `mail_read_plan`）。
    pub annexs: Option<Vec<Reward>>,
    /// 是否已读。
    pub read: bool,
    /// 是否已领奖。
    pub receive: bool,
    /// 发件人 id。
    pub from_player_id: Option<String>,
    /// 发件人名字。
    pub from_player_name: Option<String>,
    /// 模板替换字符（`fiters`）。
    pub fiters: Option<Vec<Option<String>>>,
}

impl MailRecord {
    /// `Mail.isOutTime()`（58 B）。
    ///
    /// - `outTime > 0`：`now >= outTime` 即过期（`lcmp; iflt`）。
    /// - 否则：`now - sendTime > MailDeleteTime × 60 × 60 × 1000`。
    #[must_use]
    pub fn is_out_time(&self, now: i64, mail_delete_time_hours: i64) -> bool {
        if self.out_time > 0 {
            return now >= self.out_time;
        }
        let elapsed = now - self.send_time;
        elapsed > mail_delete_time_hours * 60 * 60 * 1000
    }

    /// `Mail.toMsgMailDataBuilder()`（235 B）的字段映射。
    ///
    /// 四个 `null → ""` 的地方：`fiters` 元素、`fromPlayerId`、`fromPlayerName`；
    /// `outTime` 只在 `> 0` 时才 set。
    #[must_use]
    pub fn to_msg_mail_data(&self) -> MsgMailData {
        MsgMailData {
            r#type: Some(self.mail_type),
            id: Some(self.db_id.to_string()),
            send_time: Some(self.send_time),
            template_id: Some(self.template_id),
            fiters: self
                .fiters
                .as_ref()
                .map(|v| v.iter().map(|f| f.clone().unwrap_or_default()).collect())
                .unwrap_or_default(),
            title: self.title.clone(),
            content: self.content.clone(),
            annexs: self
                .annexs
                .as_ref()
                .map(|v| v.iter().map(|(id, num)| msg_resources(*id, *num)).collect())
                .unwrap_or_default(),
            out_time: (self.out_time > 0).then_some(self.out_time),
            read: Some(self.read),
            receive: Some(self.receive),
            from_player_id: Some(self.from_player_id.clone().unwrap_or_default()),
            from_player_name: Some(self.from_player_name.clone().unwrap_or_default()),
        }
    }
}

fn msg_resources(id: i32, num: i64) -> MsgResources {
    MsgResources {
        id: Some(id),
        num: Some(num.to_string()),
        multiple: None,
    }
}

// ────────────────────────── 过期清理 ──────────────────────────

/// `MailBox.outTimeCheck()` 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpireResult {
    /// 是否真的扫了一遍（距离上次不足 60 秒则直接返回）。
    pub ran: bool,
    /// 被清掉的邮件 id（按 `mailMap` 迭代序）。
    pub removed: Vec<i64>,
}

/// `MailBox.outTimeCheck()`（99 B）。
///
/// **60 秒节流**：`now - last_check <= 60_000` 时直接返回，连锁都不加。
/// 注意本方法只读 `_ot`、从不写回，节流字段由别处维护。
#[must_use]
pub fn expire_mails(
    now: i64,
    last_check: i64,
    mail_delete_time_hours: i64,
    mails: &[MailRecord],
) -> ExpireResult {
    if now - last_check <= 60_000 {
        return ExpireResult {
            ran: false,
            removed: Vec::new(),
        };
    }
    let removed = mails
        .iter()
        .filter(|m| m.is_out_time(now, mail_delete_time_hours))
        .map(|m| m.db_id)
        .collect();
    ExpireResult { ran: true, removed }
}

// ────────────────────────── 10001 拉取 ──────────────────────────

/// `MailBox.mailGetReq`（67 B）：**不加锁、不做过期检查**，直接把整箱快照发回去。
#[must_use]
pub fn mail_get_resp(mails: &[MailRecord]) -> MailGetResp10001 {
    MailGetResp10001 {
        ret: RetCode::Ok.as_i32(),
        msg_mail_datas: mails.iter().map(MailRecord::to_msg_mail_data).collect(),
    }
}

// ────────────────────────── 10002 已读 ──────────────────────────

/// 一条邮件事件日志（`EventLogSendRecord.mail`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailLog {
    /// 玩家 id。
    pub player_id: String,
    /// 动作名，原版固定为 `"邮件领取"`（10002 与 10003 都用这个文案）。
    pub action: String,
    /// 标题。
    pub title: String,
    /// 正文。
    pub content: String,
    /// 附件；已读路径恒为 `None`。
    pub rewards: Option<Vec<Reward>>,
    /// `sendTime` 格式化后的字符串。
    pub send_time: String,
}

/// `MailBox.mailReadReq` 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadResult {
    /// 本次真正从未读变已读的邮件 id。
    pub newly_read: Vec<i64>,
    /// 需要落的事件日志。
    pub logs: Vec<MailLog>,
}

/// `MailBox.mailReadReq(Player, List<Long>)`（152 B）。
///
/// 逐封：不存在 → 跳过；已读 → 跳过；否则标记已读。
/// **只有 `annexs == null` 时才记日志**（字节码 72：`getAnnexs() != null` 就跳到循环尾），
/// 也就是说「有附件的邮件被阅读」不产生事件日志，反而是无附件邮件会记一条 `邮件领取`。
#[must_use]
pub fn mail_read_plan(
    player_id: &str,
    ids: &[i64],
    mails: &[MailRecord],
    format_send_time: impl Fn(i64) -> String,
) -> ReadResult {
    let mut out = ReadResult::default();
    for id in ids {
        let Some(m) = mails.iter().find(|m| m.db_id == *id) else {
            continue;
        };
        if m.read {
            continue;
        }
        out.newly_read.push(*id);
        if m.annexs.is_none() {
            out.logs.push(MailLog {
                player_id: player_id.to_owned(),
                action: "邮件领取".to_owned(),
                title: m.title.clone().unwrap_or_default(),
                content: m.content.clone().unwrap_or_default(),
                rewards: None,
                send_time: format_send_time(m.send_time),
            });
        }
    }
    out
}

/// `MailBox.mailReadReq` 的回包：恒 `ret=0`。
#[must_use]
pub fn mail_read_resp() -> i32 {
    RetCode::Ok.as_i32()
}

// ────────────────────────── 10004 删除 ──────────────────────────

/// `MailBox.mailRemoveReq` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveResult {
    /// 被删除的邮件 id。
    pub removed: Vec<i64>,
    /// 删除后的整箱快照（原版回包带的是**剩余全部**邮件，不是被删的那几封）。
    pub remaining: Vec<MailRecord>,
}

/// `MailBox.mailRemoveReq(Player, List<Long>)`（165 B）。
#[must_use]
pub fn mail_remove_plan(ids: &[i64], mails: &[MailRecord]) -> RemoveResult {
    let mut removed = Vec::new();
    let mut remaining = Vec::new();
    for m in mails {
        if ids.contains(&m.db_id) {
            removed.push(m.db_id);
        } else {
            remaining.push(m.clone());
        }
    }
    RemoveResult { removed, remaining }
}

/// `MailBox.mailRemoveReq` 的回包：剩余邮件快照 + `ret=0`。
#[must_use]
pub fn mail_remove_resp(result: &RemoveResult) -> MailRemoveResp10004 {
    MailRemoveResp10004 {
        ret: RetCode::Ok.as_i32(),
        msg_mail_datas: result
            .remaining
            .iter()
            .map(MailRecord::to_msg_mail_data)
            .collect(),
    }
}

// ────────────────────────── 10003 领奖 ──────────────────────────

/// `MailBox.mailReceiveReq` 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReceiveResult {
    /// 回包 `ret`：空请求 `2`；一封都没领到 `301`；否则 `0`。
    pub ret: i32,
    /// 本次成功领取的邮件 id。
    pub claimed: Vec<i64>,
    /// 合并后的奖励。
    pub rewards: Vec<Reward>,
    /// 事件日志。
    pub logs: Vec<MailLog>,
}

/// `MailBox.mailReceiveReq(Player, List<Long>)`（430 B）。
///
/// 关键行为：
/// 1. `ids` 为空 → `ret = COMMON_SERVER_BUSY(2)`（**不是参数错误**）。
/// 2. 附件来源优先模板：`MailTemplateDB.getRewardJson()` 非空就用它，否则回退 `Mail.getAnnexs()`。
/// 3. `checkAddResourcesJsons` 返回非 null（装不下）→ **静默跳过**，不报错也不领取。
/// 4. 顺序是「先 `setReceive()` 再 `addResourcesJsons()`」，两者之间**已经解锁** ——
///    原版就是这样：发放失败时邮件已被标记为已领，奖励永久丢失。此处按原样复刻。
/// 5. 一封都没领到 → `ret = MAIL_ERROR_1(301)`。
/// 6. 成功则回 `mergeResourcesJson(累计奖励)`。
#[must_use]
pub fn mail_receive_plan(
    player_id: &str,
    ids: &[i64],
    mails: &[MailRecord],
    template: impl Fn(i32) -> Option<MailTemplate>,
    check_add: impl Fn(&[Reward]) -> bool,
    format_send_time: impl Fn(i64) -> String,
) -> ReceiveResult {
    if ids.is_empty() {
        return ReceiveResult {
            ret: RetCode::ServerBusy.as_i32(),
            ..ReceiveResult::default()
        };
    }

    let mut claimed = Vec::new();
    let mut granted: Vec<Reward> = Vec::new();
    let mut logs = Vec::new();

    for id in ids {
        let Some(m) = mails.iter().find(|m| m.db_id == *id) else {
            continue;
        };
        let tpl = template(m.template_id);
        // 附件：模板 rewardJson 非空优先，否则回退实例 annexs（null 或空都算没有）。
        let annex_owned;
        let annex = match tpl.as_ref() {
            Some(t) if !t.reward.is_empty() => t.reward.as_slice(),
            _ => match &m.annexs {
                Some(v) if !v.is_empty() => {
                    annex_owned = v.clone();
                    annex_owned.as_slice()
                }
                _ => continue,
            },
        };
        if m.receive {
            continue;
        }
        if !check_add(annex) {
            continue;
        }
        claimed.push(*id);

        // 事件日志：模板存在就用模板的标题/正文，否则用邮件实例的。
        let (title, content) = match &tpl {
            Some(t) => (t.title.clone(), t.content.clone()),
            None => (
                m.title.clone().unwrap_or_default(),
                m.content.clone().unwrap_or_default(),
            ),
        };
        logs.push(MailLog {
            player_id: player_id.to_owned(),
            action: "邮件领取".to_owned(),
            title,
            content,
            rewards: Some(annex.to_vec()),
            send_time: format_send_time(m.send_time),
        });
        granted.extend_from_slice(annex);
    }

    if claimed.is_empty() {
        return ReceiveResult {
            ret: RetCode::MailError1.as_i32(),
            claimed,
            rewards: Vec::new(),
            logs,
        };
    }
    ReceiveResult {
        ret: RetCode::Ok.as_i32(),
        claimed,
        rewards: merge_rewards(&granted),
        logs,
    }
}

/// `ResourcesJson.mergeResourcesJson`：同 id 数量相加，保留首次出现顺序。
#[must_use]
pub fn merge_rewards(items: &[Reward]) -> Vec<Reward> {
    let mut out: Vec<Reward> = Vec::new();
    for (id, num) in items {
        if let Some(slot) = out.iter_mut().find(|(x, _)| x == id) {
            slot.1 += num;
        } else {
            out.push((*id, *num));
        }
    }
    out
}

/// `MailBox.mailReceiveReq` 的回包。
#[must_use]
pub fn mail_receive_resp(result: &ReceiveResult) -> MailReceiveResp10003 {
    MailReceiveResp10003 {
        ret: result.ret,
        annexs: result
            .rewards
            .iter()
            .map(|(id, num)| msg_resources(*id, *num))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn fmt_time(ms: i64) -> String {
        format!("t{ms}")
    }

    fn mail(id: i64, annexs: Option<Vec<Reward>>) -> MailRecord {
        MailRecord {
            db_id: id,
            template_id: 0,
            mail_type: 1,
            send_time: 1_000,
            out_time: 0,
            title: Some(format!("title{id}")),
            content: Some(format!("body{id}")),
            annexs,
            read: false,
            receive: false,
            from_player_id: None,
            from_player_name: None,
            fiters: None,
        }
    }

    #[test]
    fn out_time_prefers_explicit_deadline() {
        let mut m = mail(1, None);
        m.out_time = 5_000;
        assert!(!m.is_out_time(4_999, 1));
        assert!(m.is_out_time(5_000, 1), "相等即过期");
    }

    #[test]
    fn out_time_falls_back_to_delete_hours() {
        let m = mail(1, None); // send_time = 1000
        assert!(!m.is_out_time(1_000 + 3_600_000, 1), "恰好 1 小时不算过期");
        assert!(m.is_out_time(1_000 + 3_600_001, 1));
        assert!(
            !m.is_out_time(1_000 + 3_600_001, 2),
            "改成 2 小时就还没过期"
        );
    }

    #[test]
    fn expire_is_throttled_for_60_seconds() {
        let mails = vec![mail(1, None)];
        let a = expire_mails(1_000_000, 1_000_000 - 60_000, 1, &mails);
        assert!(!a.ran);
        assert!(a.removed.is_empty());
        let b = expire_mails(1_000_000, 1_000_000 - 60_001, 1, &mails);
        assert!(b.ran);
    }

    #[test]
    fn expire_collects_only_outdated() {
        let now = 1_000_000 + 7_200_001;
        let mut fresh = mail(1, None);
        fresh.send_time = now - 1_000; // 刚发出 1 秒
        let mut old = mail(2, None);
        old.send_time = 0; // 早就超过 1 小时
        let r = expire_mails(now, 0, 1, &[fresh, old]);
        assert_eq!(r.removed, vec![2]);
    }

    #[test]
    fn get_resp_is_full_snapshot_with_ret_zero() {
        let r = mail_get_resp(&[mail(1, None), mail(2, Some(vec![(5, 1)]))]);
        assert_eq!(r.ret, 0);
        assert_eq!(r.msg_mail_datas.len(), 2);
        assert_eq!(r.msg_mail_datas[0].id.as_deref(), Some("1"));
    }

    #[test]
    fn msg_mail_data_uses_empty_string_for_nulls() {
        let d = mail(7, Some(vec![(9, 3)])).to_msg_mail_data();
        assert_eq!(d.from_player_id.as_deref(), Some(""));
        assert_eq!(d.from_player_name.as_deref(), Some(""));
        assert_eq!(d.out_time, None, "outTime<=0 不 set");
        assert_eq!(d.annexs.len(), 1);
        assert_eq!(d.annexs[0].num.as_deref(), Some("3"));
    }

    #[test]
    fn msg_mail_data_pads_null_fiters() {
        let mut m = mail(1, None);
        m.fiters = Some(vec![Some("a".into()), None]);
        let d = m.to_msg_mail_data();
        assert_eq!(d.fiters, vec!["a".to_string(), String::new()]);
    }

    #[test]
    fn read_marks_unread_and_skips_known() {
        let mut already = mail(2, None);
        already.read = true;
        let mails = vec![mail(1, None), already];
        let r = mail_read_plan("p", &[1, 2, 99], &mails, fmt_time);
        assert_eq!(r.newly_read, vec![1]);
    }

    #[test]
    fn read_logs_only_when_annexs_is_null() {
        let with_annex = mail(1, Some(vec![(5, 1)]));
        let without = mail(2, None);
        let empty = mail(3, Some(vec![]));
        let r = mail_read_plan("p", &[1, 2, 3], &[with_annex, without, empty], fmt_time);
        assert_eq!(r.newly_read, vec![1, 2, 3], "三封都应置为已读");
        assert_eq!(r.logs.len(), 1, "只有 annexs==null 的那封记日志");
        assert_eq!(r.logs[0].title, "title2");
        assert_eq!(r.logs[0].action, "邮件领取");
        assert_eq!(r.logs[0].rewards, None);
    }

    #[test]
    fn read_never_touches_already_read_for_logs() {
        let mut m = mail(1, None);
        m.read = true;
        let r = mail_read_plan("p", &[1], &[m], fmt_time);
        assert!(r.newly_read.is_empty());
        assert!(r.logs.is_empty());
    }

    #[test]
    fn remove_returns_remaining_not_removed() {
        let mails = vec![mail(1, None), mail(2, None), mail(3, None)];
        let r = mail_remove_plan(&[1, 3, 42], &mails);
        assert_eq!(r.removed, vec![1, 3]);
        assert_eq!(r.remaining.len(), 1);
        assert_eq!(r.remaining[0].db_id, 2);
        let resp = mail_remove_resp(&r);
        assert_eq!(resp.ret, 0);
        assert_eq!(resp.msg_mail_datas.len(), 1);
    }

    #[test]
    fn receive_empty_ids_is_busy_not_param_error() {
        let r = mail_receive_plan("p", &[], &[], |_| None, |_| true, fmt_time);
        assert_eq!(r.ret, 2);
        assert!(r.claimed.is_empty());
    }

    #[test]
    fn receive_nothing_claimable_is_mail_error_1() {
        let m = mail(1, Some(vec![(5, 1)]));
        let r = mail_receive_plan("p", &[99], &[m], |_| None, |_| true, fmt_time);
        assert_eq!(r.ret, 301);
    }

    #[test]
    fn receive_prefers_template_reward_over_instance() {
        let m = mail(1, Some(vec![(5, 1)]));
        let tpl: HashMap<i32, MailTemplate> = HashMap::from([(
            0,
            MailTemplate {
                reward: vec![(7, 9)],
                title: "tpl-t".into(),
                content: "tpl-c".into(),
            },
        )]);
        let r = mail_receive_plan(
            "p",
            &[1],
            &[m],
            |id| tpl.get(&id).cloned(),
            |_| true,
            fmt_time,
        );
        assert_eq!(r.ret, 0);
        assert_eq!(r.rewards, vec![(7, 9)], "模板奖励覆盖实例附件");
    }

    #[test]
    fn receive_falls_back_when_template_reward_empty() {
        let m = mail(1, Some(vec![(5, 1)]));
        let empty_tpl = MailTemplate {
            reward: vec![],
            title: "t".into(),
            content: "c".into(),
        };
        let r = mail_receive_plan(
            "p",
            &[1],
            &[m],
            |_| Some(empty_tpl.clone()),
            |_| true,
            fmt_time,
        );
        assert_eq!(r.rewards, vec![(5, 1)]);
        assert_eq!(r.logs[0].title, "t", "日志用模板标题");
    }

    #[test]
    fn receive_skips_when_check_add_fails_without_error() {
        let m = mail(1, Some(vec![(5, 1)]));
        let r = mail_receive_plan("p", &[1], &[m], |_| None, |_| false, fmt_time);
        assert_eq!(r.claimed, Vec::<i64>::new());
        assert_eq!(r.ret, 301, "装不下等同于没领到");
    }

    #[test]
    fn receive_skips_already_received() {
        let mut m = mail(1, Some(vec![(5, 1)]));
        m.receive = true;
        let r = mail_receive_plan("p", &[1], &[m], |_| None, |_| true, fmt_time);
        assert_eq!(r.ret, 301);
    }

    #[test]
    fn receive_merges_multiple_mails() {
        let a = mail(1, Some(vec![(5, 1), (6, 2)]));
        let b = mail(2, Some(vec![(5, 3)]));
        let r = mail_receive_plan("p", &[1, 2], &[a, b], |_| None, |_| true, fmt_time);
        assert_eq!(r.ret, 0);
        assert_eq!(r.claimed, vec![1, 2]);
        assert_eq!(r.rewards, vec![(5, 4), (6, 2)], "同 id 相加，保留首见顺序");
        assert_eq!(r.logs.len(), 2);
    }

    #[test]
    fn receive_resp_maps_rewards_to_msg() {
        let a = mail(1, Some(vec![(5, 2)]));
        let r = mail_receive_plan("p", &[1], &[a], |_| None, |_| true, fmt_time);
        let resp = mail_receive_resp(&r);
        assert_eq!(resp.ret, 0);
        assert_eq!(resp.annexs.len(), 1);
        assert_eq!(resp.annexs[0].id, Some(5));
        assert_eq!(resp.annexs[0].num.as_deref(), Some("2"));
    }

    #[test]
    fn merge_keeps_first_seen_order() {
        assert_eq!(
            merge_rewards(&[(9, 1), (8, 2), (9, 3)]),
            vec![(9, 4), (8, 2)]
        );
    }
}
