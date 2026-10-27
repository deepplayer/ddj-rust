//! 二级密码 17351-17355 的接线。
//!
//! 纯逻辑在 [`crate::second_psw`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## ⚠ 17352 是唯一失败也走 Resp 的一条
//! 其他四条 `throws HandlerException` → 失败走 `ErrorResp_1`（帧号 1）。
//! `verifyPsw` 自己 catch 住并转成 `MsgVerifyPswResp_17352{ret=code}`，
//! 所以帧号仍是 **17352**。实测已验证。
//!
//! ## 状态存放
//! `SecondPsw` 原版挂在 `SysActivitySet` 下的 `TYPE_SECOND_PSW` 模型里。
//! 过渡期存在 `MemWorld.second_psw`。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::sys_activity_message::{
    MsgApplyResetPswReq17354, MsgApplyResetPswResp17354, MsgQuerySecondPswDataReq17355,
    MsgQuerySecondPswDataResp17355, MsgSecondPsw, MsgSetEnabledPswReq17353,
    MsgSetEnabledPswResp17353, MsgSetPswReq17351, MsgSetPswResp17351, MsgVerifyPswReq17352,
    MsgVerifyPswResp17352,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::runtime::{decode_err, FnHandler, MemWorld};
use crate::second_psw::{
    self, PswEffect, PswOutcome, SecondPswConfig, SecondPswState,
};

/// `ErrorResp_1` 的帧消息号。
const ERROR_RESP_CMD: i32 = 1;

static CONFIG: OnceLock<SecondPswConfig> = OnceLock::new();

/// 注入二级密码配置（只第一次生效）。
pub fn install_config(c: SecondPswConfig) {
    let _ = CONFIG.set(c);
}

fn cfg() -> SecondPswConfig {
    CONFIG.get().copied().unwrap_or_default()
}

/// 失败 → `ErrorResp_1`。
fn reject_reply(cmd: i32, code: ddj_pb::RetCode) -> Reply {
    Reply::with_cmd(
        ERROR_RESP_CMD,
        crate::sign::error_resp(cmd, code).encode_to_vec(),
    )
}

/// 注册 5 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 17351 设置/修改密码
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(17351, move |ctx, body| {
            let req = MsgSetPswReq17351::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let state = w.peek(p, |b| b.second_psw.clone());
            let psw = req.new_psw.as_deref();
            match second_psw::set_psw_verdict(&state, psw, now, &cfg()) {
                PswOutcome::Reject(code) => Ok(Some(reject_reply(17351, code))),
                PswOutcome::Accept(PswEffect::SetPsw { psw, first_set }) => {
                    let enabled = if first_set { true } else { state.enabled };
                    w.with(p, |b| {
                        b.second_psw.psw = Some(psw);
                        b.second_psw.enabled = enabled;
                    });
                    Ok(Some(
                        MsgSetPswResp17351 {
                            ret: 0,
                            have_set_psw: Some(true),
                            enabled: Some(enabled),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
                // 其它 effect 不该出现在 17351
                PswOutcome::Accept(_) | PswOutcome::RespRet(_) => Ok(Some(reject_reply(
                    17351,
                    ddj_pb::cmd::RetCode::ServerBusy,
                ))),
            }
        })));
        n += 1;
    }

    // 17352 校验密码（⚠ 失败也走 Resp，帧号 17352）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17352, move |ctx, body| {
            let req = MsgVerifyPswReq17352::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let psw = req.psw.clone().unwrap_or_default();
            let state = w.peek(p, |b| b.second_psw.clone());
            let out = second_psw::verify_psw_verdict(&state, &psw, &cfg());
            match out {
                PswOutcome::Accept(PswEffect::VerifyOk) => {
                    w.with(p, |b| {
                        b.second_psw.is_verify_success = true;
                        b.second_psw.input_error_num = 0;
                    });
                    Ok(Some(
                        MsgVerifyPswResp17352 {
                            ret: 0,
                            input_error_num: Some(0),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
                // 失败：走 Resp 里的 ret，帧号仍是 17352
                PswOutcome::RespRet(code) => {
                    // 密码错误要 +1 错误次数（原版在 throw 之前就 save 了）
                    let mut err = state.input_error_num;
                    if code == ddj_pb::cmd::RetCode::SecondPswPswError {
                        err += 1;
                        w.with(p, |b| b.second_psw.input_error_num = err);
                    }
                    Ok(Some(
                        MsgVerifyPswResp17352 {
                            ret: code.as_i32(),
                            input_error_num: Some(err),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
                PswOutcome::Reject(code) => Ok(Some(reject_reply(17352, code))),
                PswOutcome::Accept(_) => Ok(Some(reject_reply(
                    17352,
                    ddj_pb::cmd::RetCode::ServerBusy,
                ))),
            }
        })));
        n += 1;
    }

    // 17353 开关二级密码
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17353, move |ctx, body| {
            let req = MsgSetEnabledPswReq17353::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let enabled = req.enabled.unwrap_or(false);
            let state = w.peek(p, |b| b.second_psw.clone());
            match second_psw::set_enabled_verdict(&state, enabled) {
                PswOutcome::Reject(code) => Ok(Some(reject_reply(17353, code))),
                PswOutcome::Accept(PswEffect::SetEnabled { enabled }) => {
                    w.with(p, |b| b.second_psw.enabled = enabled);
                    Ok(Some(
                        MsgSetEnabledPswResp17353 {
                            ret: 0,
                            enabled: Some(enabled),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
                PswOutcome::Accept(_) | PswOutcome::RespRet(_) => Ok(Some(reject_reply(
                    17353,
                    ddj_pb::cmd::RetCode::ServerBusy,
                ))),
            }
        })));
        n += 1;
    }

    // 17354 申请/取消重置（按 req.type 分）
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(17354, move |ctx, body| {
            let req = MsgApplyResetPswReq17354::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let ty = req.r#type.unwrap_or(0);
            let state = w.peek(p, |b| b.second_psw.clone());
            // 原版：type == 1 走 apply，否则走 cancel
            let out = if ty == 1 {
                second_psw::apply_reset_verdict(&state, now, &cfg())
            } else {
                second_psw::cancel_reset_verdict(&state, now)
            };
            match out {
                PswOutcome::Reject(code) => Ok(Some(reject_reply(17354, code))),
                PswOutcome::Accept(PswEffect::ApplyReset { end_time }) => {
                    w.with(p, |b| b.second_psw.apply_reset_end_time = end_time);
                    Ok(Some(
                        MsgApplyResetPswResp17354 {
                            ret: 0,
                            r#type: Some(1),
                            reset_end_time: Some(end_time),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
                PswOutcome::Accept(PswEffect::CancelReset) => {
                    w.with(p, |b| b.second_psw.apply_reset_end_time = 0);
                    Ok(Some(
                        MsgApplyResetPswResp17354 {
                            ret: 0,
                            r#type: Some(2),
                            reset_end_time: Some(0),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
                PswOutcome::Accept(_) | PswOutcome::RespRet(_) => Ok(Some(reject_reply(
                    17354,
                    ddj_pb::cmd::RetCode::ServerBusy,
                ))),
            }
        })));
        n += 1;
    }

    // 17355 查询状态
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17355, move |ctx, body| {
            let _req = MsgQuerySecondPswDataReq17355::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let state: SecondPswState = w.peek(p, |b| b.second_psw.clone());
            let snap = state.to_snapshot();
            Ok(Some(
                MsgQuerySecondPswDataResp17355 {
                    ret: 0,
                    data: Some(MsgSecondPsw {
                        have_set_psw: Some(snap.have_set_psw),
                        enabled: Some(snap.enabled),
                        input_error_num: Some(snap.input_error_num),
                        reset_end_time: Some(snap.reset_end_time),
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
