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

/// 东八区偏移（毫秒）。原版 `LogicUtils.isSameDay` 走服务器默认时区。
const TZ_OFFSET_MS: i64 = 8 * 3_600 * 1_000;

/// `AmberWelfareModel.checkHeyTapVip`（336-356）逐条复刻。
///
/// 顺序与原版一致，任一条失败即 `false`（对应 `onExpireHeyTapVip()`）。
fn check_hey_tap_vip(
    player: PlayerId,
    cid: &str,
    flag: i32,
    timestamp: &str,
    md5_sign: &str,
    now: i64,
) -> bool {
    // `"web".equals(player.getCid())` → true，直接放行。
    if cid == "web" {
        return true;
    }
    // `Long.parseLong(timestamp)` 抛 NumberFormatException → catch → false。
    let Ok(millis) = timestamp.parse::<i64>() else {
        return false;
    };
    // 60 秒有效期。
    if millis + 60_000 < now {
        return false;
    }
    // `oppoCids.contains(cid)`：过渡期只有 [`ASSUMED_AMBER_CID`] 在名单里。
    if cid != ASSUMED_AMBER_CID {
        return false;
    }
    // `md5(playerId + "|" + cid + "|" + flag + "|" + timestamp)`，小写十六进制。
    let origin = format!("{}|{}|{}|{}", player.get(), cid, flag, timestamp);
    let want = md5_hex(&origin);
    // 原版 `md5 != null && md5.equals(md5Sign) && heyTapVipFlag == 1`。
    want == md5_sign && flag == 1
}

/// `MD5Helper.encode`：UTF-8 → MD5 → 小写十六进制。
fn md5_hex(s: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(s.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

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
        amber_vip: state.amber_vip,
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
            // 原版 `getMsgAmberWelfareData`：cid 不在 `oppoCids` 里时返回
            // `createNotAmberCidVipData`（cid + amberVip=false + heyTapVip=false，
            // 其余字段全默认）；在名单里才回 `model.toMsgAmberWelfareData()`。
            // [`ASSUMED_AMBER_CID`] 按定义就在 `oppoCids` 名单里，所以走
            // `model.toMsgAmberWelfareData()` 这一支；`None` 那支
            // （`createNotAmberCidVipData`）等真实渠道号接进来后再启用。
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
            // 原版 `AmberWelfareModule.receiveExclusiveReward`：与 17092 同一个门禁，
            // `isNotAmberVip(model)` → `HandlerException(AMBER_WELFARE_IS_NOT_VIP=4701)`。
            if let Err(code) = amber::check_vip(Some(&amber_data(&state))) {
                return Ok(Some(Reply::with_cmd(
                    ERROR_RESP_CMD,
                    error_resp(17093, code).encode_to_vec(),
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
            // 同上：`receiveChargeGift` 也先过 `isNotAmberVip`。
            if let Err(code) = amber::check_vip(Some(&amber_data(&state))) {
                return Ok(Some(Reply::with_cmd(
                    ERROR_RESP_CMD,
                    error_resp(17094, code).encode_to_vec(),
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
    //
    // ## 原版到底做了什么（`AmberWelfareModel.syncHeyTapVip` 323-334）
    // `checkHeyTapVip(...)` 决定 `onBecomeHeyTapVip()` 还是 `onExpireHeyTapVip()`，
    // 然后**恒**回 `ret=0` + 最新 `amberData`。也就是说校验失败不是错误码，
    // 而是把 `heyTapVip` 置为 false。客户端靠回包里的字段判断，不看 ret。
    //
    // ## `checkHeyTapVip`（336-356）
    // ```text
    // cid == "web"                                  → true
    // timestamp 解析失败                             → false（catch NumberFormatException）
    // timestamp + 60000 < now                       → false   // 60 秒有效期
    // oppoCids 不含 cid                             → false
    // md5(playerId|cid|heyTapVipFlag|timestamp) 不等 → false
    // heyTapVipFlag != 1                            → false
    // ```
    // 「md5 算不出来」原版会 return null 然后 `md5 != null && ...` 短路成 false。
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(17097, move |ctx, body| {
            let req = AmberHeyTapVipReq17097::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let cid = ASSUMED_AMBER_CID;
            let flag = req.hey_tap_vip_flag.unwrap_or(0);
            let sign = req.md5_sign.clone().unwrap_or_default();
            let ts = req.timestamp.clone().unwrap_or_default();
            let ok = check_hey_tap_vip(p, cid, flag, &ts, &sign, now);
            // 原版 `onBecomeHeyTapVip()` / `onExpireHeyTapVip()` 只改 vip 位，
            // 然后 `toMsgAmberWelfareData()` 原样下发。
            let state = w.with(p, |b| {
                b.amber.hey_tap_vip = ok;
                b.amber.clone()
            });
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
