//! 背包 5 条消息的接线（11002 / 11003 / 11005 / 11006 / 11008）。
//!
//! ## 过渡期的 `MemLookups`
//! 真正的模板表（`ItemDB` / `GiftDB`）要从 `ddj-template` 的 CSV 里读，
//! 仓储接口还没接。这里用「**背包里存在的模板就合成一条最小模板**」顶住：
//!
//! - 有道具 → `useLimit = 999`、`type = 0`（普通道具）→ 能走到扣数量那一步；
//! - 没有道具 → `item_template = None` → 原版在此处 NPE，被 handler 遮罩成 `ret=2`。
//!
//! 这样 5 条消息在冒烟里都有确定性输出，等 `TemplateStore` 接好后只需替换
//! [`MemLookups`] 的实现，闭包与回包逻辑不用动。

use std::sync::Arc;

use ddj_core::{Clock, PlayerId, Result};
use ddj_pb::gen::item_message::{
    ConfirmOverdueReq11008, ConfirmOverdueResp11008, ItemCompoundReq11003, ItemCompoundResp11003,
    ItemEffectiveOneKeyUseReq11006, ItemEffectiveOneKeyUseResp11006, ItemEffectiveUseReq11005,
    ItemEffectiveUseResp11005, ItemUseReq11002, ItemUseResp11002, MsgUseResult,
};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::item::{
    self, BagSnapshot, CompoundInput, CompoundLookup, CompoundOutcome, CompoundTemplate,
    EffectiveItem, EffectiveLookup, EffectiveUseLookup, EffectiveUseTemplate, Gift, ItemLookup,
    ItemTemplate, OneKeyApplied, OneKeyLookup, OneKeyOutcome, UseApply, UseFailure, UseOutcome,
};
use crate::runtime::{decode_err, resources_to_msg, FnHandler, MemWorld};

/// 过渡期用的最小模板上限（真实值来自 `ItemDB.useLimit`）。
const ASSUMED_USE_LIMIT: i32 = 999;

/// 从内存态合成模板与资源判定的查询器。
struct MemLookups<'a> {
    world: &'a MemWorld,
    player: PlayerId,
    now: i64,
}

impl ItemLookup for MemLookups<'_> {
    fn item_template(&self, template_id: i32) -> Result<Option<ItemTemplate>> {
        // 背包里没有这条模板 → 原版 `getItemTemplate` 也不会凭空造出来，
        // 但调用点在 `allItem.get` 之后，所以拿不到模板就等于「数据不一致」。
        let has = self
            .world
            .peek(self.player, |p| p.bag.contains_key(&template_id));
        Ok(has.then_some(ItemTemplate {
            use_limit: ASSUMED_USE_LIMIT,
            item_type: 0,
        }))
    }

    fn gift(&self, template_id: i32) -> Result<Option<Gift>> {
        let _ = template_id;
        // 过渡期：所有道具都按「无礼包配置」处理（`useCostJson = null`）。
        Ok(None)
    }

    fn gift_check_for_using(&self, gift: &Gift) -> Result<Option<ddj_pb::cmd::RetCode>> {
        let _ = gift;
        Ok(None)
    }

    fn check_item(&self, res_id: i32, num: i64) -> Result<bool> {
        let have = self
            .world
            .peek(self.player, |p| p.bag.get(&res_id).copied().unwrap_or(0));
        Ok(have >= num)
    }

    fn apply_use(&self, template: &ItemTemplate, num: i64) -> Result<UseApply> {
        let _ = (template, num);
        // 原版 `use()` 的副作用（加资源、开礼包）由各 `ItemFun_NN` 实现，
        // 过渡期返回「成功且无额外回包字段」。
        Ok(UseApply {
            err: None,
            has_msg_use_result: false,
        })
    }
}

impl CompoundLookup for MemLookups<'_> {
    fn item(&self, id: i32) -> Result<Option<CompoundTemplate>> {
        let has = self.world.peek(self.player, |p| p.bag.contains_key(&id));
        Ok(has.then_some(CompoundTemplate {
            item_type: 0,
            compound_item: Vec::new(),
        }))
    }

    fn has_items(&self, id: i32, num: i64) -> Result<bool> {
        <Self as ItemLookup>::check_item(self, id, num)
    }
}

impl EffectiveLookup for MemLookups<'_> {
    fn get(&self, id: i64) -> Result<Option<EffectiveItem>> {
        let raw = self
            .world
            .peek(self.player, |p| p.effective.get(&id).copied());
        Ok(raw.map(|(template_id, num, deadline)| EffectiveItem {
            id,
            template_id,
            num,
            deadline,
        }))
    }

    fn mark_sent(&self, id: i64) -> Result<()> {
        let _ = (self.now, id);
        // `sendClient = true` 只影响下次登录的补发，过渡期无对应字段。
        Ok(())
    }
}

impl EffectiveUseLookup for MemLookups<'_> {
    fn effective(&self, id: i64) -> Result<Option<EffectiveItem>> {
        <Self as EffectiveLookup>::get(self, id)
    }

    fn item_template(&self, template_id: i32) -> Result<Option<EffectiveUseTemplate>> {
        let _ = template_id;
        Ok(Some(EffectiveUseTemplate {
            use_limit: ASSUMED_USE_LIMIT,
        }))
    }

    fn use_remove_num(&self, id: i64, num: i64) -> Result<i64> {
        let _ = id;
        Ok(num)
    }

    fn gift(&self, template_id: i32) -> Result<Option<Gift>> {
        <Self as ItemLookup>::gift(self, template_id)
    }

    fn gift_check_for_using(&self, template_id: i32) -> Result<Option<ddj_pb::cmd::RetCode>> {
        let _ = template_id;
        Ok(None)
    }

    fn check_item(&self, id: i32, num: i64) -> Result<bool> {
        <Self as ItemLookup>::check_item(self, id, num)
    }

    fn apply_use(&self, id: i64, num: i64) -> Result<UseApply> {
        let _ = (id, num);
        Ok(UseApply::default())
    }
}

impl OneKeyLookup for MemLookups<'_> {
    fn gift(&self, template_id: i32) -> Result<Option<Gift>> {
        <Self as ItemLookup>::gift(self, template_id)
    }

    fn gift_check_for_using(&self, template_id: i32) -> Result<Option<ddj_pb::cmd::RetCode>> {
        let _ = template_id;
        Ok(None)
    }

    fn check_item(&self, id: i32, num: i64) -> Result<bool> {
        <Self as ItemLookup>::check_item(self, id, num)
    }

    fn use_remove_num(&self, id: i64, num: i64) -> Result<i64> {
        let _ = id;
        Ok(num)
    }

    fn apply_use(&self, id: i64, num: i64) -> Result<OneKeyApplied> {
        let _ = (id, num);
        Ok(OneKeyApplied::default())
    }
}

/// `ItemUseResp_11002`：`ret` 由 [`UseOutcome::resolve_ret`] 决定。
fn use_resp(out: &UseOutcome) -> ItemUseResp11002 {
    match out {
        UseOutcome::Accept {
            has_msg_use_result, ..
        } => ItemUseResp11002 {
            ret: 0,
            msg_use_result: has_msg_use_result.then(MsgUseResult::default),
        },
        UseOutcome::Fail(_) => ItemUseResp11002 {
            ret: out.resolve_ret().map_or(0, ddj_pb::cmd::RetCode::as_i32),
            msg_use_result: None,
        },
    }
}

/// 把 5 条背包消息挂进表，返回注册数量。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 11002 使用道具
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(11002, move |ctx, body| {
            let req = ItemUseReq11002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let template_id = req.template_id.unwrap_or(0);
            let num = req.num.unwrap_or(0);
            let bag = w.peek(p, |b| BagSnapshot {
                has_item: b.bag.contains_key(&template_id),
                item_num: b.bag.get(&template_id).copied().unwrap_or(0),
            });
            let lk = MemLookups {
                world: &w,
                player: p,
                now: c.now_millis(),
            };
            let out = item::use_item(bag, template_id, num, &lk)?;
            // 副作用放在纯函数之后，与原版分支序一致（分支 11/12）。
            if let UseOutcome::Accept {
                remove_num, cost, ..
            } = &out
            {
                w.with(p, |b| {
                    if let Some(v) = b.bag.get_mut(&template_id) {
                        *v = (*v - remove_num).max(0);
                    }
                    if let Some((res, total)) = cost {
                        if let Some(v) = b.bag.get_mut(res) {
                            *v = (*v - total).max(0);
                        }
                    }
                });
            }
            Ok(Some(use_resp(&out).encode_to_vec().into()))
        })));
        n += 1;
    }

    // 11003 合成
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(11003, move |ctx, body| {
            let req = ItemCompoundReq11003::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let input = CompoundInput {
                compound_id: req.compound_id.unwrap_or(0),
                compound_num: req.compound_num.unwrap_or(0),
                use_item_id: req.use_item_id,
            };
            let lk = MemLookups {
                world: &w,
                player: p,
                now: 0,
            };
            let out = item::compound(input, &lk)?;
            let ret = match &out {
                CompoundOutcome::Accept {
                    consume_id,
                    consume_num,
                } => {
                    let (id, num) = (*consume_id, *consume_num);
                    w.with(p, |b| {
                        if let Some(v) = b.bag.get_mut(&id) {
                            *v = (*v - num).max(0);
                        }
                    });
                    0
                }
                CompoundOutcome::Reject(code) => code.as_i32(),
            };
            Ok(Some(ItemCompoundResp11003 { ret }.encode_to_vec().into()))
        })));
        n += 1;
    }

    // 11005 时效道具使用
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(11005, move |ctx, body| {
            let req = ItemEffectiveUseReq11005::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let id = req.id.unwrap_or(0);
            let num = req.num.unwrap_or(0);
            let now = c.now_millis();
            let lk = MemLookups {
                world: &w,
                player: p,
                now,
            };
            let out = item::effective_use(id, num, now, &lk)?;
            if let UseOutcome::Accept { remove_num, .. } = &out {
                let (rid, rn) = (id, *remove_num);
                w.with(p, |b| {
                    if let Some((_, have, _)) = b.effective.get_mut(&rid) {
                        *have = (*have - rn).max(0);
                    }
                });
            }
            let resp = match &out {
                UseOutcome::Accept {
                    has_msg_use_result, ..
                } => ItemEffectiveUseResp11005 {
                    ret: 0,
                    msg_use_result: has_msg_use_result.then(MsgUseResult::default),
                },
                UseOutcome::Fail(UseFailure::Resp(code)) => ItemEffectiveUseResp11005 {
                    ret: code.as_i32(),
                    msg_use_result: None,
                },
                // 11005 的 `use()` 不捕获异常，`Handler` 类失败不该出现；出现也按原版遮罩。
                UseOutcome::Fail(UseFailure::Handler(_)) => ItemEffectiveUseResp11005 {
                    ret: ddj_pb::cmd::RetCode::ServerBusy.as_i32(),
                    msg_use_result: None,
                },
            };
            Ok(Some(resp.encode_to_vec().into()))
        })));
        n += 1;
    }

    // 11006 一键使用
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(11006, move |ctx, body| {
            let req = ItemEffectiveOneKeyUseReq11006::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let template_id = req.template_id.unwrap_or(0);
            let now = c.now_millis();
            let lk = MemLookups {
                world: &w,
                player: p,
                now,
            };
            let items: Vec<EffectiveItem> = w.peek(p, |b| {
                b.effective
                    .iter()
                    .map(|(id, (template_id, num, deadline))| EffectiveItem {
                        id: *id,
                        template_id: *template_id,
                        num: *num,
                        deadline: *deadline,
                    })
                    .filter(|e| e.template_id == template_id)
                    .collect()
            });
            let out = item::effective_one_key_use(template_id, now, &items, &lk)?;
            let resp = match &out {
                OneKeyOutcome::Accept(plan) => {
                    // 过期条目原版顺手清掉（`iterator.remove()`）。
                    let expired: Vec<i64> = plan.expired.clone();
                    let used: Vec<i64> = plan.used.iter().map(|(id, _)| *id).collect();
                    w.with(p, |b| {
                        for id in &expired {
                            b.effective.remove(id);
                        }
                        for id in &used {
                            b.effective.remove(id);
                        }
                    });
                    ItemEffectiveOneKeyUseResp11006 {
                        ret: 0,
                        msg_use_result: (!plan.gains.is_empty()).then(|| MsgUseResult {
                            msg_resources: resources_to_msg(&plan.gains),
                            ..Default::default()
                        }),
                    }
                }
                OneKeyOutcome::Reject(f) => ItemEffectiveOneKeyUseResp11006 {
                    ret: UseOutcome::Fail(*f)
                        .resolve_ret()
                        .map_or(0, ddj_pb::cmd::RetCode::as_i32),
                    msg_use_result: None,
                },
            };
            Ok(Some(resp.encode_to_vec().into()))
        })));
        n += 1;
    }

    // 11008 过期确认
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(11008, move |ctx, body| {
            let req = ConfirmOverdueReq11008::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let lk = MemLookups {
                world: &w,
                player: p,
                now: c.now_millis(),
            };
            let ids = item::confirm_overdue(&req.id, c.now_millis(), &lk)?;
            Ok(Some(
                ConfirmOverdueResp11008 { ret: 0, id: ids }
                    .encode_to_vec()
                    .into(),
            ))
        })));
        n += 1;
    }

    n
}
