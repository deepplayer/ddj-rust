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
    MsgActivatePetSkinReq7023, MsgActivatePetSkinResp7023, MsgSetPetSkinReq7025,
    MsgSetPetSkinResp7025, MsgUpgradePetSkinReq7024, MsgUpgradePetSkinResp7024, PetUpdateResp7001,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::pet::{self, PetSkinTemplate, SkinEffect, SkinOutcome};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// `ErrorResp_1` 的帧消息号。
const ERROR_RESP_CMD: i32 = 1;

/// `PetUpdateResp_7001` 的消息号。
const PET_UPDATE_RESP: i32 = 7001;

/// `MsgPetData.skinId` 的字段号（原版 `toMsgPetData(10)` 里的 10）。
const SKIN_FIELD_ID: i32 = 10;

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

    n
}
