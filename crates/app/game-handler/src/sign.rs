//! # 签到（`SignMessageHandler`：16002 / 16003 / 16004）
//!
//! 对应 Java（`data/apps/game_7010544/build/classes/`）：
//! - `bf/game/server/message/handler/SignMessageHandler.class`（`sign` 84 B、
//!   `boxReward` 90 B、`getSign` 46 B）
//! - `bf/game/server/module/sign/Sign.class`（`sign` 102 B、`boxReward` 73 B、
//!   `refresh` 79 B、`isTodaySigned` 32 B、`toMsgSign` 36 B）
//! - `bf/template/db/DailySignDB.class`（`rand` 59 B）在
//!   `libs/jb3.common.v1.jar`
//!
//! ## 三条消息
//!
//! ### 16002 签到
//! `sign.sign()` → `DailySignDBCache.get(sign.getSignedDay()).rand()` →
//! `ResourcesUtils.addResourcesJsons(p, rewards, TYPE_SIGN_SIGN, [])` →
//! 回 `MsgSignResp_16002{ret=0, msgResources, msgSign}`。
//! 奖励按**签到后的天数**取表，不是签到前的天数。
//!
//! ### 16003 领签到宝箱
//! 顺序不能颠倒：先 `DailySignBoxDBCache.get(id)`（**原版不判空，查不到就是 NPE**），
//! 再 `if (db.getDays() > sign.getSignedDay()) throw SIGN_ERROR_NOT_SIGNED(1203)`，
//! 然后 `sign.boxReward(id)`（箱内重复 → `SIGN_ERROR_REPEATED_BOX(1202)`），
//! 最后 `addResourcesJsons(p, db.getBoxRewardJsons(), TYPE_SIGN_SIGN_BOX, [])`。
//!
//! ### 16004 拉取签到状态
//! `sign.refresh()` 之后**先推一条 `MsgSignResp_16001`**，再 return
//! `MsgGetSignResp_16004{ret=0}` —— 一条请求发两个包。
//!
//! ## `refresh()` 不是每天重置
//! 字节码 32-42：只有 `signedDay >= maxDailySignDBID`（周期已满）**且**跨天，
//! 才把 `signedDay=0`、`boxIds.clear()`。周期内跨天什么都不做。
//! `maxDailySignDBID = 21`，由 `M-每日签到表-DailySign.csv`（id 1..21）与
//! `M-每日签到宝箱表-DailySignBox.csv`（days 3/7/14/21）两表共同印证。
//!
//! ## `HandlerException` 的出口（框架级，重要）
//! `MessageHandlerExtensionImpl.messageHandler` 的异常表是
//! `[749,821) → 824 catch InvocationTargetException`，
//! 命中 `HandlerException` 时构造的不是业务 Resp，而是
//! `EngineMessage$ErrorResp_1{msgId = 原始 cmdId, errorCode = failedCode.getValue()}`。
//! 所以 1201/1202/1203 到客户端时是 `ErrorResp_1`，**不是** `MsgSignResp_16002`。
//! [`error_resp`] 复刻这条出口。

use ddj_core::RngSource;
use ddj_pb::gen::engine_message::ErrorResp1;
use ddj_pb::gen::sign_message::{MsgSign, MsgSignResp16002};
use ddj_pb::RetCode;

/// 签到周期长度，等于 `DailySignBoxDBCache.maxDailySignDBID`。
pub const DAILY_SIGN_CYCLE: i32 = 21;

/// 一天的毫秒数。
pub const DAY_MS: i64 = 86_400_000;

/// `SignMessage.sign` 的 cmdId。
pub const CMD_SIGN: i32 = 16002;

/// `SignMessage.boxReward` 的 cmdId。
pub const CMD_BOX: i32 = 16003;

/// `SignMessage.getSign` 的 cmdId。
pub const CMD_GET: i32 = 16004;

/// `MsgSign` 下发的那条推送的 cmdId。
pub const CMD_SIGN_PUSH: i32 = 16001;

/// 奖励对：`(templateId, num)`。
pub type Reward = (i32, i64);

/// `Sign` 的持久化状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignState {
    /// 已签到天数，0 表示本周期还没签过。
    pub signed_day: i32,
    /// 最后一次签到时间（毫秒）。
    pub signed_time: i64,
    /// 已领的宝箱 id，保序（原版是 `ConcurrentHashSet`，但下发时遍历顺序不确定，这里按领取序）。
    pub box_ids: Vec<i32>,
}

/// 复刻 `LogicUtils.getTimeByHour(ms, hour)`：取本地时区当天 `hour` 点整。
///
/// `tz_offset_ms` 是本地时区相对 UTC 的偏移（东八区 = `8 * 3600 * 1000`）。
/// 原版用 `java.util.Calendar`，行为等价于「先平移到本地时间轴，再按整天截断」。
pub fn day_start(ms: i64, tz_offset_ms: i64) -> i64 {
    let shifted = ms + tz_offset_ms;
    shifted - shifted.rem_euclid(DAY_MS) - tz_offset_ms
}

/// `Sign.isTodaySigned()`：`signedDay != 0` 且两次签到落在同一个本地日。
pub fn is_today_signed(state: &SignState, now: i64, tz_offset_ms: i64) -> bool {
    state.signed_day != 0
        && day_start(now, tz_offset_ms) == day_start(state.signed_time, tz_offset_ms)
}

/// `Sign.refresh()` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshOutcome {
    /// 是否发生了周期重置（原版此时会 `DBSignModule.submitUpdate`）。
    pub reset: bool,
    /// 重置后的状态。
    pub state: SignState,
}

/// `Sign.refresh()`：周期满且跨天才清空。
///
/// 原版只重置 `signedDay` 与 `boxIds`，**不动 `signedTime`**：
/// ```java
/// if (this.signedDay >= DailySignBoxDBCache.maxDailySignDBID) {
///     long nowTime = LogicUtils.getNowTime();
///     if (getTimeByHour(nowTime,0) != getTimeByHour(this.signedTime,0)) {
///         this.signedDay = 0; this.boxIds.clear(); submitUpdate(this);
///     }
/// }
/// ```
/// `signedTime` 会被 `toMsgSign()` 原样下发给客户端，所以这里保留旧值，
/// 不能整条 `SignState::default()`（那会把 `signedTime` 抹成 0）。
pub fn refresh(state: &SignState, now: i64, tz_offset_ms: i64) -> RefreshOutcome {
    let crossed_day = day_start(now, tz_offset_ms) != day_start(state.signed_time, tz_offset_ms);
    if state.signed_day >= DAILY_SIGN_CYCLE && crossed_day {
        RefreshOutcome {
            reset: true,
            state: SignState {
                signed_day: 0,
                box_ids: Vec::new(),
                ..state.clone()
            },
        }
    } else {
        RefreshOutcome {
            reset: false,
            state: state.clone(),
        }
    }
}

/// `Sign.sign()` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignOutcome {
    /// 今天已经签过（`SIGN_ERROR_REPEATED = 1201`）。
    Reject(RetCode),
    /// 签到成功；原版同时会打 `StatType.DailySignUp` 与
    /// `OperationEvent(EventType.DailySignUp)`，两者都无条件执行。
    Accept(SignState),
}

/// `Sign.sign()`：重复签到拒绝，否则天数 +1 并刷新时间。
pub fn sign(state: &SignState, now: i64, tz_offset_ms: i64) -> SignOutcome {
    if is_today_signed(state, now, tz_offset_ms) {
        return SignOutcome::Reject(RetCode::SignErrorRepeated);
    }
    let mut next = state.clone();
    next.signed_day += 1;
    next.signed_time = now;
    SignOutcome::Accept(next)
}

/// `Sign.boxReward(int)` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoxOutcome {
    /// 这个箱子已经领过了（`SIGN_ERROR_REPEATED_BOX = 1202`）。
    Reject(RetCode),
    /// 领取记录已写入。
    Accept(SignState),
}

/// `Sign.boxReward(int)`：只负责记账，不发奖励。
pub fn box_reward(state: &SignState, box_id: i32) -> BoxOutcome {
    if state.box_ids.contains(&box_id) {
        return BoxOutcome::Reject(RetCode::SignErrorRepeatedBox);
    }
    let mut next = state.clone();
    next.box_ids.push(box_id);
    BoxOutcome::Accept(next)
}

/// 16003 的整体结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimBoxOutcome {
    /// 天数不够（1203）或重复领取（1202）。
    Reject(RetCode),
    /// 记账完成，`rewards` 是模板里配的 `boxRewardJsons`。
    Accept {
        /// 更新后的签到状态。
        state: SignState,
        /// 要发放的奖励。
        rewards: Vec<Reward>,
    },
}

/// 16003 全流程：先查天数门槛，再查重复，最后发模板奖励。
pub fn claim_box(
    state: &SignState,
    box_id: i32,
    box_days: i32,
    box_rewards: &[Reward],
) -> ClaimBoxOutcome {
    // 字节码顺序：`db.getDays() > signedDay` 在前，`boxReward` 在后。
    if box_days > state.signed_day {
        return ClaimBoxOutcome::Reject(RetCode::SignErrorNotSigned);
    }
    match box_reward(state, box_id) {
        BoxOutcome::Reject(code) => ClaimBoxOutcome::Reject(code),
        BoxOutcome::Accept(next) => ClaimBoxOutcome::Accept {
            state: next,
            rewards: box_rewards.to_vec(),
        },
    }
}

/// `DailySignDB.rand()`：从 `rewardJson` 里**不放回**抽 `randTimes` 个。
///
/// 原版 `pool.remove(UtilsTools.randomIndex(pool.size()))`，池子空时会抛
/// `IndexOutOfBoundsException`；这里提前 break，畸形配置（`randTimes` 大于奖励数）
/// 退化成「能抽多少抽多少」，不 panic。
pub fn rand_pick(reward_json: &[Reward], rand_times: i32, rng: &dyn RngSource) -> Vec<Reward> {
    let mut pool = reward_json.to_vec();
    let mut out = Vec::with_capacity(reward_json.len().min(rand_times.max(0) as usize));
    for _ in 0..rand_times.max(0) {
        if pool.is_empty() {
            break;
        }
        let idx = rng.gen_index(pool.len());
        out.push(pool.remove(idx));
    }
    out
}

/// `Sign.toMsgSign()`：`signedDay` + `signedTime` + `boxId[]`。
pub fn to_msg_sign(state: &SignState) -> MsgSign {
    MsgSign {
        signed_day: Some(state.signed_day),
        signed_time: Some(state.signed_time),
        box_id: state.box_ids.clone(),
    }
}

/// 16002 的响应体。
pub fn build_sign_resp(state: &SignState, rewards: &[Reward]) -> MsgSignResp16002 {
    MsgSignResp16002 {
        ret: 0,
        msg_sign: Some(to_msg_sign(state)),
        msg_resources: rewards
            .iter()
            .map(|(id, num)| ddj_pb::gen::common_msg_message::MsgResources {
                id: Some(*id),
                // 原 proto 把数量定义成 string（`optional string num = 2;`），不是数字。
                num: Some(num.to_string()),
                multiple: None,
            })
            .collect(),
    }
}

/// 同上，但错误码直接给数值。
///
/// 原版 `ErrorResp_1.errorCode` 是 `int32`，取自 `LanguageUtils.getValue()`；
/// `RetCode` 只收录了实测需要断言的码，`PLAYER_ERROR_8(108)` 这类未收录的
/// 走这个入口，**不要**给 `RetCode` 补枚举值（那是发明原版没有的码表）。
pub fn error_resp_raw(msg_id: i32, code: i32) -> ErrorResp1 {
    ErrorResp1 {
        msg_id: Some(msg_id),
        error_code: Some(code),
    }
}

/// `HandlerException` 的框架级出口：`ErrorResp_1{msgId, errorCode}`。
pub fn error_resp(msg_id: i32, code: RetCode) -> ErrorResp1 {
    ErrorResp1 {
        msg_id: Some(msg_id),
        error_code: Some(code.as_i32()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddj_core::DeterministicRng;

    const TZ: i64 = 8 * 3_600_000;
    /// 2024-01-01 00:00:00 +08:00
    const D1: i64 = 1_704_038_400_000;

    fn st(day: i32, time: i64, boxes: &[i32]) -> SignState {
        SignState {
            signed_day: day,
            signed_time: time,
            box_ids: boxes.to_vec(),
        }
    }

    #[test]
    fn day_start_truncates_to_local_midnight() {
        assert_eq!(day_start(D1, TZ), D1);
        assert_eq!(day_start(D1 + 1, TZ), D1);
        assert_eq!(day_start(D1 + DAY_MS - 1, TZ), D1);
        assert_eq!(day_start(D1 + DAY_MS, TZ), D1 + DAY_MS);
    }

    #[test]
    fn day_start_is_stable_across_utc_midnight() {
        // UTC 17:00 是东八区次日 01:00，两者不应落在同一个本地日。
        let utc_evening = D1 + 9 * 3_600_000; // 本地 09:00
        let local_next = utc_evening + 16 * 3_600_000; // 本地次日 01:00
        assert_ne!(day_start(utc_evening, TZ), day_start(local_next, TZ));
    }

    #[test]
    fn never_signed_is_not_today_signed() {
        assert!(!is_today_signed(&st(0, 0, &[]), D1, TZ));
    }

    #[test]
    fn same_local_day_is_today_signed() {
        assert!(is_today_signed(&st(1, D1 + 1_000, &[]), D1 + 5_000, TZ));
    }

    #[test]
    fn next_local_day_is_not_today_signed() {
        assert!(!is_today_signed(&st(1, D1, &[]), D1 + DAY_MS, TZ));
    }

    #[test]
    fn refresh_does_nothing_inside_cycle() {
        let r = refresh(&st(3, D1, &[1]), D1 + DAY_MS * 2, TZ);
        assert!(!r.reset);
        assert_eq!(r.state, st(3, D1, &[1]));
    }

    #[test]
    fn refresh_resets_when_cycle_full_and_day_changed() {
        let r = refresh(&st(DAILY_SIGN_CYCLE, D1, &[1, 2]), D1 + DAY_MS, TZ);
        assert!(r.reset);
        // 原版只清 signedDay 与 boxIds，signedTime 保留（会被 toMsgSign 下发）。
        assert_eq!(r.state.signed_day, 0);
        assert!(r.state.box_ids.is_empty());
        assert_eq!(r.state.signed_time, D1, "signedTime 不应被重置");
    }

    #[test]
    fn refresh_keeps_cycle_full_state_on_same_day() {
        let r = refresh(&st(DAILY_SIGN_CYCLE, D1, &[1]), D1 + 1_000, TZ);
        assert!(!r.reset);
        assert_eq!(r.state.signed_day, DAILY_SIGN_CYCLE);
    }

    #[test]
    fn sign_rejects_same_day() {
        let out = sign(&st(1, D1, &[]), D1 + 60_000, TZ);
        assert_eq!(out, SignOutcome::Reject(RetCode::SignErrorRepeated));
    }

    #[test]
    fn sign_increments_day_and_stamps_time() {
        let out = sign(&st(1, D1, &[]), D1 + DAY_MS + 7, TZ);
        assert_eq!(out, SignOutcome::Accept(st(2, D1 + DAY_MS + 7, &[])));
    }

    #[test]
    fn sign_from_zero_is_day_one() {
        let out = sign(&SignState::default(), D1, TZ);
        assert_eq!(out, SignOutcome::Accept(st(1, D1, &[])));
    }

    #[test]
    fn box_reward_rejects_duplicate() {
        let out = box_reward(&st(5, D1, &[1, 2]), 2);
        assert_eq!(out, BoxOutcome::Reject(RetCode::SignErrorRepeatedBox));
    }

    #[test]
    fn box_reward_appends_in_order() {
        let out = box_reward(&st(5, D1, &[1]), 3);
        assert_eq!(out, BoxOutcome::Accept(st(5, D1, &[1, 3])));
    }

    #[test]
    fn claim_box_checks_days_before_duplicate() {
        let rewards = [(12, 5)];
        // 天数不够，即使箱子已领过也先报 1203。
        let out = claim_box(&st(2, D1, &[1]), 1, 3, &rewards);
        assert_eq!(out, ClaimBoxOutcome::Reject(RetCode::SignErrorNotSigned));
    }

    #[test]
    fn claim_box_allows_exact_day() {
        let rewards = [(12, 5)];
        let out = claim_box(&st(3, D1, &[]), 1, 3, &rewards);
        assert_eq!(
            out,
            ClaimBoxOutcome::Accept {
                state: st(3, D1, &[1]),
                rewards: rewards.to_vec(),
            }
        );
    }

    #[test]
    fn claim_box_rejects_second_time() {
        let out = claim_box(&st(7, D1, &[1]), 1, 3, &[]);
        assert_eq!(out, ClaimBoxOutcome::Reject(RetCode::SignErrorRepeatedBox));
    }

    #[test]
    fn rand_pick_draws_without_replacement() {
        let pool = [(1, 50), (22, 1), (101, 1), (102, 1), (103, 1)];
        let rng = DeterministicRng::new(7);
        let picked = rand_pick(&pool, 3, &rng);
        assert_eq!(picked.len(), 3);
        let mut ids: Vec<i32> = picked.iter().map(|(id, _)| *id).collect();
        let mut dedup = ids.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(ids.len(), dedup.len(), "不放回抽样不应重复");
        ids.sort_unstable();
        assert!(ids.iter().all(|id| pool.iter().any(|(p, _)| p == id)));
    }

    #[test]
    fn rand_pick_is_deterministic_for_same_seed() {
        let pool = [(1, 50), (22, 1), (101, 1), (102, 1), (103, 1)];
        let a = rand_pick(&pool, 3, &DeterministicRng::new(42));
        let b = rand_pick(&pool, 3, &DeterministicRng::new(42));
        assert_eq!(a, b);
    }

    #[test]
    fn rand_pick_tolerates_short_pool() {
        let pool = [(1, 50), (22, 1)];
        let picked = rand_pick(&pool, 5, &DeterministicRng::new(1));
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn rand_pick_zero_times_is_empty() {
        assert!(rand_pick(&[(1, 50)], 0, &DeterministicRng::new(1)).is_empty());
    }

    #[test]
    fn msg_sign_carries_defaults_for_new_player() {
        let msg = to_msg_sign(&SignState::default());
        assert_eq!(msg.signed_day, Some(0));
        assert_eq!(msg.signed_time, Some(0));
        assert!(msg.box_id.is_empty());
    }

    #[test]
    fn sign_resp_is_ret_zero_with_rewards() {
        let resp = build_sign_resp(&st(3, D1, &[1]), &[(1, 50), (22, 1)]);
        assert_eq!(resp.ret, 0);
        assert_eq!(resp.msg_resources.len(), 2);
        assert_eq!(resp.msg_sign.unwrap().signed_day, Some(3));
    }

    #[test]
    fn handler_exception_maps_to_error_resp_with_original_cmd() {
        let resp = error_resp(CMD_SIGN, RetCode::SignErrorRepeated);
        assert_eq!(resp.msg_id, Some(16002));
        assert_eq!(resp.error_code, Some(1201));
    }

    #[test]
    fn error_codes_match_languageutils() {
        assert_eq!(RetCode::SignErrorRepeated.as_i32(), 1201);
        assert_eq!(RetCode::SignErrorRepeatedBox.as_i32(), 1202);
        assert_eq!(RetCode::SignErrorNotSigned.as_i32(), 1203);
    }
}
