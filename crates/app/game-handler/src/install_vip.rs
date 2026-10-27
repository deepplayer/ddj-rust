//! 1012 VVIP 微信奖励 的接线。
//!
//! 纯逻辑在 [`crate::vip`]，本文件只做解码 → 调纯函数 → 发邮件 → 编码回包。
//!
//! ## 原版依据
//! - `PlayerMessageHandler.sendVVIP_RewardReq_1012`（行 251-262）
//! - `Player.sendVVIP_RewardResp_1012` / `sendVvipRewardAug`
//!
//! ## 过渡期的两处假定
//! 1. **`VipServerModule.getServerVip()`**：原版查 `GsServerVip` 表（全服唯一一行）。
//!    过渡期用 [`VvipConfig::server_vip_exists`] 注入，默认 `false`
//!    （等价「表里没数据」→ 首月分支回 BUSY，补发分支回 0）。
//! 2. **`PlayerStatServerModule.getLongStat(VipWeChatAdd)`**：
//!    原版是玩家统计表的一列。过渡期存在 `MemWorld.key_values` 里
//!    （key 用 [`crate::vip::STAT_VIP_WECHAT_ADD`] 的负数偏移，避免与
//!    `TypePlayerCacheKey` 的 1/2 冲突）。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::player_message::{
    PlayerAdRewardReq1104, PlayerAdRewardResp1104, SendVvipRewardReq1012, SendVvipRewardResp1012,
};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::mail::MailRecord;
use crate::runtime::{decode_err, FnHandler, MemWorld};
use crate::runtime::resources_to_msg;
use crate::vip::{self, AdRewardCtx, AdRewardOutcome, VvipCtx, VvipOutcome};

/// 全服 VVIP 配置（对应 `GsServerVip` 表）。
#[derive(Debug, Clone, Copy, Default)]
pub struct VvipConfig {
    /// `VipServerModule.getServerVip() != null`。
    pub server_vip_exists: bool,
}

static CONFIG: OnceLock<Arc<VvipConfig>> = OnceLock::new();

/// 注入全服 VVIP 配置（只第一次生效）。
pub fn install_config(c: VvipConfig) {
    let _ = CONFIG.set(Arc::new(c));
}

fn config() -> VvipConfig {
    CONFIG.get().map_or(VvipConfig::default(), |c| **c)
}

/// 注册 2 条消息（1012 VVIP / 1104 广告），返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 1104 广告奖励
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(1104, move |ctx, body| {
            let _req = PlayerAdRewardReq1104::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let times = w.peek(p, |b| b.ad_reward_times);
            let tpl = ad_templates();
            let actx = AdRewardCtx {
                ad_reward_times: times,
                max_times: tpl.max_times,
                awards: tpl.awards.clone(),
            };
            match vip::ad_reward_verdict(&actx) {
                AdRewardOutcome::LimitReached => Ok(Some(
                    PlayerAdRewardResp1104 {
                        ret: AdRewardOutcome::LimitReached.resolve_ret(),
                        ad_reward_times: None,
                        reward: Vec::new(),
                    }
                    .encode_to_vec()
                    .into(),
                )),
                AdRewardOutcome::Accept { new_times, rewards } => {
                    // `addResourcesJsons(AdvertisingAwards, TYPE_1014)`
                    let gain = rewards.clone();
                    w.with(p, |b| {
                        b.ad_reward_times = new_times;
                        b.ad_reward_day = now;
                        for (id, num) in &gain {
                            *b.bag.entry(*id).or_insert(0) += num;
                        }
                    });
                    Ok(Some(
                        PlayerAdRewardResp1104 {
                            ret: 0,
                            ad_reward_times: Some(new_times),
                            reward: resources_to_msg(&rewards),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    let (w, c) = (world, clock);
    table.insert(Box::new(FnHandler::new(1012, move |ctx, body| {
        let _req = SendVvipRewardReq1012::decode(body).map_err(decode_err)?;
        let p = ctx.require_player()?;
        let now = c.now_millis();

        // 读 `getLongStat(VipWeChatAdd)` 与 `getVvipTakeRewardTime()`
        let (add_time, take_time) = w.peek(p, |b| {
            (
                b.key_values
                    .get(&vip::STAT_VIP_WECHAT_ADD)
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0),
                b.vvip_take_reward_time,
            )
        });

        let vctx = VvipCtx {
            server_vip_exists: config().server_vip_exists,
            vip_wechat_add_time: add_time,
            vvip_take_reward_time: take_time,
            now,
        };
        let out = vip::handle_1012(&vctx);

        match out {
            VvipOutcome::Resp(code) => Ok(Some(
                SendVvipRewardResp1012 { ret: code.as_i32() }
                    .encode_to_vec()
                    .into(),
            )),
            VvipOutcome::Accept {
                mail_count,
                stamp_take_time,
                stamp_wechat_add,
            } => {
                let mails: Vec<MailRecord> = (0..mail_count)
                    .map(|_| MailRecord {
                        db_id: next_mail_id(),
                        template_id: vip::VVIP_MAIL_TEMPLATE_ID,
                        // `TypeMail.SYS.getType()`
                        mail_type: 1,
                        send_time: now,
                        out_time: 0,
                        title: None,
                        content: None,
                        annexs: Some(Vec::new()),
                        read: false,
                        receive: false,
                        from_player_id: None,
                        from_player_name: None,
                        fiters: None,
                    })
                    .collect();
                w.with(p, |b| {
                    if stamp_wechat_add {
                        b.key_values
                            .insert(vip::STAT_VIP_WECHAT_ADD, now.to_string());
                    }
                    if stamp_take_time {
                        b.vvip_take_reward_time = now;
                    }
                    b.mails.extend(mails);
                });
                Ok(Some(
                    SendVvipRewardResp1012 { ret: 0 }.encode_to_vec().into(),
                ))
            }
        }
    })));
    n += 1;
    n
}

/// 广告奖励模板（`ConfigDBCache.AdvertisingTimes` / `AdvertisingAwards`）。
#[derive(Debug, Clone, Default)]
pub struct AdTemplates {
    /// 每日上限。
    pub max_times: i32,
    /// 奖励列表。
    pub awards: Vec<(i32, i64)>,
}

static AD_TEMPLATES: OnceLock<Arc<AdTemplates>> = OnceLock::new();

/// 注入广告奖励模板（只第一次生效）。
pub fn install_ad_templates(t: AdTemplates) {
    let _ = AD_TEMPLATES.set(Arc::new(t));
}

fn ad_templates() -> Arc<AdTemplates> {
    AD_TEMPLATES
        .get()
        .map_or_else(|| Arc::new(AdTemplates::default()), Arc::clone)
}

/// 过渡期的邮件自增 id。
fn next_mail_id() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static NEXT: AtomicI64 = AtomicI64::new(10_000);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
