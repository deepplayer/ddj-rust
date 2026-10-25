//! 琥珀会员 6 条消息的接线（17091 / 17092 / 17093 / 17094 / 17095 / 17097）。
//!
//! ## 过渡期的两处假定
//! 原版门禁是 `AmberWelfareDBCache.getOppoCids()`（渠道白名单）与充值回调里置
//! `amberVip`。两处都要真实配置/支付链路，仓储还没接，先用两个常量顶住：
//!
//! - [`ASSUMED_AMBER_CID`]：视为白名单内的渠道号；
//! - [`ASSUMED_AMBER_VIP`]：视为已开通会员，让 `check_vip` 放行。
//!
//! 接好模板后只替换这两个常量的使用点，业务函数与回包逻辑不动。

use std::sync::{Arc, OnceLock};

use ddj_core::{Clock, PlayerId};
use ddj_pb::gen::sys_activity_message::{
    AmberHeyTapVipReq17097, AmberHeyTapVipResp17097, AmberVipReq17095, AmberVipResp17095,
    AmberWelfareChargeGiftReq17094, AmberWelfareDataReq17091, AmberWelfareExclusiveReq17093,
    AmberWelfareSignReq17092,
};
use ddj_registry::HandlerTable;
use ddj_registry::Reply;
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

/// 东八区偏移（毫秒）。原版 `LogicUtils.isSameDay` 走服务器默认时区。
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

/// 真实琥珀表（由 `game-core` 启动时注入）。
///
/// 实测 `H-琥珀福利-AmberWelfare.csv` 共 31 行：
/// - `type=1` 7 行（day 1..7，每日签到奖励，如 `219=1`）
/// - `type=2` 1 行（专属奖励，`11=1;33=1;215=3;216=3;30=1;234=1`）
/// - `type=3` 23 行（充值豪礼，`payValue` 是门槛 98/198/328/648…）
#[derive(Debug, Default)]
pub struct AmberTables {
    /// `type=1` 的行，按 `day` 升序。
    pub sign_in: Vec<AmberTemplate>,
    /// `type=2` 的行（只有一行）。
    pub exclusive: Option<AmberTemplate>,
    /// `type=3` 的行。
    pub charges: Vec<AmberTemplate>,
}

static AMBER_TABLES: OnceLock<Arc<AmberTables>> = OnceLock::new();

/// 注入真实琥珀表（只第一次生效）。
pub fn install_amber_tables(t: Arc<AmberTables>) {
    let _ = AMBER_TABLES.set(t);
}

fn tables() -> Option<&'static Arc<AmberTables>> {
    AMBER_TABLES.get()
}

/// 只读查询器。
struct MemLookup {
    empty: Vec<AmberTemplate>,
}

impl AmberLookup for MemLookup {
    fn sign_in_dbs(&self) -> &[AmberTemplate] {
        tables().map_or(&self.empty[..], |t| &t.sign_in[..])
    }

    fn exclusive_db(&self) -> Option<&AmberTemplate> {
        tables().and_then(|t| t.exclusive.as_ref())
    }

    fn by_id(&self, id: i32) -> Option<&AmberTemplate> {
        tables().and_then(|t| t.charges.iter().find(|d| d.id == id))
    }
}

/// 写回器：`save` 落内存态，`add_resources` 进背包，`event_log` 记一行。
struct MemStore<'a> {
    world: &'a MemWorld,
    player: PlayerId,
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
        tracing::info!(
            target: "ddj_game_handler::amber",
            kind, db_id, charge_num, desc, ?rewards, "琥珀领奖"
        );
    }
}

/// 注册 6 条琥珀消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 17091 拉活动数据。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17091, move |ctx, body| {
            let _req = AmberWelfareDataReq17091::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let state = w.peek(p, |b| b.amber.clone());
            let resp = amber::query_resp(ASSUMED_AMBER_CID, Some(&amber_data(&state)));
            Ok(Some(Reply::same(resp.encode_to_vec())))
        })));
        n += 1;
    }

    // 17092 签到。
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(17092, move |ctx, body| {
            let _req = AmberWelfareSignReq17092::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let state = w.peek(p, |b| b.amber.clone());
            if let Err(code) = amber::check_vip(Some(&amber_data(&state))) {
                // 原版 `HandlerException` 出口是 `ErrorResp_1`。
                return Ok(Some(Reply::with_cmd(
                    ERROR_RESP_CMD,
                    error_resp(17092, code).encode_to_vec(),
                )));
            }
            let lookup = MemLookup {
                empty: Vec::new(),
            };
            let mut store = MemStore {
                world: &w,
                player: p,
            };
            let now = c.now_millis();
            let out = amber::sign_up(&state, now, TZ_OFFSET_MS, &lookup, &mut store);
            Ok(Some(Reply::same(
                amber::build_sign_resp(&out).encode_to_vec(),
            )))
        })));
        n += 1;
    }

    // 17093 领取专属奖励。
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(17093, move |ctx, body| {
            let _req = AmberWelfareExclusiveReq17093::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let state = w.peek(p, |b| b.amber.clone());
            let lookup = MemLookup {
                empty: Vec::new(),
            };
            let mut store = MemStore {
                world: &w,
                player: p,
            };
            let now = c.now_millis();
            let out = amber::rev_exclusive_reward(&state, now, &lookup, &mut store);
            Ok(Some(Reply::same(
                amber::build_exclusive_resp(&out).encode_to_vec(),
            )))
        })));
        n += 1;
    }

    // 17094 领取充值豪礼。
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(17094, move |ctx, body| {
            let req = AmberWelfareChargeGiftReq17094::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let state = w.peek(p, |b| b.amber.clone());
            let lookup = MemLookup {
                empty: Vec::new(),
            };
            let mut store = MemStore {
                world: &w,
                player: p,
            };
            let now = c.now_millis();
            let out =
                amber::rev_charge_gift(&state, req.id.unwrap_or(0), now, &lookup, &mut store);
            Ok(Some(Reply::same(
                amber::build_charge_gift_resp(&out).encode_to_vec(),
            )))
        })));
        n += 1;
    }

    // 17095 同步琥珀 VIP 状态（原版纯转发，`AmberModelState` 不落 vip 位）。
    {
        table.insert(Box::new(FnHandler::new(17095, move |ctx, body| {
            let _req = AmberVipReq17095::decode(body).map_err(decode_err)?;
            ctx.require_player()?;
            Ok(Some(Reply::same(
                AmberVipResp17095 { ret: 0 }.encode_to_vec(),
            )))
        })));
        n += 1;
    }

    // 17097 同步 HeyTap VIP 状态。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17097, move |ctx, body| {
            let _req = AmberHeyTapVipReq17097::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let state = w.peek(p, |b| b.amber.clone());
            let resp = AmberHeyTapVipResp17097 {
                ret: 0,
                amber_data: Some(amber_data(&state).to_msg()),
            };
            Ok(Some(Reply::same(resp.encode_to_vec())))
        })));
        n += 1;
    }

    n
}
