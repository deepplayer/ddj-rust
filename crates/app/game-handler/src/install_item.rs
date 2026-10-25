//! 背包 5 条消息的接线（11002 / 11003 / 11005 / 11006 / 11008）。
//!
//! ## 过渡期的 `MemLookups`
//! 真正的模板表（`ItemDB` / `GiftDB`）要从 `ddj-template` 的 CSV 里读，
//! 仓储接口还没接。这里用「**背包里存在的模板就合成一条最小模板**」顶住：
//!
//! - 有道具 → 启动时注入了真实 `ItemDB` 就取 `useLimit`/`type`，否则回退 `useLimit = 999`、`type = 0`；
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
    ItemTemplate, OneKeyApplied, OneKeyLookup, OneKeyOutcome, UseApply, UseOutcome,
};
use crate::runtime::{decode_err, resources_to_msg, FnHandler, MemWorld};

/// 过渡期用的最小模板上限（真实值来自 `ItemDB.useLimit`）。
use std::sync::OnceLock;

const ASSUMED_USE_LIMIT: i32 = 999;

/// 真实模板表的只读视图（由 `game-core` 在启动时注入）。
///
/// 认成 trait 而不是直接依赖 `ddj-template`：`game-handler` 保持零 IO、零数据层依赖，
/// 便于单测用假表。原版对应 `ItemDBCache.getUseLimit()` / `GiftDBCache.getGiftDB()`。
pub trait ItemTemplates: Send + Sync {
    /// `ItemDB.useLimit`；`None` 表示该行没填（原版 `getUseLimit()` 得 0）。
    fn use_limit(&self, template_id: i32) -> Option<i32>;
    /// `ItemDB.type`。
    fn item_type(&self, template_id: i32) -> Option<i32>;
    /// `GiftDB.useCost` → `(资源 id, 每次数量)`。
    fn use_cost(&self, template_id: i32) -> Option<(i32, i64)>;
    /// `ItemDB.compoundItem` → `(被消耗的道具 id, 每次数量)`。
    ///
    /// 原版是 `List<Integer>`，只取前两个元素（`getCompoundItem().get(0)/(1)`）。
    /// 真表里 2674 行只有 37 行非 0，格式 `12=10`。
    fn compound_item(&self, template_id: i32) -> Option<(i32, i32)>;
}

/// 启动时装配一次；之后只读。
static TEMPLATES: OnceLock<Arc<dyn ItemTemplates>> = OnceLock::new();

/// 注入真实模板表（只第一次生效）。
pub fn install_templates(t: Arc<dyn ItemTemplates>) {
    let _ = TEMPLATES.set(t);
}

fn templates() -> Option<&'static Arc<dyn ItemTemplates>> {
    TEMPLATES.get()
}

/// 从内存态合成模板与资源判定的查询器。
struct MemLookups<'a> {
    world: &'a MemWorld,
    player: PlayerId,
    now: i64,
}

impl ItemLookup for MemLookups<'_> {
    fn item_template(&self, template_id: i32) -> Result<Option<ItemTemplate>> {
        // 原版 `ItemDBCache.getCache().getItemTemplate(templateId)`：**纯模板查表**，
        // 与背包内容无关。表里没有这一行才返回 null（之后的 NPE 由调用方复刻）。
        Ok(Some(match templates() {
            Some(t) => ItemTemplate {
                use_limit: t.use_limit(template_id).unwrap_or(0),
                item_type: t.item_type(template_id).unwrap_or(0),
            },
            None => ItemTemplate {
                use_limit: ASSUMED_USE_LIMIT,
                item_type: 0,
            },
        }))
    }

    fn gift(&self, template_id: i32) -> Result<Option<Gift>> {
        // 原版 `GiftDBCache.getGiftDB(templateId)`：表里没有这一行就返回 null。
        // 真表里 1066 行礼包配置只有 10 行填了 `useCost`。
        Ok(templates().and_then(|t| t.use_cost(template_id)).map(|(id, num)| Gift {
            use_cost: Some((id, num)),
        }))
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
        // 原版 `itemCache.getItemTemplate(templateId)`：同样只看模板表。
        let Some(t) = templates() else {
            // 无模板表时无从判断，等价「查不到」→ PARAM_ERROR（原版 null 分支）。
            return Ok(None);
        };
        let item_type = t.item_type(id).unwrap_or(0);
        // `getCompoundItem()` 恒为 2 元素列表；表里没填时原版拿到的是 [0, 0]。
        let compound_item = match t.compound_item(id) {
            Some((a, b)) => vec![a, b],
            None => vec![0, 0],
        };
        Ok(Some(CompoundTemplate {
            item_type,
            compound_item,
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
        Ok(Some(EffectiveUseTemplate {
            use_limit: templates()
                .and_then(|t| t.use_limit(template_id))
                .unwrap_or(0),
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

/// `TypeResources.ITEM`：真正进背包的道具类型。`ItemSet.addItem` 只对这类生效。
pub const TYPE_ITEM: i32 = 2;

/// `ItemServerModule.sendItemUpdate(playerId, item)` → `ItemUpdateResp_11004`。
///
/// 原版这是与请求回包**并列的第二条消息**（`MessageUtil.sendToPlayerId`）。
/// 客户端靠它刷新背包格子，缺了它玩家看不到数量变化。
fn push_item_update(ctx: &ddj_registry::HandlerCtx, template_id: i32, num: i64) {
    let Some(sess) = ctx.session.as_ref() else {
        return;
    };
    let Some(msg) = crate::push::build_item_update(&[crate::push::BagItem { template_id, num }])
    else {
        return;
    };
    let cmd = ddj_pb::cmd::item::UPDATE_RESP_11004;
    sess.push(cmd, msg.encode_to_vec().into());
}

/// `ItemServerModule.sendItemEffectiveUpdate(playerId, item)` → `ItemEffectiveUpdateResp_11007`。
fn push_effective_update(ctx: &ddj_registry::HandlerCtx, item: crate::push::BagEffectiveItem) {
    let Some(sess) = ctx.session.as_ref() else {
        return;
    };
    let Some(msg) = crate::push::build_effective_update(&[item]) else {
        return;
    };
    let cmd = ddj_pb::cmd::item::EFFECTIVE_UPDATE_RESP_11007;
    sess.push(cmd, msg.encode_to_vec().into());
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
            // 副作用放在纯函数之后，与原版分支序一致（分支 12/13）。
            if let UseOutcome::Accept {
                remove_num, cost, ..
            } = &out
            {
                // `Item.removeNum` 的 clample：`num -= removeNum; num = num < 0 ? 0 : num`。
                let left = w.with(p, |b| {
                    if let Some(v) = b.bag.get_mut(&template_id) {
                        *v = (*v - remove_num).max(0);
                        *v
                    } else {
                        0
                    }
                });
                if let Some((res, total)) = cost {
                    w.with(p, |b| {
                        if let Some(v) = b.bag.get_mut(res) {
                            *v = (*v - total).max(0);
                        }
                    });
                }
                // 原版 `ItemServerModule.sendItemUpdate(playerId, item)` → `ItemUpdateResp_11004`。
                // 这是**额外的推送包**，与请求的回包（11002）是两条消息。
                push_item_update(ctx, template_id, left);
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
                    let (cid, cnum) = (*consume_id, *consume_num);
                    // 原版 `removeItem(..., check=true, TYPE_506, ...)`：
                    // check 通过后逐条 `item.removeNum`，末尾 `sendItemUpdate(list)`。
                    let left = w.with(p, |b| {
                        if let Some(v) = b.bag.get_mut(&cid) {
                            *v = (*v - cnum).max(0);
                        }
                        b.bag.get(&cid).copied().unwrap_or(0)
                    });
                    push_item_update(ctx, cid, left);
                    // 原版 L384 `addItem(player, compoundId, compoundNum, TYPE_506, [])`：
                    // 只有 `ItemDB.type == TypeResources.ITEM(2)` 且 num > 0 才真的加，
                    // 且 addItem 内部**无条件** `sendItemUpdate(list)`（列表为空时自己 return）。
                    let out_id = input.compound_id;
                    let out_num = input.compound_num;
                    if let Some(t) = templates() {
                        let is_item = t.item_type(out_id).unwrap_or(0) == TYPE_ITEM;
                        if is_item && out_num > 0 {
                            let added = w.with(p, |b| {
                                let slot = b.bag.entry(out_id).or_insert(0);
                                *slot += out_num;
                                *slot
                            });
                            push_item_update(ctx, out_id, added);
                        }
                    }
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
                let snap = w.with(p, |b| {
                    b.effective.get_mut(&rid).map(|(tid, have, deadline)| {
                        *have = (*have - rn).max(0);
                        crate::push::BagEffectiveItem {
                            id: rid,
                            template_id: *tid,
                            num: *have,
                            deadline: *deadline,
                        }
                    })
                });
                // 原版 `item.removeNum(player, removeNum, true, ...)`：
                // 第 3 参 `sendClient=true` → `sendItemEffectiveUpdate` → 11007。
                // 11002 那边传的是 `false`，所以 11002 不推 11007。
                if let Some(it) = snap {
                    push_effective_update(ctx, it);
                }
            }
            // 原版 11005 的失败出口有两个，**都不是** `ItemEffectiveUseResp_11005`：
            //   1. `sendToPlayerId(ItemUseResp_11002{ret})` —— 数量不足(1102)、
            //      `checkForUsing` 非空时走这条（复制粘贴自 11002）；
            //   2. `catch(Exception)` → `sendToPlayerId(ItemUseResp_11002{ret=2})`
            //      —— `HandlerException` 全部被遮罩。
            // 只有成功分支才发 11005。这里照抄，不做「修正」。
            if let UseOutcome::Fail(_) = &out {
                return Ok(Some(use_resp(&out).encode_to_vec().into()));
            }
            let resp = match &out {
                UseOutcome::Accept {
                    has_msg_use_result, ..
                } => ItemEffectiveUseResp11005 {
                    ret: 0,
                    msg_use_result: has_msg_use_result.then(MsgUseResult::default),
                },
                UseOutcome::Fail(_) => unreachable!("上一段已提前返回"),
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
                    // 1) 过期条目：原版在扫描阶段就 `iterator.remove()`。
                    let expired: Vec<i64> = plan.expired.clone();
                    // 2) 用掉的条目：`removeNum(player, removeNum, false, ...)`
                    //    第 3 参是 **false**，所以只把 `num` 扣到 0（含 clamp），
                    //    **不从 map 里删除** —— 删除只有 `Iterator.remove` 才做。
                    let used: Vec<(i64, i64)> = plan.used.clone();
                    let pushed: Vec<crate::push::BagEffectiveItem> = w.with(p, |b| {
                        for id in &expired {
                            b.effective.remove(id);
                        }
                        let mut out = Vec::new();
                        for (id, rn) in &used {
                            if let Some((tid, have, deadline)) = b.effective.get_mut(id) {
                                *have = (*have - rn).max(0);
                                out.push(crate::push::BagEffectiveItem {
                                    id: *id,
                                    template_id: *tid,
                                    num: *have,
                                    deadline: *deadline,
                                });
                            }
                        }
                        out
                    });
                    // 原版循环结束后：`if (useChessPot.size() > 0) sendItemEffectiveUpdate(...)`
                    // → 一条 11007 带全部用过的条目（空列表时不发）。
                    if !pushed.is_empty() {
                        if let Some(sess) = ctx.session.as_ref() {
                            if let Some(msg) = crate::push::build_effective_update(&pushed) {
                                let cmd = ddj_pb::cmd::item::EFFECTIVE_UPDATE_RESP_11007;
                                sess.push(cmd, msg.encode_to_vec().into());
                            }
                        }
                    }
                    // 3) 消耗的资源：`removeKeysJson` 合并后 `revmoeResources`。
                    //    当前 MemWorld 的 `check_item` 只看背包，实际扣减在此补齐。
                    for (cid, cnum) in &plan.cost {
                        w.with(p, |b| {
                            if let Some(v) = b.bag.get_mut(cid) {
                                *v = (*v - cnum).max(0);
                            }
                        });
                    }
                    // 原版 L56-58：先 `builder.setMsgUseResult(useResultBuilder)`，
                    // **无条件**设置 —— 即使 `getItemGain()` 为空也带一个空的
                    // `MsgUseResult`（proto2 下表现为 presence=true、无子字段）。
                    ItemEffectiveOneKeyUseResp11006 {
                        ret: 0,
                        msg_use_result: Some(MsgUseResult {
                            msg_resources: resources_to_msg(&plan.gains),
                            ..Default::default()
                        }),
                    }
                }
                // 原版 11006 的 `catch(Exception)` 同样发 `ItemUseResp_11002{ret=2}`
                // （从 11002 复制粘贴，连消息类型都没改）。照抄。
                OneKeyOutcome::Reject(f) => {
                    let fail = UseOutcome::Fail(*f);
                    return Ok(Some(use_resp(&fail).encode_to_vec().into()));
                }
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
