//! 每日签到的接线（16002 / 16003 / 16004）。
//!
//! ## 时区
//! 原版用 `LogicUtils.getTimeByHour(ms, 0)` → `java.util.Calendar`，即**服务器本地时区**。
//! 容器里 `date.timezone = Asia/Shanghai`，所以这里固定 `+08:00`。
//! 换成 `chrono-tz` 按配置读会更正确，但骨架阶段先与部署环境对齐。
//!
//! ## 16004 的两个包
//! 原版 `getSign` 先 `sendToPlayerId(MsgSignResp_16001)` 再返回
//! `MsgGetSignResp_16004{ret=0}` —— **一条请求发两个包**。
//! 当前 `ErasedHandler` 只能返回一份回包，所以这里只发 16004，
//! 16001 的推送等出站通道（`SessionControl::push`）接上后补。已记入 AUDIT。

use std::sync::{Arc, Mutex, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::sign_message::{
    MsgGetSignReq16004, MsgGetSignResp16004, MsgSignBoxReq16003, MsgSignBoxResp16003,
    MsgSignReq16002,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::runtime::{decode_err, FnHandler, MemWorld};
use crate::sign::{self, ClaimBoxOutcome, SignOutcome};

/// 东八区偏移（毫秒）。对应部署环境的 `Asia/Shanghai`。
const TZ_MS: i64 = 8 * 3600 * 1000;

/// `ErrorResp_1` 自己的消息号（原版 `HandlerException` 出口用它组帧）。
const ERROR_RESP_CMD: i32 = 1;

/// `DailySignDB.randTimes` 的默认值。
///
/// 实测 `DailySignDB.<clinit>` 是 `iconst_3; putstatic randTimes` —— **硬编码 3**，
/// 不在 CSV 列里。所以每行配 5 个奖励、不放回抽 3 个，抽数由代码而非数据决定。
pub const DEFAULT_RAND_TIMES: i32 = 3;

/// 真实签到/宝箱表的只读视图（由 `game-core` 启动时注入）。
///
/// 原版对应 `DailySignDBCache.get(day).rand()` 与 `DailySignBoxDBCache.get(id)`。
pub trait SignTemplates: Send + Sync {
    /// `DailySignDB.reward`（`资源id=数量` 已解析）。
    fn rewards_of_day(&self, day: i32) -> Vec<(i32, i64)>;
    /// `DailySignBoxDB.days`。
    fn box_days(&self, box_id: i32) -> Option<i32>;
    /// `DailySignBoxDB.boxReward`。
    fn box_rewards(&self, box_id: i32) -> Vec<(i32, i64)>;
}

static SIGN_TEMPLATES: OnceLock<Arc<dyn SignTemplates>> = OnceLock::new();

/// 注入真实签到表（只第一次生效）。
pub fn install_sign_templates(t: Arc<dyn SignTemplates>) {
    let _ = SIGN_TEMPLATES.set(t);
}

fn sign_templates() -> Option<&'static Arc<dyn SignTemplates>> {
    SIGN_TEMPLATES.get()
}

/// 把 3 条签到消息挂进表。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let rng = Arc::new(Mutex::new(ddj_core::DeterministicRng::new(0x5EED_5147)));
    let mut n = 0;

    // 16002 签到
    {
        let (w, c, r) = (world.clone(), clock.clone(), rng.clone());
        table.insert(Box::new(FnHandler::new(16002, move |ctx, body| {
            let _req = MsgSignReq16002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            // 先 refresh（跨周期重置），再签到 —— 与原版 handler 的分支序一致。
            let cur = w.peek(p, |b| b.sign.clone());
            let state = sign::refresh(&cur, now, TZ_MS).state;

            let next = match sign::sign(&state, now, TZ_MS) {
                SignOutcome::Reject(code) => {
                    return Ok(Some(Reply::with_cmd(
                        ERROR_RESP_CMD,
                        sign::error_resp(16002, code).encode_to_vec(),
                    )));
                }
                SignOutcome::Accept(next) => next,
            };
            w.with(p, |b| b.sign = next.clone());

            // `DailySignDBCache.get(signedDay).rand()`：当天那行的 5 个奖励里不放回抽 3 个。
            let pool = sign_templates().map_or_else(Vec::new, |t| {
                t.rewards_of_day(next.signed_day)
            });
            let guard = r.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let rewards = sign::rand_pick(&pool, DEFAULT_RAND_TIMES, &*guard);
            drop(guard);
            Ok(Some(
                sign::build_sign_resp(&next, &rewards)
                    .encode_to_vec()
                    .into(),
            ))
        })));
        n += 1;
    }

    // 16003 领宝箱
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(16003, move |ctx, body| {
            let req = MsgSignBoxReq16003::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let id = req.id;
            let days = sign_templates()
                .and_then(|t| t.box_days(id))
                .unwrap_or(0);
            let cur = w.peek(p, |b| b.sign.clone());
            let state = sign::refresh(&cur, now, TZ_MS).state;
            let box_rewards = sign_templates().map_or_else(Vec::new, |t| t.box_rewards(id));
            match sign::claim_box(&state, id, days, &box_rewards) {
                ClaimBoxOutcome::Reject(code) => {
                    Ok(Some(Reply::with_cmd(
                        ERROR_RESP_CMD,
                        sign::error_resp(16003, code).encode_to_vec(),
                    )))
                }
                ClaimBoxOutcome::Accept { state, .. } => {
                    w.with(p, |b| b.sign = state);
                    Ok(Some(MsgSignBoxResp16003 { ret: 0 }.encode_to_vec().into()))
                }
            }
        })));
        n += 1;
    }

    // 16004 拉取（原版另发 16001 推送，见模块文档）
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(16004, move |ctx, body| {
            let _req = MsgGetSignReq16004::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            let cur = w.peek(p, |b| b.sign.clone());
            let outcome = sign::refresh(&cur, now, TZ_MS);
            if outcome.reset {
                let next = outcome.state.clone();
                w.with(p, |b| b.sign = next);
            }
            Ok(Some(MsgGetSignResp16004 { ret: 0 }.encode_to_vec().into()))
        })));
        n += 1;
    }

    n
}
