//! 焚香祈愿 17071 / 17072 的接线。
//!
//! 纯逻辑在 [`crate::pray`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## ⚠ 17071 一次请求发两个包
//! ```java
//! MessageUtil.sendToPlayerId(MsgPrayDataResp_17074{...}, player.getId());   // 先推
//! return PrayResp_17071{ret=0, rewards};                                    // 再回
//! ```
//! 推送走 [`ddj_registry::SessionControl::push`]，回包走 `Ok(Some(...))`。
//!
//! ## 失败出口
//! `pray` / `getNotices` 都是 `throws HandlerException` → **`ErrorResp_1`（帧号 1）**。
//! | cmd | 失败 |
//! |---|---|
//! | 17071 | `3901`（未开放）/ `3902`（资源不足） |
//! | 17072 | `3901`（未开放） |
//!
//! ## 抽取的随机源
//! 三个位置：
//! 1. `isHitForTenThousand(3000)` → `gen_index(10000) < 3000`
//! 2. 掉落权重 → `gen_range_i64(1, weight_sum)`
//! 3. 掉落数量 → `gen_range_i64(num_min, num_max)`（闭区间）
//!
//! ## 系统开放判定（`SystemOpenUtils.isOpen(playerId, 120)`）
//! `SystemOpenDB` 未接，过渡期视为**已开放**。
//! 这样 17071 才能走到「资源不足」而不是「未开放」——
//! 与实测（`G-功能开启表-SystemOpen.csv` 里 120 存在）一致。

use std::sync::{Arc, OnceLock};

use ddj_core::{Clock, RngSource};
use ddj_pb::gen::sys_activity_message::{
    MsgPrayDataResp17074, MsgPrayNoticeReq17072, MsgPrayNoticeResp17072, NoticeData, PrayData,
    PrayReq17071, PrayResp17071,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::drop::{self, WeightPick, CANNOT_ATTACK_TIME_OFFSET};
use crate::pray::{
    self, DrawOutcome, PrayState, PrayTemplate, PrayTimes, PRAY_ERROR_COST, PRAY_ERROR_UN_OPEN,
};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// `ErrorResp_1` 自己的消息号。
const ERROR_RESP_CMD: i32 = 1;

/// `pray` 用的系统开放 id（`SystemOpenUtils.isOpen(playerId, 120)`）。
const SYSTEM_OPEN_PRAY: i32 = 120;

/// 东八区偏移（毫秒）。
const TZ_MS: i64 = 8 * 3600 * 1000;

/// `PrayDBCache` 的注入点。
static TEMPLATES: OnceLock<Vec<PrayTemplate>> = OnceLock::new();

/// 注入祈愿表（只第一次生效）。
pub fn install_templates(t: Vec<PrayTemplate>) {
    let _ = TEMPLATES.set(t);
}

fn templates() -> &'static [PrayTemplate] {
    TEMPLATES.get().map_or(&[], Vec::as_slice)
}

/// 开服天数（`GameServer.getDay(openTime, now)`）。
///
/// 过渡期：没有 `GsServer.open_time`，固定按「开服已久」处理（返回一个大数），
/// 这样 `getByServerOpenDay` 总能拿到 `day=1` 那一行。
fn open_day(now: i64) -> i32 {
    // 原版 `getDay` 是「零点对齐后的天数差」。用纪元日序号即可。
    i32::try_from((now + TZ_MS).div_euclid(86_400_000)).unwrap_or(i32::MAX)
}

/// `LogicUtils.getTodayZeroTimeStamp()`。
fn today_zero(now: i64) -> i64 {
    (now + TZ_MS).div_euclid(86_400_000) * 86_400_000 - TZ_MS
}

fn reject(cmd: i32, code: ddj_pb::RetCode) -> Reply {
    Reply::with_cmd(
        ERROR_RESP_CMD,
        crate::sign::error_resp(cmd, code).encode_to_vec(),
    )
}

/// 进程级随机源。
static RNG: OnceLock<std::sync::Mutex<ddj_core::DeterministicRng>> = OnceLock::new();

fn rng() -> &'static std::sync::Mutex<ddj_core::DeterministicRng> {
    RNG.get_or_init(|| std::sync::Mutex::new(ddj_core::DeterministicRng::new(0x5EED_1707)))
}

/// `DropUtils.rand(player, dropId)` 的一次抽取。
///
/// `DropOne.extractNum` 次循环，每次：
/// 1. `givenDropId` 命中 → 用它；否则
/// 2. 公式命中 → 用公式指定的二级；否则
/// 3. 从 `idTwo` 里按顺序取第一个 `formulaId == 0` 的二级
/// 4. 用该二级的 `randItem()` 抽一项
fn rand_drop(one_id: i32, counters: &mut drop::DropCounters) -> Vec<(i32, i64)> {
    let Some(tables) = drop::tables() else {
        return Vec::new();
    };
    let Some(one) = tables.one(one_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let r = rng().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for _ in 0..one.extract_num.max(0) {
        // 1) 强制掉落
        let done = counters.private.get(&one.id).copied().unwrap_or(0);
        let mut picked = drop::given_drop_id(&one, done).and_then(|id| tables.two(id));
        // ⚠ 原版 `modelOne.addTimes(one.getId(), 1)` 在 givenDrop 非空时**无条件**执行
        if !one.given_drop.is_empty() {
            *counters.private.entry(one.id).or_insert(0) += 1;
        }
        // 2) 公式
        if picked.is_none() {
            for id in &one.id_two {
                let Some(two) = tables.two(*id) else { continue };
                if two.formula_id == 0 {
                    picked = Some(two);
                    break;
                }
                let value = drop::formula_value(&two, 0, |sid| {
                    tables.sign(sid).map_or(0, |s| counters.times(&s))
                });
                let roll = r.gen_range_i64(0, CANNOT_ATTACK_TIME_OFFSET);
                let ok = drop::formula_hit(value, roll);
                // 下标累加/重置
                for sid in &two.sign_id {
                    if let Some(s) = tables.sign(*sid) {
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
        // 3) 权重抽取
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
            // 哑元：这一抽什么都不出
            WeightPick::Empty | WeightPick::ZeroWeight => {}
        }
    }
    drop::merge(out)
}

/// 注册 2 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 17071 抽奖
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(17071, move |ctx, body| {
            let req = PrayReq17071::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let times = PrayTimes::from_times(req.times.unwrap_or(0));

            // 系统开放 / 开服天数选表
            let _ = SYSTEM_OPEN_PRAY;
            let Some(tpl) = pray::by_open_day(templates(), open_day(now)).cloned() else {
                return Ok(Some(reject(17071, PRAY_ERROR_UN_OPEN)));
            };

            let state = w.peek(p, |b| b.pray.clone());
            let zero = today_zero(now);
            let free = times == PrayTimes::One && pray::is_free(&state, zero);

            // 扣费
            let cost: Vec<(i32, i64)> = match times {
                PrayTimes::One if free => Vec::new(),
                PrayTimes::One => tpl.one_cost.clone(),
                PrayTimes::Ten => tpl.ten_cost.clone(),
            };
            let affordable = w.peek(p, |b| {
                cost.iter()
                    .all(|(id, num)| b.bag.get(id).copied().unwrap_or(0) >= *num)
            });
            if !affordable {
                return Ok(Some(reject(17071, PRAY_ERROR_COST)));
            }

            let mut next = state.clone();
            if free {
                next.free_time = zero;
            }

            // 抽 draws 次
            let mut counters = drop::DropCounters::default();
            let mut rewards: Vec<(i32, i64)> = Vec::new();
            let mut core_hits: Vec<(i32, i64)> = Vec::new();
            for _ in 0..times.draws() {
                let special = next.first_reward <= 0 && next.times < 10;
                let hit = if special {
                    let r = rng().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let roll = r.gen_index(usize::try_from(pray::WAN_FEN).unwrap_or(10_000));
                    drop::formula_hit(i64::from(pray::FIRST_REWARD_CHANCE), roll as i64)
                } else {
                    false
                };
                match pray::draw_verdict(special, hit, next.luck_num, tpl.luck_num) {
                    DrawOutcome::Core => {
                        let core = tpl.core_reward.clone();
                        rewards.extend(core.iter().copied());
                        core_hits.extend(core);
                        if special {
                            next.times += 1;
                            next.first_reward = next.times;
                        }
                    }
                    DrawOutcome::Drop => {
                        if special {
                            next.times += 1;
                        }
                        rewards.extend(rand_drop(tpl.drop_id, &mut counters));
                    }
                }
                next = next.add_luck_num(tpl.every_luck_num, tpl.luck_num);
            }

            let merged = drop::merge(rewards);
            // 落状态 + 扣费 + 发奖
            w.with(p, |b| {
                for (id, num) in &cost {
                    if let Some(v) = b.bag.get_mut(id) {
                        *v = (*v - num).max(0);
                    }
                }
                for (id, num) in &merged {
                    *b.bag.entry(*id).or_insert(0) += num;
                }
                b.pray = next.clone();
            });

            // 全服公告（核心奖励）
            let core_id = tpl.core_reward_id();
            if let Some(cid) = core_id {
                let hit_num: i64 = core_hits
                    .iter()
                    .filter(|(id, _)| *id == cid)
                    .map(|(_, n)| *n)
                    .sum();
                if hit_num > 0 {
                    let name = w.peek(p, |b| b.name.clone());
                    let mut list = w.pray_notices();
                    pray::push_notice(
                        &mut list,
                        pray::Notice {
                            name,
                            rewards: vec![(cid, hit_num)],
                        },
                    );
                }
            }

            // 先推 17074
            if let Some(sess) = ctx.session.as_ref() {
                let (ft, luck, cycle) = next.to_msg_parts();
                let push = MsgPrayDataResp17074 {
                    pray_data: Some(PrayData {
                        free_time: Some(ft),
                        luck_num: Some(luck),
                        cycle: Some(cycle),
                    }),
                };
                sess.push(17074, push.encode_to_vec().into());
            }

            Ok(Some(
                PrayResp17071 {
                    ret: Some(0),
                    rewards: crate::runtime::resources_to_msg(&merged),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // 17072 公告栏
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17072, move |ctx, body| {
            let _req = MsgPrayNoticeReq17072::decode(body).map_err(decode_err)?;
            let _p = ctx.require_player()?;
            let notices = w.pray_notices();
            let list: Vec<NoticeData> = notices
                .iter()
                .map(|n| NoticeData {
                    name: Some(n.name.clone()),
                    rewards: crate::runtime::resources_to_msg(&n.rewards),
                })
                .collect();
            Ok(Some(
                MsgPrayNoticeResp17072 { notices: list }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    let _ = PrayState::default;
    n
}
