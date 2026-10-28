//! 游历 15002 / 15004 / 15009 的接线。
//!
//! 纯逻辑在 [`crate::travel`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## 三条消息的骨架
//! | cmd | 入口 | 前置 |
//! |---|---|---|
//! | 15002 | `Travel.travel(player, req, false)` | `place ∈ {0,12}`；`isOpen(3)`；`allTimes != 0 → isOpen(102)` |
//! | 15004 | `Travel.travelTenTimes(player)` | `isOpen(133)` |
//! | 15009 | 直接改 `partnerSex` | `sex ∈ {0,1}`；与当前不同；`isOpen(161)`；扣 `PartnerChangeCost` |
//!
//! ## 失败出口
//! 三条都是 `throws HandlerException` → **`ErrorResp_1`（帧号 1）**：
//! | cmd | 场景 | 码 |
//! |---|---|---|
//! | 15002 | place 非 0/12 | `1006` `TRAVEL_ERROR_PLACE_LIMIT` |
//! | 15002 | `isOpen(3)` 假 | `1003` `TRAVEL_ERROR_UNOPEN_SYSTEM` |
//! | 15002 | `allTimes!=0 && !isOpen(102)` | `1004` `TRAVEL_ERROR_ALL_UNOPEN_SYSTEM` |
//! | 15002 | 体力不足（第一个事件就没有） | `1001` `TRAVEL_ERROR_PHYSICAL_LIMIT` |
//! | 15002 | 印章不足（已经抽了几个之后） | `1005` `TRAVEL_ERROR_RESOURCES_LIMIT` |
//! | 15002 | `place==12 && !isOpen(26)` | `2356` `NPC_NO_OPEN` |
//! | 15002 | `place==12 && travelCnt>10 && !isOpen(139)` | `17` `VipNo` |
//! | 15004 | `isOpen(133)` 假 | `1003` |
//! | 15004 | 计数 0 且没道具 22 | `2` `CommonServerBusy` |
//! | 15009 | sex 非 0/1，或与当前相同，或扣费失败 | `2` |
//! | 15009 | `isOpen(161)` 假 | `1003` |
//!
//! ## 先推后回（同 17071）
//! 15002 / 15004 末尾都是：
//! ```java
//! MessageUtil.sendToPlayerId(MsgTravelDataResp_15001{place, progress...}, playerId);  // 先推
//! return MsgTravelResp_15002{ret=0, events};                                          // 再回
//! ```
//! 推送走 [`ddj_registry::SessionControl::push`]。15004 同理推 **15001**。
//!
//! ## 本轮未接的部分（AUDIT A22）
//! - 门客（事件 1）/ 珍兽（事件 2）/ 美人（事件 4）的**实体落地**。
//!   `Msgs` 照发（`targetId × 1` 表示「获得」），但不写实体表 ——
//!   因为 `TalentSet` / `PetSet` / `ChildrenSet` / `NpcSpouseModel` 都还没接。
//! - 时装加亲密度（`FashionModel.getFashionSuitPartAddIntimacy()`）。
//! - `PetMiracleType.ID_NPC_INTIMACY` 加成（`multiple` 恒 0）。
//! - 奇遇、子女出生（`bornByTravel`）、`childDataUpdateResp`。
//! - `EventLogSendRecord.tour` / `mansion_use_carriage`（事件日志）。
//! - `EventModule` 事件（`ACTIVITY_TASK_10_TRAVEL_TIMES` 等）。
//! - 自动游历（`AutoPlayTravel`）。

use std::sync::{Arc, OnceLock};

use ddj_core::{Clock, RngSource};
use ddj_pb::gen::common_msg_message::MsgResources;
use ddj_pb::gen::pet_message::{MsgPetData, PetUpdateResp7001};
use ddj_pb::gen::travel_message::{
    MsgTravelDataResp15001, MsgTravelEventData, MsgTravelProgress, MsgTravelReq15002,
    MsgTravelResp15002, MsgTravelTenReq15004, MsgTravelTenResp15004, PartnerSexReq15009,
    PartnerSexResp15009,
};
use ddj_pb::RetCode;
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::runtime::{decode_err, FnHandler, MemWorld, PlayerBox};
use crate::travel::{
    self, EventAcc, EventMsg, TravelConfig, TravelEvent,
};

/// `ErrorResp_1` 自己的消息号。
const ERROR_RESP_CMD: i32 = 1;

/// 游历系统开放 id（`TravelMessageHandler` 的 `isOpen(3)`）。
const OPEN_TRAVEL: i32 = 3;
/// 一键游历（`isOpen(102)`）。
const OPEN_ALL_TIMES: i32 = 102;
/// 十连（`isOpen(133)`）。
const OPEN_TEN: i32 = 133;
/// 桃花岛 / 知己（`isOpen(26)`）。
const OPEN_NPC: i32 = 26;
/// 十连超过 10 次要 VIP（`isOpen(139)`）。
const OPEN_VIP_CNT: i32 = 139;
/// 知己变性（`isOpen(161)`）。
const OPEN_PARTNER_SEX: i32 = 161;

/// `CounterRecovery` 里游历用的模板 id。
const COUNTER_TRAVEL: i32 = 1;
/// 十连要消耗的道具（`ItemServerModule.getItem(22)`）。
const ITEM_TEN_COST: i32 = 22;

/// 事件表 + 配置的注入点。
static TEMPLATES: OnceLock<Vec<TravelEvent>> = OnceLock::new();
static CONFIG: OnceLock<TravelConfig> = OnceLock::new();

/// 注入游历事件表（只第一次生效）。
pub fn install_templates(t: Vec<TravelEvent>) {
    let _ = TEMPLATES.set(t);
}

/// 注入配置（只第一次生效）。
pub fn install_config(c: TravelConfig) {
    let _ = CONFIG.set(c);
}

fn templates() -> &'static [TravelEvent] {
    TEMPLATES.get().map_or(&[], Vec::as_slice)
}

fn config() -> &'static TravelConfig {
    CONFIG.get_or_init(TravelConfig::default)
}

fn reject(cmd: i32, code: RetCode) -> Reply {
    Reply::with_cmd(
        ERROR_RESP_CMD,
        crate::sign::error_resp(cmd, code).encode_to_vec(),
    )
}

/// 进程级随机源。
static RNG: OnceLock<std::sync::Mutex<ddj_core::DeterministicRng>> = OnceLock::new();

fn rng() -> &'static std::sync::Mutex<ddj_core::DeterministicRng> {
    RNG.get_or_init(|| std::sync::Mutex::new(ddj_core::DeterministicRng::new(0x5EED_1502)))
}

/// 抽一次事件（`TravelEventsDBUtils.randTravel`）。
///
/// 顺序照抄：
/// 1. `tryGetSpecialTravelEvent`（**只在 `place == 0`**，且会自增计数）
/// 2. 否则 `filter` → 算权重 → `WeightRandomModelUtils.random()`
///
/// 返回 `(事件, 自增后的 NotPlaceTravelTimes)`。
fn rand_travel(
    box_now: &mut PlayerBox,
    place: i32,
    events: &[TravelEvent],
) -> (Option<TravelEvent>, i64) {
    let cfg = config();
    // 1) 固定事件
    if place == 0 {
        let before = box_now.travel_not_place_times;
        // `if (times >= 100) return null` —— 判的是自增前
        if before < 100 {
            box_now.travel_not_place_times = before + 1;
        }
        if let Some(id) = travel::special_event_id(cfg, before) {
            if let Some(e) = events.iter().find(|e| e.id == id) {
                return (Some(e.clone()), box_now.travel_not_place_times);
            }
            // 原版 `cache.get(id)` 拿不到会 NPE；这里退回随机。
        }
    }
    // 2) 权重抽取
    let level = box_now.level;
    // `unlockType == 2` 要「拥有该珍兽」；PetSet 未接 → 空集（= 全部不满足）。
    let owned: Vec<i32> = Vec::new();
    let list = travel::filter(events, place, level, &owned);
    if list.is_empty() {
        // 原版 `.random()` 在空列表上抛 `IllegalArgumentException`
        return (None, box_now.travel_not_place_times);
    }
    let mut weights = Vec::with_capacity(list.len());
    for e in &list {
        weights.push(travel::extract_weight_scale(e, travel::progress_of(&box_now.travel_progress, e.target_id)));
    }
    let total: i32 = weights.iter().sum();
    if total <= 0 {
        return (None, box_now.travel_not_place_times);
    }
    let r = rng().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let roll = i32::try_from(r.gen_range_i64(1, i64::from(total))).unwrap_or(total);
    drop(r);
    match travel::pick_weighted(&weights, roll) {
        travel::Pick::Hit(i) => (Some(list[i].clone()), box_now.travel_not_place_times),
        travel::Pick::Empty => (None, box_now.travel_not_place_times),
    }
}

/// 背包里某个 id 的数量。
fn bag_of(b: &PlayerBox, id: i32) -> i64 {
    b.bag.get(&id).copied().unwrap_or(0)
}

/// 计数器 1 的当前次数。
fn counter_num(b: &PlayerBox) -> i64 {
    b.counter_map
        .get(&(COUNTER_TRAVEL, 0))
        .map_or(0, |s| s.num)
}

/// 扣计数器（`removeNum(playerId, 1, n, false)`）。
fn remove_counter(b: &mut PlayerBox, n: i64) {
    if n <= 0 {
        return;
    }
    if let Some(s) = b.counter_map.get_mut(&(COUNTER_TRAVEL, 0)) {
        s.num = (s.num - n).max(0);
    }
}

/// 扣背包。
fn remove_item(b: &mut PlayerBox, id: i32, n: i64) {
    if n <= 0 {
        return;
    }
    if let Some(v) = b.bag.get_mut(&id) {
        *v = (*v - n).max(0);
    }
}

/// 加背包（`ResourcesUtils.addResourcesJsons`）。
fn add_items(b: &mut PlayerBox, list: &[(i32, i64)]) {
    for (id, n) in list {
        if *n == 0 {
            continue;
        }
        *b.bag.entry(*id).or_insert(0) += n;
    }
}

/// 把 [`EventMsg`] 转成 protobuf。
#[must_use]
pub fn to_msg(e: &EventMsg) -> MsgTravelEventData {
    MsgTravelEventData {
        event_id: Some(e.event_id),
        msg_resources: crate::runtime::resources_to_msg(&e.resources),
        progress: e.progress.map(|(id, p)| MsgTravelProgress {
            id: Some(id),
            progress: Some(p),
        }),
    }
}

/// `Pet.toMsgPetData(5)`：只带 `growth`。
///
/// ```java
/// builder.addUpdateFieldsList(1);
/// builder.setPetId(this.petId);
/// for (field : fieldType) { addUpdateFieldsList(field); ... case 5: setGrowth(growth + skinAddGrowth) }
/// ```
/// `skinAddGrowth` 依赖皮肤模块，本模块恒 0。
#[must_use]
pub fn pet_growth_msg(pet_id: i32, growth: i64) -> MsgPetData {
    MsgPetData {
        update_fields_list: vec![1, 5],
        pet_id: Some(pet_id),
        growth: Some(growth),
        ..MsgPetData::default()
    }
}

/// 注册 3 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // ── 15002 游历 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(15002, move |ctx, body| {
            let req = MsgTravelReq15002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let place = req.place.unwrap_or(0);
            let all_times = req.all_times.unwrap_or(0);
            let travel_cnt = req.travel_cnt.unwrap_or(0);

            // `if (getPlace() != 0 && getPlace() != 12) throw TRAVEL_ERROR_PLACE_LIMIT`
            if place != 0 && place != 12 {
                return Ok(Some(reject(15002, RetCode::TravelErrorPlaceLimit)));
            }
            let srv = crate::install_system_open::server();
            if !crate::install_system_open::is_open(&w, p, c.as_ref(), srv, OPEN_TRAVEL) {
                return Ok(Some(reject(15002, RetCode::TravelErrorUnopenSystem)));
            }
            if all_times != 0
                && !crate::install_system_open::is_open(&w, p, c.as_ref(), srv, OPEN_ALL_TIMES)
            {
                return Ok(Some(reject(15002, RetCode::TravelErrorAllUnopenSystem)));
            }

            // ── randomEvent ──
            if place == 12 {
                if !crate::install_system_open::is_open(&w, p, c.as_ref(), srv, OPEN_NPC) {
                    return Ok(Some(reject(15002, RetCode::NpcNoOpen)));
                }
                if travel_cnt > 10
                    && !crate::install_system_open::is_open(&w, p, c.as_ref(), srv, OPEN_VIP_CNT)
                {
                    return Ok(Some(reject(15002, RetCode::VipNo)));
                }
            }
            let cfg = config();
            let (cur_counter, stamp) = w.peek(p, |b| (counter_num(b), bag_of(b, cfg.fixed_point_travel.0)));

            let event_list: Vec<TravelEvent>;
            let stamps_used: i64;
            if place == 12 {
                let (cnt, err) = travel::plan_fixed(all_times, travel_cnt, cur_counter, stamp, cfg);
                if err {
                    return Ok(Some(reject(15002, RetCode::TravelErrorResourcesLimit)));
                }
                if cnt == 0 {
                    // `if (travelTimes == 0) break;` —— eventDBs 为空，
                    // 原版随后 `eventDBs.get(size-1)` 抛 AIOOBE。这里回 1005。
                    return Ok(Some(reject(15002, RetCode::TravelErrorResourcesLimit)));
                }
                let mut acc = Vec::with_capacity(cnt);
                for _ in 0..cnt {
                    let (e, _) = w.with(p, |b| rand_travel(b, place, templates()));
                    if let Some(e) = e {
                        acc.push(e);
                    }
                }
                event_list = acc;
                stamps_used = cfg.fixed_point_travel.1 * i64::try_from(cnt).unwrap_or(0);
            } else {
                let (cnt, empty) = travel::plan_random(cur_counter, cfg);
                if empty {
                    return Ok(Some(reject(15002, RetCode::TravelErrorPhysicalLimit)));
                }
                let mut acc = Vec::with_capacity(cnt);
                for _ in 0..cnt {
                    let (e, _) = w.with(p, |b| rand_travel(b, place, templates()));
                    if let Some(e) = e {
                        acc.push(e);
                    }
                }
                event_list = acc;
                stamps_used = 0;
            }
            if event_list.is_empty() {
                // 理论上不会到这里（上面都在空时返回了）
                return Ok(Some(reject(15002, RetCode::TravelErrorResourcesLimit)));
            }

            // ── 逐事件结算 ──
            let mut acc = EventAcc::default();
            w.with(p, |b| {
                for e in &event_list {
                    // `TalentSet.getTalent(targetId) != null`：两个实体集都未接 → false
                    let owned = false;
                    let _ = acc.on_event(e, &mut b.travel_progress, owned);
                }
                // 扣资源
                remove_counter(b, cfg.physical_strength * i64::try_from(event_list.len()).unwrap_or(0));
                remove_item(b, cfg.fixed_point_travel.0, stamps_used);
                // 统计
                b.travel_times += i64::try_from(event_list.len()).unwrap_or(0);
                // `this.place = eventDBs.get(eventDBs.size()-1).getPlace();`
                if let Some(last) = event_list.last() {
                    b.travel_place = last.place;
                }
            });
            let progress = w.peek(p, |b| b.travel_progress.clone());
            let outcome = acc.apply(&progress);

            // 加资源
            w.with(p, |b| add_items(b, &outcome.gains));

            // 先推 15001
            let place_now = w.peek(p, |b| b.travel_place);
            if let Some(sess) = ctx.session.as_ref() {
                let push = MsgTravelDataResp15001 {
                    place: Some(place_now),
                    msg_travel_progress: outcome
                        .progresses
                        .iter()
                        .map(|(id, pr)| MsgTravelProgress {
                            id: Some(*id),
                            progress: Some(*pr),
                        })
                        .collect(),
                };
                sess.push(15001, push.encode_to_vec().into());
            }

            // 再回 15002
            Ok(Some(
                MsgTravelResp15002 {
                    ret: 0,
                    msg_travel_event_data: outcome.events.iter().map(to_msg).collect(),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // ── 15004 十连 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(15004, move |ctx, body| {
            let _req = MsgTravelTenReq15004::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let srv = crate::install_system_open::server();
            if !crate::install_system_open::is_open(&w, p, c.as_ref(), srv, OPEN_TEN) {
                return Ok(Some(reject(15004, RetCode::TravelErrorUnopenSystem)));
            }
            let (cur_num, item22) = w.peek(p, |b| (counter_num(b), bag_of(b, ITEM_TEN_COST)));
            // `vipDB.getCounterMax(1)`：VipDBCache 已接（csv_counter），但这里只用到
            // `counterMax[0]`。当前玩家 VIP 等级未接 → 按 1 处理（= 不翻倍）。
            let Some(plan) = travel::plan_ten(cur_num, item22, 1) else {
                return Ok(Some(reject(15004, RetCode::ServerBusy)));
            };

            // 抽 plan.total 次（全部 `place = 0`）
            let mut event_list = Vec::with_capacity(plan.total);
            for _ in 0..plan.total {
                let (e, _) = w.with(p, |b| rand_travel(b, 0, templates()));
                if let Some(e) = e {
                    event_list.push(e);
                }
            }

            // 扣费
            w.with(p, |b| {
                remove_counter(b, plan.counter_used);
                remove_item(b, ITEM_TEN_COST, plan.item22_used);
            });

            // 结算
            let mut acc = EventAcc::default();
            w.with(p, |b| {
                for e in &event_list {
                    let _ = acc.on_event(e, &mut b.travel_progress, false);
                }
                b.travel_times += i64::try_from(event_list.len()).unwrap_or(0);
            });
            let progress = w.peek(p, |b| b.travel_progress.clone());
            let outcome = acc.apply(&progress);
            w.with(p, |b| add_items(b, &outcome.gains));

            // 先推 15001（⚠ 十连用 `this.place`，不改成事件的 place）
            let place_now = w.peek(p, |b| b.travel_place);
            if let Some(sess) = ctx.session.as_ref() {
                let push = MsgTravelDataResp15001 {
                    place: Some(place_now),
                    msg_travel_progress: outcome
                        .progresses
                        .iter()
                        .map(|(id, pr)| MsgTravelProgress {
                            id: Some(*id),
                            progress: Some(*pr),
                        })
                        .collect(),
                };
                sess.push(15001, push.encode_to_vec().into());
            }

            // 珍兽成长要推 7001
            if !outcome.pet_growth.is_empty() {
                if let Some(sess) = ctx.session.as_ref() {
                    let mut msg = PetUpdateResp7001::default();
                    for (pet_id, g) in &outcome.pet_growth {
                        let total = w.peek(p, |b| {
                            b.pet_extra.get(pet_id).map_or(*g, |e| e.growth + g)
                        });
                        msg.msg_pet_data.push(pet_growth_msg(*pet_id, total));
                    }
                    sess.push(7001, msg.encode_to_vec().into());
                }
            }

            // `filtrateChild`
            let msgs: Vec<EventMsg> = outcome.events.clone();
            let child = travel::filtrate_child(&msgs);

            Ok(Some(
                MsgTravelTenResp15004 {
                    ret: 0,
                    msg_travel_event_data: msgs.iter().map(to_msg).collect(),
                    msg_child_travel_event_data: child.iter().map(to_msg).collect(),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // ── 15009 知己变性 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(15009, move |ctx, body| {
            let req = PartnerSexReq15009::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let sex = req.sex.unwrap_or(-1);
            // `if (getSex() != 0 && getSex() != 1) throw COMMON_SERVER_BUSY`
            if sex != 0 && sex != 1 {
                return Ok(Some(reject(15009, RetCode::ServerBusy)));
            }
            if w.peek(p, |b| b.partner_sex) == sex {
                return Ok(Some(reject(15009, RetCode::ServerBusy)));
            }
            let srv = crate::install_system_open::server();
            if !crate::install_system_open::is_open(&w, p, c.as_ref(), srv, OPEN_PARTNER_SEX) {
                return Ok(Some(reject(15009, RetCode::TravelErrorUnopenSystem)));
            }
            let cfg = config();
            let (id, num) = cfg.partner_change_cost;
            if w.peek(p, |b| bag_of(b, id)) < num {
                return Ok(Some(reject(15009, RetCode::ServerBusy)));
            }
            w.with(p, |b| {
                remove_item(b, id, num);
                b.partner_sex = sex;
            });
            Ok(Some(
                PartnerSexResp15009 {
                    ret: 0,
                    sex: Some(sex),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    n
}

/// 供 15004 引用的空实现占位（避免未用告警）。
const _: Option<MsgResources> = None;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pet_growth_msg_has_fields() {
        let m = pet_growth_msg(1120, 5);
        assert_eq!(m.update_fields_list, vec![1, 5]);
        assert_eq!(m.pet_id, Some(1120));
        assert_eq!(m.growth, Some(5));
    }

    #[test]
    fn to_msg_carries_progress() {
        let e = EventMsg {
            event_id: 1101,
            resources: vec![(20, 1)],
            progress: Some((1003, 1)),
        };
        let m = to_msg(&e);
        assert_eq!(m.event_id, Some(1101));
        assert_eq!(m.msg_resources.len(), 1);
        assert_eq!(m.msg_resources[0].id, Some(20));
        let p = m.progress.expect("progress");
        assert_eq!(p.id, Some(1003));
        assert_eq!(p.progress, Some(1));
    }

    #[test]
    fn counter_num_reads_template_1() {
        let mut b = PlayerBox::default();
        assert_eq!(counter_num(&b), 0);
        b.counter_map.insert(
            (COUNTER_TRAVEL, 0),
            crate::counter::CounterState {
                num: 7,
                ..crate::counter::CounterState::default()
            },
        );
        assert_eq!(counter_num(&b), 7);
        remove_counter(&mut b, 3);
        assert_eq!(counter_num(&b), 4);
        // 扣超了不减到负数
        remove_counter(&mut b, 99);
        assert_eq!(counter_num(&b), 0);
    }

    #[test]
    fn add_items_merges_in_place() {
        let mut b = PlayerBox::default();
        add_items(&mut b, &[(20, 3), (21, 1)]);
        add_items(&mut b, &[(20, 2)]);
        assert_eq!(bag_of(&b, 20), 5);
        assert_eq!(bag_of(&b, 21), 1);
        // 0 增量不创建条目
        add_items(&mut b, &[(99, 0)]);
        assert!(!b.bag.contains_key(&99));
    }
}
