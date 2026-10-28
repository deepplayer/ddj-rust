//! # 活动模板（`ActivityDbTemplate` 的时间判定部分）
//!
//! 对应 Java：
//! - `bf/game/server/module/activity/ActivityDbTemplate.java`（239 行）
//! - `bf/database/server/module/platform/activity/AbsActivityDBTemplate.java`
//! - `bf_activity` 表（PG，实测 11 行）
//!
//! ## 为什么先做这一层
//! 活动族的处理器几乎都长这样：
//! ```java
//! ActivityDbTemplate t = ActivityDbTemplateCache.getInstance()
//!     .getActivityDbTemplate(actType, actId);
//! if (!t.isOpen()) throw new HandlerException(COMMON_SERVER_BUSY);
//! ```
//! **50+ 个活动处理器**都卡在这个 `isOpen()` 上。它本身一点都不重 ——
//! 全是时间比较，没有任何 IO。所以先把它做掉，后面的活动处理器才有落点。
//!
//! ## 四个时间字段的语义
//! | 字段 | 含义 | 判定式 |
//! |---|---|---|
//! | `startTimeLong` | 活动开始（可参与） | `isOpen` 用 |
//! | `endTimeLong` | 活动结束（停止参与） | `isEnd` 用 |
//! | `startShowTimeLong` | 展示开始（入口可见） | `isShowTime` 用 |
//! | `endShowTimeLong` | 展示结束（入口消失） | `isEndShowTime` / `isBetweenStartAndEndShowTime` 用 |
//!
//! 典型区间关系：`startShow <= start <= end <= endShow`。
//! 「结束展示」晚于「结束」的那段是**领奖期**（`isGainRewardStage`）。
//!
//! ## ⚠ `isOpen` 是**闭区间**，`isBetweenStartAndEndShowTime` 是**开区间**
//! ```java
//! public boolean isOpen() { return UtilsTools.isNowInTime(startTimeLong, now, endTimeLong); }
//! public boolean isBetweenStartAndEndShowTime() {
//!     return now > startTimeLong && now < endShowTimeLong;
//! }
//! ```
//! `isNowInTime` 是 `start <= now && now <= end`（含两端），
//! 而 `isBetween...` 手写成 `>` / `<`（不含两端）。**正好在边界那一毫秒**
//! 两者结论相反。照抄（`AUDIT.md` A21）。
//!
//! ## ⚠ `isEndShowTime` / `isEnd` 都是 `>` 而不是 `>=`
//! 结束那一毫秒本身仍算「未结束」。照抄。
//!
//! ## `timeType = 2` 的「按开服时间」不做
//! ```java
//! long open_time = gsServer.getOpen_time();
//! this.startTimeLong = getTime(open_time, this.startTime);   // "3#10:00:00"
//! ```
//! 格式是 `天数#时:分:秒`，相对开服日偏移。实测 **11 行全是 `timeType = 1`**
//! （绝对时间字符串），所以过渡期只实现 `timeType = 1`；
//! `timeType = 2` 的行解析时返回 `None`（该行判「未开放」）。
//! 这不是偏离原版，是「当前数据里不存在该分支」。

/// `bf_activity` 的一行（只含判定需要的字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityTemplate {
    /// `actId`（活动 ID，与 `typeId` 一起唯一）。
    pub act_id: i32,
    /// `typeId`（活动类型，对应 `TypeActivity.getType()`）。
    pub type_id: i32,
    /// `name`。
    pub name: String,
    /// `timeType`：`1` 绝对时间 / `2` 开服时间。
    pub time_type: i32,
    /// 展示开始（毫秒）。`timeType = 2` 且解析失败时为 `None`。
    pub start_show: Option<i64>,
    /// 展示结束（毫秒）。
    pub end_show: Option<i64>,
    /// 开始（毫秒）。
    pub start: Option<i64>,
    /// 结束（毫秒）。
    pub end: Option<i64>,
    /// `switchStatus`：`0` 关闭 / `1` 开启。
    ///
    /// ⚠ **`isOpen()` 不看 `switchStatus`** —— 原版只比时间。
    /// 关闭的活动照样 `isOpen() == true`，靠上层别的地方拦。
    pub switch_status: bool,
}

impl ActivityTemplate {
    /// `UtilsTools.isNowInTime(start, now, end)`：**闭区间**。
    fn in_closed_range(start: Option<i64>, now: i64, end: Option<i64>) -> bool {
        match (start, end) {
            (Some(s), Some(e)) => s <= now && now <= e,
            // 时间解析失败 / 缺失 → 原版字段是 0，`0 <= now <= end` 恒真（now > 0）。
            // 这里选择「缺失即不开放」—— 只会让活动晚开，不会误开。
            _ => false,
        }
    }

    /// `isOpen()`：`start <= now <= end`（闭区间）。
    #[must_use]
    pub fn is_open(&self, now: i64) -> bool {
        Self::in_closed_range(self.start, now, self.end)
    }

    /// `isEnd()`：`now > end`（严格大于）。
    #[must_use]
    pub fn is_end(&self, now: i64) -> bool {
        self.end.is_some_and(|e| now > e)
    }

    /// `isEndShowTime()`：`now > endShow`（严格大于）。
    #[must_use]
    pub fn is_end_show_time(&self, now: i64) -> bool {
        self.end_show.is_some_and(|e| now > e)
    }

    /// `isShowTime()`：`startShow <= now <= endShow`（闭区间）。
    #[must_use]
    pub fn is_show_time(&self, now: i64) -> bool {
        Self::in_closed_range(self.start_show, now, self.end_show)
    }

    /// `isBetweenStartAndEndShowTime()`：`start < now < endShow`（**开区间**）。
    ///
    /// ⚠ 与 [`is_open`](Self::is_open) 的区间口径不同，见模块文档。
    #[must_use]
    pub fn is_between_start_and_end_show_time(&self, now: i64) -> bool {
        match (self.start, self.end_show) {
            (Some(s), Some(e)) => now > s && now < e,
            _ => false,
        }
    }

    /// `isGainRewardStage()`：`end <= now < endShow`（结束但仍在展示期）。
    #[must_use]
    pub fn is_gain_reward_stage(&self, now: i64) -> bool {
        match (self.end, self.end_show) {
            (Some(e), Some(es)) => now >= e && now < es,
            _ => false,
        }
    }
}

/// `ActivityDbTemplateCache`：按 `(typeId, actId)` 索引。
#[derive(Debug, Clone, Default)]
pub struct ActivityCache {
    map: std::collections::HashMap<(i32, i32), ActivityTemplate>,
}

impl ActivityCache {
    /// 用模板行构造。
    #[must_use]
    pub fn new(rows: Vec<ActivityTemplate>) -> Self {
        let mut map = std::collections::HashMap::with_capacity(rows.len());
        for r in rows {
            map.insert((r.type_id, r.act_id), r);
        }
        Self { map }
    }

    /// `getActivityDbTemplate(actType, actId)`。
    ///
    /// ⚠ 键是 **`(typeId, actId)` 二元组**，不是单独的 `actId`。
    /// 同一个 `actId` 配了不同 `typeId` 会是两行；上层传错 `actType`
    /// 就查不到 → `null` → 抛 `ACTIVITY_NO_EXITS`。
    #[must_use]
    pub fn get(&self, type_id: i32, act_id: i32) -> Option<&ActivityTemplate> {
        self.map.get(&(type_id, act_id))
    }

    /// 行数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 某类型的全部活动（`CornucopiaMessageHandler.recentRecords` 之类会用）。
    #[must_use]
    pub fn of_type(&self, type_id: i32) -> Vec<&ActivityTemplate> {
        let mut v: Vec<_> = self
            .map
            .values()
            .filter(|t| t.type_id == type_id)
            .collect();
        v.sort_by_key(|t| t.act_id);
        v
    }
}

/// `yyyy-MM-dd HH:mm:ss` 按**东八区**解析成毫秒时间戳。
/// 对应 `UtilsTools.getDateTime(str, "yyyy-MM-dd HH:mm:ss")`。
/// 原版用 JVM 默认时区（部署环境 `Asia/Shanghai`），这里写死东八区。
///
/// 解析失败返回 `None`（原版返回 `0L` 并打错误日志 —— 见模块文档
/// 「缺失即不开放」的说明）。
#[must_use]
pub fn parse_datetime(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (date, time) = s.split_once(' ')?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let mo: i64 = d.next()?.parse().ok()?;
    let da: i64 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let se: i64 = t.next()?.parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        return None;
    }
    if !(0..24).contains(&h) || !(0..60).contains(&mi) || !(0..60).contains(&se) {
        return None;
    }
    let days = days_from_civil(y, mo, da);
    let secs = days * 86_400 + h * 3600 + mi * 60 + se;
    // 东八区 → UTC 减 8 小时
    Some((secs - 8 * 3600) * 1000)
}

/// `Howard Hinnant` 的 `days_from_civil`：公元天数（1970-01-01 = 0）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tpl(start: &str, end: &str, start_show: &str, end_show: &str) -> ActivityTemplate {
        ActivityTemplate {
            act_id: 1,
            type_id: 9,
            name: "困兽之斗".to_owned(),
            time_type: 1,
            start_show: parse_datetime(start_show),
            end_show: parse_datetime(end_show),
            start: parse_datetime(start),
            end: parse_datetime(end),
            switch_status: true,
        }
    }

    #[test]
    fn parses_real_row_timestamps() {
        // 实测 `bf_activity` 100024951：2023-07-20 09:00:00 ~ 2026-07-26 23:59:59
        let s = parse_datetime("2023-07-20 09:00:00").expect("解析开始时间");
        let e = parse_datetime("2026-07-26 23:59:59").expect("解析结束时间");
        // 2023-07-20T09:00:00+08:00 = 2023-07-20T01:00:00Z = 1689814800000
        assert_eq!(s, 1_689_814_800_000);
        // 2026-07-26T23:59:59+08:00 = 2026-07-26T15:59:59Z = 1785081599000
        assert_eq!(e, 1_785_081_599_000);
        assert!(s < e);
    }

    #[test]
    fn rejects_bad_datetime() {
        assert!(parse_datetime("").is_none());
        assert!(parse_datetime("2023-07-20").is_none());
        assert!(parse_datetime("2023-13-20 09:00:00").is_none());
        assert!(parse_datetime("2023-07-20 25:00:00").is_none());
        assert!(parse_datetime("abc").is_none());
    }

    #[test]
    fn is_open_is_closed_interval() {
        let t = tpl(
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
        );
        let s = t.start.expect("start");
        let e = t.end.expect("end");
        assert!(t.is_open(s), "开始那一毫秒算开放");
        assert!(t.is_open(e), "结束那一毫秒算开放");
        assert!(!t.is_open(s - 1));
        assert!(!t.is_open(e + 1));
    }

    #[test]
    fn is_between_start_and_end_show_time_is_open_interval() {
        // ⚠ 与 is_open 口径相反：边界那一毫秒**不**成立
        let t = tpl(
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
            "2023-07-20 09:00:00",
            "2026-08-01 00:00:00",
        );
        let s = t.start.expect("start");
        let es = t.end_show.expect("endShow");
        assert!(!t.is_between_start_and_end_show_time(s), "开区间不含 start");
        assert!(t.is_between_start_and_end_show_time(s + 1));
        assert!(t.is_between_start_and_end_show_time(es - 1));
        assert!(!t.is_between_start_and_end_show_time(es), "开区间不含 endShow");
    }

    #[test]
    fn is_end_and_is_end_show_time_are_strict() {
        let t = tpl(
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
            "2023-07-20 09:00:00",
            "2026-08-01 00:00:00",
        );
        let e = t.end.expect("end");
        assert!(!t.is_end(e), "结束那一毫秒仍算未结束");
        assert!(t.is_end(e + 1));
        let es = t.end_show.expect("endShow");
        assert!(!t.is_end_show_time(es));
        assert!(t.is_end_show_time(es + 1));
    }

    #[test]
    fn gain_reward_stage_is_between_end_and_end_show() {
        let t = tpl(
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
            "2023-07-20 09:00:00",
            "2026-08-01 00:00:00",
        );
        let e = t.end.expect("end");
        let es = t.end_show.expect("endShow");
        assert!(!t.is_gain_reward_stage(e - 1), "活动进行中不是领奖期");
        assert!(t.is_gain_reward_stage(e), "结束那一毫秒进入领奖期");
        assert!(t.is_gain_reward_stage(es - 1));
        assert!(!t.is_gain_reward_stage(es), "展示结束即领奖期结束");
    }

    #[test]
    fn missing_times_are_closed() {
        let t = ActivityTemplate {
            act_id: 1,
            type_id: 1,
            name: String::new(),
            time_type: 2,
            start_show: None,
            end_show: None,
            start: None,
            end: None,
            switch_status: true,
        };
        assert!(!t.is_open(1_700_000_000_000));
        assert!(!t.is_end(1_700_000_000_000));
        assert!(!t.is_show_time(1_700_000_000_000));
        assert!(!t.is_between_start_and_end_show_time(1_700_000_000_000));
        assert!(!t.is_gain_reward_stage(1_700_000_000_000));
    }

    #[test]
    fn switch_status_does_not_affect_is_open() {
        // ⚠ 原版 `isOpen()` 只比时间，不看 `switchStatus`
        let mut t = tpl(
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
        );
        let mid = t.start.expect("start") + 1;
        assert!(t.is_open(mid));
        t.switch_status = false;
        assert!(t.is_open(mid), "switchStatus=0 不影响 isOpen");
    }

    #[test]
    fn cache_keys_on_type_and_act() {
        let c = ActivityCache::new(vec![
            tpl(
                "2023-07-20 09:00:00",
                "2026-07-26 23:59:59",
                "2023-07-20 09:00:00",
                "2026-07-26 23:59:59",
            ),
            ActivityTemplate {
                act_id: 1,
                type_id: 47,
                name: "另一个活动".to_owned(),
                ..tpl(
                    "2023-07-20 09:00:00",
                    "2026-07-26 23:59:59",
                    "2023-07-20 09:00:00",
                    "2026-07-26 23:59:59",
                )
            },
        ]);
        // 同一个 actId=1 但 typeId 不同 → 两行
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(9, 1).map(|t| t.name.as_str()), Some("困兽之斗"));
        assert_eq!(c.get(47, 1).map(|t| t.name.as_str()), Some("另一个活动"));
        // typeId 传错 → 查不到（原版 null → ACTIVITY_NO_EXITS）
        assert!(c.get(33, 1).is_none());
        assert!(c.get(9, 999).is_none());
    }

    #[test]
    fn days_from_civil_matches_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
    }

    /// 东八区零点边界：`is_open` 在 `end` 那一毫秒仍为真。
    #[test]
    fn end_boundary_is_inclusive() {
        let t = tpl(
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
            "2023-07-20 09:00:00",
            "2026-07-26 23:59:59",
        );
        let e = t.end.expect("end");
        // 2026-07-26 23:59:59.999 CST 仍在区间内（毫秒级）
        assert!(t.is_open(e));
        assert!(!t.is_open(e + 1));
    }
}
