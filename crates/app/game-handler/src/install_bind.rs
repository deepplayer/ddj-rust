//! 1004 设置连接标记 / 1115 绑定奖励 的接线。
//!
//! 纯逻辑在 [`crate::bind_reward`]，本文件只做解码 → 调纯函数 → 写状态 → 编码回包。
//!
//! ## 1004 不回包
//! 原版 `UpdatePlayerVarReq` 返回 `void` 且只调 `channelPlayer.setBor()`，
//! 所以这里返回 `Ok(None)`。**不是漏写**。
//!
//! ## 1115 的邮件
//! 原版 `Mail.createSysCustomMail(...)` + `MailServerModule.sendMail(...)`。
//! 过渡期把邮件直接写进 `MemWorld.mails`（与 `install_mail` 同一容器）。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::player_message::{MsgGainBindRewardReq1115, MsgGainBindRewardResp1115};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::bind_reward::{self, BindRewardCtx, BindRewardOutcome};
use crate::mail::{MailRecord, Reward};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 绑定奖励所需的模板数据（`ConfigDBCache.TwBindReward` / `TwBindHighReward` / `TwBindMail`）。
#[derive(Debug, Clone, Default)]
pub struct BindRewardTemplates {
    /// `TwBindReward`。
    pub base_rewards: Vec<Reward>,
    /// `TwBindHighReward`。
    pub high_rewards: Vec<Reward>,
    /// `TwBindMail`。
    pub mail_template_id: i32,
}

static TEMPLATES: OnceLock<Arc<BindRewardTemplates>> = OnceLock::new();

/// 注入绑定奖励模板（只第一次生效）。
pub fn install_templates(t: BindRewardTemplates) {
    let _ = TEMPLATES.set(Arc::new(t));
}

fn templates() -> Option<&'static Arc<BindRewardTemplates>> {
    TEMPLATES.get()
}

/// 注册 2 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 1004 设置连接标记（无回包）。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(1004, move |ctx, body| {
            // 原版不读 `req.var`，但仍然要解包 —— 解包失败会断开连接。
            let _req = ddj_pb::gen::player_message::UpdatePlayerVarReq1004::decode(body)
                .map_err(decode_err)?;
            let p = ctx.require_player()?;
            bind_reward::apply_set_bor()?;
            w.with(p, |b| b.bor = true);
            // 原版 `return void` → 不回包
            Ok(None)
        })));
        n += 1;
    }

    // 1115 绑定奖励。
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(1115, move |ctx, body| {
            let _req = MsgGainBindRewardReq1115::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();

            let (stored_value, create_time) =
                w.peek(p, |b| (b.key_values.get(&bind_reward::KEY_TW_BIND_ACCOUNT).cloned(), b.create_time));

            let tpl = templates().cloned().unwrap_or_default();
            let bctx = BindRewardCtx {
                stored_value,
                create_time,
                base_rewards: tpl.base_rewards.clone(),
                high_rewards: tpl.high_rewards.clone(),
                mail_template_id: tpl.mail_template_id,
            };
            // 过渡期没有 `MailTemplateDBCache`：只要配了模板 id 就视为存在。
            // 未注入模板（id == 0）时视为缺失 → ret=2，与原版「模板表空」一致。
            let template_exists = tpl.mail_template_id != 0;

            let out = bind_reward::bind_reward_verdict(&bctx, now, template_exists);
            match out {
                BindRewardOutcome::Accept {
                    rewards,
                    mail_template_id,
                } => {
                    // `playerCommon.setKeyValue(TWBindAccount, "1")`
                    let marker = bind_reward::bind_marker_value();
                    let mail = MailRecord {
                        db_id: next_mail_id(),
                        template_id: mail_template_id,
                        // `TypeMail.SYS.getType()`
                        mail_type: 1,
                        send_time: now,
                        out_time: 0,
                        title: None,
                        content: None,
                        annexs: Some(rewards),
                        read: false,
                        receive: false,
                        from_player_id: None,
                        from_player_name: None,
                        fiters: None,
                    };
                    w.with(p, |b| {
                        b.key_values
                            .insert(bind_reward::KEY_TW_BIND_ACCOUNT, marker.to_owned());
                        // `MailServerModule.sendMail` 追加到邮箱
                        b.mails.push(mail);
                    });
                    Ok(Some(
                        MsgGainBindRewardResp1115 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
                // 已领：原版 `HandlerException` → 框架出口 `ErrorResp_1`
                BindRewardOutcome::AlreadyGained => Ok(Some(Reply::with_cmd(
                    1,
                    crate::sign::error_resp_raw(
                        1115,
                        ddj_pb::cmd::RetCode::AppraisalGainAchievement.as_i32(),
                    )
                    .encode_to_vec(),
                ))),
                // 模板缺失：原版 NPE → 非 HandlerException → ErrorResp_1{ret=2}
                BindRewardOutcome::MailTemplateMissing => Ok(Some(Reply::with_cmd(
                    1,
                    crate::sign::error_resp_raw(
                        1115,
                        ddj_pb::cmd::RetCode::ServerBusy.as_i32(),
                    )
                    .encode_to_vec(),
                ))),
            }
        })));
        n += 1;
    }

    n
}

/// 过渡期的邮件自增 id（原版由 `DBMailModule` 发号）。
fn next_mail_id() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static NEXT: AtomicI64 = AtomicI64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
