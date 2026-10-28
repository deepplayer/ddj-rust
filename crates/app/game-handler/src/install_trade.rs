//! 通商 19002 / 19003 / 19004 / 19005 的接线。
//!
//! 纯逻辑在 [`crate::trade`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## 四条消息的骨架
//! | cmd | 入口 | 前置 |
//! |---|---|---|
//! | 19004 | `getTraders(player)` | **无** |
//! | 19002 | `tradeByItem(player, req)` | `trader == null` → `2201`；`num <= 0` → `2208`；`num > useLimit(37)` → `2`；酒/银两不足 → `2208` |
//! | 19003 | `trade(player, req)` | `all > 0 && !isOpen(106) && 商人数 < 3` → `2202`；`trader == null` → `2201`；`leftTimes <= 0` → `2205`；`nextTradeTime >= now` → `2206`；银两不足 → `2207` |
//! | 19005 | `tradeAll(player)` | 无商人可通商 → `2210` |
//!
//! ## 失败出口
//! 四条都是 `throws HandlerException` → **`ErrorResp_1`（帧号 1）**。
//!
//! ## 每条消息都先 `refresh()`
//! ```java
//! public TraderSet getTraderSet(String playerId) {
//!     TraderSet traderSet = getAndLoadPetSet(playerId);
//!     traderSet.refresh();      // ⚠ 无条件刷，每条消息都刷
//!     return traderSet;
//! }
//! ```
//! 这里由 [`refresh_all`] 在每个入口调一次。
//!
//! ## 19003 / 19005 先推 19001 再回包
//! ```java
//! MessageUtil.sendToPlayerId(MsgTraderResp_19001{...}, playerId);   // 先推
//! return builder.setRet(0).build();                                 // 再回
//! ```
//! 推送走 [`ddj_registry::SessionControl::push`]，回包走 `Ok(Some(...))`。
//!
//! ## 本轮未接的部分（AUDIT A25）
//! - **商人来源**：原版 `TraderSet.addTrader` 的唯一非 GM 调用方是
//!   `ResourcesUtils.addResourcesJsons` 里 `TypeResources.TRADER`(5) 的分支
//!   （VIP/活动奖励里的 `1201=40` 这类项）。该通用发奖链路在全项目**尚未实现**，
//!   所以新号的 `traders` 为空 —— 这与原版一致（原版新号 DB 里也没有 trader 行）。
//!   [`grant_traders`] 已按原版语义写好，等发奖链路落地后直接接上。
//! - `PetSet.useMiracle(ID_TRADE_REWARD)`：门客奇迹未接 → 恒 null，
//!   `miracleReward` / `multipleRewardList` 里不会出现奇迹加成。
//! - `PlayerStatServerModule.addLongStat`（`TradeTimes` / `TradeCompleteTimes`）、
//!   `EventModule.notifyEventMonitorAsynchronous`、`EventLogSendRecord.businessman`：
//!   统计与日志未接。
//! - `trader` 表落库：过渡期放 `PlayerBox.traders`（内存），T9 迁 PG。
//! - `TraderSet.autoPlay` / `Trader.autoPlay`（`IAutoPlay` 离线播放）、
//!   `Trader.addTimesByItem`（`data_d` 里零调用点）：本期不接。

use std::sync::{Arc, OnceLock};

use ddj_core::{Clock, PlayerId, RngSource};
use ddj_pb::gen::common_msg_message::MsgResources;
use ddj_pb::gen::trade_message::{
    MsgTradeAllReq19005, MsgTradeAllResp19005, MsgTradeByItemReq19002, MsgTradeByItemResp19002,
    MsgTradeDataReq19004, MsgTradeDataResp19004, MsgTradeReq19003, MsgTradeResp19003, MsgTrader,
    MsgTraderResp19001,
};
use ddj_pb::RetCode;
use ddj_registry::{HandlerCtx, HandlerTable, Reply};
use num_bigint::BigInt;
use num_traits::Signed;
use prost::Message;

use crate::drop::{self, WeightPick, CANNOT_ATTACK_TIME_OFFSET};
use crate::runtime::{decode_err, FnHandler, MemWorld, PlayerBox};
use crate::trade::{
    self, ByItemPlan, TradeAllOutcome, TradeOutcome, TraderRow, TraderState, CODE_ALL_LIMIT,
    CODE_CD, CODE_FULL_TRADE_LIMIT, CODE_MONEY_LIMIT, CODE_NOT_EXIST_TRADER, CODE_RESOURCES_LIMIT,
    CODE_TRADE_ALL_LIMIT, CODE_ZERO_TRADE_TIMES, MONEY_ID, TRADER_ID_BEGIN, WINE_ITEM_ID,
};

/// `ErrorResp_1` 自己的消息号。
const ERROR_RESP_CMD: i32 = 1;

/// 东八区偏移（毫秒）。
const TZ_MS: i64 = 8 * 3600 * 1000;

/// 一键通商的系统开放 id（`SystemOpenUtils.isOpen(playerId, 106)`）。
const SYSTEM_OPEN_TRADE_ALL: i32 = 106;

/// 阅历的资源 id（`everyReward` 发的是它）。
const EXP_ID: i32 = 3;

/// 进程级随机源（掉落用）。
static RNG: OnceLock<std::sync::Mutex<ddj_core::DeterministicRng>> = OnceLock::new();

fn rng() -> &'static std::sync::Mutex<ddj_core::DeterministicRng> {
    RNG.get_or_init(|| std::sync::Mutex::new(ddj_core::DeterministicRng::new(0x5EED_1900)))
}

/// `LogicUtils.getTimeByHour(now, 0)`：当天零点（东八区）。
fn today_zero(now: i64) -> i64 {
    (now + TZ_MS).div_euclid(86_400_000) * 86_400_000 - TZ_MS
}

fn tables() -> Option<&'static Arc<dyn trade::TradeTableSource>> {
    trade::tables()
}

/// 一次通商的冷却毫秒数（`TraderTime * Coliseum.MAX_INDEX`）。
fn cd_of() -> i64 {
    trade::cd_ms(tables().map_or(trade::TRADER_TIME_DEFAULT, |t| t.trader_time()))
}

fn reject(cmd: i32, code: i32) -> Reply {
    Reply::with_cmd(
        ERROR_RESP_CMD,
        crate::sign::error_resp(cmd, ret_code(code)).encode_to_vec(),
    )
}

/// 码值 → [`RetCode`]；未登记的码退回 `ServerBusy`（`2`）。
fn ret_code(code: i32) -> RetCode {
    match code {
        CODE_NOT_EXIST_TRADER => RetCode::TradeNotExistTrader,
        CODE_TRADE_ALL_LIMIT => RetCode::TradeAllLimit,
        CODE_FULL_TRADE_LIMIT => RetCode::TradeFullLimit,
        CODE_ZERO_TRADE_TIMES => RetCode::TradeZeroTimes,
        CODE_CD => RetCode::TradeCd,
        CODE_MONEY_LIMIT => RetCode::TradeMoneyLimit,
        CODE_RESOURCES_LIMIT => RetCode::TradeResourcesLimit,
        CODE_ALL_LIMIT => RetCode::TradeAllNothing,
        _ => RetCode::ServerBusy,
    }
}

// ---------------------------------------------------------------- 状态

/// `TraderSet.refresh()`：把每个商人都刷一遍。
fn refresh_all(b: &mut PlayerBox, now: i64, zero: i64, cd: i64) {
    let Some(t) = tables() else {
        return;
    };
    for st in b.traders.values_mut() {
        if let Some(row) = t.row(st.trader_id) {
            trade::refresh(st, row, now, zero, cd);
        }
    }
}

/// `TraderSet.addTrader(player, traderIds, typeModule, notify)`。
///
/// 只加「还没有」的商人，返回新加的那些（调用方据此逐个推 19001）。
///
/// ⚠ 原版把 `DBTraderModule.add(arrayList)` 与 `traderDataUpdateResp` 写在
/// **for 循环体内**（复制粘贴 bug：`arrayList` 会被重复入库、每加一个就推一条）。
/// 本项目没有 DB 层，入库不可观察；「每加一个推一条」由调用方按返回的
/// `Vec` 逐个推送复刻。
pub fn grant_traders(b: &mut PlayerBox, trader_ids: &[i32], now: i64) -> Vec<TraderState> {
    let Some(t) = tables() else {
        return Vec::new();
    };
    let mut added = Vec::new();
    for id in trader_ids {
        if b.traders.contains_key(id) {
            continue;
        }
        let Some(row) = t.row(*id) else {
            continue;
        };
        let st = TraderState::new(*id, row.trade_num, now);
        b.traders.insert(*id, st.clone());
        added.push(st);
    }
    added
}

/// `Trader.toMsgTraderData(new Integer[0])`。
fn msg_trader(st: &TraderState) -> MsgTrader {
    let d = trade::to_msg_trader(st);
    MsgTrader {
        id: Some(d.id),
        next_trade_time: Some(d.next_trade_time),
        left_times: Some(d.left_times),
        money: Some(d.money),
    }
}

/// 按 id 升序取全部商人（原版 `ConcurrentHashMap` 顺序不可依赖，这里固定）。
fn sorted_traders(b: &PlayerBox) -> Vec<TraderState> {
    let mut list: Vec<TraderState> = b.traders.values().cloned().collect();
    list.sort_by_key(|s| s.trader_id);
    list
}

// ---------------------------------------------------------------- 银两 / 奖励

fn money_of(b: &PlayerBox) -> BigInt {
    trade::parse_big(&b.money)
}

/// `Player.removeMoney`：扣到 0 为止（原版 `if (money < 0) money = "0"`）。
fn take_money(b: &mut PlayerBox, amount: &BigInt) {
    let left = money_of(b) - amount;
    b.money = if left.is_negative() {
        "0".to_owned()
    } else {
        left.to_string()
    };
}

/// `ResourcesUtils.addResourcesJsons` 的最小实现。
///
/// 只覆盖本模块会产出的两类：`PROPERTY + MONEY`(2) 进银两，其余进背包。
fn add_rewards(b: &mut PlayerBox, list: &[(i32, i64)]) {
    for (id, num) in list {
        if *num == 0 {
            continue;
        }
        if *id == MONEY_ID {
            let next = money_of(b) + BigInt::from(*num);
            b.money = next.to_string();
        } else {
            *b.bag.entry(*id).or_insert(0) += num;
        }
    }
}

fn to_msg(list: &[(i32, i64)]) -> Vec<MsgResources> {
    crate::runtime::resources_to_msg(list)
}

/// `BigInt` → `i64`（回包 `MsgResources.num` 是字符串；本模块量级远小于 `i64::MAX`）。
fn big_to_i64(v: &BigInt) -> i64 {
    i64::try_from(v.clone()).unwrap_or(i64::MAX)
}

// ---------------------------------------------------------------- 掉落

/// `DropUtils.rand(player, dropId)`：`dropId` 是 `List<Integer>`，
/// 对每个一级掉落各抽一轮，最后 `mergeResourcesJson` 合并。
fn rand_drop(drop_ids: &[i32], counters: &mut drop::DropCounters) -> Vec<(i32, i64)> {
    let mut out = Vec::new();
    for one_id in drop_ids {
        out.extend(rand_drop_one(*one_id, counters));
    }
    trade::merge_plain(&out)
}

/// 单个 `DropOne` 的抽取（与 `install_pray::rand_drop` 同一套语义）。
fn rand_drop_one(one_id: i32, counters: &mut drop::DropCounters) -> Vec<(i32, i64)> {
    let Some(t) = drop::tables() else {
        return Vec::new();
    };
    let Some(one) = t.one(one_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let r = rng()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for _ in 0..one.extract_num.max(0) {
        let done = counters.private.get(&one.id).copied().unwrap_or(0);
        let mut picked = drop::given_drop_id(&one, done).and_then(|id| t.two(id));
        // ⚠ 原版 `modelOne.addTimes(one.getId(), 1)` 在 givenDrop 非空时**无条件**执行
        if !one.given_drop.is_empty() {
            *counters.private.entry(one.id).or_insert(0) += 1;
        }
        if picked.is_none() {
            for id in &one.id_two {
                let Some(two) = t.two(*id) else { continue };
                if two.formula_id == 0 {
                    picked = Some(two);
                    break;
                }
                let value = drop::formula_value(&two, 0, |sid| {
                    t.sign(sid).map_or(0, |s| counters.times(&s))
                });
                let roll = r.gen_range_i64(0, CANNOT_ATTACK_TIME_OFFSET);
                let ok = drop::formula_hit(value, roll);
                for sid in &two.sign_id {
                    if let Some(s) = t.sign(*sid) {
                        let act = drop::sign_action(&s, ok);
                        if act.add_times {
                            *counters.private.entry(*sid).or_insert(0) += 1;
                        }
                        if act.reset_times {
                            counters.private.insert(*sid, 0);
                        }
                    }
                }
                if ok {
                    picked = Some(two);
                    break;
                }
            }
        }
        let Some(two) = picked else { break };
        let total: i32 = two.items.iter().map(|i| i.weight).sum();
        let sum = if drop::MIN_WEIGHT_SUM - total > 0 {
            drop::MIN_WEIGHT_SUM
        } else {
            total
        };
        if sum <= 0 {
            continue;
        }
        let pick_roll = r.gen_range_i64(1, i64::from(sum));
        let roll_i32 = i32::try_from(pick_roll).unwrap_or(sum);
        match drop::pick_weight(&two.items, drop::MIN_WEIGHT_SUM, roll_i32) {
            WeightPick::Hit(i) => {
                let it = two.items[i];
                let num = if it.num_min == it.num_max {
                    it.num_min
                } else {
                    r.gen_range_i64(it.num_min, it.num_max)
                };
                out.push((it.item_id, num));
            }
            WeightPick::Empty | WeightPick::ZeroWeight => {}
        }
    }
    out
}

// ---------------------------------------------------------------- 19003 结算

/// `Trader.trade(int all)` 的副作用，顺序照抄：
/// `removeResources` → `addResourcesJsons(reward)` → 推 19001 → `fullReward` 才抽掉落。
fn settle_trade(
    w: &MemWorld,
    p: PlayerId,
    ctx: &HandlerCtx,
    out: &TradeOutcome,
    row: &TraderRow,
) -> Vec<(i32, i64)> {
    let (cost, reward) = (out.cost_money.clone(), out.reward.clone());
    w.with(p, |b| {
        take_money(b, &cost);
        add_rewards(b, &[(EXP_ID, big_to_i64(&reward))]);
    });
    // `save()` → `MessageUtil.sendToPlayerId(MsgTraderResp_19001{...})`
    push_trader(ctx, w, p, row.id);
    if !out.full_reward {
        return Vec::new();
    }
    let mut counters = drop::DropCounters::default();
    let drops = rand_drop(&row.drop_id, &mut counters);
    let list = drops.clone();
    w.with(p, |b| add_rewards(b, &list));
    // ⚠ 原版 `rewardPond` 直接 `addAllRewardPond(toMsgAwardBuilder(dropReward))`，
    // **没有** merge —— 保持原样（掉落组内可能重复同一 id）。
    drops
}

/// `Trader.trade(checkCnd, nowTime, isAutoPlay)` 的副作用，顺序同 [`settle_trade`]。
fn settle_trade_all(
    w: &MemWorld,
    p: PlayerId,
    ctx: &HandlerCtx,
    out: &TradeAllOutcome,
    row: &TraderRow,
) -> Vec<(i32, i64)> {
    let (cost, gain) = (out.cost_money.clone(), out.gain.clone());
    w.with(p, |b| {
        take_money(b, &cost);
        add_rewards(b, &[(EXP_ID, big_to_i64(&gain))]);
    });
    push_trader(ctx, w, p, row.id);
    if !out.full_reward {
        return Vec::new();
    }
    let mut counters = drop::DropCounters::default();
    let drops = rand_drop(&row.drop_id, &mut counters);
    let list = drops.clone();
    w.with(p, |b| add_rewards(b, &list));
    drops
}

/// `TradeServerModule.traderDataUpdateResp`：推 19001。
fn push_trader(ctx: &HandlerCtx, w: &MemWorld, p: PlayerId, id: i32) {
    let Some(sess) = ctx.session.as_ref() else {
        return;
    };
    let Some(st) = w.peek(p, |b| b.traders.get(&id).cloned()) else {
        return;
    };
    let msg = MsgTraderResp19001 {
        msg_trader: Some(msg_trader(&st)),
    };
    sess.push(19001, msg.encode_to_vec().into());
}

// ---------------------------------------------------------------- 19002 结算

/// `Trader.tradeByItem` 的副作用：扣酒 + 扣钱 + 发奖，返回 `rewardPond`。
fn settle_by_item(
    w: &MemWorld,
    p: PlayerId,
    plan: &ByItemPlan,
    row: &TraderRow,
) -> Vec<(i32, i64)> {
    let mut counters = drop::DropCounters::default();
    let mut drops: Vec<(i32, i64)> = Vec::new();
    for _ in 0..plan.item_num.max(0) {
        drops.extend(rand_drop(&row.drop_id, &mut counters));
    }
    let merged = trade::merge_resources_json(&drops, MONEY_ID);
    let (item_num, need_money, reward) =
        (plan.item_num, plan.need_money.clone(), plan.reward.clone());
    w.with(p, |b| {
        if let Some(v) = b.bag.get_mut(&WINE_ITEM_ID) {
            *v = (*v - i64::from(item_num)).max(0);
        }
        take_money(b, &need_money);
        // `addResourcesJsons(player, rewardList)`：rewardList = [(3,reward)] + 掉落
        let mut list = vec![(EXP_ID, big_to_i64(&reward))];
        list.extend(drops.iter().copied());
        add_rewards(b, &list);
    });
    merged
}

// ---------------------------------------------------------------- 装配

/// 注册 4 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 19004 商人列表
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(19004, move |ctx, body| {
            let _req = MsgTradeDataReq19004::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let (zero, cd) = (today_zero(now), cd_of());
            let list = w.with(p, |b| {
                refresh_all(b, now, zero, cd);
                sorted_traders(b).iter().map(msg_trader).collect::<Vec<_>>()
            });
            Ok(Some(
                MsgTradeDataResp19004 {
                    ret: 0,
                    msg_trader: list,
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // 19002 用葡萄美酒通商
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(19002, move |ctx, body| {
            let req = MsgTradeByItemReq19002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let id = req.id.unwrap_or(0);
            let num = req.num.unwrap_or(0);
            let now = c.now_millis();
            let (zero, cd) = (today_zero(now), cd_of());
            let Some(t) = tables() else {
                return Ok(Some(reject(19002, CODE_NOT_EXIST_TRADER)));
            };
            let Some(row) = t.row(id) else {
                return Ok(Some(reject(19002, CODE_NOT_EXIST_TRADER)));
            };
            let row = row.clone();
            // `getTraderSet` 会先 refresh
            w.with(p, |b| refresh_all(b, now, zero, cd));

            let use_limit = crate::install_item::item_use_limit(WINE_ITEM_ID);
            let plan = match trade::by_item_verdict(&row, num, use_limit) {
                Ok(v) => v,
                Err(code) => return Ok(Some(reject(19002, code))),
            };

            // `revmoeResources(player, costList, ...)`：酒 + 银两，**全有或全无**
            let need_wine = i64::from(plan.item_num);
            let (have_wine, money) = w.peek(p, |b| {
                (b.bag.get(&WINE_ITEM_ID).copied().unwrap_or(0), money_of(b))
            });
            if have_wine < need_wine || money < plan.need_money {
                return Ok(Some(reject(19002, CODE_RESOURCES_LIMIT)));
            }

            let pond = settle_by_item(&w, p, &plan, &row);
            Ok(Some(
                MsgTradeByItemResp19002 {
                    ret: 0,
                    id: Some(id),
                    // ⚠ 原版只回银两，不回葡萄美酒
                    cost: to_msg(&[(MONEY_ID, big_to_i64(&plan.need_money))]),
                    reward: to_msg(&[(EXP_ID, big_to_i64(&plan.reward))]),
                    reward_pond: to_msg(&pond),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // 19003 单商人通商
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(19003, move |ctx, body| {
            let req = MsgTradeReq19003::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let id = req.id.unwrap_or(0);
            let all = req.all.unwrap_or(0);
            let now = c.now_millis();
            let (zero, cd) = (today_zero(now), cd_of());

            // ⚠ 2202 判定在**取 trader 之前**，且是三重与
            if all > 0 {
                let open = crate::install_system_open::is_open(
                    &w,
                    p,
                    c.as_ref(),
                    crate::install_system_open::server(),
                    SYSTEM_OPEN_TRADE_ALL,
                );
                if !open && w.peek(p, |b| b.traders.len()) < 3 {
                    return Ok(Some(reject(19003, CODE_TRADE_ALL_LIMIT)));
                }
            }

            let Some(t) = tables() else {
                return Ok(Some(reject(19003, CODE_NOT_EXIST_TRADER)));
            };
            let Some(row) = t.row(id) else {
                return Ok(Some(reject(19003, CODE_NOT_EXIST_TRADER)));
            };
            let row = row.clone();

            let res = w.with(p, |b| {
                refresh_all(b, now, zero, cd);
                let mut st = b.traders.get(&id).cloned().ok_or(CODE_NOT_EXIST_TRADER)?;
                let cur = money_of(b);
                let out = trade::trade_verdict(&mut st, &row, all, &cur, now, cd)?;
                b.traders.insert(id, st);
                Ok::<TradeOutcome, i32>(out)
            });
            let out = match res {
                Ok(v) => v,
                Err(code) => return Ok(Some(reject(19003, code))),
            };

            let pond = settle_trade(&w, p, ctx, &out, &row);
            Ok(Some(
                MsgTradeResp19003 {
                    ret: 0,
                    id: Some(id),
                    cost: to_msg(&[(MONEY_ID, big_to_i64(&out.cost_money))]),
                    reward: to_msg(&[(EXP_ID, big_to_i64(&out.reward))]),
                    reward_pond: to_msg(&pond),
                    all: Some(all),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // 19005 一键通商
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(19005, move |ctx, body| {
            let _req = MsgTradeAllReq19005::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let (zero, cd) = (today_zero(now), cd_of());
            let Some(t) = tables() else {
                return Ok(Some(reject(19005, CODE_ALL_LIMIT)));
            };
            let max = t.max_trader_id();
            w.with(p, |b| refresh_all(b, now, zero, cd));

            let mut ids: Vec<i32> = Vec::new();
            let mut costs: Vec<(i32, i64)> = Vec::new();
            let mut gains: Vec<(i32, i64)> = Vec::new();
            let mut multiples: Vec<(i32, i64)> = Vec::new();

            for i in TRADER_ID_BEGIN..=max {
                let Some(row) = t.row(i).cloned() else {
                    continue;
                };
                let res = w.with(p, |b| {
                    let mut st = b.traders.get(&i).cloned()?;
                    let cur = money_of(b);
                    let out = trade::trade_cnd_verdict(&mut st, &row, &cur, now, true, cd)?;
                    b.traders.insert(i, st);
                    Some(out)
                });
                let Some(out) = res else { continue };
                let drops = settle_trade_all(&w, p, ctx, &out, &row);
                ids.push(i);
                costs.push((MONEY_ID, big_to_i64(&out.cost_money)));
                gains.push((EXP_ID, big_to_i64(&out.gain)));
                multiples.extend(drops);
            }

            if ids.is_empty() {
                return Ok(Some(reject(19005, CODE_ALL_LIMIT)));
            }

            // `addAllReward(merge(rewards))` 再 `addAllReward(merge(rewardsMultiple))`
            let mut reward = to_msg(&trade::merge_resources_json(&gains, MONEY_ID));
            reward.extend(to_msg(&trade::merge_resources_json(&multiples, MONEY_ID)));
            Ok(Some(
                MsgTradeAllResp19005 {
                    ret: 0,
                    ids,
                    cost: to_msg(&trade::merge_resources_json(&costs, MONEY_ID)),
                    reward,
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    n
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    /// 1201：`tradeNum=3` / `totalCost=25000000` / `everyCost=500000` / `everyReward=30000`。
    fn row_1201() -> TraderRow {
        TraderRow {
            id: 1201,
            name: "天竺商人".to_owned(),
            unlock_chapter: 6,
            trade_num: 3,
            total_cost: "25000000".to_owned(),
            every_cost: "500000".to_owned(),
            every_reward: "30000".to_owned(),
            drop_id: vec![51],
        }
    }

    fn big(s: &str) -> BigInt {
        BigInt::from(s.parse::<i64>().expect("测试值"))
    }

    /// 注入一张只有 1201 的表（进程级 `OnceLock`，所以只能装一次）。
    fn install_fixture() {
        struct One(TraderRow);
        impl trade::TradeTableSource for One {
            fn row(&self, id: i32) -> Option<&TraderRow> {
                (id == self.0.id).then_some(&self.0)
            }
            fn max_trader_id(&self) -> i32 {
                self.0.id
            }
        }
        trade::install_tables(Arc::new(One(row_1201())));
    }

    #[test]
    fn today_zero_is_midnight_in_utc8() {
        // 2026-10-29T03:45:35+08:00 = 2026-10-28T19:45:35Z
        let now = 1_793_216_735_000;
        let zero = today_zero(now);
        assert_eq!(zero, 1_793_203_200_000, "2026-10-29T00:00:00+08:00");
        assert!(zero <= now && now - zero < 86_400_000);
    }

    #[test]
    fn cd_uses_trader_time_times_max_index() {
        // 表未注入 → 默认 3600 秒 → 3_600_000 ms
        assert_eq!(cd_of(), 3_600_000);
    }

    #[test]
    fn grant_traders_adds_only_missing() {
        install_fixture();
        let mut b = PlayerBox::new_player();
        let added = grant_traders(&mut b, &[1201, 1201, 9999], 1_000);
        assert_eq!(added.len(), 1, "1201 加一次，重复与未知 id 都跳过");
        assert_eq!(added[0].left_times, 3);
        assert_eq!(b.traders.len(), 1);
        // 再加同一个 → 不重复
        assert!(grant_traders(&mut b, &[1201], 2_000).is_empty());
        assert_eq!(b.traders[&1201].next_trade_time, 1_000, "已存在的不被重写");
    }

    #[test]
    fn sorted_traders_is_by_id() {
        let mut b = PlayerBox::new_player();
        b.traders.insert(1210, TraderState::new(1210, 3, 0));
        b.traders.insert(1201, TraderState::new(1201, 3, 0));
        b.traders.insert(1205, TraderState::new(1205, 3, 0));
        let ids: Vec<i32> = sorted_traders(&b).iter().map(|s| s.trader_id).collect();
        assert_eq!(ids, vec![1201, 1205, 1210]);
    }

    #[test]
    fn take_money_clamps_at_zero() {
        let mut b = PlayerBox::new_player();
        assert_eq!(b.money, "20000");
        take_money(&mut b, &big("5000"));
        assert_eq!(b.money, "15000");
        // 扣超额 → 归零，不留负数
        take_money(&mut b, &big("999999"));
        assert_eq!(b.money, "0");
    }

    #[test]
    fn add_rewards_routes_money_to_wallet_and_rest_to_bag() {
        let mut b = PlayerBox::new_player();
        add_rewards(
            &mut b,
            &[(MONEY_ID, 500), (EXP_ID, 30), (EXP_ID, 20), (99, 0)],
        );
        assert_eq!(b.money, "20500", "银两进钱包");
        assert_eq!(b.bag.get(&EXP_ID), Some(&50), "同 id 累加");
        assert_eq!(b.bag.get(&99), None, "num=0 不进背包");
    }

    #[test]
    fn ret_code_maps_every_trade_error() {
        assert_eq!(
            ret_code(CODE_NOT_EXIST_TRADER),
            RetCode::TradeNotExistTrader
        );
        assert_eq!(ret_code(CODE_TRADE_ALL_LIMIT), RetCode::TradeAllLimit);
        assert_eq!(ret_code(CODE_FULL_TRADE_LIMIT), RetCode::TradeFullLimit);
        assert_eq!(ret_code(CODE_ZERO_TRADE_TIMES), RetCode::TradeZeroTimes);
        assert_eq!(ret_code(CODE_CD), RetCode::TradeCd);
        assert_eq!(ret_code(CODE_MONEY_LIMIT), RetCode::TradeMoneyLimit);
        assert_eq!(ret_code(CODE_RESOURCES_LIMIT), RetCode::TradeResourcesLimit);
        assert_eq!(ret_code(CODE_ALL_LIMIT), RetCode::TradeAllNothing);
        // 未登记的码退 ServerBusy（`COMMON_SERVER_BUSY`）
        assert_eq!(ret_code(12345), RetCode::ServerBusy);
    }

    #[test]
    fn msg_trader_carries_four_fields() {
        let mut st = TraderState::new(1201, 3, 777);
        st.money = "12345".to_owned();
        let m = msg_trader(&st);
        assert_eq!(m.id, Some(1201));
        assert_eq!(m.next_trade_time, Some(777));
        assert_eq!(m.left_times, Some(3));
        assert_eq!(m.money.as_deref(), Some("12345"));
    }

    #[test]
    fn refresh_all_skips_unknown_trader_ids() {
        install_fixture();
        let mut b = PlayerBox::new_player();
        // 9999 不在表里 → refresh 时直接跳过，不 panic
        b.traders.insert(9999, TraderState::new(9999, 3, 0));
        b.traders.insert(1201, TraderState::new(1201, 3, 0));
        let now = 1_793_216_735_000;
        refresh_all(&mut b, now, today_zero(now), 3_600_000);
        assert_eq!(b.traders[&1201].left_times, 3);
        assert_eq!(b.traders[&1201].last_refresh_time, now, "跨天刷新过了");
        assert_eq!(b.traders[&9999].last_refresh_time, 0, "表里没有的不动");
    }

    #[test]
    fn rand_drop_without_tables_is_empty() {
        let mut counters = drop::DropCounters::default();
        // 掉落表未注入 → 空（不是 panic）
        assert!(rand_drop(&[51], &mut counters).is_empty());
    }
}
