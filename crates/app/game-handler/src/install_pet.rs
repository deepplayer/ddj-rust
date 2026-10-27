//! 门客皮肤 7023 / 7024 / 7025 的接线。
//!
//! 纯逻辑在 [`crate::pet`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## 三处的回包差异（容易写错）
//! | cmd | 成功回包 | 失败回包 |
//! |---|---|---|
//! | 7023 | `MsgActivatePetSkinResp_7023{ret=0}` | `HandlerException` → **`ErrorResp_1`** |
//! | 7024 | `MsgUpgradePetSkinResp_7024{ret=0}` | 同上 |
//! | 7025 | `MsgSetPetSkinResp_7025{ret=0}` **+ 推 `PetUpdateResp_7001`** | 同上 |
//!
//! 原版 handler 全部 `throws HandlerException`，所以失败走框架出口
//! `ErrorResp_1{msgId, errorCode}`，帧号是 **1** 而不是请求号。
//!
//! ## 7025 的推送
//! `PetSet.setSkin` 末尾 `sendToPlayerId(PetUpdateResp_7001{msgPetData=[toMsgPetData(10)]})`。
//! 这是与回包并列的第二条消息，走 [`SessionControl::push`]。
//!
//! ## 过渡期状态
//! `PetSet` 未移植，这里在 `MemWorld` 里存一个最小 `pet_skins` 表：
//! `pet_id → PetSkinState`。模板走 [`install_templates`] 注入。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::pet_message::{
    MsgActivatePetSkinReq7023, MsgActivatePetSkinResp7023, MsgPetRateLvReplaceReq7029,
    MsgPetRateLvReplaceResp7029, MsgSetPetRateLvReq7026, MsgSetPetRateLvResp7026,
    MsgSetPetSkinReq7025, MsgSetPetSkinResp7025, MsgUpgradePetSkinReq7024,
    MsgUpgradePetSkinResp7024, PetUpdateResp7001,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::pet::{
    self, PetRateCtx, PetRateOutcome, PetRateReplaceCtx, PetRateReplaceOutcome, PetRateTemplate,
    PetSkinTemplate, SkinEffect, SkinOutcome,
};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// `ErrorResp_1` 的帧消息号。
const ERROR_RESP_CMD: i32 = 1;

/// `PetUpdateResp_7001` 的消息号。
const PET_UPDATE_RESP: i32 = 7001;

/// `MsgPetData.skinId` 的字段号（原版 `toMsgPetData(10)` 里的 10）。
const SKIN_FIELD_ID: i32 = 10;

/// 珍兽评级模板表（`PetRateDBCache`）。
pub trait PetRateTemplates: Send + Sync {
    /// `PetRateDBCache.getMaxLv()`。
    fn max_lv(&self) -> i32;
    /// `PetRateDBCache.getPetRateByLv(lv)`。
    fn by_lv(&self, lv: i32) -> Option<PetRateTemplate>;
}

static RATE_TEMPLATES: OnceLock<Arc<dyn PetRateTemplates>> = OnceLock::new();

/// 注入评级模板（只第一次生效）。
pub fn install_rate_templates(t: Arc<dyn PetRateTemplates>) {
    let _ = RATE_TEMPLATES.set(t);
}

fn rate_max_lv() -> i32 {
    RATE_TEMPLATES.get().map_or(0, |t| t.max_lv())
}

fn rate_tpl(lv: i32) -> Option<PetRateTemplate> {
    RATE_TEMPLATES.get().and_then(|t| t.by_lv(lv))
}

/// 皮肤模板表（`PetSkinDBCache`）。
pub trait PetSkinTemplates: Send + Sync {
    /// `PetSkinDBCache.get(skinId)`。
    fn get(&self, skin_id: i32) -> Option<PetSkinTemplate>;
}

static TEMPLATES: OnceLock<Arc<dyn PetSkinTemplates>> = OnceLock::new();

/// 注入皮肤模板（只第一次生效）。
pub fn install_templates(t: Arc<dyn PetSkinTemplates>) {
    let _ = TEMPLATES.set(t);
}

fn template(skin_id: i32) -> Option<PetSkinTemplate> {
    TEMPLATES.get().and_then(|t| t.get(skin_id))
}

/// 把 `SkinOutcome` 的失败分支转成 `ErrorResp_1`。
fn reject_reply(cmd: i32, code: ddj_pb::RetCode) -> Reply {
    Reply::with_cmd(
        ERROR_RESP_CMD,
        crate::sign::error_resp(cmd, code).encode_to_vec(),
    )
}

/// 注册 3 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, _clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 7023 激活皮肤
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(7023, move |ctx, body| {
            let req = MsgActivatePetSkinReq7023::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let pet_id = req.pet_id.unwrap_or(0);
            let skin_id = req.skin_id.unwrap_or(0);
            let tpl = template(skin_id);
            let state = w.peek(p, |b| b.pet_skins.get(&pet_id).cloned());
            // 过渡期不查资源系统，按「付得起」放行；接上 `ResourcesConsumeUtils` 后改这里
            let can_pay = true;
            let out = pet::activate_skin_verdict(state.as_ref(), pet_id, skin_id, tpl.as_ref(), can_pay);
            match out {
                SkinOutcome::Reject(code) => Ok(Some(reject_reply(7023, code))),
                SkinOutcome::Accept(SkinEffect::Activate {
                    skin_id, growth, ..
                }) => {
                    w.with(p, |b| {
                        let s = b.pet_skins.entry(pet_id).or_default();
                        if !s.owned_skins.contains(&skin_id) {
                            s.owned_skins.push(skin_id);
                        }
                        s.current_skin = skin_id;
                        // `addGrowth` / `addIntimacy` 累加到该门客
                        let extra = b.pet_extra.entry(pet_id).or_default();
                        extra.growth += growth;
                    });
                    Ok(Some(
                        MsgActivatePetSkinResp7023 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
                // 其它 effect 不该出现在 7023
                SkinOutcome::Accept(_) => Ok(Some(reject_reply(
                    7023,
                    ddj_pb::cmd::RetCode::ServerBusy,
                ))),
            }
        })));
        n += 1;
    }

    // 7024 升级皮肤
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(7024, move |ctx, body| {
            let req = MsgUpgradePetSkinReq7024::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let pet_id = req.pet_id.unwrap_or(0);
            let skin_id = req.skin_id.unwrap_or(0);
            let tpl = template(skin_id);
            let state = w.peek(p, |b| b.pet_skins.get(&pet_id).cloned());
            let can_pay = true;
            let out = pet::upgrade_skin_verdict(state.as_ref(), skin_id, tpl.as_ref(), can_pay);
            match out {
                SkinOutcome::Reject(code) => Ok(Some(reject_reply(7024, code))),
                SkinOutcome::Accept(SkinEffect::Upgrade {
                    skin_id, growth, ..
                }) => {
                    w.with(p, |b| {
                        // `addSkinLv`：等级存在 `pet_extra` 里
                        let extra = b.pet_extra.entry(pet_id).or_default();
                        *extra.skin_lv.entry(skin_id).or_insert(1) += 1;
                        extra.growth += growth;
                    });
                    Ok(Some(
                        MsgUpgradePetSkinResp7024 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
                SkinOutcome::Accept(_) => Ok(Some(reject_reply(
                    7024,
                    ddj_pb::cmd::RetCode::ServerBusy,
                ))),
            }
        })));
        n += 1;
    }

    // 7025 更换/卸下皮肤（成功时额外推 7001）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(7025, move |ctx, body| {
            let req = MsgSetPetSkinReq7025::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let pet_id = req.pet_id.unwrap_or(0);
            let skin_id = req.skin_id.unwrap_or(0);
            let state = w.peek(p, |b| b.pet_skins.get(&pet_id).cloned());
            let out = pet::set_skin_verdict(state.as_ref(), skin_id);
            match out {
                SkinOutcome::Reject(code) => Ok(Some(reject_reply(7025, code))),
                SkinOutcome::Accept(SkinEffect::Set { skin_id }) => {
                    w.with(p, |b| {
                        let s = b.pet_skins.entry(pet_id).or_default();
                        s.current_skin = skin_id;
                    });
                    // 原版 `sendToPlayerId(PetUpdateResp_7001{msgPetData=[toMsgPetData(10)]})`。
                    // `toMsgPetData(10)` 的 `10` 是**字段号**：只填 `skinId`（tag 10），
                    // 并把 10 放进 `updateFieldsList`（tag 90）告诉客户端只更新这个字段。
                    if let Some(sess) = ctx.session.as_ref() {
                        let msg = PetUpdateResp7001 {
                            msg_pet_data: vec![ddj_pb::gen::pet_message::MsgPetData {
                                update_fields_list: vec![SKIN_FIELD_ID],
                                pet_id: Some(pet_id),
                                skin_id: Some(skin_id),
                                ..Default::default()
                            }],
                        };
                        sess.push(PET_UPDATE_RESP, msg.encode_to_vec().into());
                    }
                    Ok(Some(
                        MsgSetPetSkinResp7025 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
                SkinOutcome::Accept(_) => Ok(Some(reject_reply(
                    7025,
                    ddj_pb::cmd::RetCode::ServerBusy,
                ))),
            }
        })));
        n += 1;
    }

    // 7026 珍兽评级（失败走 ErrorResp_1）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(7026, move |ctx, body| {
            let req = MsgSetPetRateLvReq7026::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let pet_id = req.pet_id.unwrap_or(0);
            let (pet_exists, current_lv, intimacy, growth) = w.peek(p, |b| {
                match b.pet_skins.get(&pet_id) {
                    Some(_) => {
                        let e = b.pet_extra.get(&pet_id);
                        (
                            true,
                            b.pet_rate.get(&pet_id).copied().unwrap_or(1),
                            e.map_or(0, |x| x.intimacy),
                            e.map_or(0, |x| x.growth),
                        )
                    }
                    None => (false, 1, 0, 0),
                }
            });
            // `getSameRateLvNume(current_lv + 1)`：数一下有多少只已经在这个等级
            let same_lv_count = w.peek(p, |b| {
                b.pet_rate
                    .values()
                    .filter(|v| **v == current_lv + 1)
                    .count() as i32
            });
            let rctx = PetRateCtx {
                // `SystemOpenUtils.isOpen(pid, 23)` 未接，按已开启放行
                system_open: true,
                pet_exists,
                current_lv,
                max_lv: rate_max_lv(),
                same_lv_count,
                intimacy,
                growth,
            };
            let tpl = rate_tpl(current_lv + 1);
            match pet::set_pet_rate_verdict(&rctx, tpl.as_ref()) {
                PetRateOutcome::Reject(code) => Ok(Some(reject_reply(7026, code))),
                PetRateOutcome::Accept { new_lv } => {
                    w.with(p, |b| {
                        b.pet_rate.insert(pet_id, new_lv);
                    });
                    // 原版推 `PetUpdateResp_7001{toMsgPetData(11)}`（rateLv 字段）
                    push_pet_field(ctx, pet_id, RATE_FIELD_ID, new_lv);
                    Ok(Some(
                        MsgSetPetRateLvResp7026 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    // 7029 评级互换（失败走 Resp 里的 ret，不是 ErrorResp_1）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(7029, move |ctx, body| {
            let req = MsgPetRateLvReplaceReq7029::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let id1 = req.pet_id1.unwrap_or(0);
            let id2 = req.pet_id2.unwrap_or(0);
            let (e1, e2) = w.peek(p, |b| {
                let pick = |id: i32| {
                    (
                        b.pet_skins.contains_key(&id),
                        b.pet_rate.get(&id).copied().unwrap_or(1),
                        b.pet_extra.get(&id).map_or(0, |x| x.intimacy),
                        b.pet_extra.get(&id).map_or(0, |x| x.growth),
                    )
                };
                (pick(id1), pick(id2))
            });
            let rctx = PetRateReplaceCtx {
                system_open: true,
                pet1_exists: e1.0,
                pet2_exists: e2.0,
                lv1: e1.1,
                lv2: e2.1,
                intimacy1: e1.2,
                growth1: e1.3,
                intimacy2: e2.2,
                growth2: e2.3,
            };
            // 原版查的是**高等级那方**的配置
            let high_lv = rctx.lv1.max(rctx.lv2);
            let tpl = rate_tpl(high_lv);
            match pet::replace_pet_rate_verdict(&rctx, tpl.as_ref()) {
                PetRateReplaceOutcome::Reject(code) => Ok(Some(
                    MsgPetRateLvReplaceResp7029 { ret: code.as_i32() }
                        .encode_to_vec()
                        .into(),
                )),
                PetRateReplaceOutcome::Accept {
                    lv1_after,
                    lv2_after,
                } => {
                    w.with(p, |b| {
                        b.pet_rate.insert(id1, lv1_after);
                        b.pet_rate.insert(id2, lv2_after);
                    });
                    // 原版推两条 7001（各一条 msgPetData）
                    push_pet_field(ctx, id1, RATE_FIELD_ID, lv1_after);
                    push_pet_field(ctx, id2, RATE_FIELD_ID, lv2_after);
                    Ok(Some(
                        MsgPetRateLvReplaceResp7029 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    n
}

/// `MsgPetData.rateLv` 的字段号（原版 `toMsgPetData(11)` 里的 11）。
const RATE_FIELD_ID: i32 = 11;

/// 推一条只含单个字段的 `PetUpdateResp_7001`。
///
/// 原版 `toMsgPetData(fieldId)` 的语义：把该字段填上，并把字段号放进
/// `updateFieldsList`，告诉客户端只更新这一个字段。
fn push_pet_field(ctx: &ddj_registry::HandlerCtx, pet_id: i32, field_id: i32, value: i32) {
    let Some(sess) = ctx.session.as_ref() else {
        return;
    };
    let mut data = ddj_pb::gen::pet_message::MsgPetData {
        update_fields_list: vec![field_id],
        pet_id: Some(pet_id),
        ..Default::default()
    };
    if field_id == SKIN_FIELD_ID {
        data.skin_id = Some(value);
    } else if field_id == RATE_FIELD_ID {
        data.rate_lv = Some(value);
    }
    let msg = PetUpdateResp7001 {
        msg_pet_data: vec![data],
    };
    sess.push(PET_UPDATE_RESP, msg.encode_to_vec().into());
}
