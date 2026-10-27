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

use ddj_core::{Clock, RngSource};
use ddj_pb::gen::pet_message::{
    MsgActivatePetSkinReq7023, MsgActivatePetSkinResp7023, MsgInteractResult,
    MsgInteractTimesReq7030, MsgInteractTimesResp7030, MsgPetRateLvReplaceReq7029,
    MsgPetRateLvReplaceResp7029, MsgSetPetRateLvReq7026, MsgSetPetRateLvResp7026,
    MsgSetPetSkinReq7025, MsgSetPetSkinResp7025, MsgUpgradePetSkinReq7024,
    MsgUpgradePetSkinResp7024, PetInteractReq7002, PetInteractResp7002, PetStarReq7003,
    PetStarResp7003, PetUpgradeSkillClassReq7006, PetUpgradeSkillClassResp7006,
    PetUpgradeSkillReq7005, PetUpgradeSkillResp7005, PetUpdateResp7001,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::counter::{self, CounterTemplate};
use crate::pet::{
    self, InteractExpParams, InteractOutcome, InteractTimesOutcome, MiracleBonus, PetRateCtx,
    PetRateOutcome, PetRateReplaceCtx, PetRateReplaceOutcome, PetRateTemplate, PetSkillClassCtx,
    PetSkillClassOutcome, PetSkillClassTemplate, PetSkillLevelCtx, PetSkillLevelEffect,
    PetSkillLevelOutcome, PetSkillTemplate, PetSkillUpgradeTemplate, PetSkinTemplate, PetStarCtx,
    PetStarOutcome, PetStarTemplate, SkinEffect, SkinOutcome,
};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// `ErrorResp_1` 的帧消息号。
const ERROR_RESP_CMD: i32 = 1;

/// `PetUpdateResp_7001` 的消息号。
const PET_UPDATE_RESP: i32 = 7001;

/// `MsgPetData.skinId` 的字段号（原版 `toMsgPetData(10)` 里的 10）。
const SKIN_FIELD_ID: i32 = 10;

/// 升星与技能升阶模板。
pub trait PetGrowthTemplates: Send + Sync {
    /// `PetStarDBCache.getPetStarDB(star)`。
    fn star(&self, star: i32) -> Option<PetStarTemplate>;
    /// `PetSkillClassUpgradeDBCache.get(cls)`。
    fn skill_class(&self, cls: i32) -> Option<PetSkillClassTemplate>;
    /// `PetSkillDBCache.get(skillId)` → `(petId, stepAdd)`。
    fn skill(&self, skill_id: i32) -> Option<(i32, i32)>;
    /// `PetSkillDBCache.get(skillId)` 的完整模板（7005 用）。
    fn skill_full(&self, skill_id: i32) -> Option<PetSkillTemplate>;
    /// `PetSkillUpgradeDBCache.get(lv)`。
    fn skill_upgrade(&self, lv: i32) -> Option<PetSkillUpgradeTemplate>;
}

static GROWTH_TEMPLATES: OnceLock<Arc<dyn PetGrowthTemplates>> = OnceLock::new();

/// 注入升星/技能模板（只第一次生效）。
pub fn install_growth_templates(t: Arc<dyn PetGrowthTemplates>) {
    let _ = GROWTH_TEMPLATES.set(t);
}

fn growth_tpl() -> Option<&'static Arc<dyn PetGrowthTemplates>> {
    GROWTH_TEMPLATES.get()
}

/// 互动相关模板（`PetInteractExpValue` / 道具 17 的 useLimit / 计数器模板）。
#[derive(Debug, Clone, Copy, Default)]
pub struct InteractTemplates {
    /// `ConfigDBCache.PetInteractExpValue` 的 5 个元素。
    pub exp: InteractExpParams,
    /// 道具 17 的 `useLimit`（`ItemDBCache.getItemTemplate(17).getUseLimit()`）。
    pub item_use_limit: i32,
    /// `CounterRecoveryDBCache.getCounterRecoveryDB(2)`。
    pub counter: CounterTemplate,
}

static INTERACT_TEMPLATES: OnceLock<InteractTemplates> = OnceLock::new();

/// 注入互动模板（只第一次生效）。
pub fn install_interact_templates(t: InteractTemplates) {
    let _ = INTERACT_TEMPLATES.set(t);
}

fn interact_tpl() -> InteractTemplates {
    INTERACT_TEMPLATES.get().copied().unwrap_or_default()
}

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
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
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

    // 7002 宠物互动（单次 / 全部）
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(7002, move |ctx, body| {
            let req = PetInteractReq7002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let all = req.all.unwrap_or(0);
            let now = c.now_millis();
            let tpl = interact_tpl();

            let (pet_ids, counter) = w.peek(p, |b| {
                let ids: Vec<i32> = b.pet_skins.keys().copied().collect();
                let st = b
                    .counters
                    .get(&counter::PET_INTERACT_TEMPLATE_ID)
                    .copied()
                    .unwrap_or_default();
                (ids, st)
            });
            // 先 refresh 再判精力
            let refreshed = counter::refresh(counter, &tpl.counter, COUNTER_MAX_LIMIT, now);
            let out = pet::interact_verdict(pet_ids.len(), refreshed.state.num, all);
            match out {
                InteractOutcome::Reject(code) => Ok(Some(reject_reply(7002, code))),
                InteractOutcome::Accept { times, .. } => {
                    // 随机挑 times 次（可能重复），统计每只宠物的次数
                    let rng = fast_rng().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let mut picked: Vec<i32> = Vec::new();
                    for _ in 0..times {
                        let i = rng.gen_index(pet_ids.len());
                        picked.push(pet_ids[i]);
                    }
                    drop(rng);
                    // 扣精力
                    let removed = counter::remove_num(refreshed.state, times, now);
                    // 按宠物聚合经验
                    let mut results: Vec<MsgInteractResult> = Vec::new();
                    w.with(p, |b| {
                        b.counters
                            .insert(counter::PET_INTERACT_TEMPLATE_ID, removed.state);
                        for id in &picked {
                            let intimacy = b.pet_extra.get(id).map_or(0, |x| x.intimacy);
                            let base = pet::interact_exp(&tpl.exp, intimacy);
                            let (exp, multiple) = pet::apply_miracle(base, None::<MiracleBonus>);
                            let e = b.pet_extra.entry(*id).or_default();
                            e.interact += exp;
                            match results.iter_mut().find(|r| r.pet_id == Some(*id)) {
                                Some(slot) => {
                                    slot.exp = Some(slot.exp.unwrap_or(0) + exp);
                                    slot.multiple = Some(slot.multiple.unwrap_or(0) + multiple);
                                }
                                None => results.push(MsgInteractResult {
                                    pet_id: Some(*id),
                                    exp: Some(exp),
                                    multiple: Some(multiple),
                                }),
                            }
                        }
                    });
                    // 原版推 `PetUpdateResp_7001{toMsgPetData(4)}`（interact 字段）
                    for r in &results {
                        if let Some(id) = r.pet_id {
                            let v = w.peek(p, |b| b.pet_extra.get(&id).map_or(0, |x| x.interact));
                            push_pet_i64_field(ctx, id, INTERACT_FIELD_ID, v);
                        }
                    }
                    Ok(Some(
                        PetInteractResp7002 {
                            ret: 0,
                            msg_interact_result: results,
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    // 7030 批量互动
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(7030, move |ctx, body| {
            let req = MsgInteractTimesReq7030::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let times = req.num.unwrap_or(0);
            let now = c.now_millis();
            let tpl = interact_tpl();

            // 原版 handler 先判 `num <= 0` → BUSY
            if times <= 0 {
                return Ok(Some(reject_reply(7030, ddj_pb::cmd::RetCode::ServerBusy)));
            }
            let (pet_ids, item_num, counter) = w.peek(p, |b| {
                (
                    b.pet_skins.keys().copied().collect::<Vec<i32>>(),
                    b.bag.get(&INTERACT_ITEM_ID).copied().unwrap_or(0),
                    b.counters
                        .get(&counter::PET_INTERACT_TEMPLATE_ID)
                        .copied()
                        .unwrap_or_default(),
                )
            });
            let refreshed = counter::refresh(counter, &tpl.counter, COUNTER_MAX_LIMIT, now);
            let out = pet::interact_times_verdict(
                pet_ids.len(),
                times,
                tpl.item_use_limit,
                item_num,
                COUNTER_MAX_LIMIT,
            );
            match out {
                InteractTimesOutcome::Reject(code) => Ok(Some(reject_reply(7030, code))),
                InteractTimesOutcome::Accept { cost, loops, .. } => {
                    let rng = fast_rng().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let mut picked: Vec<i32> = Vec::new();
                    for _ in 0..loops {
                        let i = rng.gen_index(pet_ids.len());
                        picked.push(pet_ids[i]);
                    }
                    drop(rng);
                    let mut results: Vec<MsgInteractResult> = Vec::new();
                    w.with(p, |b| {
                        // 扣道具 17
                        if let Some(v) = b.bag.get_mut(&INTERACT_ITEM_ID) {
                            *v = (*v - cost).max(0);
                        }
                        for id in &picked {
                            let intimacy = b.pet_extra.get(id).map_or(0, |x| x.intimacy);
                            let base = pet::interact_exp(&tpl.exp, intimacy);
                            let (exp, multiple) = pet::apply_miracle(base, None::<MiracleBonus>);
                            let e = b.pet_extra.entry(*id).or_default();
                            e.interact += exp;
                            match results.iter_mut().find(|r| r.pet_id == Some(*id)) {
                                Some(slot) => {
                                    slot.exp = Some(slot.exp.unwrap_or(0) + exp);
                                    slot.multiple = Some(slot.multiple.unwrap_or(0) + multiple);
                                }
                                None => results.push(MsgInteractResult {
                                    pet_id: Some(*id),
                                    exp: Some(exp),
                                    multiple: Some(multiple),
                                }),
                            }
                        }
                        b.counters.insert(counter::PET_INTERACT_TEMPLATE_ID, refreshed.state);
                    });
                    for r in &results {
                        if let Some(id) = r.pet_id {
                            let v = w.peek(p, |b| b.pet_extra.get(&id).map_or(0, |x| x.interact));
                            push_pet_i64_field(ctx, id, INTERACT_FIELD_ID, v);
                        }
                    }
                    Ok(Some(
                        MsgInteractTimesResp7030 {
                            ret: 0,
                            msg_interact_result: results,
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    // 7003 升星
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(7003, move |ctx, body| {
            let req = PetStarReq7003::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let pet_id = req.pet_id.unwrap_or(0);
            let (exists, star, intimacy, growth) = w.peek(p, |b| {
                (
                    b.pet_skins.contains_key(&pet_id),
                    b.pet_star.get(&pet_id).copied().unwrap_or(0),
                    b.pet_extra.get(&pet_id).map_or(0, |x| x.intimacy),
                    b.pet_extra.get(&pet_id).map_or(0, |x| x.growth),
                )
            });
            let sctx = PetStarCtx {
                pet_exists: exists,
                current_star: star,
                intimacy,
                growth,
            };
            let (cur, next) = match growth_tpl() {
                Some(t) => (t.star(star), t.star(star + 1)),
                None => (None, None),
            };
            match pet::pet_star_verdict(&sctx, cur.as_ref(), next.as_ref()) {
                PetStarOutcome::Reject(code) => Ok(Some(reject_reply(7003, code))),
                PetStarOutcome::Accept { new_star } => {
                    w.with(p, |b| {
                        b.pet_star.insert(pet_id, new_star);
                    });
                    Ok(Some(PetStarResp7003 { ret: 0 }.encode_to_vec().into()))
                }
            }
        })));
        n += 1;
    }

    // 7006 技能升阶
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(7006, move |ctx, body| {
            let req = PetUpgradeSkillClassReq7006::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let pet_id = req.pet_id.unwrap_or(0);
            let skill_id = req.skill_id.unwrap_or(0);
            let (exists, pet_count, cur_class) = w.peek(p, |b| {
                (
                    b.pet_skins.contains_key(&pet_id),
                    b.pet_skins.len(),
                    b.pet_skill_class
                        .get(&(pet_id, skill_id))
                        .copied()
                        .unwrap_or(0),
                )
            });
            // `PetSkillDB.get(skillId)`：判断技能属于谁、能否升阶
            let skill = growth_tpl().and_then(|t| t.skill(skill_id));
            let belongs = skill.map(|(pid, _)| pid == pet_id);
            let step_add = skill.map_or(0, |(_, s)| s);
            let cctx = PetSkillClassCtx {
                pet_exists: exists,
                // 原版用 `indexOf` 得到下标；过渡期只要属于该宠物就算 offset 0
                skill_offset: belongs.and_then(|ok| ok.then_some(0usize)),
                // 过渡期没有逐技能等级，默认已解锁（1）
                skill_lv: 1,
                step_add,
                current_class: cur_class,
                pet_count,
                // 过渡期不查资源系统
                can_pay: true,
            };
            let (up, next) = match growth_tpl() {
                Some(t) => (t.skill_class(cur_class), t.skill_class(cur_class + 1)),
                None => (None, None),
            };
            match pet::pet_skill_class_verdict(&cctx, up.as_ref(), next.as_ref()) {
                PetSkillClassOutcome::Reject(code) => Ok(Some(reject_reply(7006, code))),
                PetSkillClassOutcome::Accept {
                    new_class, offset, ..
                } => {
                    w.with(p, |b| {
                        b.pet_skill_class.insert((pet_id, skill_id), new_class);
                    });
                    // 原版 `pet.sendToPlayer(7, 6)`：推 skillClass 字段（tag 7）
                    push_pet_skill_class(ctx, pet_id, offset, new_class);
                    Ok(Some(
                        PetUpgradeSkillClassResp7006 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    // 7005 技能升级（两条分支：皮肤 / 普通）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(7005, move |ctx, body| {
            let req = PetUpgradeSkillReq7005::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let pet_id = req.pet_id.unwrap_or(0);
            let skill_id = req.skill_id.unwrap_or(0);
            let (exists, pet_count, interaction, skin_lv, skill_lv) = w.peek(p, |b| {
                (
                    b.pet_skins.contains_key(&pet_id),
                    b.pet_skins.len(),
                    b.pet_extra.get(&pet_id).map_or(0, |x| x.interact),
                    b.pet_skin_skill_lv.get(&skill_id).copied(),
                    b.pet_skill_lv
                        .get(&(pet_id, skill_id))
                        .copied()
                        .unwrap_or(0),
                )
            });
            let t = growth_tpl();
            let skill_tpl = t.and_then(|x| x.skill_full(skill_id));
            let belongs = skill_tpl.map(|s| s.pet_id == pet_id);
            let is_skin = skin_lv.is_some();
            let cur_lv = if is_skin {
                skin_lv.unwrap_or(0)
            } else {
                skill_lv
            };
            let (up, next) = match t {
                Some(x) => (x.skill_upgrade(cur_lv), x.skill_upgrade(cur_lv + 1)),
                None => (None, None),
            };
            let lctx = PetSkillLevelCtx {
                pet_exists: exists,
                skill_belongs: belongs.unwrap_or(false),
                is_skin_skill: is_skin,
                skin_lv_present: skin_lv.is_some(),
                // 过渡期皮肤技能视为已解锁
                skin_skill_open: true,
                skin_lv: cur_lv,
                skill_offset: belongs.and_then(|ok| ok.then_some(0usize)),
                skill_lv,
                pet_count,
                interaction,
                // 过渡期解锁条件视为满足
                unlock_condition_met: true,
                can_pay_good: true,
            };
            match pet::pet_skill_level_verdict(&lctx, skill_tpl.as_ref(), up.as_ref(), next.as_ref())
            {
                PetSkillLevelOutcome::Reject(code) => Ok(Some(reject_reply(7005, code))),
                PetSkillLevelOutcome::Accept(effect) => {
                    match effect {
                        PetSkillLevelEffect::Skin {
                            new_lv,
                            good_id,
                            cost,
                            ..
                        } => {
                            w.with(p, |b| {
                                b.pet_skin_skill_lv.insert(skill_id, new_lv);
                                if let Some(v) = b.bag.get_mut(&good_id) {
                                    *v = (*v - cost).max(0);
                                }
                            });
                        }
                        PetSkillLevelEffect::Normal {
                            new_lv, cost, ..
                        } => {
                            // ⚠ 普通技能扣的是互动经验
                            w.with(p, |b| {
                                b.pet_skill_lv.insert((pet_id, skill_id), new_lv);
                                if let Some(e) = b.pet_extra.get_mut(&pet_id) {
                                    e.interact = (e.interact - cost).max(0);
                                }
                            });
                        }
                    }
                    // 原版 `pet.sendToPlayer(6, 4)`：推 skillLv（tag 6）
                    push_pet_skill_lv(ctx, pet_id, 0, cur_lv + 1);
                    Ok(Some(
                        PetUpgradeSkillResp7005 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    n
}

/// `MsgPetData.skillLv` 的字段号（tag 6）。
const SKILL_LV_FIELD_ID: i32 = 6;

/// 推一条含 `skillLv` 的 `PetUpdateResp_7001`。
fn push_pet_skill_lv(ctx: &ddj_registry::HandlerCtx, pet_id: i32, offset: usize, new_lv: i32) {
    let Some(sess) = ctx.session.as_ref() else {
        return;
    };
    let mut skills = vec![0_i32; offset + 1];
    skills[offset] = new_lv;
    let msg = PetUpdateResp7001 {
        msg_pet_data: vec![ddj_pb::gen::pet_message::MsgPetData {
            update_fields_list: vec![SKILL_LV_FIELD_ID],
            pet_id: Some(pet_id),
            skill_lv: skills,
            ..Default::default()
        }],
    };
    sess.push(PET_UPDATE_RESP, msg.encode_to_vec().into());
}

/// `MsgPetData.skillClass` 的字段号（tag 7）。
const SKILL_CLASS_FIELD_ID: i32 = 7;

/// 推一条含 `skillClass` 的 `PetUpdateResp_7001`。
fn push_pet_skill_class(ctx: &ddj_registry::HandlerCtx, pet_id: i32, offset: usize, new_class: i32) {
    let Some(sess) = ctx.session.as_ref() else {
        return;
    };
    let mut skills = vec![0_i32; offset + 1];
    skills[offset] = new_class;
    let msg = PetUpdateResp7001 {
        msg_pet_data: vec![ddj_pb::gen::pet_message::MsgPetData {
            update_fields_list: vec![SKILL_CLASS_FIELD_ID],
            pet_id: Some(pet_id),
            skill_class: skills,
            ..Default::default()
        }],
    };
    sess.push(PET_UPDATE_RESP, msg.encode_to_vec().into());
}

/// 互动消耗的道具 id（原版硬编码 `17`）。
const INTERACT_ITEM_ID: i32 = 17;

/// `MsgPetData.interaction` 的字段号（原版 `toMsgPetData(4)` 里的 4）。
const INTERACT_FIELD_ID: i32 = 4;

/// 计数器上限（原版由 VIP 模板 `getCounterMax(templateId)` 决定）。
///
/// 过渡期用常量；接上 `VipDBCache` 后按玩家 VIP 等级取值。
const COUNTER_MAX_LIMIT: i64 = 100;

/// 互动随机挑选用的 rng（进程级，避免每次 handler 重建）。
///
/// `DeterministicRng::new` 不是 const，所以用 `OnceLock` 惰性建。
static FAST_RNG: OnceLock<std::sync::Mutex<ddj_core::DeterministicRng>> = OnceLock::new();

fn fast_rng() -> &'static std::sync::Mutex<ddj_core::DeterministicRng> {
    FAST_RNG.get_or_init(|| std::sync::Mutex::new(ddj_core::DeterministicRng::new(0x5EED_7002)))
}

/// 推一条含 `i64` 字段的 `PetUpdateResp_7001`。
fn push_pet_i64_field(ctx: &ddj_registry::HandlerCtx, pet_id: i32, field_id: i32, value: i64) {
    let Some(sess) = ctx.session.as_ref() else {
        return;
    };
    let mut data = ddj_pb::gen::pet_message::MsgPetData {
        update_fields_list: vec![field_id],
        pet_id: Some(pet_id),
        ..Default::default()
    };
    if field_id == INTERACT_FIELD_ID {
        data.interaction = Some(value);
    }
    let msg = PetUpdateResp7001 {
        msg_pet_data: vec![data],
    };
    sess.push(PET_UPDATE_RESP, msg.encode_to_vec().into());
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
