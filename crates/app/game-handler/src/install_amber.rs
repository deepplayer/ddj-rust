//! 琥珀会员 6 条消息的接线（17091 / 17092 / 17093 / 17094 / 17095 / 17097）。
//!
//! ## 过渡期的两处假定
//! 原版的门禁是 `AmberWelfareDBCache.getOppoCids()`（渠道白名单）与「充值回调里置
//! `amberVip`」。这两处都要真实配置/支付链路，仓储还没接，所以先用两个常量顶住：
//!
//! - [`ASSUMED_AMBER_CID`]：视为白名单内的渠道号；
//! - [`ASSUMED_AMBER_VIP`]：视为已开通会员，让 `check_vip` 放行。
//!
//! 接好模板后只需把这两个常量的使用点换掉，业务函数与回包逻辑不用动。

use std::sync::Arc;

use ddj_core::{Clock, PlayerId};
use ddj_pb::gen::sys_activity_message::{
    AmberHeyTapVipReq17097, AmberHeyTapVipResp17097, AmberVipReq17095, AmberVipResp17095,
    AmberWelfareChargeGiftReq17094, AmberWelfareDataReq17091, AmberWelfareExclusiveReq17093,
    AmberWelfareSignReq17092,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::amber::{self, AmberData, AmberLookup, AmberModelState, AmberStore, AmberTemplate};
use crate::runtime::{decode_err, FnHandler, MemWorld};
use crate::sign::{error_resp, Reward};

/// `ErrorResp_1` 的帧消息号。
const ERROR_RESP_CMD: i32 = 1;

/// 过渡期假定的渠道号。
const ASSUMED_AMBER_CID: &str = "1";

/// 过渡期假定已开通琥珀会员。
const ASSUMED_AMBER_VIP: bool = true;

/// 东八区偏移（毫秒）。原版用 `LogicUtils.isSameDay` 走服务器默认时区。
const TZ_OFFSET_MS: i64 = 8 * 3_600 * 1_000;

/// 从内存态合成 `AmberData`。
fn amber_data(state: &AmberModelState) -> AmberData {
    AmberData {
        cid: ASSUMED_AMBER_CID.to_owned(),
        sign_in_progress: state.sign_in_progress,
        sign_in_cycle_day: state.sign_in_progress,
        sign_reward_status: state.sign_reward_status,
        exclusive_reward_status: state.exclusive_reward_status,
        charge_num: state.charge_num,
        charge_reward_ids: state.charge_reward_ids.clone(),
        amber_vip: ASSUMED_AMBER_VIP,
        hey_tap_vip: false,
    }
}

/// 只读查询器（模板表暂空）。
struct MemLookup {
    sign_in: Vec<AmberTemplate>,
    charge: Vec<AmberTemplate>,
}

impl AmberLookup for MemLookup {
    fn sign_in_dbs(&self) -> &[AmberTemplate] {
        &self.sign_in
    }

    fn exclusive_db(&self) -> Option<&AmberTemplate> {
        None
    }

    fn by_id(&self, id: i32) -> Option<&AmberTemplate> {
        self.charge.iter().find(|d| d.id == id)
    }
}

/// 写回器：`save` 落内存态，`add_resources` 进背包，`event_log` 记一行。
struct MemStore<'a> {
    world: &'a MemWorld,
    player: PlayerId,
    events: Vec<String>,
}

impl AmberStore for MemStore<'_> {
    fn save(&mut self, state: &AmberModelState) {
        self.world
            .with(self.player, |boxed| boxed.amber = state.clone());
    }

    fn add_resources(&mut self, rewards: &[Reward], _source: i32) {
        self.world.with(self.player, |boxed| {
            for (id, num) in rewards {
                *boxed.bag.entry(*id).or_insert(0) += num;
            }
        });
    }

    fn event_log(
        &mut self,
        kind: i32,
        db_id: i32,
        charge_num: i32,
        desc: &str,
        rewards: &[Reward],
    ) {
        self.events
            .push(format!("amber kind={kind} db={db_id} charge={charge_num} {desc} {rewards:?}"));
    }
}

/// 注册 6 条琥珀消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 17091 拉活动数据。
    {
        let w = world.clone();
        table.insert(Arc::new(FnHandler::new(17091, move |ctx: ddj_registry::HandlerCtx, body| {
            let w = w.clone();
            async move {
                let pid = ctx.require_player()?;
                let _req = AmberWelfareDataReq17091::decode(body).map_err(decode_err)?;
                let state = w.peek(pid, |b| b.amber.clone());
                let data = amber_data(&state);
                let resp = amber::query_resp(ASSUMED_AMBER_CID, Some(&data));
                Ok(Some(Reply::same(resp.encode_to_vec())))
            }
        })));
        n += 1;
    }

    // 17092 签到。
    {
        let w = world.clone();
        let c = clock.clone();
        table.insert(Arc::new(FnHandler::new(17092, move |ctx: ddj_registry::HandlerCtx, body| {
            let (w, c) = (w.clone(), c.clone());
            async move {
                let pid = ctx.require_player()?;
                let _req = AmberWelfareSignReq17092::decode(body).map_err(decode_err)?;
                let state = w.peek(pid, |b| b.amber.clone());
                let data = amber_data(&state);
                if let Err(code) = amber::check_vip(Some(&data)) {
                    return Ok(Some(Reply::with_cmd(
                        ERROR_RESP_CMD,
                        error_resp(17092, code).encode_to_vec(),
                    )));
                }
                let lookup = MemLookup { sign_in: Vec::new(), charge: Vec::new() };
                let mut store = MemStore { world: &w, player: pid, events: Vec::new() };
                let now = c.now_millis();
                let out = amber::sign_up(&state, now, TZ_OFFSET_MS, &lookup, &mut store);
                Ok(Some(Reply::same(
                    amber::build_sign_resp(&out).encode_to_vec(),
                )))
            }
        })));
        n += 1;
    }

    // 17093 领取专属奖励。
    {
        let w = world.clone();
        let c = clock.clone();
        table.insert(Arc::new(FnHandler::new(17093, move |ctx: ddj_registry::HandlerCtx, body| {
            let (w, c) = (w.clone(), c.clone());
            async move {
                let pid = ctx.require_player()?;
                let _req = AmberWelfareExclusiveReq17093::decode(body).map_err(decode_err)?;
                let state = w.peek(pid, |b| b.amber.clone());
                let lookup = MemLookup { sign_in: Vec::new(), charge: Vec::new() };
                let mut store = MemStore { world: &w, player: pid, events: Vec::new() };
                let now = c.now_millis();
                let out = amber::rev_exclusive_reward(&state, now, &lookup, &mut store);
                Ok(Some(Reply::same(
                    amber::build_exclusive_resp(&out).encode_to_vec(),
                )))
            }
        })));
        n += 1;
    }

    // 17094 领取充值豪礼。
    {
        let w = world.clone();
        let c = clock.clone();
        table.insert(Arc::new(FnHandler::new(17094, move |ctx: ddj_registry::HandlerCtx, body| {
            let (w, c) = (w.clone(), c.clone());
            async move {
                let pid = ctx.require_player()?;
                let req = AmberWelfareChargeGiftReq17094::decode(body).map_err(decode_err)?;
                let state = w.peek(pid, |b| b.amber.clone());
                let lookup = MemLookup { sign_in: Vec::new(), charge: Vec::new() };
                let mut store = MemStore { world: &w, player: pid, events: Vec::new() };
                let now = c.now_millis();
                let out = amber::rev_charge_gift(
                    &state,
                    req.id.unwrap_or(0),
                    now,
                    &lookup,
                    &mut store,
                );
                Ok(Some(Reply::same(
                    amber::build_charge_gift_resp(&out).encode_to_vec(),
                )))
            }
        })));
        n += 1;
    }

    // 17095 同步琥珀 VIP 状态。
    {
        let w = world.clone();
        table.insert(Arc::new(FnHandler::new(17095, move |ctx: ddj_registry::HandlerCtx, body| {
            let w = w.clone();
            async move {
                let pid = ctx.require_player()?;
                let _req = AmberVipReq17095::decode(body).map_err(decode_err)?;
                let _ = w.peek(pid, |_| ());
                Ok(Some(Reply::same(AmberVipResp17095 { ret: 0 }.encode_to_vec())))
            }
        })));
        n += 1;
    }

    // 17097 同步 HeyTap VIP 状态。
    {
        let w = world.clone();
        table.insert(Arc::new(FnHandler::new(17097, move |ctx: ddj_registry::HandlerCtx, body| {
            let w = w.clone();
            async move {
                let pid = ctx.require_player()?;
                let _req = AmberHeyTapVipReq17097::decode(body).map_err(decode_err)?;
                let state = w.peek(pid, |b| b.amber.clone());
                let data = amber_data(&state);
                let resp = AmberHeyTapVipResp17097 {
                    ret: 0,
                    amber_data: Some(data.to_msg()),
                };
                Ok(Some(Reply::same(resp.encode_to_vec())))
            }
        })));
        n += 1;
    }

    n
}
