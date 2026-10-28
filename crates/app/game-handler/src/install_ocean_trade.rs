//! 远洋贸易 41001 / 41002 / 41003 的接线。
//!
//! 纯逻辑在 [`crate::ocean_trade`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## 三条消息的骨架
//! | cmd | 入口 | 前置 |
//! |---|---|---|
//! | 41001 | `enterOceanTrade(player)` | `isOpen(19)` |
//! | 41002 | `oceanTradeRankList(player, type)` | **无**（连 `isOpen` 都不判） |
//! | 41003 | `negotiate(player, type, num)` | `isOpen(19)`；`seasonId == curSeasonId`；`!isEndGame()` |
//!
//! ## 失败出口
//! 三条都是 `throws HandlerException` → **`ErrorResp_1`（帧号 1）**：
//! | cmd | 场景 | 码 |
//! |---|---|---|
//! | 41001 | `isOpen(19)` 假 | `3001` `OCEAN_TRADE_NOT_OPEN` |
//! | 41003 | `isOpen(19)` 假 | `3001` |
//! | 41003 | `seasonId != curSeasonId` | `3004` `OCEAN_TRADE_SEASON_ID_ILLEGAL` |
//! | 41003 | 已通关（`isEndGame()`） | `3002` `OCEAN_TRADE_NUM_ILLEGAL` |
//! | 41003 | `negotiate` 内 `checkSeasonEndOrNextSeason` | `3004` |
//! | 41003 | 一键未解锁 `isOpen(112)` | `3003` `OCEAN_TRADE_NEGOTIATION_QUICKLY_BE_LOCKED` |
//! | 41003 | `num` 越界（`<=0` 或 `> maxCheckpointQuicklySize`） | `3002` |
//!
//! ## 本轮未接的部分（AUDIT A24）
//! - `Player.getTotalMoney()` 依赖 `TalentSet` / `ChildrenSet`，两者都没接
//!   → 恒 0 → `getRate()` 原版会除零抛 `ArithmeticException`；这里退化为
//!   `CheckpointCostInterval[0] = 0.2`，让「至少按 0.2 打」成立。
//! - 排行榜（`OceanTradeService.addOrUpdate` / `getRankingIndex`）：
//!   需要跨玩家的内存排行结构 + `PlayerOfflineDataServerModule`（名字/头像/装扮），
//!   这里用 [`MemWorld`] 里的 `ocean_rank` 维护一个最小实现（只存 `playerId → checkpointId`）。
//! - `DropUtils.rand(player, dropId)` 的额外掉落：走 [`ddj_game_handler::drop`] 的重量抽取
//!   **已接**，但 `DropTables` 未注入时退化为无掉落。
//! - 赛季存档（`bf_ocean_trade_time` / `id_generator`）仍在内存。
//! - `EventLogSendRecord` / `LastOceanTrade.handleTopThree` 的 `MsgRankingEntryData`
//!   只填 `key` + `var_str`，名字/等级/装扮字段留空。

use std::sync::{Arc, OnceLock};

use ddj_core::{Clock, RngSource};
use ddj_pb::gen::common_msg_message::MsgResources;
use ddj_pb::gen::ocean_trade_message::{
    MsgOceanTradeData, MsgOceanTradeRankingData, OceanTradeEnterReq41001, OceanTradeEnterResp41001,
    OceanTradeGetRankingReq41002, OceanTradeGetRankingResp41002, OceanTradeNegotiationReq41003,
    OceanTradeNegotiationResp41003,
};
use ddj_pb::gen::ranking_message::MsgRankingEntryData;
use ddj_pb::RetCode;
use ddj_registry::{HandlerTable, Reply};
use num_bigint::BigInt;
use prost::Message;

use crate::ocean_trade::{
    self, NegotiateInput, NegotiateOutcome, OceanTradeTables, CODE_NOT_OPEN,
    CODE_NUM_ILLEGAL, CODE_QUICKLY_LOCKED, CODE_SEASON_ID_ILLEGAL, SYSTEM_OPEN_ID, TOP_PLAYER_SIZE,
};
use crate::runtime::{decode_err, FnHandler, MemWorld, PlayerBox};

/// `ErrorResp_1` 自己的消息号。
const ERROR_RESP_CMD: i32 = 1;
/// 一键谈判的系统开放 id（`isOpen(playerId, 112)`）。
const OPEN_QUICKLY: i32 = 112;

/// 进程级随机源（掉落用）。
static RNG: OnceLock<std::sync::Mutex<ddj_core::DeterministicRng>> = OnceLock::new();

fn rng() -> &'static std::sync::Mutex<ddj_core::DeterministicRng> {
    RNG.get_or_init(|| std::sync::Mutex::new(ddj_core::DeterministicRng::new(0x5EED_4100)))
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
        CODE_NOT_OPEN => RetCode::OceanTradeNotOpen,
        CODE_NUM_ILLEGAL => RetCode::OceanTradeNumIllegal,
        CODE_QUICKLY_LOCKED => RetCode::OceanTradeQuicklyLocked,
        CODE_SEASON_ID_ILLEGAL => RetCode::OceanTradeSeasonIdIllegal,
        _ => RetCode::ServerBusy,
    }
}

fn tables() -> Option<&'static Arc<dyn OceanTradeTables>> {
    ocean_trade::tables()
}

/// `OceanTradePlayer.getNpcHpLeft().toString()`。
fn npc_hp(b: &PlayerBox, t: &dyn OceanTradeTables) -> String {
    b.ocean_trade.npc_hp_left(t.checkpoints()).to_string()
}

/// 某关奖励 + `DropUtils.rand(player, dropId)` 的额外掉落。
///
/// 原版：
/// ```java
/// reward = db.getRewardJson();                       // 克隆
/// if (db.getDropId() > 0 && (rand = DropUtils.rand(player, db.getDropId())) != null && !rand.isEmpty())
///     list.addAll(rand);
/// ```
fn reward_of(t: &dyn OceanTradeTables, checkpoint_id: i32) -> Vec<(i32, i64)> {
    let mut list = t.reward(checkpoint_id);
    let drop_id = t.drop_id(checkpoint_id);
    if drop_id > 0 {
        let mut extra = drop_once(drop_id);
        list.append(&mut extra);
    }
    list
}

/// 抽一次掉落组（`DropUtils.rand`），未注入掉落表时返回空。
fn drop_once(drop_id: i32) -> Vec<(i32, i64)> {
    let Some(t) = crate::drop::tables() else {
        return Vec::new();
    };
    let Some(one) = t.one(drop_id) else {
        return Vec::new();
    };
    // 原版 `DropUtils.rand` 会走 `DropOneDB` → `givenDropId` / `idTwo` → `DropTwoDB`
    // 的完整权重链路。这里只做「一层展开」：直接取 `idTwo` 的第一组，
    // 因为远洋贸易 400 行的 `dropId` 全是 121/122 这两个「固定道具组」。
    let Some(two_id) = one.id_two.first().copied() else {
        return Vec::new();
    };
    let Some(two) = t.two(two_id) else {
        return Vec::new();
    };
    let total: i64 = two.items.iter().map(|i| i64::from(i.weight)).sum();
    if total <= 0 {
        return Vec::new();
    }
    let r = rng().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let roll = r.gen_range_i64(1, total);
    drop(r);
    let mut acc: i64 = 0;
    for item in &two.items {
        acc += i64::from(item.weight);
        if roll <= acc {
            let num = if item.num_max > item.num_min {
                let r = rng().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let n = r.gen_range_i64(item.num_min, item.num_max);
                drop(r);
                n
            } else {
                item.num_min
            };
            return vec![(item.item_id, num)];
        }
    }
    Vec::new()
}


pub use crate::ocean_trade::{rank_cmp, RankEntry};

/// `OceanTradeService.addOrUpdate` 的纯逻辑版。
///
/// ```java
/// if (checkSeasonEndOrNextSeason(seasonId)) { log.error(...); return; }
/// entity = map.get(playerId);
/// if (entity != null) { entity.setCheckpointId(cp); entity.setUpdateTime(now); list.sort(...); return; }
/// if (list.size() >= OceanTradeRankListActualNumber
///     && list.get(list.size()-1).getCheckpointId() >= cp) return;
/// entity = new ...; list.add(entity); map.put(...); list.sort(...);
/// if (list.size() > ActualNumber) { removed = list.pollLast(); map.remove(removed.getPlayerId()); }
/// ```
pub fn rank_add_or_update(
    list: &mut Vec<RankEntry>,
    player_id: &str,
    checkpoint_id: i32,
    now: i64,
    actual: usize,
) {
    if let Some(e) = list.iter_mut().find(|e| e.player_id == player_id) {
        e.checkpoint_id = checkpoint_id;
        e.update_time = now;
        list.sort_by(rank_cmp);
        return;
    }
    if list.len() >= actual {
        if let Some(last) = list.last() {
            if last.checkpoint_id >= checkpoint_id {
                return;
            }
        }
    }
    list.push(RankEntry {
        player_id: player_id.to_owned(),
        checkpoint_id,
        update_time: now,
    });
    list.sort_by(rank_cmp);
    if list.len() > actual {
        list.pop();
    }
}

/// `OceanTradeService.getRankingIndex(playerId)`：没有实体返回 `-1`，否则 `index + 1`。
#[must_use]
pub fn rank_index(list: &[RankEntry], player_id: &str) -> i32 {
    list.iter()
        .position(|e| e.player_id == player_id)
        .map_or(ocean_trade::NOT_IN_RANKING, |i| {
            i32::try_from(i + 1).unwrap_or(ocean_trade::NOT_IN_RANKING)
        })
}

/// `makeOceanTradeEntity(playerId, checkpointId, index)`。
///
/// 原版还会从 `PlayerOfflineDataServerModule` 取 `keyName` / `fashionIds` /
/// `address` / `sex` / `npcSex` / `lv` / `moneySpeed` / `vipLv`；这些字段本轮留空
/// （见文件头「本轮未接的部分」）。
#[must_use]
pub fn make_entry(player_id: &str, checkpoint_id: i32, index: i32) -> MsgRankingEntryData {
    MsgRankingEntryData {
        key: Some(player_id.to_owned()),
        var_str: vec![checkpoint_id.to_string()],
        rank: Some(index),
        ..MsgRankingEntryData::default()
    }
}

/// 把 [`NegotiateOutcome`] 的奖励转 protobuf。
fn resources_to_msg(list: &[(i32, i64)]) -> Vec<MsgResources> {
    crate::runtime::resources_to_msg(list)
}

/// 把结算结果写回 `PlayerBox`：扣银两、加奖励。
fn apply_outcome(b: &mut PlayerBox, out: &NegotiateOutcome, rewards: &[(i32, i64)]) {
    // `player.removeMoney(costMoney)` —— 原版会自己 clamp 到 0。
    let spent = out.cost_money.to_string();
    let cur = b
        .money
        .trim()
        .parse::<BigInt>()
        .unwrap_or_else(|_| BigInt::from(0));
    let left = cur - &spent.parse::<BigInt>().unwrap_or_else(|_| BigInt::from(0));
    b.money = if left < BigInt::from(0) {
        "0".to_owned()
    } else {
        left.to_string()
    };
    // `addResourcesJsons`
    for (id, num) in rewards {
        if *num == 0 {
            continue;
        }
        *b.bag.entry(*id).or_insert(0) += num;
    }
}

/// 装配三条消息。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // ── 41001 进入远洋贸易 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(41001, move |ctx, body| {
            let _req = OceanTradeEnterReq41001::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let srv = crate::install_system_open::server();
            if !crate::install_system_open::is_open(&w, p, c.as_ref(), srv, SYSTEM_OPEN_ID) {
                return Ok(Some(reject(41001, CODE_NOT_OPEN)));
            }
            let Some(t) = tables() else {
                return Ok(Some(reject(41001, CODE_NOT_OPEN)));
            };
            let cp_tables = t.checkpoints();
            let season_days = t.season_days();
            let now = c.now_millis();
            let zero = crate::lantern_riddles::zero_ms_of(now);

            let data = w.with(p, |b| {
                // `service.getCurrentSeasonId()` / `getCurrentSeasonStartTime()`
                let (season_id, season_start) = current_season(b, cp_tables, zero, season_days);
                // `getAndLoadOceanTradePlayer` 里会 `refresh()`
                b.ocean_trade
                    .refresh(cp_tables, now, season_days, season_id, season_start);
                // `enterOceanTrade(builder)`
                b.ocean_trade.enter(cp_tables, season_id);
                let mut data = MsgOceanTradeData {
                    checkpoint_id: b.ocean_trade.checkpoint_id,
                    season_id,
                    start_time: Some(season_start),
                    end_time: season_end(b, zero, season_days),
                    npc_hp: Some(npc_hp(b, t.as_ref())),
                    end_game: Some(b.ocean_trade.end_game),
                    top_three: Vec::new(),
                    top_checkpoint_id: None,
                };
                // `lastOceanTrade.handleTopThree(builder)`：最多 3 条
                let top: Vec<MsgRankingEntryData> = b
                    .ocean_rank
                    .iter()
                    .take(TOP_PLAYER_SIZE)
                    .enumerate()
                    .map(|(i, e)| {
                        make_entry(
                            &e.player_id,
                            e.checkpoint_id,
                            i32::try_from(i + 1).unwrap_or(1),
                        )
                    })
                    .collect();
                data.top_three = top;
                data
            });

            Ok(Some(
                OceanTradeEnterResp41001 {
                    ret: 0,
                    msg_ocean_trade_data: Some(data),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // ── 41002 排行榜 ──
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(41002, move |ctx, body| {
            let req = OceanTradeGetRankingReq41002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let ty = req.r#type;
            let display = tables().map_or(500, |t| t.rank_display());
            let pid = p.get().to_string();
            let data = w.with(p, |b| {
                let idx = rank_index(&b.ocean_rank, &pid);
                let progress = if idx > 0 {
                    b.ocean_rank
                        .get(usize::try_from(idx - 1).unwrap_or(0))
                        .map_or(b.ocean_trade.current_progress(), |e| e.checkpoint_id)
                } else {
                    b.ocean_trade.current_progress()
                };
                let rank_data = MsgOceanTradeRankingData {
                    season_id: Some(b.ocean_season_id),
                    progress: Some(progress),
                    index: Some(idx),
                    msg_ocean_trade_entities: b
                        .ocean_rank
                        .iter()
                        .take(display)
                        .enumerate()
                        .map(|(i, e)| {
                            make_entry(
                                &e.player_id,
                                e.checkpoint_id,
                                i32::try_from(i + 1).unwrap_or(1),
                            )
                        })
                        .collect(),
                };
                // 原版 `type != 0` 走上赛季榜；本轮没有跨赛季存档，
                // 两者都回当前榜（见 AUDIT A24）。
                rank_data
            });
            Ok(Some(
                OceanTradeGetRankingResp41002 {
                    ret: 0,
                    rank_data: Some(data),
                    r#type: Some(ty),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // ── 41003 谈判 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(41003, move |ctx, body| {
            let req = OceanTradeNegotiationReq41003::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let ty = req.r#type;
            let num = req.num.unwrap_or(0);
            let season_id = req.season_id.unwrap_or(0);

            let srv = crate::install_system_open::server();
            if !crate::install_system_open::is_open(&w, p, c.as_ref(), srv, SYSTEM_OPEN_ID) {
                return Ok(Some(reject(41003, CODE_NOT_OPEN)));
            }
            let Some(t) = tables() else {
                return Ok(Some(reject(41003, CODE_NOT_OPEN)));
            };
            let cp_tables = t.checkpoints();
            if cp_tables.is_empty() {
                return Ok(Some(reject(41003, CODE_NUM_ILLEGAL)));
            }
            // `if (req.getSeasonId() != oceanTradePlayer.getCurSeasonId()) throw SEASON_ID_ILLEGAL`
            let cur_season = w.peek(p, |b| b.ocean_trade.cur_season_id);
            if season_id != cur_season {
                return Ok(Some(reject(41003, CODE_SEASON_ID_ILLEGAL)));
            }
            // `if (oceanTradePlayer.isEndGame()) throw NUM_ILLEGAL`
            if w.peek(p, PlayerBox::is_end_game) {
                return Ok(Some(reject(41003, CODE_NUM_ILLEGAL)));
            }
            let open_112 =
                crate::install_system_open::is_open(&w, p, c.as_ref(), srv, OPEN_QUICKLY);
            let season_days = t.season_days();
            let now = c.now_millis();
            let zero = crate::lantern_riddles::zero_ms_of(now);

            let pid = p.get().to_string();
            let out = w.with(p, |b| {
                let (sid, sstart) = current_season(b, cp_tables, zero, season_days);
                // `checkSeasonEndOrNextSeason(curSeasonId)`
                let invalid = b.ocean_trade_season_invalid(sid);
                let money = b.money.trim().parse::<BigInt>().unwrap_or_else(|_| BigInt::from(0));
                let total = b
                    .total_money
                    .trim()
                    .parse::<BigInt>()
                    .unwrap_or_else(|_| BigInt::from(0));
                let input = NegotiateInput {
                    season_id,
                    negotiate_type: ty,
                    num,
                    money,
                    total_money: total,
                    open_19: true,
                    open_112,
                    tables: cp_tables,
                    season_invalid: invalid,
                };
                let mut st = std::mem::take(&mut b.ocean_trade);
                let r = ocean_trade::negotiate_verdict(&mut st, &input, &|cp| {
                    reward_of(t.as_ref(), cp)
                });
                b.ocean_trade = st;
                let _ = sstart;
                r.inspect(|out| {
                    let rewards = out.resources.clone();
                    apply_outcome(b, out, &rewards);
                    // `if (checkpointId >= oldCheckpointId && progress > 0) addOrUpdate`
                    if b.ocean_trade.progress > 0 {
                        let pid = pid.clone();
                        let cp = b.ocean_trade.checkpoint_id;
                        let actual = t.rank_actual();
                        rank_add_or_update(&mut b.ocean_rank, &pid, cp, now, actual);
                    }
                })
            });
            let out = match out {
                Ok(o) => o,
                Err(code) => return Ok(Some(reject(41003, code))),
            };
            Ok(Some(
                OceanTradeNegotiationResp41003 {
                    ret: 0,
                    pre_money: Some(out.pre_money.to_string()),
                    cost_money: Some(out.cost_money.to_string()),
                    checkpoint_id: Some(out.checkpoint_id),
                    npc_hp: Some(out.npc_hp.to_string()),
                    msg_resources: resources_to_msg(&out.resources),
                    win_num: Some(out.win_num),
                    damage_hp: Some(out.damage_hp.to_string()),
                    end_game: Some(out.end_game),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    n
}

/// `service.getCurrentSeasonId()` + `getCurrentSeasonStartTime()`。
///
/// 原版两个方法在 `!isOpenOceanTrade()` 时都返回 0；这里用
/// `b.ocean_common` 是否存在来近似「服务是否 init」。
fn current_season(
    b: &mut PlayerBox,
    tables: &ocean_trade::CheckpointTables,
    zero: i64,
    season_days: i64,
) -> (i64, i64) {
    if b.ocean_common.current.is_none() {
        // `initDelay()`：第一次进来时创建首赛季
        if let Some(t) =
            ocean_trade::OceanTradeCommonData::create_first_start_time(zero, zero, season_days, 1)
        {
            b.ocean_common.season_id = 1;
            b.ocean_common.current = Some(t);
        } else {
            return (0, 0);
        }
    }
    // 赛季自然滚动：`now > current.endTime` 时进下一赛季
    if let Some(cur) = b.ocean_common.current {
        if zero >= cur.end_time {
            if let Some(next) = b.ocean_common.next_time(zero, season_days) {
                b.ocean_common.handle_new_season(next);
            }
        }
    }
    let Some(cur) = b.ocean_common.current else {
        return (0, 0);
    };
    b.ocean_season_id = cur.id;
    let _ = tables;
    (cur.id, cur.start_time)
}

/// `builder.setEndTime(...)`：当前赛季终点。
fn season_end(b: &mut PlayerBox, zero: i64, season_days: i64) -> i64 {
    b.ocean_common
        .current
        .map_or(0, |t| if t.end_time > 0 { t.end_time } else { zero + season_days * ocean_trade::DAY_MS })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(pid: &str, cp: i32, t: i64) -> RankEntry {
        RankEntry {
            player_id: pid.to_owned(),
            checkpoint_id: cp,
            update_time: t,
        }
    }

    #[test]
    fn rank_cmp_is_checkpoint_desc_then_time_asc() {
        assert_eq!(rank_cmp(&e("a", 5, 100), &e("b", 3, 100)), std::cmp::Ordering::Less);
        assert_eq!(rank_cmp(&e("a", 3, 100), &e("b", 5, 100)), std::cmp::Ordering::Greater);
        // 同关卡：updateTime 小的在前
        assert_eq!(rank_cmp(&e("a", 5, 50), &e("b", 5, 100)), std::cmp::Ordering::Less);
        assert_eq!(rank_cmp(&e("a", 5, 100), &e("b", 5, 100)), std::cmp::Ordering::Equal);
    }

    #[test]
    fn rank_add_inserts_sorts_and_caps() {
        let mut list = Vec::new();
        rank_add_or_update(&mut list, "a", 3, 10, 5);
        rank_add_or_update(&mut list, "b", 5, 20, 5);
        rank_add_or_update(&mut list, "c", 4, 30, 5);
        assert_eq!(
            list.iter().map(|x| x.player_id.as_str()).collect::<Vec<_>>(),
            vec!["b", "c", "a"]
        );
        // 重复更新同一玩家 → 改值不改数量
        rank_add_or_update(&mut list, "a", 9, 40, 5);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].player_id, "a");
        assert_eq!(list[0].checkpoint_id, 9);
    }

    #[test]
    fn rank_add_rejects_when_full_and_not_better() {
        let mut list = vec![e("a", 9, 1), e("b", 8, 2)];
        // actual = 2，满；新玩家 cp = 5 < 尾部 8 → 丢弃
        rank_add_or_update(&mut list, "c", 5, 3, 2);
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|x| x.player_id != "c"));
        // cp = 50 > 尾部 8 → 进榜并弹出尾部
        rank_add_or_update(&mut list, "c", 50, 4, 2);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].player_id, "c");
    }

    #[test]
    fn rank_index_is_one_based_and_negative_when_absent() {
        let list = vec![e("a", 9, 1), e("b", 8, 2)];
        assert_eq!(rank_index(&list, "a"), 1);
        assert_eq!(rank_index(&list, "b"), 2);
        assert_eq!(rank_index(&list, "z"), ocean_trade::NOT_IN_RANKING);
    }

    #[test]
    fn make_entry_fills_key_varstr_rank() {
        let m = make_entry("p1", 42, 3);
        assert_eq!(m.key.as_deref(), Some("p1"));
        assert_eq!(m.var_str, vec!["42".to_owned()]);
        assert_eq!(m.rank, Some(3));
        assert!(m.family_name.is_none(), "本轮不填家族字段");
    }

    #[test]
    fn ret_code_maps_ocean_trade_codes() {
        assert_eq!(ret_code(CODE_NOT_OPEN), RetCode::OceanTradeNotOpen);
        assert_eq!(ret_code(CODE_NUM_ILLEGAL), RetCode::OceanTradeNumIllegal);
        assert_eq!(ret_code(CODE_QUICKLY_LOCKED), RetCode::OceanTradeQuicklyLocked);
        assert_eq!(
            ret_code(CODE_SEASON_ID_ILLEGAL),
            RetCode::OceanTradeSeasonIdIllegal
        );
        assert_eq!(ret_code(99_999), RetCode::ServerBusy);
    }
}
