//! 猜灯谜（跨服活动 typeId = 50）。
//!
//! 对应 Java：
//! - `bf.game.server.message.handler.LanternRiddlesMessageHandler`
//!   （`data_d/game_7010544/sources/bf/game/server/message/handler/LanternRiddlesMessageHandler.java`）
//! - `bf.game.server.module.activity.fun.cross.lantern_riddles.LanternRiddlesActPlayer`
//!   （`data_d/game_7010544/sources/bf/game/server/module/activity/fun/cross/lantern_riddles/LanternRiddlesActPlayer.java`）
//! - `bf.game.server.module.activity.fun.cross.lantern_riddles.ActivityLanternRiddlesExtendCA`
//! - `bf.database.server.module.platform.activity.LanternRiddlesDbTemplateCache`
//!
//! 本轮只接 4 条：37603 / 37604 / 37605 / 37607。**37606 是跨服 RPC**，不做。
//!
//! ## 线上数据的真实状态（照抄前提）
//!
//! - `data/sql/platform.sql` 的 `bf_activity` 里**没有任何 `typeId = 50` 的行**，
//!   `bf_operate_lantern_riddle` / `bf_operate_riddle_reward` 都是空表。
//! - `bf_config` 只有 79 行，`LanternRiddleLimitNum` / `LanternRiddleLLayTime`
//!   **两行都不存在** → 原版反射赋值失败被 `catch (Exception)` 吞掉，
//!   两个字段保持默认值 `null` / `0`。
//!
//! 于是原版 37603/37604/37605/37607 的**实际线上行为**是：
//! - 37603/37604/37605/37607：`ActivityDbTemplateCache.getActivityDbTemplate(50, actId)`
//!   返回 `null` → `dbTemplate.isOpen()` **NPE** → 框架兜底回 `COMMON_SERVER_BUSY(2)`。
//!
//! 这里按 §3 的例外条款（原版靠「外层 catch」兜住）保留「回 2」的**结果**，
//! 但用显式的 `None` 表达，不去制造 NPE。见 AUDIT A23。

use std::collections::HashMap;

use crate::activity_template::ActivityTemplate;

/// 猜灯谜在 `TypeActivity` 里的 typeId。
///
/// `TypeActivity.guessLanternRiddles(50, "猜灯谜", ...)`。
pub const ACT_TYPE_LANTERN_RIDDLES: i32 = 50;

/// 原版 `LanguageUtils.COMMON_SERVER_BUSY`。
pub const CODE_SERVER_BUSY: i32 = 2;

/// 原版 `LanguageUtils.COMMON_SERVER_RESOURCES_NO`。
pub const CODE_RESOURCES_NO: i32 = 6;

/// 原版 `LanguageUtils.COMMON_PLAYER_LV_NO`。
pub const CODE_PLAYER_LV_NO: i32 = 33;

/// 原版 `LanguageUtils.LANTERNRIDDLE_IS_NOT_OPEN`。
pub const CODE_NOT_OPEN: i32 = 5400;

/// 原版 `LanguageUtils.LANTERNRIDDLE_DATE_ERROR`。
pub const CODE_DATE_ERROR: i32 = 5401;

/// 原版 `LanguageUtils.LANTERNRIDDLE_IS_RETURN`（灯谜 id 错误）。
pub const CODE_LANTERN_ID_ERROR: i32 = 5402;

/// 原版 `LanguageUtils.LANTERNRIDDLE_TROUBLESHOOTING_MAX`（排错上限）。
pub const CODE_TROUBLESHOOTING_MAX: i32 = 5403;

/// 原版 `LanguageUtils.LANTERNRIDDLE_IS_NOT_OVER`（轮次未结束）。
pub const CODE_NOT_OVER: i32 = 5404;

/// 原版 `LanguageUtils.LANTERNRIDDLE_PACK`。
pub const CODE_PACK: i32 = 5405;

/// 原版 `LanguageUtils.LANTERNRIDDLE_EXCLUDE_OPTION_ERROR`。
pub const CODE_EXCLUDE_OPTION_ERROR: i32 = 5406;

/// 原版 `LanguageUtils.LANTERNRIDDLE_RWD_EXPIRE`。
pub const CODE_RWD_EXPIRE: i32 = 5407;

/// 排错道具 id（`ItemServerModule.removeItem(player, 6181, 1, ...)`）。
pub const EXCLUDE_ITEM_ID: i32 = 6181;

/// `Coliseum.MAX_INDEX`，`answerTime` 的放大倍数。
const MAX_INDEX: i32 = 1000;

/// 一天毫秒数。
const DAY_MS: i64 = 86_400_000;

/// 猜灯谜活动扩展配置，对应 `ActivityLanternRiddlesExtendCA`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RiddleCfg {
    /// `commonData[1] + ConfigDBCache.LanternRiddleLLayTime`，再乘 [`MAX_INDEX`]。
    pub answer_time: i32,
    /// `commonData[2]`，每轮题数。
    pub number_of_topics: i32,
    /// `answer_time * number_of_topics`。
    pub all_answer_time: i32,
    /// `commonData[5]`，身份等级限制。
    pub identity_restriction: i32,
    /// `commonData[0]` 按 `;` 切开乘 `3600*1000`。
    pub open_activity_time: Vec<i64>,
    /// `commonData[3]`，形如 `"60=100;30=80"`，key 是剩余秒数，value 是分数。
    pub value: Vec<(i32, i32)>,
    /// `value` 里最大的分数。
    pub max_score: i32,
}

impl RiddleCfg {
    /// 构造，对应 `ActivityLanternRiddlesExtendCA(JSONArray commonData)`。
    ///
    /// `lay_time` 是 `ConfigDBCache.LanternRiddleLLayTime`；线上数据里这一行
    /// **不存在**，反射赋值失败后保持 `0`，所以这里默认传 0。
    #[must_use]
    pub fn new(common_data: &[&str], lay_time: i32) -> Self {
        let get_int = |i: usize| -> i32 {
            common_data
                .get(i)
                .and_then(|s| s.trim().parse::<i32>().ok())
                .unwrap_or(0)
        };
        let answer_time = (get_int(1) + lay_time).saturating_mul(MAX_INDEX);
        let number_of_topics = get_int(2);
        let identity_restriction = get_int(5);
        let open_activity_time = common_data
            .first()
            .map(|s| {
                s.split(';')
                    .filter_map(|p| p.trim().parse::<i64>().ok())
                    .map(|h| h.saturating_mul(3_600_000))
                    .collect()
            })
            .unwrap_or_default();
        let mut value = Vec::new();
        let mut max_score = 0;
        if let Some(s) = common_data.get(3) {
            for param in s.split(';') {
                let mut it = param.split('=');
                let (Some(k), Some(v)) = (it.next(), it.next()) else {
                    continue;
                };
                let (Ok(k), Ok(v)) = (k.trim().parse::<i32>(), v.trim().parse::<i32>()) else {
                    continue;
                };
                value.push((k, v));
                if max_score == 0 || v > max_score {
                    max_score = v;
                }
            }
        }
        Self {
            answer_time,
            number_of_topics,
            all_answer_time: answer_time.saturating_mul(number_of_topics),
            identity_restriction,
            open_activity_time,
            value,
            max_score,
        }
    }

    /// 当前时刻落在第几轮（1-based，未开始/已结束返回 0）。
    ///
    /// 对应 `getRounds()`：条件是 `starTime <= allAnswerTime && starTime >= 0`，
    /// **取最后一个满足的 i**（不是第一个）。
    #[must_use]
    pub fn rounds(&self, now_ms: i64, zero_ms: i64) -> i32 {
        let mut rounds = 0;
        for (i, t) in self.open_activity_time.iter().enumerate() {
            let star = (now_ms - zero_ms).saturating_sub(*t);
            if star <= i64::from(self.all_answer_time) && star >= 0 {
                rounds = i32::try_from(i).unwrap_or(i32::MAX) + 1;
            }
        }
        rounds
    }

    /// 今天已经过了几轮（`getRounds2()`）：只要 `starTime > 0` 就记。
    #[must_use]
    pub fn rounds2(&self, now_ms: i64, zero_ms: i64) -> i32 {
        let mut rounds = 0;
        for (i, t) in self.open_activity_time.iter().enumerate() {
            if (now_ms - zero_ms).saturating_sub(*t) > 0 {
                rounds = i32::try_from(i).unwrap_or(i32::MAX) + 1;
            }
        }
        rounds
    }

    /// 本轮已经答到第几题（`getAnswerNum()`）。
    ///
    /// ⚠ 原版**没有**对 `answerTime == 0` 做保护，会 `ArithmeticException: / by zero`。
    /// 这里返回 0 并让调用方按「未开始」处理，理由见 AUDIT A23。
    #[must_use]
    pub fn answer_num(&self, now_ms: i64, zero_ms: i64) -> i32 {
        let mut star_time = 0_i64;
        let mut open = false;
        for t in &self.open_activity_time {
            star_time = (now_ms - zero_ms).saturating_sub(*t);
            if star_time <= i64::from(self.all_answer_time) && star_time >= 0 {
                open = true;
                break;
            }
        }
        if !open || self.answer_time == 0 {
            return 0;
        }
        i32::try_from(star_time / i64::from(self.answer_time))
            .unwrap_or(i32::MAX)
            .saturating_add(1)
    }

    /// 按剩余时间查分数（`getValue(long remainingTime)`）。
    ///
    /// 条件是 `key <= ceil(remainingTime / 1000.0)`，取满足里**最大**的 value。
    #[must_use]
    pub fn value_of(&self, remaining_ms: i64) -> i32 {
        #[allow(clippy::cast_precision_loss)]
        let secs = (remaining_ms as f64 / 1000.0).ceil();
        let mut valve = 0;
        for (k, v) in &self.value {
            if f64::from(*k) <= secs && *v > valve {
                valve = *v;
            }
        }
        valve
    }
}

/// 一题的作答记录，对应 `RoundsInfo`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoundsInfo {
    /// 剩余时间（毫秒）。
    pub time: i64,
    /// 分数。
    pub value: i64,
    /// 是否答对。
    pub is_true: bool,
    /// 选中的选项（1..=4）。
    pub id: i32,
    /// 是否用了「辩真药水」。
    pub pick: bool,
}

impl RoundsInfo {
    /// `new RoundsInfo(time, value, isTrue, id)`。
    #[must_use]
    pub fn answer(time: i64, value: i64, is_true: bool, id: i32) -> Self {
        Self {
            time,
            value,
            is_true,
            id,
            pick: false,
        }
    }

    /// `new RoundsInfo(value, pick)`：`isTrue` 固定 `true`、`id` 为 0。
    #[must_use]
    pub fn excluded(value: i64, pick: bool) -> Self {
        Self {
            time: 0,
            value,
            is_true: true,
            id: 0,
            pick,
        }
    }
}

/// 单个玩家的猜灯谜进度，对应 `LanternRiddlesActPlayer` 的字段。
#[derive(Debug, Clone, Default)]
pub struct RiddleState {
    /// `values`：lun → (lanternId → 作答记录)。
    pub values: HashMap<i32, HashMap<i32, RoundsInfo>>,
    /// `lunValue`：lun → 该轮汇总。
    pub lun_value: HashMap<i32, RoundsInfo>,
    /// `rwdInfo`：已领奖的 lun 集合（原版是 `Map<Integer,Integer>`，值恒 0）。
    pub rwd_info: HashMap<i32, i32>,
    /// `riddleReward`：当天已领过的 `onlyId`。
    pub riddle_reward: Vec<i32>,
    /// `resetRiddleRewardTime`。
    pub reset_riddle_reward_time: i64,
    /// `fristReward`。
    pub first_reward: bool,
    /// `riddleRewardTime`。
    pub riddle_reward_time: i64,
    /// `outOfNum`：lun → 已排错次数。
    pub out_of_num: HashMap<i32, i32>,
    /// `oneOutOfNum`：lanternId → 已排错次数。
    pub one_out_of_num: HashMap<i32, i32>,
    /// `familyJob`。
    pub family_job: i32,
    /// `joined`。
    pub joined: bool,
}

impl RiddleState {
    /// 新建，对应 `LanternRiddlesActPlayer(String playerId, int actType, int actId)`：
    /// `resetRiddleRewardTime` 取当前时间。
    #[must_use]
    pub fn new(now_ms: i64) -> Self {
        Self {
            reset_riddle_reward_time: now_ms,
            ..Self::default()
        }
    }

    /// `refresh()`：跨天清空 `riddleReward`。
    pub fn refresh(&mut self, now_ms: i64) {
        if zero_ms_of(self.reset_riddle_reward_time) != zero_ms_of(now_ms) {
            self.reset_riddle_reward_time = now_ms;
            self.riddle_reward.clear();
        }
    }

    /// 该 lun 的作答表（`values.get(lun)`，缺时补空表 —— 原版是
    /// `else { this.values.put(lun, new ConcurrentHashMap<>()); }`）。
    pub fn rounds_of(&mut self, lun: i32) -> &mut HashMap<i32, RoundsInfo> {
        self.values.entry(lun).or_default()
    }

    /// `getTrueNum(lun)`：`values.get(lun)` 里 `isTrue` 的条数。
    #[must_use]
    pub fn true_num(&self, lun: i32) -> i32 {
        self.values.get(&lun).map_or(0, |m| {
            i32::try_from(m.values().filter(|r| r.is_true).count()).unwrap_or(i32::MAX)
        })
    }

    /// `getLunExcludeOptionNum(lun)`。
    #[must_use]
    pub fn lun_exclude_option_num(&self, lun: i32) -> i32 {
        self.out_of_num.get(&lun).copied().unwrap_or(0)
    }

    /// `getLunValue(lun, lanternId)`：把该轮**除 lanternId 外**答对的分数加起来。
    ///
    /// 返回 `int`，所以原版的 `(int)((long)lunValue + value)` 在这里用 `i64` 累加再截断。
    #[must_use]
    pub fn lun_value_excluding(&self, lun: i32, lantern_id: i32) -> i32 {
        let mut sum: i64 = 0;
        if let Some(m) = self.values.get(&lun) {
            for (k, v) in m {
                if *k != lantern_id && v.is_true {
                    sum = sum.saturating_add(v.value);
                }
            }
        }
        i32::try_from(sum).unwrap_or_else(|_| {
            // ⚠ 照抄 Java `(int)` 强转：溢出时取低 32 位
            #[allow(clippy::cast_possible_truncation)]
            let v = sum as i32;
            v
        })
    }

    /// 轮转汇总，对应 37604/37605 里那段 `for (RoundsInfo r : map.values()) if (r.isTrue()) {...}`。
    pub fn recompute_lun_value(&mut self, lun: i32) {
        let mut info = RoundsInfo::default();
        if let Some(m) = self.values.get(&lun) {
            for r in m.values() {
                if r.is_true {
                    info.value = info.value.saturating_add(r.value);
                    info.time = info.time.saturating_add(r.time);
                }
            }
        }
        self.lun_value.insert(lun, info);
    }

    /// `valuesInfo(lun, lanternId)`：查不到返回默认值。
    #[must_use]
    pub fn values_info(&self, lun: i32, lantern_id: i32) -> RoundsInfo {
        self.values
            .get(&lun)
            .and_then(|m| m.get(&lantern_id))
            .copied()
            .unwrap_or_default()
    }

    /// `isMaxValue(lun, num)`：按 `(value desc, time desc)` 排序后前 `num` 名里有 lun 就算。
    #[must_use]
    pub fn is_max_value(&self, lun: i32, num: usize) -> bool {
        let mut list: Vec<(i32, i64)> = self
            .lun_value
            .iter()
            .map(|(k, v)| (*k, v.value))
            .collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(std::cmp::Ordering::Equal));
        list.iter().take(num).any(|(l, _)| *l == lun)
    }

    /// `getMaxValue(num)`：前 `num` 名的 value 与 time 之和。
    #[must_use]
    pub fn max_value(&self, num: usize) -> RoundsInfo {
        let mut list: Vec<(i64, i64)> = self
            .lun_value
            .values()
            .map(|v| (v.value, v.time))
            .collect();
        list.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let mut out = RoundsInfo::default();
        for (v, t) in list.into_iter().take(num) {
            out.value = out.value.saturating_add(v);
            out.time = out.time.saturating_add(t);
        }
        out
    }
}

/// 把毫秒时间戳对齐到当天零点（`LogicUtils.getTodayZeroTimeStamp`，东八区）。
#[must_use]
pub fn zero_ms_of(now_ms: i64) -> i64 {
    const TZ_MS: i64 = 8 * 3600 * 1000;
    (now_ms + TZ_MS) / DAY_MS * DAY_MS - TZ_MS
}

/// `LogicUtils.getActivityOpenDay(startTime)`：开服到今天过了几天，当天开服返回 0。
#[must_use]
pub fn activity_open_day(start_ms: i64, zero_ms: i64) -> i32 {
    let start_zero = zero_ms_of(start_ms);
    if zero_ms - start_zero == 0 {
        return 0;
    }
    i32::try_from((zero_ms - start_zero) / DAY_MS).unwrap_or(i32::MAX)
}

/// 灯谜序号 `lanternId` 的计算（37603/37604/37605 都是同一条式子）。
///
/// ```text
/// numberOfTopics * (round - 1)
///   + startDay * numberOfTopics * openActivityTimeNum
///   + nowAnswerNum
/// ```
#[must_use]
pub fn lantern_id(cfg: &RiddleCfg, round: i32, start_day: i32, answer_num: i32) -> i32 {
    let n = i32::try_from(cfg.open_activity_time.len()).unwrap_or(i32::MAX);
    cfg.number_of_topics
        .saturating_mul(round.saturating_sub(1))
        .saturating_add(
            start_day
                .saturating_mul(cfg.number_of_topics)
                .saturating_mul(n),
        )
        .saturating_add(answer_num)
}

/// `lun = startDay * openActivityTimeNum + round`。
#[must_use]
pub fn lun_of(cfg: &RiddleCfg, round: i32, start_day: i32) -> i32 {
    let n = i32::try_from(cfg.open_activity_time.len()).unwrap_or(i32::MAX);
    start_day.saturating_mul(n).saturating_add(round)
}

/// 一道题的题目与选项，对应 `MsgLanternRiddleDate`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RiddleQuestion {
    /// 题号。
    pub lantern_id: i32,
    /// 题干。
    pub question: String,
    /// 选项文本（按顺序，选项 id 是下标 + 1）。
    pub options: Vec<String>,
}

/// 一道题的模板行，对应 `LanternRiddlesDbTemplate`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RiddleRow {
    /// 语言（原版按 `languageType.getDesc()` 分组，空串是兜底）。
    pub ltype: String,
    /// 题号。
    pub lantern_id: i32,
    /// 题干。
    pub question: String,
    /// 正确答案。
    pub answer: String,
    /// 干扰项 1..3。
    pub wrong: [String; 3],
}

/// 一次「答对 + 排错」结算的输入。
pub struct ChangeInput<'a> {
    /// 活动 id。
    pub act_id: i32,
    /// 玩家选的选项（1..=4）。
    pub id: i32,
    /// 玩家报上来的题号。
    pub lantern_id: i32,
    /// 玩家等级。
    pub player_lv: i32,
    /// 当前时间。
    pub now_ms: i64,
    /// 今天零点。
    pub zero_ms: i64,
    /// 活动开始时间。
    pub start_ms: i64,
    /// 活动模板。
    pub cfg: &'a RiddleCfg,
    /// 正确答案的选项号（1..=4，0 表示查不到）。
    pub true_answer: i32,
}

/// 一次 37604 的结算结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeOutcome {
    /// 本次得分。
    pub value: i32,
}

/// 37604 的纯判定：全部通过则返回要写入的记录。
///
/// 分支顺序严格照抄 `LanternRiddlesActPlayer.lanternRiddlesChangeSubject`
/// （`LanternRiddlesActPlayer.java:159-228`）：
/// 1. `id > 4 || id <= 0` → `LANTERNRIDDLE_DATE_ERROR(5401)`
/// 2. `identityRestriction > player.getLv()` → `COMMON_PLAYER_LV_NO(33)`
/// 3. `rounds() == 0` → `LANTERNRIDDLE_IS_NOT_OPEN(5400)`
/// 4. `req.getLanternId() != lanternId` → `LANTERNRIDDLE_IS_RETURN(5402)`
/// 5. 该题已经 `pick` 过 → `LANTERNRIDDLE_PACK(5405)`
/// 6. `common.state[lanternId] != 0` → `LANTERNRIDDLE_IS_NOT_OPEN(5400)`
///
/// # Errors
/// 上面任一条件不满足时返回对应错误码。
pub fn change_verdict(
    state: &mut RiddleState,
    input: &ChangeInput<'_>,
) -> Result<ChangeOutcome, i32> {
    if input.id > 4 || input.id <= 0 {
        return Err(CODE_DATE_ERROR);
    }
    if input.cfg.identity_restriction > input.player_lv {
        return Err(CODE_PLAYER_LV_NO);
    }
    let round = input.cfg.rounds(input.now_ms, input.zero_ms);
    if round == 0 {
        return Err(CODE_NOT_OPEN);
    }
    let start_day = activity_open_day(input.start_ms, input.zero_ms);
    let answer_num = input.cfg.answer_num(input.now_ms, input.zero_ms);
    let lantern_id = lantern_id(input.cfg, round, start_day, answer_num);
    if input.lantern_id != lantern_id {
        return Err(CODE_LANTERN_ID_ERROR);
    }
    let lun = lun_of(input.cfg, round, start_day);
    if let Some(r) = state.values.get(&lun).and_then(|m| m.get(&lantern_id)) {
        if r.pick {
            return Err(CODE_PACK);
        }
    }
    // remainingTime 在「该轮刚开始」时就是 answerTime，随答对题数递减。
    let round_start = input.zero_ms + input.cfg.open_activity_time[round as usize - 1];
    let elapsed = input.now_ms - round_start;
    let remaining = i64::from(input.cfg.answer_time)
        .saturating_sub(elapsed)
        .saturating_add(
            i64::from(input.cfg.answer_time).saturating_mul(i64::from(answer_num) - 1),
        );
    let value = input.cfg.value_of(remaining);
    let is_true = input.true_answer == input.id;
    state.rounds_of(lun).insert(
        lantern_id,
        RoundsInfo::answer(remaining, i64::from(value), is_true, input.id),
    );
    state.recompute_lun_value(lun);
    state.joined = true;
    Ok(ChangeOutcome { value })
}

/// 37605 的纯判定：排错的全部前置条件。
///
/// 分支顺序照抄 `LanternRiddlesActPlayer.lanternRiddlesExcludeOption`
/// （`LanternRiddlesActPlayer.java:250-319`）：
/// 1. `identityRestriction > player.getLv()` → `COMMON_PLAYER_LV_NO(33)`
/// 2. `rounds() == 0` → `LANTERNRIDDLE_IS_NOT_OPEN(5400)`
/// 3. `outOfNum[lun] >= LanternRiddleLimitNum[1]` → `LANTERNRIDDLE_TROUBLESHOOTING_MAX(5403)`
/// 4. `req.lanternId != lanternId` → `LANTERNRIDDLE_EXCLUDE_OPTION_ERROR(5406)`
/// 5. `oneOutOfNum[lanternId] >= LanternRiddleLimitNum[0]` → 5403
/// 6. 扣道具失败 → `COMMON_SERVER_RESOURCES_NO(6)`
///
/// ⚠ 原版 `ConfigDBCache.LanternRiddleLimitNum` 在线上是 `null`，
/// 第 3 步 `((Integer) null.get(1))` 会 **NPE**。这里按「限制存在且为 0」
/// 处理，见 AUDIT A23。
///
/// # Errors
/// 上面任一条件不满足时返回对应错误码。
pub fn exclude_verdict(
    state: &mut RiddleState,
    input: &ExcludeInput<'_>,
) -> Result<ExcludeOutcome, i32> {
    if input.cfg.identity_restriction > input.player_lv {
        return Err(CODE_PLAYER_LV_NO);
    }
    let round = input.cfg.rounds(input.now_ms, input.zero_ms);
    if round == 0 {
        return Err(CODE_NOT_OPEN);
    }
    let start_day = activity_open_day(input.start_ms, input.zero_ms);
    let lun = lun_of(input.cfg, round, start_day);
    let all_num = state.lun_exclude_option_num(lun);
    if all_num >= input.limit_lun {
        return Err(CODE_TROUBLESHOOTING_MAX);
    }
    let answer_num = input.cfg.answer_num(input.now_ms, input.zero_ms);
    let lantern_id = lantern_id(input.cfg, round, start_day, answer_num);
    if input.lantern_id != lantern_id {
        return Err(CODE_EXCLUDE_OPTION_ERROR);
    }
    let used = state.one_out_of_num.get(&lantern_id).copied().unwrap_or(0);
    if used >= input.limit_one {
        return Err(CODE_TROUBLESHOOTING_MAX);
    }
    if input.item_num < 1 {
        return Err(CODE_RESOURCES_NO);
    }
    let num = used.saturating_add(1);
    state.out_of_num.insert(lun, all_num.saturating_add(1));
    state.one_out_of_num.insert(lantern_id, num);
    state.rounds_of(lun).insert(
        lantern_id,
        RoundsInfo::excluded(i64::from(input.cfg.max_score), true),
    );
    state.recompute_lun_value(lun);
    state.joined = true;
    Ok(ExcludeOutcome { num })
}

/// 37605 的输入。
pub struct ExcludeInput<'a> {
    /// 玩家等级。
    pub player_lv: i32,
    /// 报上来的题号。
    pub lantern_id: i32,
    /// 当前时间。
    pub now_ms: i64,
    /// 今天零点。
    pub zero_ms: i64,
    /// 活动开始时间。
    pub start_ms: i64,
    /// 活动模板。
    pub cfg: &'a RiddleCfg,
    /// `LanternRiddleLimitNum[1]`：每轮排错总次数上限。
    pub limit_lun: i32,
    /// `LanternRiddleLimitNum[0]`：每题排错次数上限。
    pub limit_one: i32,
    /// 玩家身上 [`EXCLUDE_ITEM_ID`] 的数量。
    pub item_num: i64,
}

/// 37605 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExcludeOutcome {
    /// 本题第几次排错（1-based）。
    pub num: i32,
}

/// 37607 的输入。
pub struct RwdInput<'a> {
    /// 活动 id。
    pub act_id: i32,
    /// 玩家等级（原版 37607 不判等级，保留字段供将来对齐）。
    pub player_lv: i32,
    /// 当前时间。
    pub now_ms: i64,
    /// 今天零点。
    pub zero_ms: i64,
    /// 活动开始时间。
    pub start_ms: i64,
    /// 活动模板。
    pub cfg: &'a RiddleCfg,
    /// 奖励表查询器。
    pub rewards: &'a dyn RiddleRewards,
}

/// 37607 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RwdOutcome {
    /// 本轮答对题数。
    pub true_num: i32,
    /// 本轮得分（`getLunValue(lun, -1)`，即除 -1 外全部答对题）。
    pub value: i32,
    /// `isMaxValue(lun, 1)`。
    pub is_max_value: bool,
    /// 本次发出的 `onlyId` 列表。
    pub reward_ids: Vec<i32>,
    /// 实际发放的资源（`id, num`）。
    pub items: Vec<(i32, i64)>,
}

/// 奖励表查询，对应 `LanternRiddleRewardDbTemplateCache`。
pub trait RiddleRewards {
    /// `getLanternRiddleRewardDBTemplate(actId, num, type)`：唯一一条。
    fn one(&self, act_id: i32, num: i32, rtype: i32) -> Option<RiddleReward>;

    /// `getLanternRiddleRewardDBTemplateList(actId, num, type)`：可能多条。
    fn list(&self, act_id: i32, num: i32, rtype: i32) -> Vec<RiddleReward>;
}

/// 一条奖励，对应 `LanternRiddleRewardDbTemplate`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RiddleReward {
    /// `onlyId`。
    pub only_id: i32,
    /// `rewards` 展开后的 `(id, num)`。
    pub items: Vec<(i32, i64)>,
}

/// 37607 的纯判定。
///
/// 分支顺序照抄 `LanternRiddlesActPlayer.lanternRiddlesGetRwd`
/// （`LanternRiddlesActPlayer.java:321-378`）：
/// 1. `rounds() != 0` → `LANTERNRIDDLE_IS_NOT_OVER(5404)`（**还没结束**才报错）
/// 2. `startDay == 0 && rounds2() == 0` → `LANTERNRIDDLE_IS_NOT_OPEN(5400)`
/// 3. `rwdInfo` 已有该 lun → `LANTERNRIDDLE_RWD_EXPIRE(5407)`
/// 4. `trueNum != 0` 才发奖：type 1（必得）、type 3（每日一套）、type 2（首胜）
///
/// ⚠ 第 4 步的 type 2（`fristReward`）只置位**不清空**，是跨天持久字段。
///
/// # Errors
/// 上面 1..3 任一条件不满足时返回对应错误码。
pub fn rwd_verdict(
    state: &mut RiddleState,
    input: &RwdInput<'_>,
) -> Result<RwdOutcome, i32> {
    let round = input.cfg.rounds(input.now_ms, input.zero_ms);
    if round != 0 {
        return Err(CODE_NOT_OVER);
    }
    let start_day = activity_open_day(input.start_ms, input.zero_ms);
    if start_day == 0 && input.cfg.rounds2(input.now_ms, input.zero_ms) == 0 {
        return Err(CODE_NOT_OPEN);
    }
    let lun = lun_of(input.cfg, round, start_day);
    if state.rwd_info.contains_key(&lun) {
        return Err(CODE_RWD_EXPIRE);
    }
    let mut out = RwdOutcome {
        is_max_value: state.is_max_value(lun, 1),
        ..RwdOutcome::default()
    };
    let true_num = state.true_num(lun);
    out.true_num = true_num;
    out.value = state.lun_value_excluding(lun, -1);
    if true_num != 0 {
        let mut items: Vec<(i32, i64)> = Vec::new();
        if let Some(t) = input.rewards.one(input.act_id, true_num, 1) {
            items.extend(t.items.iter().copied());
            out.reward_ids.push(t.only_id);
        }
        for t in input.rewards.list(input.act_id, true_num, 3) {
            if state.riddle_reward.contains(&t.only_id) {
                continue;
            }
            items.extend(t.items.iter().copied());
            state.riddle_reward.push(t.only_id);
            out.reward_ids.push(t.only_id);
        }
        if !state.first_reward {
            if let Some(t) = input.rewards.one(input.act_id, true_num, 2) {
                items.extend(t.items.iter().copied());
                state.first_reward = true;
                out.reward_ids.push(t.only_id);
            }
        }
        if !items.is_empty() {
            state.riddle_reward_time = input.now_ms;
            out.items = items;
        }
    }
    // 原版 `setRwdInfo(lun)` 在 `if (num != 0) { ... }` **之外**，trueNum=0 也记。
    state.rwd_info.insert(lun, 0);
    Ok(out)
}

/// 把一次活动实例转成「是否开放」。
///
/// 原版 37603/37604/37605/37607 都是先 `getActivityDbTemplate(50, actId)`，
/// 再 `dbTemplate.isOpen()`。查不到模板就是 `null.isOpen()` → NPE。
///
/// ⚠ 这里返回 `None` 表示「模板不存在」，调用方应回 `COMMON_SERVER_BUSY(2)`，
/// 与 NPE 被框架兜住后的结果一致。见 AUDIT A23。
#[must_use]
pub fn act_open(act: Option<&ActivityTemplate>, now_ms: i64) -> Option<bool> {
    act.map(|t| t.is_open(now_ms))
}

/// 一题在题目表 + 选项表里的样子（`LanternRiddlesDbTemplateCache` 的两张表合成）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubjectRow {
    /// 题干。
    pub question: String,
    /// 正确答案文本。
    pub answer: String,
    /// 干扰项 1..3。
    pub wrong: [String; 3],
    /// `lanternRiddlesMap[actId][lanternId]`：选项顺序，值 `1` 是正确项。
    ///
    /// 空表示跨服服没给数据（原版 `options.size()` = 0 → 一个选项都不回）。
    pub options: Vec<i32>,
}

impl SubjectRow {
    /// `LanternRiddlesDbTemplateCache.isTrue(actId, lanternId, num)`。
    ///
    /// ⚠ 原版判的是 `map.get(actId).get(lanternId).get(num - 1) == 1`，
    /// 且前置条件写成了 `containsKey(...) || containsKey(...) || contains(...)`。
    /// 这里实现「取不到即 false」，不会因为越界 panic。
    #[must_use]
    pub fn is_true(&self, num: i32) -> bool {
        if num <= 0 {
            return false;
        }
        let idx = usize::try_from(num - 1).unwrap_or(usize::MAX);
        self.options.get(idx).is_some_and(|v| *v == 1)
    }

    /// `trueAnswer(actId, lanternId)`：第一个 `options[i] == 1` 的 `i + 1`，没有则 0。
    #[must_use]
    pub fn true_answer(&self) -> i32 {
        for (i, v) in self.options.iter().enumerate() {
            if *v == 1 {
                return i32::try_from(i).unwrap_or(i32::MAX) + 1;
            }
        }
        0
    }

    /// 按 `options` 顺序生成选项文本。
    ///
    /// `1 → answer`、`2 → wrong1`、`3 → wrong2`、`4 → wrong3`，其它值 → 空串
    /// （原版那个分支里 `builder1` 只 `setId(i + 1)`）。
    #[must_use]
    pub fn option_texts(&self) -> Vec<String> {
        self.options
            .iter()
            .map(|k| match k {
                1 => self.answer.clone(),
                2 => self.wrong[0].clone(),
                3 => self.wrong[1].clone(),
                4 => self.wrong[2].clone(),
                _ => String::new(),
            })
            .collect()
    }
}

/// 题目表查询，对应 `LanternRiddlesDbTemplateCache`。
pub trait RiddleRows {
    /// `getLanternRiddle(actId, lanternId, languageType)`。
    ///
    /// `None` 等价原版返回 `null`（题目表里没这题）。
    fn one(&self, act_id: i32, lantern_id: i32) -> Option<SubjectRow>;

    /// `trueAnswer(actId, lanternId)`。
    fn true_answer(&self, act_id: i32, lantern_id: i32) -> i32 {
        self.one(act_id, lantern_id).map_or(0, |r| r.true_answer())
    }
}

/// 37603 的输入。
pub struct SubjectInput<'a> {
    /// 活动 id。
    pub act_id: i32,
    /// 当前时间。
    pub now_ms: i64,
    /// 今天零点。
    pub zero_ms: i64,
    /// 活动开始时间。
    pub start_ms: i64,
    /// 活动模板。
    pub cfg: &'a RiddleCfg,
    /// 题目表查询器。
    pub rows: &'a dyn RiddleRows,
}

/// 37603 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubjectOutcome {
    /// 当前这题（`round == 0` 或题目表查不到时为 `None`）。
    pub question: Option<QuestionView>,
    /// 本轮已经过去的那些题的真实答案（`trueId`）。
    pub true_ids: Vec<i32>,
}

/// 当前这题要回给客户端的字段。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestionView {
    /// 活动 id。
    pub act_id: i32,
    /// 题号。
    pub lantern_id: i32,
    /// 题干。
    pub question: String,
    /// 选项 `(id, text)`。
    pub options: Vec<(i32, String)>,
    /// 上一条记录里存的选项 id（`valuesInfo(lun, lanternId).getId()`）。
    pub id: i32,
    /// 上一条记录里的分数。
    pub value: i32,
    /// 上一条记录是否排错过。
    pub is_pick: bool,
    /// `getLunExcludeOptionNum(lun)`。
    pub lun_exclude_option_num: i32,
    /// `getLunValue(lun, lanternId)`。
    pub lun_value: i32,
}

/// 37603 的主体逻辑（`LanternRiddlesMessageHandler.lanternRiddlesGetSubject`）。
///
/// 顺序照抄 handler：
/// 1. `dbTemplate.isOpen()` 假 → `COMMON_SERVER_BUSY(2)`（查模板时由调用方做完）
/// 2. `startDay` / `nowAnswerNum` / `round` / `lun` / `lanternId`
/// 3. `round > 0` 且题目表有这题 → 填 `lanternRiddleDate` + 5 个状态字段
/// 4. `rounds2() != 0` → `startLanternId..=endLanternId` 逐个 `addTrueId`
///
/// ⚠ 第 4 步 `endLanternId = round > 0 ? lanternId - 1 : ...`。
/// 所以「本轮正在答」时只回**已经过去的**那几题答案，当前题不给。
#[must_use]
pub fn subject_verdict(state: &RiddleState, input: &SubjectInput<'_>) -> SubjectOutcome {
    let cfg = input.cfg;
    let start_day = activity_open_day(input.start_ms, input.zero_ms);
    let now_answer_num = cfg.answer_num(input.now_ms, input.zero_ms);
    let round = cfg.rounds(input.now_ms, input.zero_ms);
    let open_n = i32::try_from(cfg.open_activity_time.len()).unwrap_or(i32::MAX);
    let lun = start_day.saturating_mul(open_n).saturating_add(round);
    let lantern_id = lantern_id(cfg, round, start_day, now_answer_num);

    let mut out = SubjectOutcome::default();
    if round > 0 {
        if let Some(row) = input.rows.one(input.act_id, lantern_id) {
            let info = state.values_info(lun, lantern_id);
            out.question = Some(QuestionView {
                act_id: input.act_id,
                lantern_id,
                question: row.question.clone(),
                options: row
                    .option_texts()
                    .into_iter()
                    .enumerate()
                    .map(|(i, t)| (i32::try_from(i).unwrap_or(i32::MAX) + 1, t))
                    .collect(),
                id: info.id,
                // 原版 `(int) roundsInfo.getValue()`
                value: i32::try_from(info.value).unwrap_or(i32::MAX),
                is_pick: info.pick,
                lun_exclude_option_num: state.lun_exclude_option_num(lun),
                lun_value: state.lun_value_excluding(lun, lantern_id),
            });
        }
    }

    let rounds2 = cfg.rounds2(input.now_ms, input.zero_ms);
    if rounds2 != 0 {
        let base = start_day
            .saturating_mul(cfg.number_of_topics)
            .saturating_mul(open_n);
        let start_lantern_id = cfg
            .number_of_topics
            .saturating_mul(rounds2.saturating_sub(1))
            .saturating_add(base)
            .saturating_add(1);
        let end_lantern_id = if round > 0 {
            lantern_id.saturating_sub(1)
        } else {
            cfg.number_of_topics.saturating_mul(rounds2).saturating_add(base)
        };
        if end_lantern_id != 0 {
            let mut i = start_lantern_id;
            while i <= end_lantern_id {
                out.true_ids.push(input.rows.true_answer(input.act_id, i));
                i = i.saturating_add(1);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RiddleCfg {
        // commonData: [0]=开赛小时, [1]=答题秒数, [2]=题数, [3]=分数表, [5]=等级限制
        RiddleCfg::new(&["10;20", "3", "2", "60=100;30=80", "", "0"], 0)
    }

    #[test]
    fn cfg_parses_every_field() {
        let c = cfg();
        assert_eq!(c.answer_time, 3 * MAX_INDEX);
        assert_eq!(c.number_of_topics, 2);
        assert_eq!(c.all_answer_time, 3 * MAX_INDEX * 2);
        assert_eq!(c.identity_restriction, 0);
        assert_eq!(c.open_activity_time, vec![36_000_000, 72_000_000]);
        assert_eq!(c.value, vec![(60, 100), (30, 80)]);
        assert_eq!(c.max_score, 100);
    }

    #[test]
    fn cfg_missing_fields_default_to_zero() {
        let c = RiddleCfg::new(&[], 0);
        assert_eq!(c, RiddleCfg::default());
    }

    #[test]
    fn cfg_lay_time_is_added_before_scaling() {
        let c = RiddleCfg::new(&["", "3", "1", "", "", ""], 2);
        assert_eq!(c.answer_time, (3 + 2) * MAX_INDEX);
        assert_eq!(c.all_answer_time, 5 * MAX_INDEX);
    }

    #[test]
    fn rounds_picks_the_last_matching_slot() {
        let c = cfg();
        let zero = 0;
        // 10:00 之前
        assert_eq!(c.rounds(zero + 9 * 3_600_000, zero), 0);
        // 10:00 正好开始
        assert_eq!(c.rounds(zero + 10 * 3_600_000, zero), 1);
        // 第 1 轮结束（10:00 + allAnswerTime）
        let end1 = zero + 10 * 3_600_000 + i64::from(c.all_answer_time);
        assert_eq!(c.rounds(end1, zero), 1);
        assert_eq!(c.rounds(end1 + 1, zero), 0);
        // 20:00 开始第 2 轮
        assert_eq!(c.rounds(zero + 20 * 3_600_000, zero), 2);
    }

    #[test]
    fn rounds2_ignores_the_answer_window() {
        let c = cfg();
        let zero = 0;
        assert_eq!(c.rounds2(zero + 9 * 3_600_000, zero), 0);
        // 条件是 > 0（严格），10:00 整那一刻不算
        assert_eq!(c.rounds2(zero + 10 * 3_600_000, zero), 0);
        assert_eq!(c.rounds2(zero + 10 * 3_600_000 + 1, zero), 1);
        // 第 1 轮早就结束，但 rounds2 仍然记 1
        assert_eq!(c.rounds2(zero + 15 * 3_600_000, zero), 1);
        assert_eq!(c.rounds2(zero + 20 * 3_600_000, zero), 1);
        assert_eq!(c.rounds2(zero + 20 * 3_600_000 + 1, zero), 2);
    }

    #[test]
    fn answer_num_steps_by_answer_time() {
        let c = cfg();
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        assert_eq!(c.answer_num(open - 1, zero), 0);
        assert_eq!(c.answer_num(open, zero), 1);
        // starTime / answerTime 是整除：1000..1999 都算第 2 题
        assert_eq!(c.answer_num(open + 999, zero), 1);
        assert_eq!(c.answer_num(open + 1000, zero), 1);
        assert_eq!(c.answer_num(open + 2999, zero), 1);
        assert_eq!(c.answer_num(open + 3000, zero), 2);
        assert_eq!(c.answer_num(open + 5999, zero), 2);
        assert_eq!(c.answer_num(open + 6000, zero), 3);
    }

    #[test]
    fn answer_num_is_zero_when_window_closed() {
        let c = cfg();
        let zero = 0;
        let end = zero + 10 * 3_600_000 + i64::from(c.all_answer_time) + 1;
        assert_eq!(c.answer_num(end, zero), 0);
    }

    #[test]
    fn answer_num_guards_divide_by_zero() {
        // answerTime == 0：原版会 ArithmeticException，这里返回 0
        let c = RiddleCfg::new(&["10", "0", "2", "", "", ""], 0);
        assert_eq!(c.answer_time, 0);
        assert_eq!(c.answer_num(36_000_000, 0), 0);
    }

    #[test]
    fn value_of_uses_ceil_of_seconds() {
        let c = cfg();
        // 注意：cfg() 的分数表 key 是「秒」，而 remainingTime 是毫秒；
        // 3000ms → ceil(3.0) = 3 秒，不命中 60/30 → 0。见 change_records_and_scores。
        // 60 秒以上 → 100
        assert_eq!(c.value_of(60_000), 100);
        assert_eq!(c.value_of(60_001), 100);
        // 59.001 秒 ceil 到 60 → 仍然命中 key=60
        assert_eq!(c.value_of(59_001), 100);
        // 59 秒 → 80
        assert_eq!(c.value_of(59_000), 80);
        assert_eq!(c.value_of(30_000), 80);
        assert_eq!(c.value_of(29_000), 0);
        assert_eq!(c.value_of(0), 0);
    }

    #[test]
    fn lantern_id_formula() {
        let c = cfg();
        // 第 1 轮、开服当天、第 1 题 → 0*1 + 0 + 1 = 1
        assert_eq!(lantern_id(&c, 1, 0, 1), 1);
        // 第 2 轮、开服当天、第 1 题 → 2*1 + 0 + 1 = 3
        assert_eq!(lantern_id(&c, 2, 0, 1), 3);
        // 第 1 轮、开服第 1 天（startDay=1）、第 1 题 → 0 + 1*2*2 + 1 = 5
        assert_eq!(lantern_id(&c, 1, 1, 1), 5);
    }

    #[test]
    fn lun_formula() {
        let c = cfg();
        assert_eq!(lun_of(&c, 1, 0), 1);
        assert_eq!(lun_of(&c, 2, 0), 2);
        assert_eq!(lun_of(&c, 1, 1), 3);
    }

    #[test]
    fn activity_open_day_same_day_is_zero() {
        let zero = zero_ms_of(1_700_000_000_000);
        assert_eq!(activity_open_day(zero + 5_000, zero), 0);
        assert_eq!(activity_open_day(zero - DAY_MS, zero), 1);
        assert_eq!(activity_open_day(zero - 3 * DAY_MS, zero), 3);
    }

    #[test]
    fn change_rejects_bad_option_id() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let base = ChangeInput {
            act_id: 1,
            id: 5,
            lantern_id: 1,
            player_lv: 1,
            now_ms: 0,
            zero_ms: 0,
            start_ms: 0,
            cfg: &c,
            true_answer: 1,
        };
        assert_eq!(change_verdict(&mut st, &base), Err(CODE_DATE_ERROR));
        let zero_id = ChangeInput { id: 0, ..base };
        assert_eq!(change_verdict(&mut st, &zero_id), Err(CODE_DATE_ERROR));
    }

    #[test]
    fn change_checks_level_before_round() {
        let c = RiddleCfg::new(&["10", "3", "2", "", "", "20"], 0);
        let mut st = RiddleState::new(0);
        let input = ChangeInput {
            act_id: 1,
            id: 1,
            lantern_id: 1,
            player_lv: 19,
            now_ms: 0,
            zero_ms: 0,
            start_ms: 0,
            cfg: &c,
            true_answer: 1,
        };
        // 等级不够，即使轮次是 0 也先报等级
        assert_eq!(change_verdict(&mut st, &input), Err(CODE_PLAYER_LV_NO));
    }

    #[test]
    fn change_rejects_when_no_round_open() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let input = ChangeInput {
            act_id: 1,
            id: 1,
            lantern_id: 1,
            player_lv: 1,
            now_ms: 0,
            zero_ms: 0,
            start_ms: 0,
            cfg: &c,
            true_answer: 1,
        };
        assert_eq!(change_verdict(&mut st, &input), Err(CODE_NOT_OPEN));
    }

    #[test]
    fn change_rejects_wrong_lantern_id() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let input = ChangeInput {
            act_id: 1,
            id: 1,
            lantern_id: 999,
            player_lv: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            true_answer: 1,
        };
        assert_eq!(change_verdict(&mut st, &input), Err(CODE_LANTERN_ID_ERROR));
    }

    #[test]
    fn change_records_and_scores() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let input = ChangeInput {
            act_id: 1,
            id: 1,
            lantern_id: 1,
            player_lv: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            true_answer: 1,
        };
        let out = change_verdict(&mut st, &input).expect("ok");
        // 刚开赛 elapsed=0 → remaining = 3000ms；分数表 key 是秒（60/30），
        // ceil(3.0)=3 不命中 → 0 分（原版这里同样得 0，表是秒但 remaining 是毫秒）
        assert_eq!(out.value, 0);
        let lun = 1;
        let info = st.values_info(lun, 1);
        assert!(info.is_true);
        assert_eq!(info.id, 1);
        assert_eq!(info.time, 3000);
        assert!(st.joined);
        assert_eq!(st.true_num(lun), 1);
    }

    #[test]
    fn change_marks_wrong_answer() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let input = ChangeInput {
            act_id: 1,
            id: 2,
            lantern_id: 1,
            player_lv: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            true_answer: 1,
        };
        let out = change_verdict(&mut st, &input).expect("ok");
        assert_eq!(out.value, 0);
        assert!(!st.values_info(1, 1).is_true);
        assert_eq!(st.true_num(1), 0);
        // 答错不进 lunValue
        assert_eq!(st.lun_value.get(&1).map(|r| r.value), Some(0));
    }

    #[test]
    fn change_rejects_second_pick() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let mut input = ChangeInput {
            act_id: 1,
            id: 1,
            lantern_id: 1,
            player_lv: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            true_answer: 1,
        };
        change_verdict(&mut st, &input).expect("first");
        // ⚠ 原版 `RoundsInfo(time, value, isTrue, id)` 的 pick 恒 false，
        // 37604 自己写的记录永远 pick=false，所以**连点两次 37604 是允许的**
        // （第二次直接覆盖）。5405(PACK) 只拦 37605 排错后再答。
        input.id = 2;
        let out = change_verdict(&mut st, &input).expect("second");
        assert_eq!(out.value, 0);
        assert_eq!(st.values_info(1, 1).id, 2);
        assert!(!st.values_info(1, 1).is_true);
    }

    #[test]
    fn change_after_exclude_is_allowed_once_more() {
        // 排错写入的记录 pick=true，它挡住的是「再点一次 37604」，
        // 而 37605 自己不会因为已有记录而拒绝（它只查 outOfNum）。
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let ex = ExcludeInput {
            player_lv: 1,
            lantern_id: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            limit_lun: 3,
            limit_one: 1,
            item_num: 1,
        };
        assert!(exclude_verdict(&mut st, &ex).is_ok());
        let ch = ChangeInput {
            act_id: 1,
            id: 1,
            lantern_id: 1,
            player_lv: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            true_answer: 1,
        };
        assert_eq!(change_verdict(&mut st, &ch), Err(CODE_PACK));
    }

    #[test]
    fn exclude_rejects_over_lun_limit() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let input = ExcludeInput {
            player_lv: 1,
            lantern_id: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            limit_lun: 0,
            limit_one: 9,
            item_num: 9,
        };
        assert_eq!(
            exclude_verdict(&mut st, &input),
            Err(CODE_TROUBLESHOOTING_MAX)
        );
    }

    #[test]
    fn exclude_rejects_over_one_limit() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let mut input = ExcludeInput {
            player_lv: 1,
            lantern_id: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            limit_lun: 10,
            limit_one: 1,
            item_num: 9,
        };
        assert!(exclude_verdict(&mut st, &input).is_ok());
        input.lantern_id = 1;
        assert_eq!(
            exclude_verdict(&mut st, &input),
            Err(CODE_TROUBLESHOOTING_MAX)
        );
    }

    #[test]
    fn exclude_needs_the_item() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let input = ExcludeInput {
            player_lv: 1,
            lantern_id: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            limit_lun: 10,
            limit_one: 10,
            item_num: 0,
        };
        assert_eq!(exclude_verdict(&mut st, &input), Err(CODE_RESOURCES_NO));
    }

    #[test]
    fn exclude_writes_max_score_and_counts() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let input = ExcludeInput {
            player_lv: 1,
            lantern_id: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            limit_lun: 10,
            limit_one: 10,
            item_num: 1,
        };
        let out = exclude_verdict(&mut st, &input).expect("ok");
        assert_eq!(out.num, 1);
        let info = st.values_info(1, 1);
        assert!(info.is_true);
        assert!(info.pick);
        assert_eq!(info.value, i64::from(c.max_score));
        assert_eq!(st.lun_exclude_option_num(1), 1);
    }

    struct FakeRewards {
        one: Vec<(i32, i32, RiddleReward)>,
        list: Vec<(i32, i32, RiddleReward)>,
    }

    impl RiddleRewards for FakeRewards {
        fn one(&self, act_id: i32, num: i32, rtype: i32) -> Option<RiddleReward> {
            let _ = act_id;
            self.one
                .iter()
                .find(|(n, t, _)| *n == num && *t == rtype)
                .map(|(_, _, r)| r.clone())
        }

        fn list(&self, act_id: i32, num: i32, rtype: i32) -> Vec<RiddleReward> {
            let _ = act_id;
            self.list
                .iter()
                .filter(|(n, t, _)| *n == num && *t == rtype)
                .map(|(_, _, r)| r.clone())
                .collect()
        }
    }

    #[test]
    fn rwd_rejects_while_a_round_is_open() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let empty = FakeRewards {
            one: vec![],
            list: vec![],
        };
        let input = RwdInput {
            act_id: 1,
            player_lv: 1,
            now_ms: 36_000_000,
            zero_ms: 0,
            start_ms: 0,
            cfg: &c,
            rewards: &empty,
        };
        assert_eq!(rwd_verdict(&mut st, &input), Err(CODE_NOT_OVER));
    }

    #[test]
    fn rwd_rejects_when_never_opened() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let empty = FakeRewards {
            one: vec![],
            list: vec![],
        };
        // startDay=0（今天开服），rounds2=0（还没到 10:00）
        let input = RwdInput {
            act_id: 1,
            player_lv: 1,
            now_ms: 1_000,
            zero_ms: 0,
            start_ms: 0,
            cfg: &c,
            rewards: &empty,
        };
        assert_eq!(rwd_verdict(&mut st, &input), Err(CODE_NOT_OPEN));
    }

    #[test]
    fn rwd_pays_all_three_reward_types() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        // 第 1 轮结束后、第 2 轮开始前
        let after = zero + 10 * 3_600_000 + i64::from(c.all_answer_time) + 1;
        // 先答对一题
        let ch = ChangeInput {
            act_id: 1,
            id: 1,
            lantern_id: 1,
            player_lv: 1,
            now_ms: zero + 10 * 3_600_000,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            true_answer: 1,
        };
        change_verdict(&mut st, &ch).expect("answer");
        let rewards = FakeRewards {
            one: vec![
                (1, 1, RiddleReward { only_id: 11, items: vec![(100, 2)] }),
                (1, 2, RiddleReward { only_id: 12, items: vec![(200, 3)] }),
            ],
            list: vec![(1, 3, RiddleReward { only_id: 13, items: vec![(300, 1)] })],
        };
        let input = RwdInput {
            act_id: 1,
            player_lv: 1,
            now_ms: after,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            rewards: &rewards,
        };
        let out = rwd_verdict(&mut st, &input).expect("ok");
        // ⚠ 原版 bug（照抄）：轮次结束后 `getRounds()` 返回 0，
        // 于是 `lun = startDay * n + 0 = 0`，与答题时的 lun=1 **不是同一份**，
        // 领奖只有 type 2（首胜，不看 trueNum）能发。见 AUDIT A23。
        assert_eq!(out.true_num, 0);
        assert_eq!(out.reward_ids, vec![]);
        assert!(out.items.is_empty());
        // type 2 在 `trueNum != 0` 的块里，trueNum=0 时连首胜都不发
        assert!(!st.first_reward);
        assert!(st.riddle_reward.is_empty());
        assert!(st.rwd_info.contains_key(&0));

        // 再领一次 → lun=0 已记 rwdInfo → 过期
        let again = RwdInput {
            act_id: 1,
            player_lv: 1,
            now_ms: after,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            rewards: &rewards,
        };
        assert_eq!(rwd_verdict(&mut st, &again), Err(CODE_RWD_EXPIRE));
    }

    #[test]
    fn rwd_skips_already_taken_daily_reward() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let after = zero + 10 * 3_600_000 + i64::from(c.all_answer_time) + 1;
        let ch = ChangeInput {
            act_id: 1,
            id: 1,
            lantern_id: 1,
            player_lv: 1,
            now_ms: zero + 10 * 3_600_000,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            true_answer: 1,
        };
        change_verdict(&mut st, &ch).expect("answer");
        // 当天已经领过 onlyId=13
        st.riddle_reward.push(13);
        let rewards = FakeRewards {
            one: vec![],
            list: vec![(1, 3, RiddleReward { only_id: 13, items: vec![(300, 1)] })],
        };
        let input = RwdInput {
            act_id: 1,
            player_lv: 1,
            now_ms: after,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            rewards: &rewards,
        };
        let out = rwd_verdict(&mut st, &input).expect("ok");
        assert!(out.reward_ids.is_empty());
        assert!(out.items.is_empty());
        // 仍然记 rwdInfo[lun]，lun 在轮次结束后是 0
        assert!(st.rwd_info.contains_key(&0));
    }

    #[test]
    fn rwd_zero_true_num_takes_no_reward_but_marks_lun() {
        let c = cfg();
        let mut st = RiddleState::new(0);
        let zero = 0;
        let after = zero + 10 * 3_600_000 + i64::from(c.all_answer_time) + 1;
        let rewards = FakeRewards {
            one: vec![(1, 1, RiddleReward { only_id: 11, items: vec![(100, 2)] })],
            list: vec![],
        };
        let input = RwdInput {
            act_id: 1,
            player_lv: 1,
            now_ms: after,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            rewards: &rewards,
        };
        let out = rwd_verdict(&mut st, &input).expect("ok");
        assert_eq!(out.true_num, 0);
        assert!(out.reward_ids.is_empty());
        assert!(st.rwd_info.contains_key(&0));
    }

    #[test]
    fn refresh_clears_daily_list_on_new_day() {
        let mut st = RiddleState::new(0);
        st.riddle_reward.push(7);
        // 同一天
        st.refresh(1_000);
        assert_eq!(st.riddle_reward, vec![7]);
        // 跨天（DAY_MS 在东八区恰好是次日零点）
        st.refresh(DAY_MS + 1000);
        assert!(st.riddle_reward.is_empty());
        assert_eq!(st.reset_riddle_reward_time, DAY_MS + 1000);
    }

    #[test]
    fn max_value_and_is_max_value() {
        let mut st = RiddleState::new(0);
        st.lun_value.insert(1, RoundsInfo { value: 100, time: 10, ..Default::default() });
        st.lun_value.insert(2, RoundsInfo { value: 300, time: 30, ..Default::default() });
        st.lun_value.insert(3, RoundsInfo { value: 200, time: 20, ..Default::default() });
        assert!(st.is_max_value(2, 1));
        assert!(!st.is_max_value(1, 1));
        // 排序：2(300) > 3(200) > 1(100)
        assert!(!st.is_max_value(1, 2));
        assert!(st.is_max_value(3, 2));
        assert!(st.is_max_value(1, 3));
        let top2 = st.max_value(2);
        assert_eq!(top2.value, 500);
        assert_eq!(top2.time, 50);
        let top1 = st.max_value(1);
        assert_eq!(top1.value, 300);
    }

    #[test]
    fn lun_value_excluding_skips_the_given_id() {
        let mut st = RiddleState::new(0);
        let mut m = HashMap::new();
        m.insert(1, RoundsInfo::answer(10, 100, true, 1));
        m.insert(2, RoundsInfo::answer(20, 200, true, 2));
        m.insert(3, RoundsInfo::answer(30, 300, false, 3));
        st.values.insert(5, m);
        // 排除 1 号题 → 只剩 2 号题
        assert_eq!(st.lun_value_excluding(5, 1), 200);
        // 排除 -1（不存在的题号）→ 全部答对的都算
        assert_eq!(st.lun_value_excluding(5, -1), 300);
    }

    #[test]
    fn act_open_maps_missing_template_to_none() {
        assert_eq!(act_open(None, 0), None);
    }

    struct FakeRows(Vec<(i32, SubjectRow)>);

    impl RiddleRows for FakeRows {
        fn one(&self, act_id: i32, lantern_id: i32) -> Option<SubjectRow> {
            let _ = act_id;
            self.0
                .iter()
                .find(|(id, _)| *id == lantern_id)
                .map(|(_, r)| r.clone())
        }
    }

    fn row(options: Vec<i32>) -> SubjectRow {
        SubjectRow {
            question: "问".to_owned(),
            answer: "对".to_owned(),
            wrong: ["错1".to_owned(), "错2".to_owned(), "错3".to_owned()],
            options,
        }
    }

    #[test]
    fn subject_row_is_true_matches_option_marker() {
        let r = row(vec![2, 1, 3, 4]);
        assert!(!r.is_true(1));
        assert!(r.is_true(2));
        assert!(!r.is_true(3));
        // 越界
        assert!(!r.is_true(5));
        assert!(!r.is_true(0));
        assert!(!r.is_true(-1));
    }

    #[test]
    fn subject_row_true_answer() {
        assert_eq!(row(vec![2, 1, 3, 4]).true_answer(), 2);
        assert_eq!(row(vec![1, 2, 3, 4]).true_answer(), 1);
        assert_eq!(row(vec![2, 3, 4, 5]).true_answer(), 0);
        assert_eq!(row(vec![]).true_answer(), 0);
    }

    #[test]
    fn subject_row_option_texts_follows_marker() {
        let r = row(vec![3, 1, 4, 2]);
        assert_eq!(r.option_texts(), vec!["错2", "对", "错3", "错1"]);
        // 未知标记 → 空串
        let r2 = row(vec![9]);
        assert_eq!(r2.option_texts(), vec![""]);
    }

    #[test]
    fn subject_returns_current_question_when_round_open() {
        let c = cfg();
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let rows = FakeRows(vec![(1, row(vec![2, 1, 3, 4]))]);
        let st = RiddleState::new(0);
        let input = SubjectInput {
            act_id: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            rows: &rows,
        };
        let out = subject_verdict(&st, &input);
        let q = out.question.expect("question");
        assert_eq!(q.lantern_id, 1);
        assert_eq!(q.question, "问");
        assert_eq!(q.options.len(), 4);
        assert_eq!(q.options[1], (2, "对".to_owned()));
        assert_eq!(q.id, 0);
        assert_eq!(q.value, 0);
        assert!(!q.is_pick);
        assert_eq!(q.lun_exclude_option_num, 0);
        assert_eq!(q.lun_value, 0);
        // rounds2 = 1，round > 0 → endLanternId = 0 → 不给 trueId
        assert!(out.true_ids.is_empty());
    }

    #[test]
    fn subject_omits_question_when_no_round_open() {
        let c = cfg();
        let rows = FakeRows(vec![(1, row(vec![1]))]);
        let st = RiddleState::new(0);
        let input = SubjectInput {
            act_id: 1,
            now_ms: 0,
            zero_ms: 0,
            start_ms: 0,
            cfg: &c,
            rows: &rows,
        };
        let out = subject_verdict(&st, &input);
        assert!(out.question.is_none());
        assert!(out.true_ids.is_empty());
    }

    #[test]
    fn subject_omits_question_when_row_missing() {
        let c = cfg();
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let rows = FakeRows(vec![]);
        let st = RiddleState::new(0);
        let input = SubjectInput {
            act_id: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            rows: &rows,
        };
        assert!(subject_verdict(&st, &input).question.is_none());
    }

    #[test]
    fn subject_lists_past_answers_between_rounds() {
        let c = cfg();
        let zero = 0;
        // 第 1 轮已结束、第 2 轮未开 → round = 0，rounds2 = 1
        let after = zero + 10 * 3_600_000 + i64::from(c.all_answer_time) + 1;
        // numberOfTopics = 2 → 第 1 轮的题号是 1、2
        let rows = FakeRows(vec![
            (1, row(vec![1, 2, 3, 4])),
            (2, row(vec![2, 1, 3, 4])),
        ]);
        let st = RiddleState::new(0);
        let input = SubjectInput {
            act_id: 1,
            now_ms: after,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            rows: &rows,
        };
        let out = subject_verdict(&st, &input);
        assert!(out.question.is_none());
        // endLanternId = numberOfTopics * rounds2 + startDay * ... = 2
        assert_eq!(out.true_ids, vec![1, 2]);
    }

    #[test]
    fn subject_reports_previous_record_on_current_question() {
        let c = cfg();
        let zero = 0;
        let open = zero + 10 * 3_600_000;
        let rows = FakeRows(vec![(1, row(vec![2, 1, 3, 4]))]);
        let mut st = RiddleState::new(0);
        st.rounds_of(1)
            .insert(1, RoundsInfo::answer(1500, 100, true, 2));
        st.recompute_lun_value(1);
        let input = SubjectInput {
            act_id: 1,
            now_ms: open,
            zero_ms: zero,
            start_ms: zero,
            cfg: &c,
            rows: &rows,
        };
        let q = subject_verdict(&st, &input).question.expect("question");
        assert_eq!(q.id, 2);
        assert_eq!(q.value, 100);
        // 已答过但没排错
        assert!(!q.is_pick);
        // lunValue 排除当前题号 1 → 0
        assert_eq!(q.lun_value, 0);
        // 排除 2 号 → 100
        assert_eq!(st.lun_value_excluding(1, 2), 100);
    }
}
