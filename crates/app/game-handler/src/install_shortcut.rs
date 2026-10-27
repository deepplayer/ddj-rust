//! 保存桌面 17301 / 17302 / 17303 的接线。
//!
//! 纯逻辑在 [`crate::shortcut`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## 回包差异
//! | cmd | 成功 | 失败 |
//! |---|---|---|
//! | 17301 | `ShortcutDataResp_17301{ret=0, shortcutData}` | 无失败分支 |
//! | 17302 | `ShortcutInstallResp_17302{ret=0, shortcutData}` | `ErrorResp_1`（帧号 **1**）|
//! | 17303 | `ShortcutRewardResp_17303{ret=0, shortcutData, rewards}` | `ErrorResp_1`（帧号 **1**）|
//!
//! 17301 的方法签名**没有** `throws HandlerException`，方法体也没有 `throw`，
//! 所以它恒成功。
//!
//! ## 渠道配置来源
//! `PlatformServerModule.shortcutConfigMap` ← `bf_config.configId=59`。
//! 实测该配置为空串，所以 `eligible` 恒 `false`、17302/17303 恒回 `4731`。
//! 这不是移植 bug，是原版线上数据的真实状态。等运营配了渠道才有意义。
//!
//! ## 奖励来源
//! `ConfigDBCache.SaveIconReward`（`B-变量表-Config.csv:509`，id=801）：
//! `11=1;215=3;216=3;30=1;234=1`。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::sys_activity_message::{
    MsgShortcutData, ShortcutDataReq17301, ShortcutDataResp17301, ShortcutInstallReq17302,
    ShortcutInstallResp17302, ShortcutRewardReq17303, ShortcutRewardResp17303,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::runtime::{decode_err, FnHandler, MemWorld};
use crate::shortcut::{self, InstallOutcome, RewardOutcome, ShortcutConfig};

/// `shortcutConfigMap` 的注入点（`bf_config.configId=59`）。
static CONFIG: OnceLock<ShortcutConfig> = OnceLock::new();

/// 注入渠道白名单（只第一次生效）。
pub fn install_config(c: ShortcutConfig) {
    let _ = CONFIG.set(c);
}

fn cfg() -> &'static ShortcutConfig {
    CONFIG.get_or_init(ShortcutConfig::default)
}

/// `ConfigDBCache.SaveIconReward` 的注入点。
static REWARDS: OnceLock<Vec<(i32, i64)>> = OnceLock::new();

/// 注入保存桌面奖励（只第一次生效）。
pub fn install_rewards(r: Vec<(i32, i64)>) {
    let _ = REWARDS.set(r);
}

fn rewards() -> Vec<(i32, i64)> {
    REWARDS.get().cloned().unwrap_or_default()
}

/// `ErrorResp_1` 自己的消息号（原版 `HandlerException` 出口用它组帧）。
const ERROR_RESP_CMD: i32 = 1;

/// `throw new HandlerException(code)` → `ErrorResp_1{msgId, errorCode}`，帧号 **1**。
fn reject_reply(cmd: i32, code: ddj_pb::RetCode) -> Reply {
    Reply::with_cmd(
        ERROR_RESP_CMD,
        crate::sign::error_resp(cmd, code).encode_to_vec(),
    )
}

fn to_msg_data(d: &shortcut::ShortcutData) -> MsgShortcutData {
    MsgShortcutData {
        channel_id: Some(d.channel_id),
        cid: Some(d.cid.clone()),
        state: Some(d.state),
        eligible: Some(d.eligible),
    }
}

/// 注册 3 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, _clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 17301 查询（恒成功，无 throws）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17301, move |ctx, body| {
            let _req = ShortcutDataReq17301::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let (channel_id, cid, state) = w.peek(p, |b| {
                (
                    b.channel_id,
                    b.cid.clone(),
                    b.shortcut_state,
                )
            });
            let data = shortcut::to_msg(channel_id, &cid, state, cfg());
            Ok(Some(
                ShortcutDataResp17301 {
                    ret: 0,
                    shortcut_data: Some(to_msg_data(&data)),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // 17302 标记已保存（失败走 ErrorResp_1，帧号 1）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17302, move |ctx, body| {
            let _req = ShortcutInstallReq17302::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let (channel_id, cid, state) = w.peek(p, |b| {
                (b.channel_id, b.cid.clone(), b.shortcut_state)
            });
            match shortcut::install_verdict(channel_id, &cid, state, cfg()) {
                InstallOutcome::Reject(code) => Ok(Some(reject_reply(17302, code))),
                InstallOutcome::Accept { state: next } => {
                    w.with(p, |b| b.shortcut_state = next);
                    let data = shortcut::to_msg(channel_id, &cid, next, cfg());
                    Ok(Some(
                        ShortcutInstallResp17302 {
                            ret: 0,
                            shortcut_data: Some(to_msg_data(&data)),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    // 17303 领奖（失败走 ErrorResp_1，帧号 1）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17303, move |ctx, body| {
            let _req = ShortcutRewardReq17303::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let (channel_id, cid, state) = w.peek(p, |b| {
                (b.channel_id, b.cid.clone(), b.shortcut_state)
            });
            match shortcut::reward_verdict(channel_id, &cid, state, cfg()) {
                RewardOutcome::Reject(code) => Ok(Some(reject_reply(17303, code))),
                RewardOutcome::Accept { state: next } => {
                    let r = rewards();
                    w.with(p, |b| {
                        b.shortcut_state = next;
                        for (id, num) in &r {
                            *b.bag.entry(*id).or_insert(0) += num;
                        }
                    });
                    let data = shortcut::to_msg(channel_id, &cid, next, cfg());
                    Ok(Some(
                        ShortcutRewardResp17303 {
                            ret: 0,
                            shortcut_data: Some(to_msg_data(&data)),
                            rewards: crate::runtime::resources_to_msg(&r),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    n
}
