//! # 计数器恢复（`CounterRecovery`）
//!
//! 对应 Java：
//! - `bf/game/server/module/counterRecovery/CounterRecovery.java`（逐方法移植）
//! - `bf/game/server/module/counterRecovery/CounterRecoverySet.java`
//! - `bf/game/server/module/counterRecovery/CounterRecoveryServerModule.java`
//! - `bf/template/db/CounterRecoveryDB.java`（模板）
//! - `bf/template/db/VipDB.getCounterMax(templateId)`（上限来源）
//!
//! ## 这是什么
//! 一个通用「体力条」：宠物精力、商战挑战次数、宴会次数……都挂在它上面。
//! 每条记录键是 `(templateId, param1)`，字段 `num`（当前值）、`time`（结算基准时间）、
//! `expectMaxNumTime`（预计回满时间）。
//!
//! ## ⚠ 三个反直觉的地方（照抄，不修）
//!
//! ### 1. `time == -1` 表示「停表」
//! 满值时 `time` 被置为 `-1`，不是置成当前时间。`cdRecovery` 首行
//! `if (time == -1 || time >= nowTime) return;` 直接跳过。
//!
//! ### 2. 新建的计数器是**满的**
//! `CounterRecoverySet.dbInit` 里 `new CounterRecovery(playerId, templateId, 0L, counterMax, -1L)`，
//! 构造签名是 `(playerId, templateId, param1, num, time)` —— 所以
//! `num = counterMax`、`time = -1`。新玩家直接满体力。
//!
//! ### 3. `isLimit()` 是「不允许超过上限」
//! CSV 列注释写「是否允许超出上限」，但 `CounterRecoveryDB.isLimit()` 返回
//! `this.isLimit == 0`。即列值 `0` → 受限（到上限停表），列值 `1` → 不受限。
//! 命名与语义相反，照抄。
//!
//! ## `refresh()` 的类型分派（`CounterRecoverySet.refresh`）
//! | `type` | 行为 |
//! |---|---|
//! | 1 | 每次 `refresh` 都走 `cdRecovery`（按 CD 恢复） |
//! | 2 | **跨天**才 `autoRecovery`（直接补满） |
//! | 3 | **跨周**才 `autoRecovery` |
//! | 其它 | 不动 |
//!
//! 集合自身的 `time` 在 `!sameDay || !sameWeek` 时更新（注意是 `||`）。

use ddj_pb::RetCode;

/// 计数器模板（`CounterRecoveryDB` 的一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CounterTemplate {
    /// `id`。
    pub id: i32,
    /// `type`：`1` 按 CD 恢复 / `2` 跨天补满 / `3` 跨周补满。
    pub r#type: i32,
    /// `cdTime`：`cd * 1000`（`CounterRecoveryDB.dbinit` 里算的）。
    pub cd_time: i64,
    /// `everyNum`：每个 CD 恢复多少。
    pub every_num: i64,
    /// `itemId`：关联道具。
    pub item_id: i32,
    /// `isLimit` 列的**原值**（`0` 表示受限，见模块文档第 3 条）。
    pub is_limit: i32,
}

impl CounterTemplate {
    /// `CounterRecoveryDB.isLimit()`：`isLimit == 0`。
    ///
    /// ⚠ 名字与语义相反：返回 `true` 表示**受限**（到上限停表）。
    #[must_use]
    pub const fn limit(&self) -> bool {
        self.is_limit == 0
    }
}

/// 一条计数器记录（`CounterRecovery` 的可见字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterState {
    /// `num`。
    pub num: i64,
    /// `time`：`-1` 表示停表。
    pub time: i64,
    /// `expectMaxNumTime`。
    pub expect_max_num_time: i64,
}

impl Default for CounterState {
    /// 「尚未 dbInit」的形态：`num = 0`、`time = -1`。
    ///
    /// ⚠ 真正新建时用的是 [`CounterState::new_full`]（`num = counterMax`）。
    fn default() -> Self {
        Self {
            num: 0,
            time: -1,
            expect_max_num_time: 0,
        }
    }
}

impl CounterState {
    /// `new CounterRecovery(playerId, templateId, param1, counterMax, -1L)`。
    #[must_use]
    pub const fn new_full(max_limit: i64) -> Self {
        Self {
            num: max_limit,
            time: -1,
            expect_max_num_time: 0,
        }
    }

    /// 是否处于停表状态。
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        self.time == -1
    }
}

/// `cdRecovery` / `autoRecovery` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshResult {
    /// 结算后的状态。
    pub state: CounterState,
    /// 本次实际恢复的点数（`0` 表示没有恢复）。
    pub recovered: i64,
}

impl RefreshResult {
    const fn noop(state: CounterState) -> Self {
        Self {
            state,
            recovered: 0,
        }
    }
}

/// `CounterRecovery.calcExpectMaxNumTime(maxLimit)`：
/// 只在 `maxLimit > num && time > 0` 时写。
fn calc_expect_max_num_time(state: &CounterState, tpl: &CounterTemplate, max_limit: i64) -> i64 {
    if max_limit > state.num && state.time > 0 {
        state.time + (max_limit - state.num) * tpl.cd_time
    } else {
        state.expect_max_num_time
    }
}

/// `CounterRecovery._addNumWithoutLock(num, checkLimit, sendClient, cdRecovery)`。
///
/// 返回 `(新状态, 是否成功)`；`num == Long.MAX_VALUE` 时原版返回 `false`。
/// `send_client` 在纯函数里没有意义（推送由接线层做），故省略；
/// `cd_recovery` 为真时**不**清 `expectMaxNumTime`。
fn add_num_without_lock(
    state: CounterState,
    add: i64,
    check_limit: bool,
    cd_recovery: bool,
    max_limit: i64,
    tpl: &CounterTemplate,
) -> (CounterState, bool) {
    if state.num == i64::MAX {
        return (state, false);
    }
    let mut next = state;
    // `if (Long.MAX_VALUE - num >= _num) num += _num; else num = Long.MAX_VALUE;`
    next.num = if i64::MAX - state.num >= add {
        state.num + add
    } else {
        i64::MAX
    };
    if check_limit {
        next.num = next.num.min(max_limit);
    }
    if tpl.limit() {
        if next.num >= max_limit {
            next.time = -1;
            if !cd_recovery {
                next.expect_max_num_time = 0;
            }
        }
    } else if next.num == i64::MAX {
        next.time = -1;
        if !cd_recovery {
            next.expect_max_num_time = 0;
        }
    }
    (next, true)
}

/// `CounterRecovery.cdRecovery(nowTime)`（`:100-149`）。
///
/// `max_limit` 来自 `VipDB.getCounterMax(templateId)`，由调用方注入。
#[must_use]
pub fn cd_recovery(
    state: CounterState,
    tpl: &CounterTemplate,
    max_limit: i64,
    now: i64,
) -> RefreshResult {
    // `if (this.time == -1 || this.time >= nowTime) return;`
    if state.time == -1 || state.time >= now {
        return RefreshResult::noop(state);
    }
    // `cdTime <= 0` 时原版除法抛 ArithmeticException（模板缺失的畸形数据）。
    // 这里当作「不恢复」—— 只在畸形配置下分叉，已记入 AUDIT A16。
    if tpl.cd_time <= 0 {
        return RefreshResult::noop(state);
    }

    if tpl.limit() {
        if state.num >= max_limit {
            return RefreshResult::noop(state);
        }
        // ⚠ `calcExpectMaxNumTime` 是**直接写字段**，早于 `_num > 0` 判断，
        // 所以哪怕一个周期都没走完，`expectMaxNumTime` 也已经落库。
        let mut seeded = state;
        seeded.expect_max_num_time = calc_expect_max_num_time(&state, tpl, max_limit);
        let cycles = (now - state.time) / tpl.cd_time;
        if cycles <= 0 {
            return RefreshResult {
                state: seeded,
                recovered: 0,
            };
        }
        let (after, ok) = add_num_without_lock(
            seeded,
            cycles.saturating_mul(tpl.every_num),
            true,
            true,
            max_limit,
            tpl,
        );
        if !ok {
            return RefreshResult::noop(state);
        }
        let mut out = after;
        if after.num < max_limit {
            // `time += _num * cdTime`（在旧 time 上累加，不是置为 now）
            out.time = state.time + cycles * tpl.cd_time;
        }
        RefreshResult {
            state: out,
            recovered: after.num - state.num,
        }
    } else {
        // 原版 `else if (this.num < Long.MAX_VALUE)`；`num` 不可能超过 MAX，故等价于 `== MAX`
        if state.num == i64::MAX {
            return RefreshResult::noop(state);
        }
        let cycles = (now - state.time) / tpl.cd_time;
        if cycles <= 0 {
            return RefreshResult::noop(state);
        }
        let (after, ok) = add_num_without_lock(
            state,
            cycles.saturating_mul(tpl.every_num),
            false,
            true,
            max_limit,
            tpl,
        );
        if !ok {
            return RefreshResult::noop(state);
        }
        let mut out = after;
        if after.num < i64::MAX {
            out.time = state.time + cycles * tpl.cd_time;
        }
        RefreshResult {
            state: out,
            recovered: after.num - state.num,
        }
    }
}

/// `CounterRecovery.autoRecovery(nowTime)`（`:167-183`）：直接补满。
#[must_use]
pub fn auto_recovery(state: CounterState, max_limit: i64, now: i64) -> RefreshResult {
    if state.num >= max_limit {
        return RefreshResult::noop(state);
    }
    RefreshResult {
        state: CounterState {
            num: max_limit,
            time: now,
            expect_max_num_time: state.expect_max_num_time,
        },
        recovered: max_limit - state.num,
    }
}

/// `CounterRecovery.getCdTime(nowTime)`（`:151-165`）。
#[must_use]
pub fn get_cd_time(state: &CounterState, tpl: &CounterTemplate, max_limit: i64) -> i64 {
    if state.time == -1 {
        return -1;
    }
    if tpl.limit() {
        if state.num >= max_limit {
            return -1;
        }
    } else if state.num == i64::MAX {
        return -1;
    }
    state.time + tpl.cd_time
}

/// `CounterRecovery.setMaxNum(timeStamp)` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetMaxResult {
    /// 补了多少。
    pub delta: i64,
    /// 新状态。
    pub state: CounterState,
}

/// `CounterRecovery.setMaxNum(timeStamp)`（`:185-207`）：补满并重置计时。
#[must_use]
pub fn set_max_num(
    state: CounterState,
    tpl: &CounterTemplate,
    max_limit: i64,
    time_stamp: i64,
) -> SetMaxResult {
    SetMaxResult {
        delta: max_limit - state.num,
        state: CounterState {
            num: max_limit,
            // `if (db.isLimit()) time = -1; else time = timeStamp;`
            time: if tpl.limit() { -1 } else { time_stamp },
            expect_max_num_time: 0,
        },
    }
}

/// `CounterRecovery.checkNum(_num)`。
#[must_use]
pub fn check_num(state: &CounterState, need: i64) -> bool {
    state.num >= need
}

/// `removeNum` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveResult {
    /// 扣减后的状态。
    pub state: CounterState,
    /// 是否扣成功。
    pub ok: bool,
}

/// `CounterRecovery.removeNum(_num, check)`（`:283-327`）。
///
/// 扣完后若原本停表（`time == -1`）且现在没满，则把 `time` 设为当前时刻 ——
/// 这是「满 → 消耗 → 开始计时」的唯一入口。
#[must_use]
pub fn remove_num(
    state: CounterState,
    tpl: &CounterTemplate,
    need: i64,
    check: bool,
    max_limit: i64,
    now: i64,
) -> RemoveResult {
    if need < 0 {
        return RemoveResult { state, ok: false };
    }
    if need == 0 {
        return RemoveResult { state, ok: true };
    }
    if check && state.num < need {
        return RemoveResult { state, ok: false };
    }
    let mut next = state;
    next.num -= need;
    if next.num < 0 {
        next.num = 0;
    }
    if next.time == -1 {
        if tpl.limit() {
            if next.num < max_limit {
                next.time = now;
            }
        } else if next.num < i64::MAX {
            next.time = now;
        }
    }
    RemoveResult { state: next, ok: true }
}

/// `CounterRecovery.toMsgCounterDataBuilder()` 的字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterDataMsg {
    /// `templateId`。
    pub template_id: i32,
    /// `num`。
    pub num: i64,
    /// `cdTime`。
    pub cd_time: i64,
}

/// `CounterRecovery.toMsgCounterDataBuilder()`（`:390-404`）。
///
/// ```text
/// id == 5   → cdTime = -1        （子嗣活力，客户端不显示倒计时）
/// type == 1 → cdTime = getCdTime(now)
/// 其它      → cdTime = time      （type 2/3 直接把基准时间给客户端）
/// ```
#[must_use]
pub fn to_counter_data(
    state: &CounterState,
    tpl: &CounterTemplate,
    max_limit: i64,
) -> CounterDataMsg {
    let cd_time = if tpl.id == 5 {
        -1
    } else if tpl.r#type == 1 {
        get_cd_time(state, tpl, max_limit)
    } else {
        state.time
    };
    CounterDataMsg {
        template_id: tpl.id,
        num: state.num,
        cd_time,
    }
}

// ─────────────────────── 集合级（`CounterRecoverySet`） ───────────────────────

/// `UtilsTools.isSameDay(a, b)`：按服务器本地时区比较年月日。
#[must_use]
pub fn is_same_day(a: i64, b: i64, tz_offset_secs: i32) -> bool {
    let day = |ms: i64| (ms.div_euclid(1000) + i64::from(tz_offset_secs)).div_euclid(86_400);
    day(a) == day(b)
}

/// `UtilsTools.isSameWeek(a, b)`（`UtilsTools.java:271-279`）。
///
/// ```java
/// if (Math.abs(nowTime - time) >= WEEK_TIME) return false;   // WEEK_TIME = 691200000（8 天）
/// return getWeekOfYear(nowTime) == getWeekOfYear(time);
/// ```
/// `getWeekOfYear` 用 `firstDayOfWeek = 2`（周一）、`minimalDaysInFirstWeek = 7`，
/// 即「年内第一个完整周一~周日」为第 1 周，年初不足一周的几天算第 0 周。
///
/// ⚠ 实测 `J-计数器恢复-CounterRecovery.csv` 里 **`type=3` 有 0 行**，
/// 这条分支没有数据可验证，年初边界（第 0 周）与 `java.util.Calendar` 可能差一周。
/// 已记入 AUDIT A16。
#[must_use]
pub fn is_same_week(a: i64, b: i64, tz_offset_secs: i32) -> bool {
    const WEEK_TIME: i64 = 691_200_000;
    if (a - b).abs() >= WEEK_TIME {
        return false;
    }
    week_of_year(a, tz_offset_secs) == week_of_year(b, tz_offset_secs)
}

/// `(year, week)`：`week` 见 [`is_same_week`]。
fn week_of_year(ms: i64, tz_offset_secs: i32) -> (i64, i64) {
    let days = (ms.div_euclid(1000) + i64::from(tz_offset_secs)).div_euclid(86_400);
    let (year, doy) = civil_from_days(days);
    // 1970-01-01 是周四。当年 1 月 1 日是周几（周一 = 0）：
    let jan1_dow = (days - doy + 3).rem_euclid(7);
    // 第一个周一的日序号
    let first_monday = (7 - jan1_dow) % 7;
    let week = if doy < first_monday {
        0
    } else {
        (doy - first_monday) / 7 + 1
    };
    (year, week)
}

/// 日序号 → `(year, day_of_year)`（Howard Hinnant `civil_from_days`）。
fn civil_from_days(z: i64) -> (i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, day_of_year(year, m, d))
}

/// 年内第几天（0 基）。
fn day_of_year(year: i64, month: i64, day: i64) -> i64 {
    const CUM: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let m = usize::try_from(month - 1).unwrap_or(0).min(11);
    CUM[m] + day - 1 + i64::from(leap && month > 2)
}

/// `CounterRecoverySet.refresh()` 的一次结算结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetRefresh {
    /// 集合自身的 `time` 是否要更新成 `now`。
    pub set_time: Option<i64>,
    /// 需要回写的计数器：`(templateId, 新状态)`。
    pub updated: Vec<(i32, CounterState)>,
}

/// `CounterRecoverySet.refresh()`（`CounterRecoverySet.java:170-195`）。
///
/// `lookup(templateId)` 查模板；`max_limit(templateId)` 查 VIP 上限。
#[must_use]
pub fn refresh_set<F, G>(
    set_time: i64,
    counters: &[(i32, CounterState)],
    lookup: F,
    max_limit: G,
    now: i64,
    tz_offset_secs: i32,
) -> SetRefresh
where
    F: Fn(i32) -> Option<CounterTemplate>,
    G: Fn(i32) -> i64,
{
    let same_day = is_same_day(set_time, now, tz_offset_secs);
    let same_week = is_same_week(set_time, now, tz_offset_secs);
    let mut out = SetRefresh {
        // `if (!sameDay || !sameWeek) { this.time = nowTime; save(); }`
        set_time: if same_day && same_week { None } else { Some(now) },
        updated: Vec::new(),
    };
    for (template_id, state) in counters {
        let Some(tpl) = lookup(*template_id) else {
            continue;
        };
        let max = max_limit(*template_id);
        let next = match tpl.r#type {
            1 => cd_recovery(*state, &tpl, max, now),
            2 if !same_day => auto_recovery(*state, max, now),
            3 if !same_week => auto_recovery(*state, max, now),
            _ => RefreshResult::noop(*state),
        };
        if next.state != *state {
            out.updated.push((*template_id, next.state));
        }
    }
    out
}

/// 宠物互动用的计数器模板 id（`CounterRecoveryServerModule.TEMPLATE_2`）。
pub const PET_INTERACT_TEMPLATE_ID: i32 = 2;

/// 子嗣活力（`TEMPLATE_5`，`toMsgCounterDataBuilder` 里 `cdTime` 恒 `-1`）。
pub const CHILD_ENERGY_TEMPLATE_ID: i32 = 5;

/// 互动次数不足的错误码（`LanguageUtils.PET_ERROR_TEMPLATE_2_LIMIT`）。
pub const PET_ERROR_TEMPLATE_2_LIMIT: RetCode = RetCode::PetErrorTemplate2Limit;

#[cfg(test)]
mod tests {
    use super::*;

    fn tpl_limited() -> CounterTemplate {
        CounterTemplate {
            id: 2,
            r#type: 1,
            cd_time: 1_000,
            every_num: 1,
            item_id: 402,
            is_limit: 0, // 受限
        }
    }

    fn tpl_unlimited() -> CounterTemplate {
        CounterTemplate {
            is_limit: 1,
            ..tpl_limited()
        }
    }

    fn st(num: i64, time: i64) -> CounterState {
        CounterState {
            num,
            time,
            expect_max_num_time: 0,
        }
    }

    // ── 模板 ──

    #[test]
    fn is_limit_zero_means_limited() {
        // ⚠ 列值 0 → 受限（命名与语义相反）
        assert!(tpl_limited().limit());
        assert!(!tpl_unlimited().limit());
    }

    #[test]
    fn new_counter_is_full_and_stopped() {
        // dbInit: new CounterRecovery(pid, tpl, 0, counterMax, -1)
        let s = CounterState::new_full(3);
        assert_eq!(s.num, 3);
        assert_eq!(s.time, -1);
        assert!(s.is_stopped());
    }

    // ── cdRecovery ──

    #[test]
    fn stopped_counter_never_recovers() {
        let r = cd_recovery(st(0, -1), &tpl_limited(), 10, 99_999);
        assert_eq!(r.recovered, 0);
        assert_eq!(r.state.time, -1, "停表状态原版首行就 return");
    }

    #[test]
    fn future_time_is_skipped() {
        let r = cd_recovery(st(0, 5_000), &tpl_limited(), 10, 5_000);
        assert_eq!(r.recovered, 0);
    }

    #[test]
    fn recovers_one_per_cd_and_advances_time() {
        let r = cd_recovery(st(0, 0), &tpl_limited(), 10, 3_000);
        assert_eq!(r.state.num, 3);
        assert_eq!(r.recovered, 3);
        assert_eq!(r.state.time, 3_000, "没满 → time 在旧值上累加");
    }

    #[test]
    fn partial_cycle_does_not_recover() {
        let r = cd_recovery(st(0, 0), &tpl_limited(), 10, 999);
        assert_eq!(r.recovered, 0);
        assert_eq!(r.state.time, 0);
    }

    #[test]
    fn reaching_max_sets_time_to_minus_one() {
        // 8 + 5 → 夹到 10 → 满 → time = -1
        let r = cd_recovery(st(8, 0), &tpl_limited(), 10, 5_000);
        assert_eq!(r.state.num, 10);
        assert_eq!(r.recovered, 2);
        assert_eq!(r.state.time, -1, "满值 → 停表（_addNumWithoutLock）");
    }

    #[test]
    fn at_max_returns_immediately() {
        let r = cd_recovery(st(10, 0), &tpl_limited(), 10, 99_999);
        assert_eq!(r.recovered, 0);
        assert_eq!(r.state, st(10, 0), "已满不动任何字段");
    }

    #[test]
    fn unlimited_counter_caps_at_i64_max_not_max_limit() {
        let t = tpl_unlimited();
        let r = cd_recovery(st(0, 0), &t, 10, 5_000);
        assert_eq!(r.state.num, 5, "不受限时不会被 maxLimit 夹");
        assert_eq!(r.state.time, 5_000);
    }

    #[test]
    fn zero_cd_time_is_noop() {
        let t = CounterTemplate {
            cd_time: 0,
            ..tpl_limited()
        };
        assert_eq!(cd_recovery(st(0, 0), &t, 10, 99_999).recovered, 0);
    }

    #[test]
    fn expect_max_num_time_is_written_when_not_full() {
        // calcExpectMaxNumTime: maxLimit > num && time > 0
        let r = cd_recovery(st(0, 100), &tpl_limited(), 10, 200);
        assert_eq!(r.state.expect_max_num_time, 100 + 10 * 1_000);
    }

    #[test]
    fn expect_max_num_time_not_written_when_time_is_zero() {
        let r = cd_recovery(st(0, 0), &tpl_limited(), 10, 200);
        assert_eq!(r.state.expect_max_num_time, 0, "time <= 0 不写");
    }

    // ── autoRecovery ──

    #[test]
    fn auto_recovery_fills_to_max() {
        let r = auto_recovery(st(2, 0), 10, 9_999);
        assert_eq!(r.state.num, 10);
        assert_eq!(r.state.time, 9_999);
        assert_eq!(r.recovered, 8);
    }

    #[test]
    fn auto_recovery_at_max_is_noop() {
        let r = auto_recovery(st(10, 0), 10, 9_999);
        assert_eq!(r.state, st(10, 0));
    }

    // ── getCdTime ──

    #[test]
    fn cd_time_of_stopped_is_minus_one() {
        assert_eq!(get_cd_time(&st(0, -1), &tpl_limited(), 10), -1);
    }

    #[test]
    fn cd_time_of_full_is_minus_one() {
        assert_eq!(get_cd_time(&st(10, 100), &tpl_limited(), 10), -1);
    }

    #[test]
    fn cd_time_normal_is_time_plus_cd() {
        assert_eq!(get_cd_time(&st(3, 100), &tpl_limited(), 10), 1_100);
    }

    // ── setMaxNum ──

    #[test]
    fn set_max_num_fills_and_stops_when_limited() {
        let r = set_max_num(st(3, 500), &tpl_limited(), 10, 9_999);
        assert_eq!(r.delta, 7);
        assert_eq!(r.state.num, 10);
        assert_eq!(r.state.time, -1, "受限 → time = -1");
        assert_eq!(r.state.expect_max_num_time, 0);
    }

    #[test]
    fn set_max_num_uses_timestamp_when_unlimited() {
        let r = set_max_num(st(3, 500), &tpl_unlimited(), 10, 9_999);
        assert_eq!(r.state.time, 9_999, "不受限 → time = timeStamp");
    }

    // ── removeNum ──

    #[test]
    fn remove_from_full_starts_timer() {
        let r = remove_num(st(10, -1), &tpl_limited(), 1, true, 10, 7_777);
        assert!(r.ok);
        assert_eq!(r.state.num, 9);
        assert_eq!(r.state.time, 7_777, "满 → 消耗 → 开始计时");
    }

    #[test]
    fn remove_while_running_keeps_time() {
        let r = remove_num(st(5, 100), &tpl_limited(), 2, true, 10, 7_777);
        assert_eq!(r.state.num, 3);
        assert_eq!(r.state.time, 100, "已在计时中，不重置");
    }

    #[test]
    fn remove_insufficient_fails() {
        let r = remove_num(st(1, 0), &tpl_limited(), 2, true, 10, 7_777);
        assert!(!r.ok);
        assert_eq!(r.state.num, 1);
    }

    #[test]
    fn remove_without_check_clamps_zero() {
        let r = remove_num(st(1, 0), &tpl_limited(), 5, false, 10, 7_777);
        assert!(r.ok);
        assert_eq!(r.state.num, 0, "负数夹到 0");
    }

    #[test]
    fn remove_negative_is_rejected() {
        assert!(!remove_num(st(5, 0), &tpl_limited(), -1, false, 10, 0).ok);
    }

    #[test]
    fn remove_zero_succeeds_without_touching_state() {
        let r = remove_num(st(5, 100), &tpl_limited(), 0, true, 10, 7_777);
        assert!(r.ok);
        assert_eq!(r.state, st(5, 100));
    }

    // ── toMsgCounterData ──

    #[test]
    fn msg_cd_time_for_template_5_is_always_minus_one() {
        let t = CounterTemplate {
            id: 5,
            ..tpl_limited()
        };
        let m = to_counter_data(&st(3, 100), &t, 10);
        assert_eq!(m.cd_time, -1, "id=5 恒 -1");
        assert_eq!(m.template_id, 5);
        assert_eq!(m.num, 3);
    }

    #[test]
    fn msg_cd_time_for_type_1_is_deadline() {
        let m = to_counter_data(&st(3, 100), &tpl_limited(), 10);
        assert_eq!(m.cd_time, 1_100, "type=1 → getCdTime");
    }

    #[test]
    fn msg_cd_time_for_type_2_is_raw_time() {
        let t = CounterTemplate {
            r#type: 2,
            ..tpl_limited()
        };
        let m = to_counter_data(&st(3, 100), &t, 10);
        assert_eq!(m.cd_time, 100, "type=2 → 直接把 time 给客户端");
    }

    // ── 集合级 ──

    #[test]
    fn same_day_across_midnight_in_utc() {
        // 2026-10-27T23:30Z 与 2026-10-28T00:30Z
        let a = 1_793_143_800_000;
        let b = a + 3_600_000;
        assert!(!is_same_day(a, b, 0), "UTC 下跨日");
        assert!(is_same_day(a, b, 8 * 3600), "东八区同在 28 日");
    }

    #[test]
    fn same_week_rejects_beyond_eight_days() {
        let a = 1_793_143_800_000;
        assert!(!is_same_week(a, a + 691_200_000, 0), "≥8 天必不同周");
    }

    #[test]
    fn same_week_within_a_week() {
        let a = 1_793_143_800_000;
        assert!(is_same_week(a, a + 86_400_000, 0), "同周内相邻两天");
    }

    #[test]
    fn week_number_advances_after_seven_days() {
        let a = 1_793_143_800_000;
        let (_, w1) = week_of_year(a, 0);
        let (_, w2) = week_of_year(a + 7 * 86_400_000, 0);
        assert_eq!(w2, w1 + 1);
    }

    #[test]
    fn refresh_set_updates_time_when_day_changes() {
        let now = 1_793_143_800_000;
        let out = refresh_set(now - 86_400_000, &[], |_| None, |_| 0, now, 0);
        assert_eq!(out.set_time, Some(now));
    }

    #[test]
    fn refresh_set_keeps_time_when_same_day() {
        let now = 1_793_143_800_000;
        let out = refresh_set(now, &[], |_| None, |_| 0, now, 0);
        assert_eq!(out.set_time, None, "同天同周 → 不写");
    }

    #[test]
    fn refresh_set_type1_runs_cd_recovery() {
        let now = 1_793_143_800_000;
        let out = refresh_set(
            now,
            &[(2, st(0, now - 3_000))],
            |_| Some(tpl_limited()),
            |_| 10,
            now,
            0,
        );
        assert_eq!(out.updated.len(), 1);
        assert_eq!(out.updated[0].1.num, 3);
    }

    #[test]
    fn refresh_set_type2_only_on_new_day() {
        let now = 1_793_143_800_000;
        let t2 = |_| {
            Some(CounterTemplate {
                r#type: 2,
                ..tpl_limited()
            })
        };
        let same = refresh_set(now, &[(4, st(2, now))], t2, |_| 5, now, 0);
        assert!(same.updated.is_empty(), "同天不补满");
        let cross = refresh_set(now - 86_400_000, &[(4, st(2, now))], t2, |_| 5, now, 0);
        assert_eq!(cross.updated.len(), 1);
        assert_eq!(cross.updated[0].1.num, 5);
    }

    #[test]
    fn refresh_set_type3_only_on_new_week() {
        let now = 1_793_143_800_000;
        let t3 = |_| {
            Some(CounterTemplate {
                r#type: 3,
                ..tpl_limited()
            })
        };
        let next_day = refresh_set(now - 86_400_000, &[(7, st(1, now))], t3, |_| 9, now, 0);
        assert!(next_day.updated.is_empty(), "同周不补满");
        let next_week = refresh_set(now - 8 * 86_400_000, &[(7, st(1, now))], t3, |_| 9, now, 0);
        assert_eq!(next_week.updated[0].1.num, 9);
    }

    #[test]
    fn refresh_set_unknown_template_is_skipped() {
        let now = 1_793_143_800_000;
        let out = refresh_set(now, &[(99, st(1, 0))], |_| None, |_| 10, now, 0);
        assert!(out.updated.is_empty());
    }
}
