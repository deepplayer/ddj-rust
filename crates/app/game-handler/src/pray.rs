//! # 焚香祈愿（常驻卡池，17071 / 17072）
//!
//! 对应 Java：
//! - `bf/game/server/message/handler/PrayMessageHandler.java`（150 行）
//! - `bf/game/server/module/sysActivity/model/PrayModel.java`（130 行）
//! - `bf/game/server/module/sysActivity/module/PrayModule.java`（公告栏）
//! - `bf/template/db/PrayDB.java` + `PrayDBCache.getByServerOpenDay`
//! - `bf/game/server/event/PrayCoreReward.java`
//!
//! ## 三条消息
//! | cmd | 作用 | 回包 |
//! |---|---|---|
//! | 17071 | 抽奖（1 次 / 10 次） | `PrayResp_17071{ret, rewards}` **+ 推 17074** |
//! | 17072 | 拉公告栏 | `MsgPrayNoticeResp_17072{notices}` |
//! | 17074 | （推送）祈愿数据 | 服务端主动发 |
//!
//! ## ⚠ 17071 一次请求发两个包
//! ```java
//! MessageUtil.sendToPlayerId(MsgPrayDataResp_17074{...}, player.getId());   // 先推
//! return PrayResp_17071{ret=0, rewards};                                    // 再回
//! ```
//! 推送在前、回包在后。
//!
//! ## ⚠ 单抽的「免费」只看日期
//! ```java
//! boolean free = prayModel.getFreeTime() < LogicUtils.getTodayZeroTimeStamp();
//! if (free) prayModel.setFreeTime(getTodayZeroTimeStamp());
//! else if (!removeResources(oneCostJson)) throw PRAY_ERROR_COST;
//! ```
//! `freeTime` 记的是「上次免费用掉的那天的零点」，
//! 所以同一天只有**第一次**免费。`freeTime` 初始为 `0` → 第一次必然免费。
//!
//! ## ⚠ 十连**没有**免费
//! `prayTenTimes` 无条件扣 `tenCostJson`。
//!
//! ## ⚠ 稀有保底的判定顺序
//! ```java
//! private List<ResourcesJson> doPray(player, prayModel, prayDB) {
//!     boolean special = prayModel.getFirstReward() <= 0 && prayModel.getTimes() < 10;
//!     List<ResourcesJson> r = doPray(player, special, prayModel, prayDB);
//!     for (ResourcesJson x : r) if (x.getId() == prayDB.getCoreRewardJson().getId())
//!         notifyEvent(PrayCoreReward(...));                                  // 全服公告
//!     prayModel.addLuckNum(prayDB.getEveryLuckNum(), prayDB.getLuckNum());    // 幸运值
//!     return r;
//! }
//!
//! private List<ResourcesJson> doPray(player, special, prayModel, prayDB) {
//!     if (special) {
//!         prayModel.setTimes(prayModel.getTimes() + 1);
//!         if (RandomUtil.isHitForTenThousand(3000)) {   // 30%
//!             prayModel.setFirstReward(prayModel.getTimes());
//!             return [prayDB.getCoreRewardJson()];
//!         }
//!     }
//!     return prayModel.getLuckNum() >= prayDB.getLuckNum()
//!         ? [prayDB.getCoreRewardJson()]                 // 幸运值满 → 必出核心
//!         : DropUtils.rand(player, prayDB.getDropId());
//! }
//! ```
//! 三层：**前 10 次 30% 提前出核心** → **幸运值满必出核心** → 否则走掉落表。
//!
//! ## ⚠ `addLuckNum` 的溢出翻转
//! ```java
//! public void addLuckNum(int addNum, int maxLuckNum) {
//!     if (this.luckNum >= maxLuckNum) { this.luckNum = addNum; this.cycle++; }
//!     else                             { this.luckNum += addNum; }
//!     this.totalTimes++;
//! }
//! ```
//! 幸运值是「先判后加」：到上限后**下一次**才回到 `addNum` 并 `cycle+1`。
//! 也就是会短暂出现 `luckNum >= maxLuckNum` 的状态 —— 而 `doPray` 的判定
//! `luckNum >= prayDB.getLuckNum()` 正是在这个状态下命中，所以逻辑自洽。
//!
//! ## ⚠ `isHitForTenThousand(3000)` 是 30%
//! `UtilsTools.randomIndex(10000) < 3000`，即 `[0,9999]` 里 `< 3000` → **恰好 30%**。
//!
//! ## 公告栏只保留最近 5 条
//! `PrayModule.handleEvent`：`while (noticeList.size() > 5) noticeList.remove(0);`

use ddj_pb::RetCode;

/// `LanguageUtils.PRAY_ERROR_UN_OPEN`。
pub const PRAY_ERROR_UN_OPEN: RetCode = RetCode::PrayErrorUnOpen;
/// `LanguageUtils.PRAY_ERROR_COST`。
pub const PRAY_ERROR_COST: RetCode = RetCode::PrayErrorCost;

/// `RandomUtil.isHitForTenThousand(3000)` 的分子（前 10 次抽核心的概率）。
pub const FIRST_REWARD_CHANCE: u32 = 3000;

/// `CommonConstant.WAN_FEN_BI_INT`。
pub const WAN_FEN: u32 = 10_000;

/// 公告栏最多保留的条数（`PrayModule.handleEvent`）。
pub const NOTICE_LIMIT: usize = 5;

/// `PrayDB` 的一行（`F-焚香祈愿-Pray.csv`，实测只有 1 行）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrayTemplate {
    /// `id`。
    pub id: i32,
    /// `day`：开服天数门槛。
    pub day: i32,
    /// `oneCost`：单抽消耗。
    pub one_cost: Vec<(i32, i64)>,
    /// `tenCost`：十连消耗。
    pub ten_cost: Vec<(i32, i64)>,
    /// `luckNum`：幸运值上限。
    pub luck_num: i32,
    /// `everyLuckNum`：每次获得幸运值。
    pub every_luck_num: i32,
    /// `dropId`。
    pub drop_id: i32,
    /// `coreReward`：核心奖励。
    pub core_reward: Vec<(i32, i64)>,
}

impl PrayTemplate {
    /// `PrayDB.getCoreRewardJson().getId()` —— 取核心奖励的**第一个** id。
    #[must_use]
    pub fn core_reward_id(&self) -> Option<i32> {
        self.core_reward.first().map(|(id, _)| *id)
    }
}

/// `PrayModel` 的持久化字段。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrayState {
    /// `freeTime`：上次免费用掉的天的零点。
    pub free_time: i64,
    /// `firstReward`：前 10 次里首次拿核心是第几次（0 = 没拿到）。
    pub first_reward: i32,
    /// `times`：前 10 次抽了几次。
    pub times: i32,
    /// `luckNum`：当前幸运值。
    pub luck_num: i32,
    /// `cycle`：幸运值满的次数。
    pub cycle: i32,
    /// `totalTimes`：累计抽奖次数。
    pub total_times: i32,
}

impl PrayState {
    /// `PrayModel.toMsg()` 的三个字段。
    #[must_use]
    pub const fn to_msg_parts(&self) -> (i64, i32, i32) {
        (self.free_time, self.luck_num, self.cycle)
    }

    /// `PrayModel.addLuckNum(addNum, maxLuckNum)`（`:70-80`）。
    ///
    /// ⚠ 先判后加：到上限后**下一次**才回到 `addNum` 并 `cycle+1`。
    #[must_use]
    pub fn add_luck_num(&self, add_num: i32, max_luck_num: i32) -> Self {
        let mut next = self.clone();
        if next.luck_num >= max_luck_num {
            next.luck_num = add_num;
            next.cycle += 1;
        } else {
            next.luck_num += add_num;
        }
        next.total_times += 1;
        next
    }
}

/// 单抽 / 十连的选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrayTimes {
    /// 单抽（`times == 1`）。
    One,
    /// 十连（`times != 1`，**任何非 1 值**都走这条）。
    Ten,
}

impl PrayTimes {
    /// `req.getTimes() == 1 ? prayOneTime : prayTenTimes`。
    #[must_use]
    pub const fn from_times(times: i32) -> Self {
        if times == 1 {
            Self::One
        } else {
            Self::Ten
        }
    }

    /// 实际抽几次。
    #[must_use]
    pub const fn draws(self) -> i32 {
        match self {
            Self::One => 1,
            Self::Ten => 10,
        }
    }
}

/// 单抽是否免费（`prayOneTime` 的第一段）。
#[must_use]
pub fn is_free(state: &PrayState, today_zero: i64) -> bool {
    state.free_time < today_zero
}

/// 一次 `doPray` 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawOutcome {
    /// 出核心奖励（`coreReward`），并需要发全服公告。
    Core,
    /// 走掉落表（`DropUtils.rand(dropId)`）。
    Drop,
}

/// `doPray(player, special, prayModel, prayDB)` 的判定（`PrayMessageHandler.java:100-112`）。
///
/// `special` 为 `first_reward <= 0 && times < 10`；
/// `first_hit` 为 `isHitForTenThousand(3000)` 的结果。
#[must_use]
pub fn draw_verdict(special: bool, first_hit: bool, luck_num: i32, luck_max: i32) -> DrawOutcome {
    if special && first_hit {
        return DrawOutcome::Core;
    }
    if luck_num >= luck_max {
        DrawOutcome::Core
    } else {
        DrawOutcome::Drop
    }
}

/// `PrayDBCache.getByServerOpenDay(openDay)`（`PrayDBCache.java`）。
///
/// ⚠ 列表为空时原版 `this.list.get(0)` 直接抛 `IndexOutOfBoundsException`。
/// 这里返回 `None`（只在畸形配置下分叉）。
///
/// 实测 `F-焚香祈愿-Pray.csv` 只有 1 行（`day=1`），所以开服第 0 天返回 `None`。
#[must_use]
pub fn by_open_day(list: &[PrayTemplate], open_day: i32) -> Option<&PrayTemplate> {
    let first = list.first()?;
    if first.day > open_day {
        return None;
    }
    let mut pick = first;
    for t in &list[1..] {
        if t.day <= open_day {
            pick = t;
        }
    }
    Some(pick)
}

/// 公告栏项（`NoticeData`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// `name`：玩家名。
    pub name: String,
    /// `rewards`：`(id, num)`。
    pub rewards: Vec<(i32, i64)>,
}

/// `PrayModule.handleEvent` 的公告栏追加（`:17-32`）。
///
/// 超过 [`NOTICE_LIMIT`] 时**从头删**（`remove(0)`）。
pub fn push_notice(list: &mut Vec<Notice>, n: Notice) {
    list.push(n);
    while list.len() > NOTICE_LIMIT {
        list.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PrayState {
        PrayState::default()
    }

    fn tpl() -> PrayTemplate {
        PrayTemplate {
            id: 1,
            day: 1,
            one_cost: vec![(263, 1)],
            ten_cost: vec![(263, 9)],
            luck_num: 150,
            every_luck_num: 1,
            drop_id: 151,
            core_reward: vec![(262, 1)],
        }
    }

    // ── 免费判定 ──

    #[test]
    fn first_pull_is_free() {
        // free_time 初始 0，today_zero 是正数 → free
        assert!(is_free(&state(), 1_700_000_000_000));
    }

    #[test]
    fn same_day_second_pull_costs() {
        let s = PrayState {
            free_time: 1_700_000_000_000,
            ..state()
        };
        assert!(!is_free(&s, 1_700_000_000_000), "同一天不免费");
    }

    #[test]
    fn next_day_is_free_again() {
        let s = PrayState {
            free_time: 1_700_000_000_000,
            ..state()
        };
        assert!(is_free(&s, 1_700_086_400_000), "隔天又免费");
    }

    // ── 次数分派 ──

    #[test]
    fn times_one_is_single() {
        assert_eq!(PrayTimes::from_times(1), PrayTimes::One);
        assert_eq!(PrayTimes::One.draws(), 1);
    }

    #[test]
    fn any_other_times_is_ten() {
        // ⚠ `times != 1` 全走十连（包括 0 和负数）
        assert_eq!(PrayTimes::from_times(10), PrayTimes::Ten);
        assert_eq!(PrayTimes::from_times(0), PrayTimes::Ten);
        assert_eq!(PrayTimes::from_times(-5), PrayTimes::Ten);
        assert_eq!(PrayTimes::Ten.draws(), 10);
    }

    // ── 抽卡判定 ──

    #[test]
    fn special_and_hit_gives_core() {
        assert_eq!(draw_verdict(true, true, 0, 150), DrawOutcome::Core);
    }

    #[test]
    fn special_miss_falls_through_to_luck_check() {
        // special 但没命中 → 看幸运值；0 < 150 → 走掉落
        assert_eq!(draw_verdict(true, false, 0, 150), DrawOutcome::Drop);
    }

    #[test]
    fn luck_full_gives_core() {
        assert_eq!(draw_verdict(false, false, 150, 150), DrawOutcome::Core);
    }

    #[test]
    fn luck_over_max_also_gives_core() {
        // addLuckNum 的先判后加会短暂留下 >= max 的状态
        assert_eq!(draw_verdict(false, false, 160, 150), DrawOutcome::Core);
    }

    #[test]
    fn not_special_not_full_uses_drop_table() {
        assert_eq!(draw_verdict(false, false, 100, 150), DrawOutcome::Drop);
    }

    #[test]
    fn special_takes_priority_over_luck() {
        // special 且命中 → Core（不看幸运值）
        assert_eq!(draw_verdict(true, true, 160, 150), DrawOutcome::Core);
    }

    // ── 幸运值 ──

    #[test]
    fn add_luck_num_accumulates_below_max() {
        let s = PrayState {
            luck_num: 10,
            ..state()
        };
        let n = s.add_luck_num(1, 150);
        assert_eq!(n.luck_num, 11);
        assert_eq!(n.cycle, 0);
        assert_eq!(n.total_times, 1);
    }

    #[test]
    fn add_luck_num_wraps_at_max_on_next_call() {
        // ⚠ 先判后加：luck=150 时这次才回落到 addNum
        let s = PrayState {
            luck_num: 150,
            cycle: 0,
            total_times: 0,
            ..state()
        };
        let n = s.add_luck_num(1, 150);
        assert_eq!(n.luck_num, 1, "回到 addNum");
        assert_eq!(n.cycle, 1, "cycle +1");
        assert_eq!(n.total_times, 1);
    }

    #[test]
    fn add_luck_num_does_not_wrap_at_max_minus_one() {
        let s = PrayState {
            luck_num: 149,
            ..state()
        };
        let n = s.add_luck_num(1, 150);
        assert_eq!(n.luck_num, 150, "149+1=150 还没超");
        assert_eq!(n.cycle, 0);
    }

    // ── 开服天数选表 ──

    #[test]
    fn by_open_day_below_first_returns_none() {
        let t = tpl();
        let list = [t];
        assert!(by_open_day(&list, 0).is_none(), "day=1，开服 0 天拿不到");
    }

    #[test]
    fn by_open_day_matches() {
        let t = tpl();
        let list = [t];
        let got = by_open_day(&list, 1).expect("开服 1 天");
        assert_eq!(got.id, 1);
    }

    #[test]
    fn by_open_day_picks_latest_matching() {
        let a = PrayTemplate {
            id: 1,
            day: 1,
            ..tpl()
        };
        let b = PrayTemplate {
            id: 2,
            day: 7,
            ..tpl()
        };
        let list = vec![a, b];
        assert_eq!(by_open_day(&list, 3).map(|t| t.id), Some(1));
        assert_eq!(by_open_day(&list, 7).map(|t| t.id), Some(2));
        assert_eq!(by_open_day(&list, 99).map(|t| t.id), Some(2));
    }

    #[test]
    fn by_open_day_empty_list() {
        // 原版会 IndexOutOfBoundsException
        assert!(by_open_day(&[], 1).is_none());
    }

    // ── 公告栏 ──

    #[test]
    fn notice_keeps_last_five() {
        let mut list = Vec::new();
        for i in 0..7 {
            push_notice(
                &mut list,
                Notice {
                    name: format!("p{i}"),
                    rewards: vec![(1, 1)],
                },
            );
        }
        assert_eq!(list.len(), NOTICE_LIMIT);
        assert_eq!(list[0].name, "p2", "最早的两条被挤掉");
        assert_eq!(list[4].name, "p6");
    }

    // ── 模板 ──

    #[test]
    fn core_reward_id_is_first() {
        assert_eq!(tpl().core_reward_id(), Some(262));
    }

    #[test]
    fn core_reward_id_none_when_empty() {
        let t = PrayTemplate {
            core_reward: vec![],
            ..tpl()
        };
        assert_eq!(t.core_reward_id(), None);
    }

    #[test]
    fn to_msg_parts_three_fields() {
        let s = PrayState {
            free_time: 111,
            luck_num: 22,
            cycle: 3,
            ..state()
        };
        assert_eq!(s.to_msg_parts(), (111, 22, 3));
    }
}
