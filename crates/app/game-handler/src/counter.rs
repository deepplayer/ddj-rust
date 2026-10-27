//! # 计数器恢复（`CounterRecovery`）
//!
//! 对应 Java：
//! - `bf/game/server/module/counterRecovery/CounterRecovery.java`
//! - `bf/game/server/module/counterRecovery/CounterRecoverySet.java`
//! - `bf/game/server/module/counterRecovery/CounterRecoveryServerModule.java`
//! - `bf/template/db/CounterRecoveryDB.java`（模板）
//!
//! ## 这是什么
//! 一个通用的「体力条」：宠物互动次数、挑战次数等都用它。每条记录是
//! `(templateId, param1)` 二元键，带 `num`（当前值）、`time`（上次结算时间）、
//! `expectMaxNumTime`（预计回满时间）。
//!
//! ## 恢复模型（`CounterRecovery.refresh`，行 107-124）
//! ```java
//! maxLimit = getMaxLimit();                  // 由 VIP 模板决定
//! if (num < maxLimit) {
//!     intervalTime = nowTime - time;
//!     _num = intervalTime / db.getCdTime();  // 经过了多少个 CD 周期
//!     if (_num > 0) {
//!         _addNumWithoutLock(_num * db.getEveryNum(), ...);
//!         if (num < maxLimit) time += _num * db.getCdTime();   // ⚠ 只在没满时推进
//!     }
//! }
//! ```
//!
//! ⚠ **关键细节**：`time` 只在「加完之后仍未满」时才推进。
//! 如果加满了，`time` **保持不变** —— 下次再 refresh 会重复算出 `_num`，
//! 但因为 `num >= maxLimit` 直接跳过。这是原版写法，影响
//! 「满值后立刻消耗一次」时的恢复速度（会瞬间补满）。照抄。
//!
//! ## `checkNum` / `removeNum`
//! - `checkNum(pid, tpl, n)`：`num >= n`（原版还有 `num == MAX_VALUE` 的无限分支）
//! - `removeNum(pid, tpl, n, ...)`：扣减，扣完刷新 `time`

use ddj_pb::RetCode;

/// 计数器模板（`CounterRecoveryDB` 的一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CounterTemplate {
    /// `cdTime`：恢复一个周期需要多少毫秒。
    pub cd_time: i64,
    /// `everyNum`：每个周期恢复多少点。
    pub every_num: i64,
    /// `itemId`：关联的道具 id（部分计数器消耗道具）。
    pub item_id: i32,
    /// `type`：计数器类型。
    pub r#type: i32,
}

/// 一条计数器记录（`CounterRecovery` 的可见字段）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CounterState {
    /// `num`：当前值。
    pub num: i64,
    /// `time`：上次结算时间（毫秒）。
    pub time: i64,
    /// `expectMaxNumTime`：预计回满时间。
    pub expect_max_num_time: i64,
}

/// `refresh` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshResult {
    /// 结算后的状态。
    pub state: CounterState,
    /// 本次实际恢复的点数（`0` 表示没有恢复）。
    pub recovered: i64,
}

/// `CounterRecovery.getMaxLimit()`：上限由 VIP 模板决定，这里直接传入。
///
/// `CounterRecovery.refresh`（行 107-124）的纯逻辑。
///
/// ⚠ `time` 只在「加完仍未满」时推进（见模块文档）。
#[must_use]
pub fn refresh(state: CounterState, tpl: &CounterTemplate, max_limit: i64, now: i64) -> RefreshResult {
    // `num >= maxLimit` → 直接返回，不动 time
    if state.num >= max_limit {
        return RefreshResult {
            state,
            recovered: 0,
        };
    }
    // `cdTime <= 0` 会让除法炸掉；原版没有防护（模板缺失时 cdTime=0 →
    // ArithmeticException → 被外层 catch）。这里当作「不恢复」。
    if tpl.cd_time <= 0 || tpl.every_num <= 0 {
        return RefreshResult {
            state,
            recovered: 0,
        };
    }
    let interval = now - state.time;
    let cycles = interval / tpl.cd_time;
    if cycles <= 0 {
        return RefreshResult {
            state,
            recovered: 0,
        };
    }
    // `_addNumWithoutLock(_num * everyNum, ...)`：加上但不超过 maxLimit
    let add = cycles.saturating_mul(tpl.every_num);
    let new_num = (state.num + add).min(max_limit);
    let recovered = new_num - state.num;
    let mut next = CounterState {
        num: new_num,
        ..state
    };
    // ⚠ 只有「没满」才推进 time
    if new_num < max_limit {
        next.time = state.time + cycles * tpl.cd_time;
    }
    RefreshResult {
        state: next,
        recovered,
    }
}

/// `CounterRecoveryServerModule.checkNum`：够不够扣。
#[must_use]
pub fn check_num(state: &CounterState, need: i64) -> bool {
    // 原版对 `num == Long.MAX_VALUE` 有「无限」特判
    state.num == i64::MAX || state.num >= need
}

/// `CounterRecoveryServerModule.removeNum` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveResult {
    /// 扣减后的状态。
    pub state: CounterState,
    /// 是否扣成功。
    pub ok: bool,
}

/// `CounterRecoveryServerModule.removeNum`：扣减。
///
/// 原版 `removeNum(pid, tpl, n, false)` 在扣完后会 `save()` 并把
/// `time` 设为当前时间（从这一刻开始重新计时）。
#[must_use]
pub fn remove_num(state: CounterState, need: i64, now: i64) -> RemoveResult {
    if need <= 0 {
        return RemoveResult { state, ok: true };
    }
    if !check_num(&state, need) {
        return RemoveResult { state, ok: false };
    }
    let next = CounterState {
        num: if state.num == i64::MAX {
            i64::MAX
        } else {
            state.num - need
        },
        // 扣减后从当前时刻重新计时
        time: now,
        expect_max_num_time: state.expect_max_num_time,
    };
    RemoveResult { state: next, ok: true }
}

/// `CounterRecoveryServerModule.addNum`：增加（不超上限）。
#[must_use]
pub fn add_num(state: CounterState, add: i64, max_limit: i64) -> CounterState {
    if add <= 0 {
        return state;
    }
    CounterState {
        num: (state.num + add).min(max_limit),
        ..state
    }
}

/// 宠物互动的计数器模板 id（`CounterRecoveryServerModule` 里 `2` 是宠物互动）。
pub const PET_INTERACT_TEMPLATE_ID: i32 = 2;

/// 互动次数不足的错误码（`PET_ERROR_TEMPLATE_2_LIMIT`）。
///
/// 对应 `LanguageUtils.PET_ERROR_TEMPLATE_2_LIMIT`。
pub const PET_ERROR_TEMPLATE_2_LIMIT: RetCode = RetCode::PetErrorTemplate2Limit;

#[cfg(test)]
mod tests {
    use super::*;

    fn tpl() -> CounterTemplate {
        CounterTemplate {
            cd_time: 1_000,
            every_num: 1,
            item_id: 0,
            r#type: 0,
        }
    }

    fn st(num: i64, time: i64) -> CounterState {
        CounterState {
            num,
            time,
            expect_max_num_time: 0,
        }
    }

    #[test]
    fn at_max_limit_does_nothing() {
        let r = refresh(st(10, 0), &tpl(), 10, 99_999);
        assert_eq!(r.recovered, 0);
        assert_eq!(r.state.time, 0, "满了不动 time");
    }

    #[test]
    fn recovers_one_per_cd() {
        // 经过 3 个 CD（3000ms），每次 +1
        let r = refresh(st(0, 0), &tpl(), 10, 3_000);
        assert_eq!(r.state.num, 3);
        assert_eq!(r.recovered, 3);
        assert_eq!(r.state.time, 3_000, "没满 → 推进 time");
    }

    #[test]
    fn partial_cycle_does_not_recover() {
        // 只过 999ms < cdTime 1000
        let r = refresh(st(0, 0), &tpl(), 10, 999);
        assert_eq!(r.recovered, 0);
        assert_eq!(r.state.time, 0, "不足一个周期不推进");
    }

    #[test]
    fn recovery_clamps_at_max_limit() {
        // 从 8 开始，过 5 个 CD 想 +5，但上限 10 → 只 +2
        let r = refresh(st(8, 0), &tpl(), 10, 5_000);
        assert_eq!(r.state.num, 10);
        assert_eq!(r.recovered, 2);
        // ⚠ 加满了 → time 不推进（原版写法）
        assert_eq!(r.state.time, 0, "加满时 time 保持不变");
    }

    #[test]
    fn full_after_recovery_keeps_old_time_this_is_original_bug() {
        // 这条专门锁住「加满不推进 time」这个原版行为。
        // 第一次 refresh：num 9 → 10，因为加满了，time 保持 0。
        let r = refresh(st(9, 0), &tpl(), 10, 5_000);
        assert_eq!(r.state.num, 10);
        assert_eq!(r.state.time, 0, "加满时 time 不推进（原版写法）");

        // 后续扣减会按 `remove_num` 把 time 重置为当前时刻，
        // 所以「瞬间补满」要发生在**不经过 remove_num** 的路径上：
        // 直接把 num 改小（例如其它模块 setMaxNum 之外的写入），
        // time 仍是旧的 0，于是下次 refresh 会重复结算这段区间。
        let shrunk = CounterState {
            num: 9,
            time: r.state.time, // 仍是 0
            expect_max_num_time: 0,
        };
        let r2 = refresh(shrunk, &tpl(), 10, 5_000);
        assert_eq!(
            r2.state.num, 10,
            "time 未推进 → 重复结算 5000ms → 立刻补满（原版后果）"
        );
        assert_eq!(r2.recovered, 1);
    }

    #[test]
    fn remove_then_refresh_does_not_double_count() {
        // 与上一条对照：走 remove_num 的正常路径会重置 time，不会重复结算
        let r = refresh(st(9, 0), &tpl(), 10, 5_000);
        assert_eq!(r.state.num, 10);
        let after_remove = remove_num(r.state, 1, 5_000);
        assert_eq!(after_remove.state.num, 9);
        assert_eq!(after_remove.state.time, 5_000, "扣减重置 time");
        // 同一时刻 refresh：interval = 0 → 不恢复
        let r2 = refresh(after_remove.state, &tpl(), 10, 5_000);
        assert_eq!(r2.state.num, 9, "不会瞬间补满");
        assert_eq!(r2.recovered, 0);
    }

    #[test]
    fn zero_cd_time_is_noop() {
        let t = CounterTemplate {
            cd_time: 0,
            ..tpl()
        };
        assert_eq!(refresh(st(0, 0), &t, 10, 99_999).recovered, 0);
    }

    #[test]
    fn check_num_semantics() {
        assert!(check_num(&st(5, 0), 5), "等于也算够");
        assert!(check_num(&st(5, 0), 4));
        assert!(!check_num(&st(5, 0), 6));
    }

    #[test]
    fn check_num_max_value_is_infinite() {
        assert!(check_num(&st(i64::MAX, 0), i64::MAX / 2));
    }

    #[test]
    fn remove_resets_time() {
        let r = remove_num(st(5, 100), 2, 9_999);
        assert!(r.ok);
        assert_eq!(r.state.num, 3);
        assert_eq!(r.state.time, 9_999, "扣减后从当前时刻重新计时");
    }

    #[test]
    fn remove_insufficient_fails() {
        let r = remove_num(st(1, 0), 2, 9_999);
        assert!(!r.ok);
        assert_eq!(r.state.num, 1, "失败不改状态");
    }

    #[test]
    fn remove_zero_is_noop() {
        let r = remove_num(st(5, 100), 0, 9_999);
        assert!(r.ok);
        assert_eq!(r.state.time, 100, "n<=0 不重置 time");
    }

    #[test]
    fn remove_max_value_stays_max() {
        let r = remove_num(st(i64::MAX, 0), 100, 1_000);
        assert!(r.ok);
        assert_eq!(r.state.num, i64::MAX, "无限计数器扣完仍是 MAX");
    }

    #[test]
    fn add_clamps_to_max() {
        assert_eq!(add_num(st(8, 0), 5, 10).num, 10);
        assert_eq!(add_num(st(1, 0), 3, 10).num, 4);
    }

    #[test]
    fn add_zero_is_noop() {
        let s = st(3, 77);
        assert_eq!(add_num(s, 0, 10), s);
    }

    #[test]
    fn multi_recovery_per_cycle() {
        // everyNum = 3，过 2 个周期 → +6
        let t = CounterTemplate {
            cd_time: 1_000,
            every_num: 3,
            ..tpl()
        };
        let r = refresh(st(0, 0), &t, 100, 2_000);
        assert_eq!(r.state.num, 6);
        assert_eq!(r.state.time, 2_000);
    }
}
