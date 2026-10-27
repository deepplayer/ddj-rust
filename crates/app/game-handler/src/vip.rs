//! # VVIP 微信奖励（`PlayerMessageHandler.sendVVIP_RewardReq_1012`）
//!
//! 对应 Java：
//! - `PlayerMessageHandler.sendVVIP_RewardReq_1012`（行 251-262）
//! - `Player.sendVVIP_RewardResp_1012(boolean)`（行 2009-2024）
//! - `Player.checkVVIP_Reward()`（行 2026-2043）
//! - `Player.sendVvipRewardAug(boolean)`（行 2045-2057）
//! - `Player.checkVVIP_Aug_Reward()`（行 2059-2075）
//!
//! ## handler 只有两行分支
//! ```text
//! if (now >= 1627747200000L)  player.sendVvipRewardAug(false)
//! else                        player.sendVVIP_RewardResp_1012(false)
//! ```
//! `1627747200000` = 2021-08-01 00:00:00 +08:00，是一个**写死的时间点**：
//! 之后走「补发」逻辑，之前走「首月」逻辑。两条路都传 `z=false`，
//! 所以都会回一个 `sendVVIPRewardResp_1012`。
//!
//! ## ⚠ 原版缺陷：跨年判断错误（必须照抄）
//! `checkVVIP_Reward` 的月份比较用的是 `Calendar.get(2)`，即 **0-based 月份（0-11）**：
//! ```java
//! long monthByTime  = getMonthByTime(longStat);   // 0-11
//! long monthByTime2 = getMonthByTime(now);        // 0-11
//! if (monthByTime == monthByTime2 || Math.abs(monthByTime - monthByTime2) != 1)
//!     return COMMON_SERVER_BUSY;
//! ```
//! 于是 **12 月 → 1 月** 得到 `abs(11 - 0) = 11`，不等于 1 → 回「服务器繁忙」。
//! 也就是说**跨年时这个奖励永远领不到**。这是原版行为，照抄并在
//! [`Divergence`] 之外单独标注（不属于 §3 的护栏例外，因为不会挂死进程）。
//!
//! 反观 `checkVVIP_Aug_Reward` 用的是 `getBetweenMonth`（`12*year + month`），
//! 跨年是正确的 —— 两个函数口径不一致，也是原版的样子。

use ddj_pb::RetCode;

/// 分界时间点：`1627747200000` = 2021-08-01 00:00:00 +08:00。
pub const AUG_REWARD_EPOCH_MS: i64 = 1_627_747_200_000;

/// `StatType.VipWeChatAdd` 在 `MemWorld.key_values` 里的存放 key。
///
/// 用负数是为了与 `TypePlayerCacheKey`（1/2 等正数）分开，避免将来撞车。
/// 接上 `PlayerStatServerModule` 后这里会删掉。
pub const STAT_VIP_WECHAT_ADD: i32 = -1012;

/// VVIP 奖励邮件的模板 id（原版硬编码 `178`）。
pub const VVIP_MAIL_TEMPLATE_ID: i32 = 178;

/// 东八区偏移（毫秒）：原版 `Calendar.getInstance()` 走服务器默认时区。
pub const TZ_OFFSET_MS: i64 = 8 * 3_600 * 1_000;

/// `LogicUtils.getMonthByTime(ms)`：**0-based** 月份（0-11）。
///
/// 原版是 `Calendar.get(2)`，所以 1 月返回 0、12 月返回 11。
/// 这个 0-based 特性正是跨年判断出错的根因，不能「修正」成 1-based。
#[must_use]
pub fn month_by_time(ms: i64) -> i32 {
    let shifted = ms + TZ_OFFSET_MS;
    let days = shifted.div_euclid(86_400_000);
    // 从 1970-01-01 起的天数 → 年月日（Howard Hinnant 的 civil_from_days）
    let (y, m, _d) = civil_from_days(days);
    let _ = y;
    m
}

/// 天数（自 1970-01-01）→ (年, 月 0-based, 日)。
fn civil_from_days(z: i64) -> (i64, i32, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, i32::try_from(m - 1).unwrap_or(0), d)
}

/// `LogicUtils.getBetweenMonth(time1, time2)`：`abs(12*y1+m1 - 12*y2-m2)`。
///
/// 注意用的是 **1-based 的月份参与计算**（`12*year + month0`），所以跨年正确。
#[must_use]
pub fn between_month(time1: i64, time2: i64) -> i32 {
    let idx = |ms: i64| -> i32 {
        let shifted = ms + TZ_OFFSET_MS;
        let days = shifted.div_euclid(86_400_000);
        let (y, m0, _d) = civil_from_days(days);
        // 原版 `12*get(1) + get(2)`，get(1) 是年、get(2) 是 0-based 月
        i32::try_from(y).unwrap_or(0) * 12 + m0
    };
    (idx(time2) - idx(time1)).abs()
}

/// 1012 的判定输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VvipCtx {
    /// `VipServerModule.getServerVip() != null`。
    pub server_vip_exists: bool,
    /// `PlayerStatServerModule.getLongStat(id, StatType.VipWeChatAdd)`。
    pub vip_wechat_add_time: i64,
    /// `Player.getVvipTakeRewardTime()`。
    pub vvip_take_reward_time: i64,
    /// 当前时间（毫秒）。
    pub now: i64,
}

/// 1012 的执行结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VvipOutcome {
    /// 回 `sendVVIPRewardResp_1012{ret}`。
    Resp(RetCode),
    /// 成功：要发 `mail_count` 封模板 178 的邮件，回 `ret=0`。
    Accept {
        /// 发几封（首月 1 封；补发按跨月数）。
        mail_count: u32,
        /// 是否要 `setVvipTakeRewardTime(now)`（只有补发分支会写）。
        stamp_take_time: bool,
        /// 是否要 `addLongStat(VipWeChatAdd, now)`（只有首月分支会写）。
        stamp_wechat_add: bool,
    },
}

impl VvipOutcome {
    /// 协议上的 `ret`；`None` 表示成功（`ret=0`）。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Resp(code) => Some(*code),
            Self::Accept { .. } => None,
        }
    }
}

/// `Player.checkVVIP_Reward()`：首月分支的前置判定。
///
/// 返回 `Some(ret)` 表示要回这个错误码；`None` 表示通过。
///
/// ⚠ 跨年时 `abs(11-0)=11 != 1` → 回 BUSY，**照抄原版缺陷**。
#[must_use]
pub fn check_vvip_reward(ctx: &VvipCtx) -> Option<RetCode> {
    // 1. `VipServerModule.getServerVip() == null` → BUSY
    if !ctx.server_vip_exists {
        return Some(RetCode::ServerBusy);
    }
    // 2. `longStat == 0` → BUSY（玩家不是 vvip）
    if ctx.vip_wechat_add_time == 0 {
        return Some(RetCode::ServerBusy);
    }
    // 3. 月份判定：同月 或 差不是 1 → BUSY
    let m1 = month_by_time(ctx.vip_wechat_add_time);
    let m2 = month_by_time(ctx.now);
    if m1 == m2 || (m1 - m2).abs() != 1 {
        return Some(RetCode::ServerBusy);
    }
    None
}

/// `Player.checkVVIP_Aug_Reward()`：补发分支能补几个月。
///
/// 返回 0 表示不发（原版此时仍回 `ret=0`）。
#[must_use]
pub fn check_vvip_aug_reward(ctx: &VvipCtx) -> u32 {
    if !ctx.server_vip_exists {
        return 0;
    }
    if ctx.vip_wechat_add_time == 0 {
        return 0;
    }
    // 从未领过 → 补 1 个月
    if ctx.vvip_take_reward_time == 0 {
        return 1;
    }
    u32::try_from(between_month(ctx.vvip_take_reward_time, ctx.now)).unwrap_or(0)
}

/// `Player.sendVVIP_RewardResp_1012(z)` 的纯逻辑（`z` 恒为 `false`）。
///
/// ```java
/// check = checkVVIP_Reward();
/// if (!z && check != 0) { send(ret = check); return; }
/// if (check != 0) return;
/// addLongStat(VipWeChatAdd, now);
/// sendMail(createSysTemplateMail(id, 178));
/// if (!z) send(ret = check);          // check == 0
/// ```
#[must_use]
pub fn first_month_reward(ctx: &VvipCtx) -> VvipOutcome {
    match check_vvip_reward(ctx) {
        Some(code) => VvipOutcome::Resp(code),
        None => VvipOutcome::Accept {
            mail_count: 1,
            stamp_take_time: false,
            stamp_wechat_add: true,
        },
    }
}

/// `Player.sendVvipRewardAug(z)` 的纯逻辑（`z` 恒为 `false`）。
///
/// ```java
/// n = checkVVIP_Aug_Reward();
/// if (n > 0) { for (i in 0..n) sendMail(178); setVvipTakeRewardTime(now); }
/// if (!z) send(ret = 0);              // 恒 ret=0
/// ```
///
/// 注意这个分支**恒回 `ret=0`**，即使一封邮件都没发。
#[must_use]
pub fn aug_reward(ctx: &VvipCtx) -> VvipOutcome {
    let n = check_vvip_aug_reward(ctx);
    if n > 0 {
        VvipOutcome::Accept {
            mail_count: n,
            stamp_take_time: true,
            stamp_wechat_add: false,
        }
    } else {
        // 没得补也回 ret=0，什么都不写
        VvipOutcome::Accept {
            mail_count: 0,
            stamp_take_time: false,
            stamp_wechat_add: false,
        }
    }
}

/// `sendVVIP_RewardReq_1012` 的总入口：按 `now` 选分支。
#[must_use]
pub fn handle_1012(ctx: &VvipCtx) -> VvipOutcome {
    if ctx.now >= AUG_REWARD_EPOCH_MS {
        aug_reward(ctx)
    } else {
        first_month_reward(ctx)
    }
}

// ─────────────────────── 1104 广告奖励 ───────────────────────
//
// 对应 `PlayerAdvertising.gainAdReward(Player)`（`PlayerAdvertising.java:72-86`）：
// ```text
// if (adRewardTimes >= ConfigDBCache.AdvertisingTimes)
//     → ret = PLAYER_ERROR_20(120)，直接 return
// addResourcesJsons(AdvertisingAwards, TYPE_1014)
// addAdRewardTime()          // adRewardTimes++
// ret = 0, adRewardTimes = 本次之后的值, reward = AdvertisingAwards
// ```
//
// 注意 `refresh()` 的每日重置在 **`getPlayerCommon()` 装载时**做，不在本方法里；
// 所以这里不重复判断跨天。

/// `LanguageUtils.PLAYER_ERROR_20`：观看广告奖励次数达上限。
pub const PLAYER_ERROR_20: RetCode = RetCode::PlayerError20;

/// 1104 的判定输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdRewardCtx {
    /// `PlayerAdvertising.adRewardTimes`（今日已领次数）。
    pub ad_reward_times: i32,
    /// `ConfigDBCache.AdvertisingTimes`（每日上限）。
    pub max_times: i32,
    /// `ConfigDBCache.AdvertisingAwards`。
    pub awards: Vec<(i32, i64)>,
}

/// 1104 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdRewardOutcome {
    /// 达上限 → `PLAYER_ERROR_20(120)`。
    LimitReached,
    /// 成功：`ad_reward_times` 要 +1，回包带新的次数与奖励。
    Accept {
        /// 自增后的 `adRewardTimes`。
        new_times: i32,
        /// 本次发的奖励。
        rewards: Vec<(i32, i64)>,
    },
}

impl AdRewardOutcome {
    /// 协议上的 `ret`。
    #[must_use]
    pub const fn resolve_ret(&self) -> i32 {
        match self {
            Self::LimitReached => PLAYER_ERROR_20.as_i32(),
            Self::Accept { .. } => 0,
        }
    }
}

/// `PlayerAdvertising.gainAdReward` 的纯逻辑。
#[must_use]
pub fn ad_reward_verdict(ctx: &AdRewardCtx) -> AdRewardOutcome {
    // `>=` 而不是 `>`：达到上限即拒
    if ctx.ad_reward_times >= ctx.max_times {
        return AdRewardOutcome::LimitReached;
    }
    AdRewardOutcome::Accept {
        new_times: ctx.ad_reward_times + 1,
        rewards: ctx.awards.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2024-03-15 12:00:00 +08:00
    const MAR_2024: i64 = 1_710_475_200_000;
    /// 2024-04-15 12:00:00 +08:00
    const APR_2024: i64 = 1_713_153_600_000;
    /// 2024-06-15 12:00:00 +08:00
    const JUN_2024: i64 = 1_718_424_000_000;
    /// 2024-12-15 12:00:00 +08:00
    const DEC_2024: i64 = 1_734_228_000_000;
    /// 2025-01-15 12:00:00 +08:00
    const JAN_2025: i64 = 1_736_906_400_000;

    fn ctx(add: i64, take: i64, now: i64) -> VvipCtx {
        VvipCtx {
            server_vip_exists: true,
            vip_wechat_add_time: add,
            vvip_take_reward_time: take,
            now,
        }
    }

    #[test]
    fn month_by_time_is_zero_based() {
        assert_eq!(month_by_time(MAR_2024), 2, "3 月 → 0-based 2");
        assert_eq!(month_by_time(DEC_2024), 11, "12 月 → 0-based 11");
        assert_eq!(month_by_time(JAN_2025), 0, "1 月 → 0-based 0");
    }

    #[test]
    fn between_month_handles_year_boundary() {
        // 这个函数用 12*year+month，跨年正确
        assert_eq!(between_month(DEC_2024, JAN_2025), 1);
        assert_eq!(between_month(MAR_2024, JUN_2024), 3);
        assert_eq!(between_month(APR_2024, MAR_2024), 1, "对称");
    }

    #[test]
    fn check_rejects_without_server_vip() {
        let mut c = ctx(MAR_2024, 0, APR_2024);
        c.server_vip_exists = false;
        assert_eq!(check_vvip_reward(&c), Some(RetCode::ServerBusy));
    }

    #[test]
    fn check_rejects_when_not_vvip() {
        // longStat == 0 → 玩家不是 vvip
        assert_eq!(
            check_vvip_reward(&ctx(0, 0, APR_2024)),
            Some(RetCode::ServerBusy)
        );
    }

    #[test]
    fn check_rejects_same_month() {
        assert_eq!(
            check_vvip_reward(&ctx(MAR_2024, 0, MAR_2024)),
            Some(RetCode::ServerBusy)
        );
    }

    #[test]
    fn check_accepts_adjacent_month() {
        assert_eq!(check_vvip_reward(&ctx(MAR_2024, 0, APR_2024)), None);
    }

    #[test]
    fn check_rejects_gap_of_two_months() {
        assert_eq!(
            check_vvip_reward(&ctx(MAR_2024, 0, JUN_2024)),
            Some(RetCode::ServerBusy)
        );
    }

    #[test]
    fn check_rejects_year_boundary_this_is_an_original_bug() {
        // ⚠ 原版缺陷照抄：12 月 → 1 月，abs(11-0)=11 != 1 → 回 BUSY
        // 也就是说跨年时首月奖励永远领不到。不能「修正」。
        assert_eq!(
            check_vvip_reward(&ctx(DEC_2024, 0, JAN_2025)),
            Some(RetCode::ServerBusy),
            "原版 Calendar.get(2) 是 0-based，跨年判断错误"
        );
    }

    #[test]
    fn aug_reward_first_time_gives_one_mail() {
        // vvipTakeRewardTime == 0 → 补 1 个月
        let out = check_vvip_aug_reward(&ctx(MAR_2024, 0, APR_2024));
        assert_eq!(out, 1);
    }

    #[test]
    fn aug_reward_counts_months_since_last_take() {
        // 3 月领过 → 6 月再领，跨 3 个月
        assert_eq!(check_vvip_aug_reward(&ctx(MAR_2024, MAR_2024, JUN_2024)), 3);
    }

    #[test]
    fn aug_reward_handles_year_boundary_correctly() {
        // 与 check_vvip_reward 不同，这个用 getBetweenMonth，跨年是对的
        assert_eq!(
            check_vvip_aug_reward(&ctx(DEC_2024, DEC_2024, JAN_2025)),
            1,
            "补发分支跨年正确 —— 原版两个函数口径不一致"
        );
    }

    #[test]
    fn aug_reward_zero_without_vip() {
        let mut c = ctx(MAR_2024, 0, APR_2024);
        c.server_vip_exists = false;
        assert_eq!(check_vvip_aug_reward(&c), 0);
    }

    #[test]
    fn handle_1012_uses_aug_branch_after_epoch() {
        // now >= 2021-08-01 → 走补发
        let out = handle_1012(&ctx(MAR_2024, 0, APR_2024));
        assert_eq!(
            out,
            VvipOutcome::Accept {
                mail_count: 1,
                stamp_take_time: true,
                stamp_wechat_add: false,
            }
        );
    }

    #[test]
    fn handle_1012_uses_first_month_branch_before_epoch() {
        // 构造一个 2021-07-31 之前的 now（用 2020 年的时间戳）
        let before = 1_594_000_000_000; // 2020-07-03 左右
        let add = before - 30 * 86_400_000; // 上个月
        let out = handle_1012(&VvipCtx {
            server_vip_exists: true,
            vip_wechat_add_time: add,
            vvip_take_reward_time: 0,
            now: before,
        });
        // 月份是否相邻取决于具体日期，这里只断言走的是首月分支的形状
        match out {
            VvipOutcome::Accept {
                stamp_wechat_add, ..
            } => assert!(stamp_wechat_add, "首月分支要写 VipWeChatAdd"),
            VvipOutcome::Resp(_) => { /* 月份不相邻也合法 */ }
        }
    }

    #[test]
    fn aug_reward_with_nothing_to_send_still_returns_ok() {
        // 同月领过 → n == 0 → 不发邮件但 ret 仍是 0
        let out = aug_reward(&ctx(MAR_2024, MAR_2024, MAR_2024));
        assert_eq!(
            out,
            VvipOutcome::Accept {
                mail_count: 0,
                stamp_take_time: false,
                stamp_wechat_add: false,
            }
        );
        assert_eq!(out.resolve_ret(), None, "原版此处仍回 ret=0");
    }

    // ── 1104 广告奖励 ──

    fn ad_ctx(times: i32, max: i32) -> AdRewardCtx {
        AdRewardCtx {
            ad_reward_times: times,
            max_times: max,
            awards: vec![(1, 50), (22, 1)],
        }
    }

    #[test]
    fn ad_reward_accept_increments_times() {
        match ad_reward_verdict(&ad_ctx(0, 5)) {
            AdRewardOutcome::Accept { new_times, rewards } => {
                assert_eq!(new_times, 1);
                assert_eq!(rewards, vec![(1, 50), (22, 1)]);
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn ad_reward_at_limit_is_rejected() {
        // 原版是 `>=`，所以 times == max 就拒
        assert_eq!(ad_reward_verdict(&ad_ctx(5, 5)), AdRewardOutcome::LimitReached);
        assert_eq!(
            AdRewardOutcome::LimitReached.resolve_ret(),
            PLAYER_ERROR_20.as_i32()
        );
        assert_eq!(PLAYER_ERROR_20.as_i32(), 120, "原版 PLAYER_ERROR_20 的值");
    }

    #[test]
    fn ad_reward_below_limit_is_ok() {
        match ad_reward_verdict(&ad_ctx(4, 5)) {
            AdRewardOutcome::Accept { new_times, .. } => assert_eq!(new_times, 5),
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn ad_reward_zero_max_blocks_everything() {
        // max=0 时第一次就被拒（原版配置缺失时 AdvertisingTimes 默认 0）
        assert_eq!(ad_reward_verdict(&ad_ctx(0, 0)), AdRewardOutcome::LimitReached);
    }

    #[test]
    fn ad_reward_passes_awards_verbatim() {
        let mut c = ad_ctx(0, 3);
        c.awards = vec![(999, 12345)];
        match ad_reward_verdict(&c) {
            AdRewardOutcome::Accept { rewards, .. } => {
                assert_eq!(rewards, vec![(999, 12345)], "奖励原样下发，不做合并")
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn first_month_reward_mails_once() {
        let out = first_month_reward(&ctx(MAR_2024, 0, APR_2024));
        match out {
            VvipOutcome::Accept {
                mail_count,
                stamp_wechat_add,
                ..
            } => {
                assert_eq!(mail_count, 1);
                assert!(stamp_wechat_add);
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }
}
