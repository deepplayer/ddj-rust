//! 道具处理器（背包）
//!
//! 对应 Java：`bf.game.server.message.handler.ItemMessageHandler`（5 个方法）＋
//! `bf.game.server.module.item.ItemSet`（20 个方法）。
//!
//! ## 原 handler 方法清单（字节码实测，仅 5 个）
//! | cmdId | 方法 | 返回 |
//! |---|---|---|
//! | 11002 | `itemUseReq` | `void`（自己回包） |
//! | 11003 | `itemCompound` | `Message`（框架回包） |
//! | 11005 | `itemEffectiveUseReq` | `void` |
//! | 11006 | `itemEffectiveOneKeyUseReq` | `void` |
//! | 11008 | `confirmOverdue` | `void` |
//!
//! 注意：**没有 11001「背包列表」**。背包数据由登录时的同步消息与
//! `ItemUpdateResp_11004` 推送维护，客户端本地持有。
//!
//! ## `ItemSet.itemUseReq` 逐指令还原（540 字节，`/tmp/itemuse.txt`）
//! ```text
//!  v6 = allItem.get(templateId)          // 14: astore v6
//!  v7 = 0L                               // removeNum
//!  lock.lockWrite()                      // 23
//!  try {
//!    if (v6 == null || num <= 0)                        // 29/35
//!        throw HandlerException(COMMON_SERVER_BUSY)      // 2
//!    v9 = ItemDBCache.getCache().getItemTemplate(templateId)   // 49
//!    if (num > v9.getUseLimit())                        // 58-66
//!        throw HandlerException(COMMON_SERVER_BUSY)      // 2
//!    v10 = v9.getType()                                 // 80
//!    if (v10 == TypeResources.EFFECTIVE.getType()       // 13
//!     || v10 == TypeResources.CHESS_SCORE.getType())    // 14
//!        throw HandlerException(COMMON_SERVER_BUSY)      // 2
//!    v7 = v6.getUseRemoveNum(player, num, param)        // 120-129
//!    if (v6.getNum() < v7) {                            // 131-139
//!        sendToPlayerId(resp{ret=ITEM_ERROR_2=1102}, playerId)  // 142
//!        unlockWrite(); return                          // 164-171
//!    }
//!    unlockWrite();                                     // 172-176
//!  } catch (any) { unlockWrite(); throw; }              // 182-193
//!  v9 = GiftDBCache.getCache().getGiftDB(v6.getTemplateId())      // 194-205
//!  v10 = GiftDBUtils.checkForUsing(v9, player)                    // 207-213
//!  if (v10 != null) { sendToPlayerId(resp{ret=v10.getValue()}); return; }  // 215-241
//!  if (v9.getUseCostJson() != null                                 // 242-247
//!      && !checkItem(costJson.getId(),
//!                    Long.valueOf(costJson.getNum()) * v7))      // 250-279
//!      throw HandlerException(COMMON_SERVER_USE_COST_RESOURCES_NO=26)  // 282-292
//!  v11 = null
//!  try { v11 = v6.use(player, num, param); }                        // 296-305
//!  catch (Exception e) {                                            // 352
//!      log.error("道具使用逻辑异常 但这里也会当做使用成功 : " + templateId);
//!      v11 = ItemUseResult.createSucceed(new UseResult());          // 392-402
//!  }
//!  v12 = v11.getLanguageUtils()
//!  if (v12 != null) {                                               // 314-316
//!      sendToPlayerId(resp{ret=v12.getValue()}, playerId);          // 319-337
//!      log.error("使用道具失败"); return;                            // 340-348
//!  }
//!  v6.removeNum(player, v7, false, TYPE_502, true, new String[0])    // 404-418
//!  if (v9.getUseCostJson() != null)                                 // 421-426
//!      revmoeResources(player,
//!          [ResourcesJson(cost.id, str(cost.num * v7))], TYPE_507, [])   // 429-485
//!  ItemServerModule.getInstance().sendItemUpdate(playerId, v6)       // 486-495
//!  v12 = v11.getMsgUseResult()                                      // 498-503
//!  builder.setRet(0); if (v12 != null) builder.setMsgUseResult(v12)  // 505-529
//!  player.sendMessage(builder.build())                              // 530-536
//! ```
//!
//! ## ⚠ 原版把 26 号错误码遮罩成了 2
//! `ItemMessageHandler.itemUseReq` 外层有 `catch (Exception)`（字节码 31-62），
//! 捕获后**无条件**发 `ItemUseResp_11002{ret = COMMON_SERVER_BUSY(2)}`。
//! 所以 `HandlerException(COMMON_SERVER_USE_COST_RESOURCES_NO=26)`（分支 14）
//! 在协议上最终表现为 **ret=2**，玩家看不到「资源不够」。
//!
//! 这是原版行为，必须 1:1 复刻（见 [`UseOutcome::resolve_ret`]）。
//! 内部用 [`UseFailure::Handler`] 保留真实原因，便于日志与后续修正。

use ddj_core::{AppError, Result};
use ddj_pb::RetCode;

/// `TypeResources.EFFECTIVE.getType()`（走 11005 时效道具流程）。
pub const TYPE_EFFECTIVE: i32 = 13;
/// `TypeResources.CHESS_SCORE.getType()`（走棋局积分流程）。
pub const TYPE_CHESS_SCORE: i32 = 14;
/// `TypeModule.TYPE_502`（`item.removeNum` 的消耗来源）。
pub const TYPE_502: i32 = 502;
/// `TypeModule.TYPE_507`（`revmoeResources` 的资源消耗来源）。
pub const TYPE_507: i32 = 507;

/// 道具模板中本流程用到的两列（`bf.template.db.ItemDB`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemTemplate {
    /// `getUseLimit()`：单次使用数量上限。
    pub use_limit: i32,
    /// `getType()`：道具类型（`TypeResources`）。
    pub item_type: i32,
}

/// 礼包配置中本流程用到的列（`bf.template.db.GiftDB`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gift {
    /// `getUseCostJson()`：使用代价（资源 id, 每次消耗量）。
    pub use_cost: Option<(i32, i64)>,
}

/// `Item.use()` 的结果（`ItemUseResult`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UseApply {
    /// `getLanguageUtils()`：非 `None` 表示使用失败。
    pub err: Option<RetCode>,
    /// `getMsgUseResult() != null` 时为 `true`（回包要带 `msgUseResult`）。
    pub has_msg_use_result: bool,
}

/// 数据源（模板表 / 背包 / 资源 / 使用逻辑）。
///
/// 生产实现从 PostgreSQL 与模板缓存取数；测试用内存实现。
pub trait ItemLookup {
    /// `ItemDBCache.getItemTemplate(templateId)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn item_template(&self, template_id: i32) -> Result<Option<ItemTemplate>>;
    /// `GiftDBCache.getGiftDB(templateId)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn gift(&self, template_id: i32) -> Result<Option<Gift>>;
    /// `GiftDBUtils.checkForUsing(gift, player)`：`Some(lu)` 表示不允许使用。
    ///
    /// # Errors
    /// 数据层故障。
    fn gift_check_for_using(&self, gift: &Gift) -> Result<Option<RetCode>>;
    /// `ItemFunUtils.get(template).getUseRemoveNum(player, num, param)`。
    ///
    /// 默认实现返回 `num`；各 `ItemFun_NN` 子类按类型覆写。
    ///
    /// # Errors
    /// 数据层故障。
    fn use_remove_num(&self, template: &ItemTemplate, num: i64) -> Result<i64> {
        let _ = template;
        Ok(num)
    }
    /// `ItemSet.checkItem(id, num)`：资源是否足够。
    ///
    /// # Errors
    /// 数据层故障。
    fn check_item(&self, res_id: i32, num: i64) -> Result<bool>;
    /// `Item.use(player, num, param)`。
    ///
    /// # Errors
    /// 数据层故障（原版把 `use()` 抛出的异常**当作成功**，见分支 352）。
    fn apply_use(&self, template: &ItemTemplate, num: i64) -> Result<UseApply>;
}

/// 失败类型 —— 决定最终回哪个 `ret`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseFailure {
    /// `throw HandlerException(lu)`：被 handler 外层 `catch` 遮罩成 ret=2。
    Handler(RetCode),
    /// 直接 `sendToPlayerId(ItemUseResp_11002{ret})`。
    Resp(RetCode),
}

/// `use` 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseOutcome {
    /// 全部校验通过，需要执行副作用后回 `ret=0`。
    Accept {
        /// `v7` —— 实际扣除数量（**不是**请求里的 `num`）。
        remove_num: i64,
        /// 代价（资源 id, 总量 = 每次量 * `remove_num`）。
        cost: Option<(i32, i64)>,
        /// 回包是否带 `msgUseResult`。
        has_msg_use_result: bool,
    },
    /// 失败。
    Fail(UseFailure),
}

impl UseOutcome {
    /// 协议上最终回给客户端的 `ret`；`None` 表示成功（`ret=0`）。
    ///
    /// [`UseFailure::Handler`] 一律映射为 [`RetCode::ServerBusy`] ——
    /// 这是原版 handler 层 `catch (Exception)` 的遮罩行为，不可「修正」。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Accept { .. } => None,
            Self::Fail(UseFailure::Handler(_)) => Some(RetCode::ServerBusy),
            Self::Fail(UseFailure::Resp(r)) => Some(*r),
        }
    }

    /// 真实失败原因（仅日志用；协议上已被遮罩）。
    #[must_use]
    pub const fn real_reason(&self) -> Option<RetCode> {
        match self {
            Self::Accept { .. } => None,
            Self::Fail(UseFailure::Handler(r) | UseFailure::Resp(r)) => Some(*r),
        }
    }
}

/// 背包侧快照（跨 await 时先取出，避免持锁）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BagSnapshot {
    /// `allItem.get(templateId) != null`。
    pub has_item: bool,
    /// `item.getNum()`。
    pub item_num: i64,
}

/// `ItemSet.itemUseReq` 的纯逻辑部分（不含锁、不含 IO 副作用）。
///
/// 严格按字节码分支序；锁在原实现里只覆盖到 `getNum()` 比较为止，
/// Rust 侧由仓储层用行级锁保证，语义等价（粒度更细）。
///
/// # Errors
/// 仅数据层故障向上冒泡；业务拒绝走 [`UseOutcome::Fail`]。
pub fn use_item(
    bag: BagSnapshot,
    template_id: i32,
    num: i64,
    lookups: &dyn ItemLookup,
) -> Result<UseOutcome> {
    // 分支 1（字节码 29/35）：item == null || num <= 0 → HandlerException(BUSY)
    if !bag.has_item || num <= 0 {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    }
    // 分支 2（49）：取模板
    let Some(tpl) = lookups.item_template(template_id)? else {
        // 原版对 null 模板会在 58 行 NPE → 被 handler 遮罩成 BUSY
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    };
    // 分支 3（58-66）：num > useLimit → BUSY
    if num > i64::from(tpl.use_limit) {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    }
    // 分支 4（80-119）：EFFECTIVE / CHESS_SCORE 走专属流程 → BUSY
    if tpl.item_type == TYPE_EFFECTIVE || tpl.item_type == TYPE_CHESS_SCORE {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    }
    // 分支 5（120-129）
    let remove_num = lookups.use_remove_num(&tpl, num)?;
    // 分支 6（131-171）：数量不足 → 直接回 ITEM_ERROR_2(1102)
    if bag.item_num < remove_num {
        return Ok(UseOutcome::Fail(UseFailure::Resp(RetCode::ItemError2)));
    }
    // 分支 7（194-205）：`GiftDBCache.getGiftDB` 是纯 map 查表（`allGiftDB.get(id)`），
    // 模板里没有这件道具时返回 **null**；原版紧接着调
    // `GiftDBUtils.checkForUsing(null, player)`，其首行就是 `giftDB.getUseLimit()`
    // → NullPointerException → 冒泡到 `ItemMessageHandler.itemUseReq` 的
    // `catch (Exception)` → 统一回 `ItemUseResp_11002{ret=COMMON_SERVER_BUSY(2)}`，
    // 道具不扣除、效果不生效。
    //
    // 实测覆盖面：`D-道具表-Item.csv` 2674 条里，只有 1066 条出现在
    // `D-道具使用效果-Gift.csv`；剩下 **1608 条**（type 非 13/14）走到这里必然 NPE。
    // 即原版对这些道具一律回「服务器繁忙」。这里按 1:1 复刻，不改原版行为。
    let Some(gift) = gift else {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    };
    // 分支 8（207-241）：礼包禁用检查 → 直接回该码
    if let Some(code) = lookups.gift_check_for_using(&gift)? {
        return Ok(UseOutcome::Fail(UseFailure::Resp(code)));
    }
    // 分支 9（242-292）：代价不足 → HandlerException(26)
    let mut cost = None;
    if let Some((res_id, per)) = gift.use_cost {
        let total = per.saturating_mul(remove_num);
        if !lookups.check_item(res_id, total)? {
            return Ok(UseOutcome::Fail(UseFailure::Handler(
                RetCode::UseCostResourcesNo,
            )));
        }
        cost = Some((res_id, total));
    }
    // 分支 10（296-402）：use()；异常按成功处理（原版注释明确）
    let applied = lookups.apply_use(&tpl, num).unwrap_or_else(|e| {
        tracing::error!(%e, template_id, "道具使用逻辑异常 但这里也会当做使用成功");
        UseApply::default()
    });
    // 分支 11（307-348）：use 内部返回错误码
    if let Some(code) = applied.err {
        tracing::error!(template_id, "使用道具失败");
        return Ok(UseOutcome::Fail(UseFailure::Resp(code)));
    }
    // 分支 12-13（404-536）：交给调用方执行 removeNum / revmoeResources / 推送
    Ok(UseOutcome::Accept {
        remove_num,
        cost,
        has_msg_use_result: applied.has_msg_use_result,
    })
}

/// 把错误码包装成内部错误（仅用于跨层返回 `Err` 的边界场景）。
#[must_use]
pub fn outcome_error(ret: RetCode) -> AppError {
    AppError::BadRequest(format!("ret={} {}", ret.as_i32(), ret.text()))
}

// ─────────────────────────── 11003 合成 ───────────────────────────

/// `ItemDB.getCompoundItem()` 的镜像：`[源道具 id, 每次消耗数量]`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundTemplate {
    /// `getType()`。
    pub item_type: i32,
    /// `getCompoundItem()`（Java `List<Integer>`）。
    pub compound_item: Vec<i32>,
}

/// 合成所需数据源。
pub trait CompoundLookup {
    /// `ItemDBCache.getItemTemplate(id)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn item(&self, id: i32) -> Result<Option<CompoundTemplate>>;
    /// `ItemSet.checkItem(map)` —— 源道具数量是否足够。
    ///
    /// # Errors
    /// 数据层故障。
    fn has_items(&self, id: i32, num: i64) -> Result<bool>;
}

/// `ItemCompoundReq_11003` 的入参。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompoundInput {
    /// `getCompoundId()`。
    pub compound_id: i32,
    /// `getCompoundNum()`。
    pub compound_num: i64,
    /// `hasUseItemId() ? getUseItemId() : null`。
    pub use_item_id: Option<i32>,
}

/// 合成结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundOutcome {
    /// 回 `ItemCompoundResp_11003{ret}`。
    Reject(RetCode),
    /// 扣 `consume_num` 个 `consume_id`，再产出 `compound_num` 个 `compound_id`。
    Accept {
        /// 被消耗的道具 id。
        consume_id: i32,
        /// 被消耗的数量（可能经过多层连乘）。
        consume_num: i64,
    },
}

/// 链式追溯的最大深度。
///
/// 原版 `while` 循环在 `compoundItem` 成环时会**死循环**（Java 里没有防护）。
/// 这里加一个上限：正常数据永远碰不到，畸形数据返回 `PARAM_ERROR`。
pub const MAX_COMPOUND_DEPTH: usize = 64;

/// `ItemSet.itemCompound` 的纯逻辑部分（字节码 487 B）。
///
/// ## 分支序（不可调换）
/// ```text
///  1. compoundNum <= 0                              → PARAM_ERROR(29)
///  2. getItemTemplate(compoundId) == null            → 29
///  3. type ∈ {EFFECTIVE(13), CHESS_SCORE(14)}        → 29
///  4. 无 useItemId：
///       ci = tpl.getCompoundItem()
///       consume = (ci[0], ci[1] * compoundNum)
///       checkItem 不足                               → ITEM_ERROR_1(1101)
///  5. 有 useItemId：沿 compoundItem[0] 链式下钻
///       loop { t = item(cur); t==null→29; ci.size<2→29;
///              qty *= ci[1];
///              if ci[0] == useItemId { break }  else { cur = ci[0] } }
///       qty <= 0                                     → 29
///       checkItem 不足                               → 1101
///  6. 扣源道具、产出目标、ret = 0
/// ```
///
/// # Errors
/// 仅数据层故障向上冒泡。
pub fn compound(input: CompoundInput, lookups: &dyn CompoundLookup) -> Result<CompoundOutcome> {
    // 1
    if input.compound_num <= 0 {
        return Ok(CompoundOutcome::Reject(RetCode::ParamError));
    }
    // 2
    let Some(tpl) = lookups.item(input.compound_id)? else {
        return Ok(CompoundOutcome::Reject(RetCode::ParamError));
    };
    // 3
    if tpl.item_type == TYPE_EFFECTIVE || tpl.item_type == TYPE_CHESS_SCORE {
        return Ok(CompoundOutcome::Reject(RetCode::ParamError));
    }
    let Some(use_item_id) = input.use_item_id else {
        // 4：单层合成
        if tpl.compound_item.len() < 2 {
            // 原版此处会抛 IndexOutOfBounds；按参数错误收敛
            return Ok(CompoundOutcome::Reject(RetCode::ParamError));
        }
        let src = tpl.compound_item[0];
        let per = i64::from(tpl.compound_item[1]);
        let total = per.saturating_mul(input.compound_num);
        if !lookups.has_items(src, total)? {
            return Ok(CompoundOutcome::Reject(RetCode::ItemError1));
        }
        return Ok(CompoundOutcome::Accept {
            consume_id: src,
            consume_num: total,
        });
    };
    // 5：沿链下钻到 useItemId
    let mut cur = input.compound_id;
    let mut qty = input.compound_num;
    for _ in 0..MAX_COMPOUND_DEPTH {
        let Some(t) = lookups.item(cur)? else {
            return Ok(CompoundOutcome::Reject(RetCode::ParamError));
        };
        if t.compound_item.len() < 2 {
            return Ok(CompoundOutcome::Reject(RetCode::ParamError));
        }
        let src = t.compound_item[0];
        qty = qty.saturating_mul(i64::from(t.compound_item[1]));
        if src == use_item_id {
            if qty <= 0 {
                return Ok(CompoundOutcome::Reject(RetCode::ParamError));
            }
            if !lookups.has_items(use_item_id, qty)? {
                return Ok(CompoundOutcome::Reject(RetCode::ItemError1));
            }
            return Ok(CompoundOutcome::Accept {
                consume_id: use_item_id,
                consume_num: qty,
            });
        }
        cur = src;
    }
    Ok(CompoundOutcome::Reject(RetCode::ParamError))
}

// ─────────────────────── 11008 过期确认 ───────────────────────

/// 时效道具（`bf.game.server.module.item.ItemEffective`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveItem {
    /// 在 `allEffectiveItem` 里的键（`Long id`）。
    pub id: i64,
    /// `getTemplateId()`。
    pub template_id: i32,
    /// `getNum()`。
    pub num: i64,
    /// `getDeadline()`（毫秒）。
    pub deadline: i64,
}

/// 时效道具数据源。
pub trait EffectiveLookup {
    /// `allEffectiveItem.get(id)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn get(&self, id: i64) -> Result<Option<EffectiveItem>>;
    /// `eff.setSendClient(true)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn mark_sent(&self, id: i64) -> Result<()>;
}

/// `ItemSet.confirmOverdue` 的纯逻辑部分（字节码 148 B）。
///
/// 对请求里每个 id：存在且 `now >= deadline` → 标记 `sendClient = true` 并
/// 收进回包 `ids`。回包恒 `ret = 0`。
///
/// 返回需要放进 `ConfirmOverdueResp_11008.ids` 的 id 列表（保持请求顺序）。
///
/// # Errors
/// 仅数据层故障向上冒泡。
pub fn confirm_overdue(ids: &[i64], now: i64, lookups: &dyn EffectiveLookup) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for &id in ids {
        let Some(eff) = lookups.get(id)? else {
            continue;
        };
        if now >= eff.deadline {
            lookups.mark_sent(id)?;
            out.push(id);
        }
    }
    Ok(out)
}

/// `ItemSet.hasEffectiveItem(templateId)`：是否还有未过期的同模板时效道具。
#[must_use]
pub fn has_effective_item(template_id: i32, now: i64, items: &[(i32, EffectiveItem)]) -> bool {
    items
        .iter()
        .any(|(tid, eff)| *tid == template_id && eff.num > 0 && eff.deadline > now)
}

// ──────────────────── 11005 时效道具使用 ────────────────────

/// `ItemDB` 中 11005 用到的字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveUseTemplate {
    /// `getUseLimit()`。
    pub use_limit: i32,
}

/// 11005 所需数据源。
pub trait EffectiveUseLookup {
    /// `allEffectiveItem.get(id)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn effective(&self, id: i64) -> Result<Option<EffectiveItem>>;
    /// `ItemDBCache.getItemTemplate(templateId)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn item_template(&self, template_id: i32) -> Result<Option<EffectiveUseTemplate>>;
    /// `ItemEffective.getUseRemoveNum(player, num, param)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn use_remove_num(&self, id: i64, num: i64) -> Result<i64>;
    /// `GiftDBCache.getGiftDB(templateId)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn gift(&self, template_id: i32) -> Result<Option<Gift>>;
    /// `GiftDBUtils.checkForUsing(gift, player)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn gift_check_for_using(&self, template_id: i32) -> Result<Option<RetCode>>;
    /// `ItemSet.checkItem(id, num)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn check_item(&self, id: i32, num: i64) -> Result<bool>;
    /// `ItemEffective.use(player, num, param)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn apply_use(&self, id: i64, num: i64) -> Result<UseApply>;
}

/// `ItemSet.itemEffectiveUse` 的纯逻辑部分（字节码 487 B）。
///
/// ## 与 11002 的差异（逐条对齐字节码）
/// | 项 | 11002 `Item.itemUseReq` | 11005 `ItemEffective.itemEffectiveUse` |
/// |---|---|---|
/// | 容器 | `allItem` | `allEffectiveItem` |
/// | 首检 | `null ‖ num<=0` → BUSY | `!containsKey` → **RESOURCES_NO(6)** |
/// | 过期 | 无 | `now >= deadline` → **ITEM_ERROR_11(1111)** |
/// | `use()` 异常 | `catch` 后按成功处理 | **不捕获，向上抛** |
/// | 成功包 | `ItemUseResp_11002` | `ItemEffectiveUseResp_11005` |
/// | `removeNum` 第 4 参 | `false` | **`true`** |
///
/// # Errors
/// 仅数据层故障向上冒泡。
pub fn effective_use(
    id: i64,
    num: i64,
    now: i64,
    lookups: &dyn EffectiveUseLookup,
) -> Result<UseOutcome> {
    // 1
    let Some(eff) = lookups.effective(id)? else {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ResourcesNo)));
    };
    // 2
    if now >= eff.deadline {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ItemError11)));
    }
    // 3
    if num <= 0 {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    }
    // 4
    let Some(tpl) = lookups.item_template(eff.template_id)? else {
        // 原版此处 NPE → Handler 外层 catch → ret=2
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    };
    // 5
    if num > i64::from(tpl.use_limit) {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    }
    // 6
    let remove_num = lookups.use_remove_num(id, num)?;
    // 7
    if eff.num < remove_num {
        return Ok(UseOutcome::Fail(UseFailure::Resp(RetCode::ItemError2)));
    }
    // 8：与 11002 相同，原版在这里不判空 —— `checkForUsing(null)` 直接 NPE，
    // 冒泡到 handler 的 catch → `ItemUseResp_11002{ret=2}`。
    let Some(gift) = lookups.gift(eff.template_id)? else {
        return Ok(UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy)));
    };
    // 9
    if let Some(code) = lookups.gift_check_for_using(eff.template_id)? {
        return Ok(UseOutcome::Fail(UseFailure::Resp(code)));
    }
    // 10
    let cost = gift.use_cost;
    if let Some((cid, cnum)) = cost {
        if !lookups.check_item(cid, cnum.saturating_mul(remove_num))? {
            return Ok(UseOutcome::Fail(UseFailure::Handler(
                RetCode::UseCostResourcesNo,
            )));
        }
    }
    // 11
    let applied = lookups.apply_use(id, num)?;
    if let Some(code) = applied.err {
        return Ok(UseOutcome::Fail(UseFailure::Resp(code)));
    }
    Ok(UseOutcome::Accept {
        remove_num,
        cost: cost.map(|(cid, cnum)| (cid, cnum.saturating_mul(remove_num))),
        has_msg_use_result: applied.has_msg_use_result,
    })
}

// ──────────────── 11006 时效道具一键使用 ────────────────

/// `ItemSet$1` 比较器：**按 `deadline` 升序**（先过期的先处理），相等返回 0。
#[must_use]
pub fn one_key_sort_key(eff: &EffectiveItem) -> i64 {
    eff.deadline
}

/// 一键使用里单条 `ItemEffective.use()` 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OneKeyApplied {
    /// `getLanguageUtils()`：非 `None` 表示失败（原版此时 **break**，不返回错误码）。
    pub err: Option<RetCode>,
    /// `getMsgUseResult().getMsgResourcesList()` 转成的 `(id, num)`。
    pub msg_resources: Vec<(i32, i64)>,
}

/// 11006 所需数据源。
pub trait OneKeyLookup {
    /// `GiftDBCache.getGiftDB(templateId)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn gift(&self, template_id: i32) -> Result<Option<Gift>>;
    /// `GiftDBUtils.checkForUsing(gift, player)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn gift_check_for_using(&self, template_id: i32) -> Result<Option<RetCode>>;
    /// `ItemSet.checkItem(id, num)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn check_item(&self, id: i32, num: i64) -> Result<bool>;
    /// `ItemEffective.getUseRemoveNum(player, num, param)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn use_remove_num(&self, id: i64, num: i64) -> Result<i64>;
    /// `ItemEffective.use(player, removeNum, param)`。
    ///
    /// # Errors
    /// 数据层故障。
    fn apply_use(&self, id: i64, num: i64) -> Result<OneKeyApplied>;
}

/// 一键使用的执行计划（原版边算边改，这里把副作用摊平成数据）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OneKeyPlan {
    /// 扫描时顺手从 `allEffectiveItem` 删掉的过期条目 id。
    pub expired: Vec<i64>,
    /// 成功使用并已被 `removeNum` 的 `(id, remove_num)`。
    pub used: Vec<(i64, i64)>,
    /// 已累计的消耗（同 id 已合并）。
    pub cost: Vec<(i32, i64)>,
    /// 累计产出（同 id 已合并）。
    pub gains: Vec<(i32, i64)>,
    /// 是否因为某条失败而提前 `break`（原版不报错，静默收尾）。
    pub interrupted: bool,
}

/// 一键使用结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneKeyOutcome {
    /// 回 `ItemUseResp_11002{ret}`（Handler 层被遮罩成 2）。
    Reject(UseFailure),
    /// 成功，回 `ItemEffectiveOneKeyUseResp_11006{ret=0, msgUseResult.msgResources}`。
    Accept(OneKeyPlan),
}

fn merge_pairs(list: &[(i32, i64)]) -> Vec<(i32, i64)> {
    let mut out: Vec<(i32, i64)> = Vec::new();
    for &(id, num) in list {
        match out.iter_mut().find(|(k, _)| *k == id) {
            Some(slot) => slot.1 = slot.1.saturating_add(num),
            None => out.push((id, num)),
        }
    }
    out
}

/// `ItemSet.itemEffectiveOneKeyUse` 的纯逻辑部分（字节码 629 B）。
///
/// ## 与 11005 的关键差异（字节码实证）
/// 1. 扫描时对过期条目执行 `iterator.remove()` —— **一键使用会顺带清理过期数据**。
/// 2. 排序用 `ItemSet$1`：**`deadline` 升序**。
/// 3. 循环内中断条件是 `removeNum < getNum()`（方向与 11005 相反）。
/// 4. 循环内 `use()` 失败**只 break**，不返回错误码，已成功的部分照常结算。
/// 5. 循环外预校验用的是**单份** `cost.num`，不是累计值。
/// 6. `useCostJson == null` 时原版直接 NPE（`invokevirtual` on null）→ 外层 catch → ret=2。
///
/// # Errors
/// 仅数据层故障向上冒泡。
pub fn effective_one_key_use(
    template_id: i32,
    now: i64,
    items: &[EffectiveItem],
    lookups: &dyn OneKeyLookup,
) -> Result<OneKeyOutcome> {
    let mut plan = OneKeyPlan::default();
    let mut candidates: Vec<EffectiveItem> = Vec::new();
    for eff in items {
        if now >= eff.deadline {
            plan.expired.push(eff.id);
            continue;
        }
        if eff.num <= 0 || eff.template_id != template_id {
            continue;
        }
        candidates.push(*eff);
    }
    // `List.sort` 是稳定排序，用 sort_by_key 保持一致语义
    candidates.sort_by_key(one_key_sort_key);

    let Some(gift) = lookups.gift(template_id)? else {
        if candidates.is_empty() {
            return Ok(OneKeyOutcome::Reject(UseFailure::Handler(
                RetCode::ResourcesNo,
            )));
        }
        return Ok(OneKeyOutcome::Reject(UseFailure::Handler(
            RetCode::ServerBusy,
        )));
    };
    if candidates.is_empty() {
        return Ok(OneKeyOutcome::Reject(UseFailure::Handler(
            RetCode::ResourcesNo,
        )));
    }
    let Some((cost_id, cost_num)) = gift.use_cost else {
        // 原版此处 NPE，被外层 catch 遮罩成 ret=2
        return Ok(OneKeyOutcome::Reject(UseFailure::Handler(
            RetCode::ServerBusy,
        )));
    };
    // 循环外：只校验单份
    if !lookups.check_item(cost_id, cost_num)? {
        return Ok(OneKeyOutcome::Reject(UseFailure::Handler(
            RetCode::UseCostResourcesNo,
        )));
    }

    let mut total_cost = 0_i64;
    for eff in &candidates {
        if lookups.gift_check_for_using(template_id)?.is_some() {
            plan.interrupted = true;
            break;
        }
        let remove_num = lookups.use_remove_num(eff.id, eff.num)?;
        // 原版：removeNum < getNum() → break
        if remove_num < eff.num {
            plan.interrupted = true;
            break;
        }
        let this_cost = cost_num.saturating_mul(remove_num);
        // 循环内：累计校验
        if !lookups.check_item(cost_id, total_cost.saturating_add(this_cost))? {
            plan.interrupted = true;
            break;
        }
        let applied = lookups.apply_use(eff.id, remove_num)?;
        if applied.err.is_some() {
            plan.interrupted = true;
            break;
        }
        plan.used.push((eff.id, remove_num));
        plan.cost.push((cost_id, this_cost));
        total_cost = total_cost.saturating_add(this_cost);
        plan.gains.extend(applied.msg_resources);
    }
    plan.cost = merge_pairs(&plan.cost);
    plan.gains = merge_pairs(&plan.gains);
    Ok(OneKeyOutcome::Accept(plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        templates: HashMap<i32, ItemTemplate>,
        gifts: HashMap<i32, Gift>,
        gift_block: Option<RetCode>,
        resources: HashMap<i32, i64>,
        apply: Option<UseApply>,
        apply_err: bool,
        remove_override: Option<i64>,
    }

    impl ItemLookup for Fake {
        fn item_template(&self, id: i32) -> Result<Option<ItemTemplate>> {
            Ok(self.templates.get(&id).copied())
        }
        fn gift(&self, id: i32) -> Result<Option<Gift>> {
            Ok(self.gifts.get(&id).copied())
        }
        fn gift_check_for_using(&self, _g: &Gift) -> Result<Option<RetCode>> {
            Ok(self.gift_block)
        }
        fn use_remove_num(&self, _t: &ItemTemplate, num: i64) -> Result<i64> {
            Ok(self.remove_override.unwrap_or(num))
        }
        fn check_item(&self, res_id: i32, num: i64) -> Result<bool> {
            Ok(self.resources.get(&res_id).copied().unwrap_or(0) >= num)
        }
        fn apply_use(&self, _t: &ItemTemplate, _num: i64) -> Result<UseApply> {
            if self.apply_err {
                return Err(AppError::BadRequest("boom".into()));
            }
            Ok(self.apply.unwrap_or_default())
        }
    }

    fn bag(num: i64) -> BagSnapshot {
        BagSnapshot {
            has_item: true,
            item_num: num,
        }
    }

    fn normal() -> Fake {
        let mut f = Fake::default();
        f.templates.insert(
            11002,
            ItemTemplate {
                use_limit: 999,
                item_type: 2,
            },
        );
        f
    }

    #[test]
    fn no_item_is_busy() {
        let f = normal();
        let out = use_item(
            BagSnapshot {
                has_item: false,
                item_num: 5,
            },
            11002,
            1,
            &f,
        )
        .expect("ok");
        assert_eq!(
            out,
            UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy))
        );
        assert_eq!(out.resolve_ret(), Some(RetCode::ServerBusy));
    }

    #[test]
    fn zero_num_is_busy() {
        let f = normal();
        assert_eq!(
            use_item(bag(10), 11002, 0, &f).expect("ok").resolve_ret(),
            Some(RetCode::ServerBusy)
        );
    }

    #[test]
    fn negative_num_is_busy() {
        let f = normal();
        assert_eq!(
            use_item(bag(10), 11002, -3, &f).expect("ok").resolve_ret(),
            Some(RetCode::ServerBusy)
        );
    }

    #[test]
    fn over_use_limit_is_busy() {
        let mut f = normal();
        f.templates.insert(
            11002,
            ItemTemplate {
                use_limit: 5,
                item_type: 2,
            },
        );
        assert_eq!(
            use_item(bag(100), 11002, 6, &f).expect("ok").resolve_ret(),
            Some(RetCode::ServerBusy)
        );
    }

    #[test]
    fn at_use_limit_is_ok() {
        let mut f = normal();
        f.templates.insert(
            11002,
            ItemTemplate {
                use_limit: 5,
                item_type: 2,
            },
        );
        assert!(matches!(
            use_item(bag(100), 11002, 5, &f).expect("ok"),
            UseOutcome::Accept { .. }
        ));
    }

    #[test]
    fn effective_type_is_busy() {
        let mut f = normal();
        f.templates.insert(
            11002,
            ItemTemplate {
                use_limit: 999,
                item_type: TYPE_EFFECTIVE,
            },
        );
        assert_eq!(
            use_item(bag(10), 11002, 1, &f).expect("ok").resolve_ret(),
            Some(RetCode::ServerBusy)
        );
    }

    #[test]
    fn chess_score_type_is_busy() {
        let mut f = normal();
        f.templates.insert(
            11002,
            ItemTemplate {
                use_limit: 999,
                item_type: TYPE_CHESS_SCORE,
            },
        );
        assert_eq!(
            use_item(bag(10), 11002, 1, &f).expect("ok").resolve_ret(),
            Some(RetCode::ServerBusy)
        );
    }

    #[test]
    fn missing_template_is_busy() {
        let f = normal();
        assert_eq!(
            use_item(bag(10), 99999, 1, &f).expect("ok").resolve_ret(),
            Some(RetCode::ServerBusy)
        );
    }

    /// 分支 6 用的是 `removeNum` 而不是 `num`。
    #[test]
    fn insufficient_uses_remove_num_not_num() {
        let mut f = normal();
        f.remove_override = Some(5); // removeNum=5 > 拥有的 3
        let out = use_item(bag(3), 11002, 2, &f).expect("ok");
        assert_eq!(out, UseOutcome::Fail(UseFailure::Resp(RetCode::ItemError2)));
        assert_eq!(out.resolve_ret(), Some(RetCode::ItemError2));
        assert_eq!(RetCode::ItemError2.as_i32(), 1102);
    }

    #[test]
    fn enough_quantity_accepts() {
        let f = normal();
        let out = use_item(bag(10), 11002, 2, &f).expect("ok");
        assert_eq!(
            out,
            UseOutcome::Accept {
                remove_num: 2,
                cost: None,
                has_msg_use_result: false
            }
        );
        assert_eq!(out.resolve_ret(), None);
    }

    #[test]
    fn gift_block_returns_its_code() {
        let mut f = normal();
        f.gifts.insert(11002, Gift { use_cost: None });
        f.gift_block = Some(RetCode::ActivityEnd);
        assert_eq!(
            use_item(bag(10), 11002, 1, &f).expect("ok").resolve_ret(),
            Some(RetCode::ActivityEnd)
        );
    }

    /// 代价 = 每次量 * removeNum（字节码 462 `lload v7`）。
    #[test]
    fn cost_scales_with_remove_num() {
        let mut f = normal();
        f.gifts.insert(
            11002,
            Gift {
                use_cost: Some((5, 100)),
            },
        );
        f.remove_override = Some(3);
        f.resources.insert(5, 1000);
        assert_eq!(
            use_item(bag(10), 11002, 2, &f).expect("ok"),
            UseOutcome::Accept {
                remove_num: 3,
                cost: Some((5, 300)),
                has_msg_use_result: false
            }
        );
    }

    /// 分支 9：代价不足原版抛 26，但 handler 遮罩成 2（协议上看到 2）。
    #[test]
    fn insufficient_cost_is_masked_to_busy() {
        let mut f = normal();
        f.gifts.insert(
            11002,
            Gift {
                use_cost: Some((5, 100)),
            },
        );
        f.resources.insert(5, 10);
        let out = use_item(bag(10), 11002, 1, &f).expect("ok");
        assert_eq!(out.real_reason(), Some(RetCode::UseCostResourcesNo));
        assert_eq!(out.resolve_ret(), Some(RetCode::ServerBusy));
        assert_eq!(RetCode::UseCostResourcesNo.as_i32(), 26);
    }

    #[test]
    fn use_error_code_is_returned_verbatim() {
        let mut f = normal();
        f.apply = Some(UseApply {
            err: Some(RetCode::ItemError4),
            has_msg_use_result: false,
        });
        assert_eq!(
            use_item(bag(10), 11002, 1, &f).expect("ok").resolve_ret(),
            Some(RetCode::ItemError4)
        );
    }

    /// 分支 10：`use()` 抛异常 → 当作成功（原版注释明确）。
    #[test]
    fn use_exception_treated_as_success() {
        let mut f = normal();
        f.apply_err = true;
        assert!(matches!(
            use_item(bag(10), 11002, 2, &f).expect("ok"),
            UseOutcome::Accept { .. }
        ));
    }

    #[test]
    fn msg_use_result_flag_passes_through() {
        let mut f = normal();
        f.apply = Some(UseApply {
            err: None,
            has_msg_use_result: true,
        });
        match use_item(bag(10), 11002, 1, &f).expect("ok") {
            UseOutcome::Accept {
                has_msg_use_result, ..
            } => assert!(has_msg_use_result),
            _ => panic!("expect accept"),
        }
    }
}

#[cfg(test)]
mod compound_tests {
    use std::cell::RefCell;

    use super::*;

    struct Fake {
        tpl: RefCell<Vec<(i32, CompoundTemplate)>>,
        have: RefCell<Vec<(i32, i64)>>,
    }

    impl Fake {
        fn new(tpl: &[(i32, i32, &[i32])], have: &[(i32, i64)]) -> Self {
            Self {
                tpl: RefCell::new(
                    tpl.iter()
                        .map(|(id, ty, ci)| {
                            (
                                *id,
                                CompoundTemplate {
                                    item_type: *ty,
                                    compound_item: ci.to_vec(),
                                },
                            )
                        })
                        .collect(),
                ),
                have: RefCell::new(have.to_vec()),
            }
        }
    }

    impl CompoundLookup for Fake {
        fn item(&self, id: i32) -> Result<Option<CompoundTemplate>> {
            Ok(self
                .tpl
                .borrow()
                .iter()
                .find(|(k, _)| *k == id)
                .map(|(_, v)| v.clone()))
        }

        fn has_items(&self, id: i32, num: i64) -> Result<bool> {
            Ok(self
                .have
                .borrow()
                .iter()
                .any(|(k, v)| *k == id && *v >= num))
        }
    }

    fn input(compound_id: i32, num: i64, use_item_id: Option<i32>) -> CompoundInput {
        CompoundInput {
            compound_id,
            compound_num: num,
            use_item_id,
        }
    }

    #[test]
    fn zero_num_is_param_error() {
        let f = Fake::new(&[(2000, 1, &[1001, 3])], &[(1001, 99)]);
        assert_eq!(
            compound(input(2000, 0, None), &f).unwrap(),
            CompoundOutcome::Reject(RetCode::ParamError)
        );
        assert_eq!(
            compound(input(2000, -1, None), &f).unwrap(),
            CompoundOutcome::Reject(RetCode::ParamError)
        );
    }

    #[test]
    fn missing_template_is_param_error() {
        let f = Fake::new(&[], &[]);
        assert_eq!(
            compound(input(2000, 1, None), &f).unwrap(),
            CompoundOutcome::Reject(RetCode::ParamError)
        );
    }

    #[test]
    fn effective_and_chess_score_types_rejected() {
        for ty in [TYPE_EFFECTIVE, TYPE_CHESS_SCORE] {
            let f = Fake::new(&[(2000, ty, &[1001, 3])], &[(1001, 99)]);
            assert_eq!(
                compound(input(2000, 1, None), &f).unwrap(),
                CompoundOutcome::Reject(RetCode::ParamError),
                "type {ty} 必须拒绝"
            );
        }
    }

    #[test]
    fn single_layer_consumes_src_times_num() {
        let f = Fake::new(&[(2000, 1, &[1001, 3])], &[(1001, 6)]);
        assert_eq!(
            compound(input(2000, 2, None), &f).unwrap(),
            CompoundOutcome::Accept {
                consume_id: 1001,
                consume_num: 6
            }
        );
    }

    #[test]
    fn single_layer_insufficient_is_item_error1() {
        let f = Fake::new(&[(2000, 1, &[1001, 3])], &[(1001, 5)]);
        assert_eq!(
            compound(input(2000, 2, None), &f).unwrap(),
            CompoundOutcome::Reject(RetCode::ItemError1)
        );
    }

    #[test]
    fn single_layer_short_compound_item_is_param_error() {
        let f = Fake::new(&[(2000, 1, &[1001])], &[(1001, 99)]);
        assert_eq!(
            compound(input(2000, 1, None), &f).unwrap(),
            CompoundOutcome::Reject(RetCode::ParamError)
        );
    }

    #[test]
    fn chain_walks_to_use_item_id_and_multiplies() {
        // 2000 ← 2001 ×2，2001 ← 2002 ×5
        let f = Fake::new(
            &[(2000, 1, &[2001, 2]), (2001, 1, &[2002, 5])],
            &[(2002, 10)],
        );
        assert_eq!(
            compound(input(2000, 1, Some(2002)), &f).unwrap(),
            CompoundOutcome::Accept {
                consume_id: 2002,
                consume_num: 10
            }
        );
    }

    #[test]
    fn chain_first_hop_matches_use_item_id() {
        let f = Fake::new(&[(2000, 1, &[2001, 2])], &[(2001, 8)]);
        assert_eq!(
            compound(input(2000, 4, Some(2001)), &f).unwrap(),
            CompoundOutcome::Accept {
                consume_id: 2001,
                consume_num: 8
            }
        );
    }

    #[test]
    fn chain_missing_link_is_param_error() {
        let f = Fake::new(&[(2000, 1, &[2001, 2])], &[(2002, 10)]);
        assert_eq!(
            compound(input(2000, 1, Some(2002)), &f).unwrap(),
            CompoundOutcome::Reject(RetCode::ParamError)
        );
    }

    #[test]
    fn chain_insufficient_src_is_item_error1() {
        let f = Fake::new(
            &[(2000, 1, &[2001, 2]), (2001, 1, &[2002, 5])],
            &[(2002, 9)],
        );
        assert_eq!(
            compound(input(2000, 1, Some(2002)), &f).unwrap(),
            CompoundOutcome::Reject(RetCode::ItemError1)
        );
    }

    #[test]
    fn cycle_hits_depth_guard_instead_of_hanging() {
        // 原版无环检测会死循环；这里必须在上限处收敛为 PARAM_ERROR
        let f = Fake::new(
            &[(3000, 1, &[3001, 2]), (3001, 1, &[3000, 2])],
            &[(9999, 1_000_000)],
        );
        assert_eq!(
            compound(input(3000, 1, Some(9999)), &f).unwrap(),
            CompoundOutcome::Reject(RetCode::ParamError)
        );
    }

    struct EffFake {
        items: RefCell<Vec<(i64, EffectiveItem)>>,
        sent: RefCell<Vec<i64>>,
    }

    impl EffectiveLookup for EffFake {
        fn get(&self, id: i64) -> Result<Option<EffectiveItem>> {
            Ok(self
                .items
                .borrow()
                .iter()
                .find(|(k, _)| *k == id)
                .map(|(_, v)| *v))
        }

        fn mark_sent(&self, id: i64) -> Result<()> {
            self.sent.borrow_mut().push(id);
            Ok(())
        }
    }

    #[test]
    fn overdue_collected_and_marked() {
        let f = EffFake {
            items: RefCell::new(vec![
                (
                    1,
                    EffectiveItem {
                        id: 0,
                        template_id: 0,
                        num: 1,
                        deadline: 100,
                    },
                ),
                (
                    2,
                    EffectiveItem {
                        id: 0,
                        template_id: 0,
                        num: 1,
                        deadline: 500,
                    },
                ),
            ]),
            sent: RefCell::new(vec![]),
        };
        let ids = confirm_overdue(&[1, 2, 3], 200, &f).unwrap();
        assert_eq!(ids, vec![1]);
        assert_eq!(*f.sent.borrow(), vec![1]);
    }

    #[test]
    fn deadline_equal_to_now_is_overdue() {
        let f = EffFake {
            items: RefCell::new(vec![(
                7,
                EffectiveItem {
                    id: 0,
                    template_id: 0,
                    num: 3,
                    deadline: 200,
                },
            )]),
            sent: RefCell::new(vec![]),
        };
        assert_eq!(confirm_overdue(&[7], 200, &f).unwrap(), vec![7]);
    }

    #[test]
    fn has_effective_item_requires_num_and_future_deadline() {
        let items = [
            (
                9001,
                EffectiveItem {
                    id: 0,
                    template_id: 0,
                    num: 0,
                    deadline: 9_999,
                },
            ),
            (
                9002,
                EffectiveItem {
                    id: 0,
                    template_id: 0,
                    num: 2,
                    deadline: 50,
                },
            ),
            (
                9003,
                EffectiveItem {
                    id: 0,
                    template_id: 0,
                    num: 1,
                    deadline: 500,
                },
            ),
        ];
        assert!(!has_effective_item(9001, 100, &items));
        assert!(!has_effective_item(9002, 100, &items));
        assert!(has_effective_item(9003, 100, &items));
        assert!(!has_effective_item(9999, 100, &items));
    }
}

#[cfg(test)]
mod effective_use_tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        eff: Option<EffectiveItem>,
        tpl: Option<EffectiveUseTemplate>,
        remove_num: Option<i64>,
        gift: Option<Gift>,
        gift_block: Option<RetCode>,
        cost_ok: bool,
        apply: Option<UseApply>,
        apply_err: bool,
        seen_cost: std::cell::Cell<Option<(i32, i64)>>,
    }

    impl EffectiveUseLookup for Fake {
        fn effective(&self, _id: i64) -> Result<Option<EffectiveItem>> {
            Ok(self.eff)
        }
        fn item_template(&self, _t: i32) -> Result<Option<EffectiveUseTemplate>> {
            Ok(self.tpl)
        }
        fn use_remove_num(&self, _id: i64, num: i64) -> Result<i64> {
            Ok(self.remove_num.unwrap_or(num))
        }
        fn gift(&self, _t: i32) -> Result<Option<Gift>> {
            Ok(self.gift)
        }
        fn gift_check_for_using(&self, _t: i32) -> Result<Option<RetCode>> {
            Ok(self.gift_block)
        }
        fn check_item(&self, id: i32, num: i64) -> Result<bool> {
            self.seen_cost.set(Some((id, num)));
            Ok(self.cost_ok)
        }
        fn apply_use(&self, _id: i64, _num: i64) -> Result<UseApply> {
            if self.apply_err {
                return Err(AppError::BadRequest("boom".into()));
            }
            Ok(self.apply.unwrap_or(UseApply {
                err: None,
                has_msg_use_result: false,
            }))
        }
    }

    fn ok_base() -> Fake {
        Fake {
            eff: Some(EffectiveItem {
                id: 0,
                template_id: 5,
                num: 10,
                deadline: 1_000,
            }),
            tpl: Some(EffectiveUseTemplate { use_limit: 999 }),
            remove_num: None,
            gift: None,
            gift_block: None,
            cost_ok: true,
            apply: None,
            apply_err: false,
            seen_cost: std::cell::Cell::new(None),
        }
    }

    const NOW: i64 = 500;

    #[test]
    fn missing_effective_item_is_resources_no() {
        let mut f = ok_base();
        f.eff = None;
        assert_eq!(
            effective_use(1, 1, NOW, &f).unwrap(),
            UseOutcome::Fail(UseFailure::Handler(RetCode::ResourcesNo))
        );
    }

    #[test]
    fn expired_is_item_error_11() {
        let mut f = ok_base();
        f.eff = Some(EffectiveItem {
            id: 0,
            template_id: 5,
            num: 10,
            deadline: NOW,
        });
        assert_eq!(
            effective_use(1, 1, NOW, &f).unwrap(),
            UseOutcome::Fail(UseFailure::Handler(RetCode::ItemError11))
        );
    }

    #[test]
    fn zero_num_is_busy() {
        let f = ok_base();
        assert_eq!(
            effective_use(1, 0, NOW, &f).unwrap(),
            UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy))
        );
    }

    #[test]
    fn over_use_limit_is_busy() {
        let mut f = ok_base();
        f.tpl = Some(EffectiveUseTemplate { use_limit: 3 });
        assert_eq!(
            effective_use(1, 4, NOW, &f).unwrap(),
            UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy))
        );
    }

    #[test]
    fn missing_template_is_busy() {
        let mut f = ok_base();
        f.tpl = None;
        assert_eq!(
            effective_use(1, 1, NOW, &f).unwrap(),
            UseOutcome::Fail(UseFailure::Handler(RetCode::ServerBusy))
        );
    }

    #[test]
    fn insufficient_count_uses_remove_num_not_num() {
        let mut f = ok_base();
        f.eff = Some(EffectiveItem {
            id: 0,
            template_id: 5,
            num: 5,
            deadline: 1_000,
        });
        f.remove_num = Some(6);
        assert_eq!(
            effective_use(1, 1, NOW, &f).unwrap(),
            UseOutcome::Fail(UseFailure::Resp(RetCode::ItemError2))
        );
    }

    #[test]
    fn gift_block_returns_its_code() {
        let mut f = ok_base();
        f.gift_block = Some(RetCode::ItemError3);
        assert_eq!(
            effective_use(1, 1, NOW, &f).unwrap(),
            UseOutcome::Fail(UseFailure::Resp(RetCode::ItemError3))
        );
    }

    #[test]
    fn cost_insufficient_is_handler_use_cost() {
        let mut f = ok_base();
        f.gift = Some(Gift {
            use_cost: Some((900, 2)),
        });
        f.cost_ok = false;
        let out = effective_use(1, 1, NOW, &f).unwrap();
        assert_eq!(
            out,
            UseOutcome::Fail(UseFailure::Handler(RetCode::UseCostResourcesNo))
        );
        // Handler 类错误会被遮罩成 ret=2
        assert_eq!(out.resolve_ret(), Some(RetCode::ServerBusy));
    }

    #[test]
    fn cost_scales_with_remove_num() {
        let mut f = ok_base();
        f.gift = Some(Gift {
            use_cost: Some((900, 2)),
        });
        f.remove_num = Some(3);
        let out = effective_use(1, 5, NOW, &f).unwrap();
        assert_eq!(
            out,
            UseOutcome::Accept {
                remove_num: 3,
                cost: Some((900, 6)),
                has_msg_use_result: false
            }
        );
        assert_eq!(f.seen_cost.get(), Some((900, 6)));
    }

    #[test]
    fn use_error_code_returned_verbatim() {
        let mut f = ok_base();
        f.apply = Some(UseApply {
            err: Some(RetCode::ItemError4),
            has_msg_use_result: false,
        });
        assert_eq!(
            effective_use(1, 1, NOW, &f).unwrap(),
            UseOutcome::Fail(UseFailure::Resp(RetCode::ItemError4))
        );
    }

    #[test]
    fn use_exception_propagates_unlike_11002() {
        // 11002 会把 use() 异常吞掉按成功处理；11005 不捕获，直接冒泡
        let mut f = ok_base();
        f.apply_err = true;
        assert!(effective_use(1, 1, NOW, &f).is_err());
    }

    #[test]
    fn success_accepts() {
        let mut f = ok_base();
        f.apply = Some(UseApply {
            err: None,
            has_msg_use_result: true,
        });
        assert_eq!(
            effective_use(1, 1, NOW, &f).unwrap(),
            UseOutcome::Accept {
                remove_num: 1,
                cost: None,
                has_msg_use_result: true
            }
        );
    }
}

#[cfg(test)]
mod one_key_tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    struct Fake {
        use_cost: Option<(i32, i64)>,
        has_gift: bool,
        gift_block: Option<RetCode>,
        remove_num: Option<i64>,
        /// `check_item` 的放行上限（`None` = 永不通过）
        cost_limit: Option<i64>,
        apply_err: Option<RetCode>,
        resources: Vec<(i32, i64)>,
        checks: RefCell<Vec<(i32, i64)>>,
        applied: RefCell<Vec<(i64, i64)>>,
    }

    impl OneKeyLookup for Fake {
        fn gift(&self, _t: i32) -> Result<Option<Gift>> {
            Ok(if self.has_gift {
                Some(Gift {
                    use_cost: self.use_cost,
                })
            } else {
                None
            })
        }
        fn gift_check_for_using(&self, _t: i32) -> Result<Option<RetCode>> {
            Ok(self.gift_block)
        }
        fn check_item(&self, id: i32, num: i64) -> Result<bool> {
            self.checks.borrow_mut().push((id, num));
            Ok(self.cost_limit.is_some_and(|lim| num <= lim))
        }
        fn use_remove_num(&self, _id: i64, num: i64) -> Result<i64> {
            Ok(self.remove_num.unwrap_or(num))
        }
        fn apply_use(&self, id: i64, num: i64) -> Result<OneKeyApplied> {
            self.applied.borrow_mut().push((id, num));
            match self.apply_err {
                Some(code) => Ok(OneKeyApplied {
                    err: Some(code),
                    msg_resources: vec![],
                }),
                None => Ok(OneKeyApplied {
                    err: None,
                    msg_resources: self.resources.clone(),
                }),
            }
        }
    }

    fn eff(id: i64, template_id: i32, num: i64, deadline: i64) -> EffectiveItem {
        EffectiveItem {
            id,
            template_id,
            num,
            deadline,
        }
    }

    fn base() -> Fake {
        Fake {
            use_cost: Some((900, 2)),
            has_gift: true,
            gift_block: None,
            remove_num: None,
            cost_limit: Some(1_000_000),
            apply_err: None,
            resources: vec![],
            checks: RefCell::new(vec![]),
            applied: RefCell::new(vec![]),
        }
    }

    const NOW: i64 = 1_000;
    const TPL: i32 = 8001;

    #[test]
    fn no_candidates_is_resources_no() {
        let f = base();
        let out = effective_one_key_use(TPL, NOW, &[], &f).unwrap();
        assert_eq!(
            out,
            OneKeyOutcome::Reject(UseFailure::Handler(RetCode::ResourcesNo))
        );
    }

    #[test]
    fn expired_entries_are_collected_and_removed() {
        let f = base();
        let items = [eff(1, TPL, 5, 900), eff(2, TPL, 5, 2_000)];
        let OneKeyOutcome::Accept(plan) = effective_one_key_use(TPL, NOW, &items, &f).unwrap()
        else {
            panic!("应成功");
        };
        assert_eq!(plan.expired, vec![1]);
        assert_eq!(plan.used, vec![(2, 5)]);
        assert!(!plan.interrupted);
    }

    #[test]
    fn only_matching_template_and_positive_num() {
        let f = base();
        let items = [
            eff(1, TPL, 0, 2_000),
            eff(2, 999, 5, 2_000),
            eff(3, TPL, 4, 2_000),
        ];
        let OneKeyOutcome::Accept(plan) = effective_one_key_use(TPL, NOW, &items, &f).unwrap()
        else {
            panic!("应成功");
        };
        assert_eq!(plan.used, vec![(3, 4)]);
    }

    #[test]
    fn sorted_by_deadline_ascending() {
        let f = base();
        let items = [
            eff(1, TPL, 1, 3_000),
            eff(2, TPL, 1, 2_000),
            eff(3, TPL, 1, 2_500),
        ];
        effective_one_key_use(TPL, NOW, &items, &f).unwrap();
        let order: Vec<i64> = f.applied.borrow().iter().map(|(id, _)| *id).collect();
        assert_eq!(order, vec![2, 3, 1], "必须按 deadline 升序");
    }

    #[test]
    fn missing_cost_is_busy_like_npe() {
        let mut f = base();
        f.use_cost = None;
        let items = [eff(1, TPL, 5, 2_000)];
        assert_eq!(
            effective_one_key_use(TPL, NOW, &items, &f).unwrap(),
            OneKeyOutcome::Reject(UseFailure::Handler(RetCode::ServerBusy))
        );
    }

    #[test]
    fn outer_check_uses_single_amount_then_inner_accumulates() {
        let f = base();
        let items = [eff(1, TPL, 5, 2_000), eff(2, TPL, 5, 2_500)];
        effective_one_key_use(TPL, NOW, &items, &f).unwrap();
        let checks = f.checks.borrow().clone();
        // 循环外只校验单份 2；循环内 10、再累计 20
        assert_eq!(checks, vec![(900, 2), (900, 10), (900, 20)]);
    }

    #[test]
    fn inner_cost_failure_breaks_and_keeps_previous() {
        let mut f = base();
        f.cost_limit = Some(15); // 只允许累计到 15
        let items = [eff(1, TPL, 5, 2_000), eff(2, TPL, 5, 2_500)];
        let OneKeyOutcome::Accept(plan) = effective_one_key_use(TPL, NOW, &items, &f).unwrap()
        else {
            panic!("应成功收尾");
        };
        assert!(plan.interrupted);
        assert_eq!(plan.used, vec![(1, 5)], "第一条成功，第二条因累计超限中断");
    }

    #[test]
    fn remove_num_less_than_num_breaks() {
        let mut f = base();
        f.remove_num = Some(1);
        let items = [eff(1, TPL, 5, 2_000)];
        let OneKeyOutcome::Accept(plan) = effective_one_key_use(TPL, NOW, &items, &f).unwrap()
        else {
            panic!("应成功收尾");
        };
        assert!(plan.interrupted);
        assert!(plan.used.is_empty());
    }

    #[test]
    fn use_error_breaks_without_code() {
        let mut f = base();
        f.apply_err = Some(RetCode::ItemError4);
        let items = [eff(1, TPL, 5, 2_000)];
        let OneKeyOutcome::Accept(plan) = effective_one_key_use(TPL, NOW, &items, &f).unwrap()
        else {
            panic!("原版 use() 失败只 break，不返回错误码");
        };
        assert!(plan.interrupted);
        assert!(plan.used.is_empty());
        assert!(plan.cost.is_empty());
    }

    #[test]
    fn gift_block_breaks_before_any_use() {
        let mut f = base();
        f.gift_block = Some(RetCode::ItemError3);
        let items = [eff(1, TPL, 5, 2_000)];
        let OneKeyOutcome::Accept(plan) = effective_one_key_use(TPL, NOW, &items, &f).unwrap()
        else {
            panic!("应成功收尾");
        };
        assert!(plan.interrupted);
        assert!(plan.used.is_empty());
        // 循环外的 checkItem 先执行，随后第一条就因 checkForUsing 中断
        assert_eq!(*f.checks.borrow(), vec![(900, 2)]);
    }

    #[test]
    fn gains_merged_by_id() {
        let mut f = base();
        f.resources = vec![(7, 1), (8, 2), (7, 3)];
        let items = [eff(1, TPL, 1, 2_000), eff(2, TPL, 1, 2_500)];
        let OneKeyOutcome::Accept(plan) = effective_one_key_use(TPL, NOW, &items, &f).unwrap()
        else {
            panic!("应成功");
        };
        // 两条各回 [(7,1),(8,2),(7,3)] → 合并后 7:1+3+1+3=8、8:2+2=4
        assert_eq!(plan.gains, vec![(7, 8), (8, 4)]);
    }

    #[test]
    fn cost_merged_by_id() {
        let f = base();
        let items = [eff(1, TPL, 5, 2_000), eff(2, TPL, 5, 2_500)];
        let OneKeyOutcome::Accept(plan) = effective_one_key_use(TPL, NOW, &items, &f).unwrap()
        else {
            panic!("应成功");
        };
        assert_eq!(plan.cost, vec![(900, 20)]);
    }
}
