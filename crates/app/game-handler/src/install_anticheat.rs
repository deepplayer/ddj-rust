//! 防骗手册 17151-17153 的接线。
//!
//! 纯逻辑在 [`crate::anticheat`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## 三条都不走 `ErrorResp_1`
//! 17151 把 `ret` 放在 `AntiCheatRewardResp_17151` 里，
//! 17152/17153 恒成功。所以**帧号永远是请求号**。
//!
//! ## 两处「不发包」
//! - 17152 的 `!canReward(index)` → 原版直接 `return`，客户端超时
//! - 17151 的外层 `catch (Exception)` → 只记日志，同样不发包
//!
//! 都照抄。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::sys_activity_message::{
    AntiCheatInfo, AntiCheatReadReq17152, AntiCheatReadResp17152, AntiCheatReq17153,
    AntiCheatResp17153, AntiCheatRewardReq17151, AntiCheatRewardResp17151,
};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::anticheat::{self, AntiCheatReadOutcome, AntiCheatRewardConfig};
use crate::runtime::{decode_err, FnHandler, MemWorld};

static CONFIG: OnceLock<AntiCheatRewardConfig> = OnceLock::new();

/// 注入防骗手册奖励配置（只第一次生效）。
pub fn install_config(c: AntiCheatRewardConfig) {
    let _ = CONFIG.set(c);
}

fn cfg() -> AntiCheatRewardConfig {
    CONFIG.get().cloned().unwrap_or_default()
}

/// 注册 3 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, _clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 17151 领奖
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17151, move |ctx, body| {
            let req = AntiCheatRewardReq17151::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let index = req.index.unwrap_or(0);
            let state = w.peek(p, |b| b.anticheat.clone());
            let out = anticheat::reward_verdict(&state, &cfg(), index);
            match out {
                anticheat::AntiCheatRewardOutcome::Reject(code) => Ok(Some(
                    AntiCheatRewardResp17151 {
                        ret: code.as_i32(),
                        index: Some(index),
                    }
                    .encode_to_vec()
                    .into(),
                )),
                anticheat::AntiCheatRewardOutcome::Accept { rewards } => {
                    w.with(p, |b| {
                        if let Some(next) = b.anticheat.add_reward(index) {
                            b.anticheat = next;
                        }
                        // `addResourcesJsons(rewards, TYPE_1801)`
                        for (id, num) in &rewards {
                            *b.bag.entry(*id).or_insert(0) += num;
                        }
                    });
                    Ok(Some(
                        AntiCheatRewardResp17151 {
                            ret: 0,
                            index: Some(index),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    // 17152 标记已读（越界时**不发包**）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17152, move |ctx, body| {
            let req = AntiCheatReadReq17152::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let index = req.index.unwrap_or(0);
            match anticheat::read_verdict(&cfg(), index) {
                // ⚠ 原版直接 return，不发包（客户端超时）
                AntiCheatReadOutcome::Silent => Ok(None),
                AntiCheatReadOutcome::Accept => {
                    w.with(p, |b| {
                        b.anticheat = b.anticheat.add_read(index);
                    });
                    Ok(Some(
                        AntiCheatReadResp17152 { ret: Some(0) }
                            .encode_to_vec()
                            .into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    // 17153 查询状态
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17153, move |ctx, body| {
            let _req = AntiCheatReq17153::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let info = w.peek(p, |b| b.anticheat.to_info());
            Ok(Some(
                AntiCheatResp17153 {
                    info: Some(AntiCheatInfo {
                        anti_cheat_read: info.read,
                        anti_cheat_reward: info.reward,
                    }),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    n
}
