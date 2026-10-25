//! 1016 福利领奖 + 1101 点击图标日志 + 1102 客户端错误上报。
//!
//! ## 三条的共同点
//! 都是「记一行日志 / 提交一个异步任务」，**成功后不回包**（原版 `return void`，
//! 客户端不等待响应）。只有异常分支才会发 `ErrorResp`。
//!
//! ## 原版的两处怪异（保留原样，不修）
//! 1. `playerClickIcon` 与 `playerClientError` 的 `catch` 里回的是
//!    **`PlayerUseGiftKeyResp_1009`**，不是自己的 Resp —— 复制粘贴留下的 bug。
//! 2. `playerClickIcon` 的入参 `reason/iconId/iconName` 直接从 req 读出，
//!    **不做任何校验**，空串也照记。

use std::sync::Arc;

use ddj_core::Clock;
use ddj_pb::gen::player_message::{
    ClientErrorSendReq1102, PlayerIconLogReq1101, PlayerUseGiftKeyResp1009, WealRewardReq1016,
    WealRewardResp1016,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::runtime::{decode_err, FnHandler, MemWorld};

/// `ErrorResp_1` 的消息号（原版 `HandlerException` 出口）。
const ERROR_RESP_CMD: i32 = 1;

/// `LanguageUtils.PLAYER_ERROR_8` 的值（实测 `<clinit>`）。
const PLAYER_ERROR_8: i32 = 108;

/// 注册 3 条消息。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, _clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 1016 福利领奖：已领过 → 抛 PLAYER_ERROR_8。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(1016, move |ctx, body| {
            let _req = WealRewardReq1016::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let status = w.peek(p, |b| b.weal_reward_status);
            if status != 0 {
                // 原版 `throw new HandlerException(PLAYER_ERROR_8)` → 框架出口 ErrorResp_1。
                return Ok(Some(Reply::with_cmd(
                    ERROR_RESP_CMD,
                    crate::sign::error_resp(1016, ddj_pb::cmd::RetCode::from_i32(PLAYER_ERROR_8))
                        .encode_to_vec(),
                )));
            }
            // 原版提交 `WealReward` 异步任务后直接 `ret=0`。
            w.with(p, |b| b.weal_reward_status = 1);
            Ok(Some(Reply::same(
                WealRewardResp1016 { ret: 0 }.encode_to_vec(),
            )))
        })));
        n += 1;
    }

    // 1101 点击图标：只记 eventlog，成功后不回包。
    {
        table.insert(Box::new(FnHandler::new(1101, move |_ctx, body| {
            let req = PlayerIconLogReq1101::decode(body).map_err(decode_err)?;
            tracing::info!(
                reason = req.reason.as_deref().unwrap_or(""),
                icon_id = req.icon_id.as_deref().unwrap_or(""),
                icon_name = req.icon_name.as_deref().unwrap_or(""),
                "客户端图标点击"
            );
            Ok(None)
        })));
        n += 1;
    }

    // 1102 客户端错误上报：只记 eventlog，成功后不回包。
    {
        table.insert(Box::new(FnHandler::new(1102, move |_ctx, body| {
            let req = ClientErrorSendReq1102::decode(body).map_err(decode_err)?;
            tracing::warn!(
                error_info = req.error_info.as_deref().unwrap_or(""),
                "客户端上报错误"
            );
            Ok(None)
        })));
        n += 1;
    }

    n
}

/// 异常出口用的 `PlayerUseGiftKeyResp_1009`（原版 1101/1102 的 catch 里就是它）。
#[must_use]
pub fn busy_as_1009() -> Vec<u8> {
    PlayerUseGiftKeyResp1009 {
        ret: ddj_pb::cmd::RetCode::ServerBusy.as_i32(),
    }
    .encode_to_vec()
}
