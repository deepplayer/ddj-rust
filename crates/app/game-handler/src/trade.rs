//! 通商（`19002` / `19003` / `19004` / `19005`）。
//!
//! 逐个对应 `data_d/game_7010544/sources/bf/game/server/module/trade/`：
//!
//! | 这里的项 | 原版 |
//! |---|---|
//! | [`TraderRow`] / [`TradeTables`] | `bf.template.db.ForeignTraderDB` + `ForeignTraderDBCache` |
//! | [`TraderState`] | `module/trade/Trader.java`（`trader` 表一行） |
//! | [`trade_verdict`] | `Trader.trade(int all)` |
//! | [`trade_cnd_verdict`] | `Trader.trade(boolean checkCnd, long nowTime, boolean isAutoPlay)` |
//! | [`by_item_verdict`] | `Trader.tradeByItem(int itemNum)` |
//! | [`refresh`] | `Trader.refresh()` + `Trader.calcYesterdayLeftTimes` |
//! | [`to_msg_trader`] | `Trader.toMsgTraderData(Integer...)` |
//! | [`merge_resources_json`] | `ResourcesJson.mergeResourcesJson(List)` |
//!
//! 本文件是**纯逻辑**：没有 IO、没有时钟、没有随机源。落库、推送、掉落、
//! 银两读写全部由 [`crate::install_trade`] 负责。
//!
//! ## 原版 bug 照抄（见 `rust/docs/AUDIT.md` A25）
//! 1. `nextTradeTime >= now` 判 CD —— 刚好等于当前毫秒也拒；
//!    而 `trade(checkCnd, ...)` 用的是 `>`，两条路径不一致。
//! 2. `calcYesterdayLeftTimes` 先 `(int)(zeroTime-nextTradeTime)` 再整数除 ——
//!    大差值会先按 `i32` 截断。
//! 3. `refresh()` 把 `nextTradeTime` 设成**今天零点**而不是 `now`，
//!    所以跨天后的第一次通商一定能过 CD 判定（零点早已过去）。
//! 4. `merge_resources_json` 对银两（`PROPERTY` + `TypeProperty.MONEY`）
//!    会同时进 `numMap` 与 `strMap`，于是**同一条资源被输出两次**。
//! 5. `trade(int)` 末尾 `dropReward.add(reward)` 之后 `dropReward` 不再使用 —— 死代码。

use std::collections::HashMap;

use num_bigint::BigInt;
use num_traits::{One, Signed, Zero};

// ---------------------------------------------------------------- 常量

/// `LanguageUtils.TRADE_ERROR_NOT_EXIST_TRADER(2201)`：商人不存在。
pub const CODE_NOT_EXIST_TRADER: i32 = 2201;
/// `LanguageUtils.TRADE_ERROR_TRADE_ALL_LIMIT(2202)`：一键通商未开放。
pub const CODE_TRADE_ALL_LIMIT: i32 = 2202;
/// `LanguageUtils.TRADE_ERROR_FULL_TRADE_LIMIT(2204)`：可使用通商次数已满。
pub const CODE_FULL_TRADE_LIMIT: i32 = 2204;
/// `LanguageUtils.TRADE_ERROR_ZERO_TRADE_TIMES(2205)`：该商人的通商次数已用完。
pub const CODE_ZERO_TRADE_TIMES: i32 = 2205;
/// `LanguageUtils.TRADE_ERROR_CD(2206)`：CD 未到。
pub const CODE_CD: i32 = 2206;
/// `LanguageUtils.TRADE_ERROR_MONEY_LIMIT(2207)`：银两不足。
pub const CODE_MONEY_LIMIT: i32 = 2207;
/// `LanguageUtils.TRADE_ERROR_RESOURCES_LIMIT(2208)`：资源不足。
pub const CODE_RESOURCES_LIMIT: i32 = 2208;
/// `LanguageUtils.TRADE_ERROR_ALL_LIMIT(2210)`：没有商人可以通商。
pub const CODE_ALL_LIMIT: i32 = 2210;
/// `LanguageUtils.COMMON_SERVER_BUSY(2)`：`tradeByItem` 里 `itemNum > useLimit` 用它。
pub const CODE_SERVER_BUSY: i32 = 2;

/// `TraderSet` / `TradeMessageHandler.tradeAll` 里遍历商人的起点（硬编码）。
pub const TRADER_ID_BEGIN: i32 = 1201;

/// `Coliseum.MAX_INDEX`（`module/coliseum/Coliseum.java:37`）。
///
/// 用途只有一个：`ConfigDBCache.TraderTime * Coliseum.MAX_INDEX` 作为一次通商的冷却。
pub const MAX_INDEX: i64 = 1000;

/// `ConfigDBCache.TraderTime` 的实测值（`B-变量表-Config.csv:122` = `3600` 秒）。
pub const TRADER_TIME_DEFAULT: i64 = 3600;

/// 银两在 `ItemDB` 里的资源 id（同时也是 `TypeProperty.MONEY.getType()`）。
pub const MONEY_ID: i32 = 2;

/// 葡萄美酒——`tradeByItem` 消耗的道具（原版硬编码 `37`）。
pub const WINE_ITEM_ID: i32 = 37;

/// `TraderTime * MAX_INDEX`：一次通商的冷却毫秒数。
///
/// ⚠ 原版两处都是 `(long)(ConfigDBCache.TraderTime * Coliseum.MAX_INDEX)`，
/// 中间是 **`int * int`**，`3600 * 1000 = 3_600_000` 不会溢出，
/// 但语义上单位是「秒 × 1000」直接当毫秒用 —— 结果恰好是 1 小时。
#[must_use]
pub const fn cd_ms(trader_time: i64) -> i64 {
    trader_time * MAX_INDEX
}

// ---------------------------------------------------------------- 模板

/// `ForeignTraderDB` 的一行（`T-通商-ForeignTrader.csv`）。
///
/// `totalCost` / `everyCost` / `everyReward` 在原版是 `String`，
/// 这里保持字符串：`totalCost` 最大 `4000000000000`（4e12）虽能进 `i64`，
/// 但 `totalCost * itemNum` 这样的中间量没必要冒溢出的风险。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraderRow {
    /// `id`（`1201..=1210`）。
    pub id: i32,
    /// `name`（`天竺商人` 等）。
    pub name: String,
    /// `unlockChapter`：解锁章节（本模块不判，只保留）。
    pub unlock_chapter: i32,
    /// `tradeNum`：每日通商次数上限。实测全是 `3`。
    pub trade_num: i32,
    /// `totalCost`：一趟通商累计要投的银两。
    pub total_cost: String,
    /// `everyCost`：单次投入。
    pub every_cost: String,
    /// `everyReward`：单次产出的阅历。
    pub every_reward: String,
    /// `dropId`：满额时的掉落组（`list;,`，实测单值 `51..60`）。
    pub drop_id: Vec<i32>,
}

/// 通商模板集合（`ForeignTraderDBCache`）。
#[derive(Debug, Clone, Default)]
pub struct TradeTables {
    rows: HashMap<i32, TraderRow>,
    max_trader_id: i32,
}

impl TradeTables {
    /// 用全部行构造。
    #[must_use]
    pub fn new(rows: Vec<TraderRow>) -> Self {
        let max_trader_id = rows.iter().map(|r| r.id).max().unwrap_or(0);
        Self {
            rows: rows.into_iter().map(|r| (r.id, r)).collect(),
            max_trader_id,
        }
    }

    /// 取一行。
    #[must_use]
    pub fn get(&self, id: i32) -> Option<&TraderRow> {
        self.rows.get(&id)
    }

    /// `ForeignTraderDBCache.maxTraderId`。
    #[must_use]
    pub const fn max_trader_id(&self) -> i32 {
        self.max_trader_id
    }

    /// 行数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 模板注入点。
pub trait TradeTableSource: Send + Sync {
    /// 取一行商人模板。
    fn row(&self, id: i32) -> Option<&TraderRow>;
    /// `ForeignTraderDBCache.maxTraderId`。
    fn max_trader_id(&self) -> i32;
    /// `ConfigDBCache.TraderTime`（秒）。
    fn trader_time(&self) -> i64 {
        TRADER_TIME_DEFAULT
    }
}

static TABLES: std::sync::OnceLock<std::sync::Arc<dyn TradeTableSource>> =
    std::sync::OnceLock::new();

/// 注入通商表（只第一次生效）。
pub fn install_tables(t: std::sync::Arc<dyn TradeTableSource>) {
    let _ = TABLES.set(t);
}

/// 取通商表。
#[must_use]
pub fn tables() -> Option<&'static std::sync::Arc<dyn TradeTableSource>> {
    TABLES.get()
}

// ---------------------------------------------------------------- 状态

/// `trader` 表的一行（`Trader.java` 的持久化字段）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraderState {
    /// `traderId`（主键的一半）。
    pub trader_id: i32,
    /// `lastRefreshTime`：上次跨天刷新的时间。
    pub last_refresh_time: i64,
    /// `nextTradeTime`：下一次可通商的时间。
    pub next_trade_time: i64,
    /// `leftTimes`：剩余通商次数。
    pub left_times: i32,
    /// `money`：本轮已投入的银两（字符串，`BigInteger` 语义）。
    pub money: String,
    /// `yesterdayLeftTimes`：昨天没用完的次数（`autoPlay` 用）。
    pub yesterday_left_times: i32,
}

impl TraderState {
    /// `new Trader(playerId, traderId)`。
    ///
    /// ```java
    /// this.nextTradeTime = LogicUtils.getNowTime();
    /// this.lastRefreshTime = LogicUtils.getNowTime();
    /// this.leftTimes = traderDB.getTradeNum();
    /// this.money = BigInteger.ZERO.toString();
    /// ```
    #[must_use]
    pub fn new(trader_id: i32, trade_num: i32, now: i64) -> Self {
        Self {
            trader_id,
            last_refresh_time: now,
            next_trade_time: now,
            left_times: trade_num,
            money: "0".to_owned(),
            yesterday_left_times: 0,
        }
    }
}

/// `Trader.toMsgTraderData(Integer...)` 的四个字段。
///
/// 原版在 `fieldType` 非空时返回**空 builder**（bug），本项目全程传
/// `new Integer[0]`，所以永远走填充分支。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsgTraderData {
    /// `id`。
    pub id: i32,
    /// `nextTradeTime`。
    pub next_trade_time: i64,
    /// `leftTimes`。
    pub left_times: i32,
    /// `money`。
    pub money: String,
}

/// `toMsgTraderData(new Integer[0])`。
#[must_use]
pub fn to_msg_trader(st: &TraderState) -> MsgTraderData {
    MsgTraderData {
        id: st.trader_id,
        next_trade_time: st.next_trade_time,
        left_times: st.left_times,
        money: st.money.clone(),
    }
}

// ---------------------------------------------------------------- 刷新

/// `Trader.calcYesterdayLeftTimes(zeroTime)`。
///
/// ⚠ 原版 `((int) (zeroTime - nextTradeTime)) / (TraderTime * MAX_INDEX)`：
/// 先转 `i32` 再做整数除法。这里保持同样的顺序。
#[must_use]
pub fn calc_yesterday_left_times(st: &TraderState, zero_time: i64, cd: i64) -> i32 {
    if zero_time > st.next_trade_time {
        #[allow(clippy::cast_possible_truncation)]
        let diff = (zero_time - st.next_trade_time) as i32;
        #[allow(clippy::cast_possible_truncation)]
        let div = cd as i32;
        if div == 0 {
            return 0;
        }
        st.left_times.min(diff / div)
    } else {
        0
    }
}

/// `Trader.refresh()`：跨天则回满次数。
///
/// 返回 `true` 表示真的刷了（调用方据此决定是否 `save()`）。
///
/// ```java
/// long curTime = LogicUtils.getNowTime();
/// long zeroTime = LogicUtils.getTimeByHour(curTime, 0);
/// if (this.lastRefreshTime < zeroTime) {
///     calcYesterdayLeftTimes(zeroTime);
///     this.leftTimes = traderDB.getTradeNum();
///     this.nextTradeTime = zeroTime;     // ⚠ 今天零点，不是 now
///     this.money = BigInteger.ZERO.toString();
///     this.lastRefreshTime = curTime;
///     save();
/// }
/// ```
pub fn refresh(st: &mut TraderState, row: &TraderRow, now: i64, today_zero: i64, cd: i64) -> bool {
    if st.last_refresh_time >= today_zero {
        return false;
    }
    st.yesterday_left_times = calc_yesterday_left_times(st, today_zero, cd);
    st.left_times = row.trade_num;
    st.next_trade_time = today_zero;
    st.money = "0".to_owned();
    st.last_refresh_time = now;
    true
}

// ---------------------------------------------------------------- 19003 通商

/// `Trader.trade(int all)` 的成功结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeOutcome {
    /// 本次实扣的银两。
    pub cost_money: BigInt,
    /// `needTimes`：本次折算了几次通商。
    pub need_times: i32,
    /// 本次产出的阅历（`everyReward * needTimes`，资源 id = 3）。
    pub reward: BigInt,
    /// 是否满额（满额才抽掉落组）。
    pub full_reward: bool,
    /// 请求里的 `all`（原版直接回填）。
    pub all: i32,
}

/// `Trader.trade(int all)`。
///
/// 前置：
/// 1. `leftTimes <= 0` → `2205`
/// 2. `nextTradeTime >= now` → `2206`（⚠ `>=`）
///
/// 然后按 `leftMoney = totalCost - money` 与 `curMoney = player.money` 三条分支
/// 决定 `costMoney` / `needTimes`，任何一步不足都 `2207`。
///
/// 成功时**就地**更新 `st`（`money` / `leftTimes` / `nextTradeTime`），
/// 扣玩家的银两与发奖由调用方完成（纯函数不碰玩家）。
///
/// # Errors
/// 返回 `LanguageUtils` 的错误码。
pub fn trade_verdict(
    st: &mut TraderState,
    row: &TraderRow,
    all: i32,
    cur_money: &BigInt,
    now: i64,
    cd: i64,
) -> Result<TradeOutcome, i32> {
    if st.left_times <= 0 {
        return Err(CODE_ZERO_TRADE_TIMES);
    }
    if st.next_trade_time >= now {
        return Err(CODE_CD);
    }
    let every_cost = parse_big(&row.every_cost);
    let total_cost = parse_big(&row.total_cost);
    let mut cost_money = every_cost.clone();
    let left_money = total_cost.clone() - parse_big(&st.money);
    let mut need_times: i32 = 1;

    if left_money < cost_money {
        if *cur_money < left_money {
            return Err(CODE_MONEY_LIMIT);
        }
        cost_money = left_money.clone();
    } else if all > 0 {
        // `new BigDecimal(leftMoney).divide(new BigDecimal(costMoney), 0, RoundingMode.UP)`
        need_times = div_int(&left_money, &cost_money, true);
        if *cur_money >= left_money {
            cost_money = left_money.clone();
        } else {
            // `RoundingMode.DOWN`
            need_times = div_int(cur_money, &cost_money, false);
            if need_times < 1 {
                return Err(CODE_MONEY_LIMIT);
            }
            cost_money *= BigInt::from(need_times);
        }
    } else if *cur_money < cost_money {
        return Err(CODE_MONEY_LIMIT);
    }

    let new_money = parse_big(&st.money) + &cost_money;
    let full_reward = new_money >= total_cost;
    if full_reward {
        st.money = "0".to_owned();
        st.left_times -= 1;
        st.next_trade_time = now + cd;
    } else {
        st.money = new_money.to_string();
    }
    let reward = parse_big(&row.every_reward) * BigInt::from(need_times);
    Ok(TradeOutcome {
        cost_money,
        need_times,
        reward,
        full_reward,
        all,
    })
}

// ---------------------------------------------------------------- 19005 一键通商

/// `Trader.trade(boolean checkCnd, long nowTime, boolean isAutoPlay)` 的结果
/// （对应 `TradeMessageHandler.TradeAllItem`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeAllOutcome {
    /// `traderId`。
    pub trader_id: i32,
    /// `cost`：本次实扣的银两。
    pub cost_money: BigInt,
    /// `gainList`：`[(3, everyReward * needTimes)]`。
    pub gain: BigInt,
    /// `multipleGainList`：满额时的掉落组（含奇迹加成；本模块奇迹恒空）。
    pub multiple_gain: Vec<(i32, i64)>,
    /// 是否满额。
    pub full_reward: bool,
}

/// `Trader.trade(boolean checkCnd, long nowTime, boolean isAutoPlay)`。
///
/// 与 [`trade_verdict`] 的差异（**都照抄**）：
/// - `checkCnd` 时判 `nextTradeTime > nowTime`（`>`，不是 `>=`）；
/// - 所有失败**返回 `None`** 而不是抛出错误码；
/// - `leftTimes--` 与 `nextTradeTime = nowTime + cd` **只在 `checkCnd` 为真时**执行。
#[must_use]
pub fn trade_cnd_verdict(
    st: &mut TraderState,
    row: &TraderRow,
    cur_money: &BigInt,
    now_time: i64,
    check_cnd: bool,
    cd: i64,
) -> Option<TradeAllOutcome> {
    if check_cnd {
        if st.left_times <= 0 {
            return None;
        }
        if st.next_trade_time > now_time {
            return None;
        }
    }
    let every_cost = parse_big(&row.every_cost);
    let total_cost = parse_big(&row.total_cost);
    let left_money = total_cost.clone() - parse_big(&st.money);
    let mut need_times: i32 = 1;
    let cost_money;
    if left_money >= every_cost {
        need_times = div_int(&left_money, &every_cost, true);
        if *cur_money >= left_money {
            cost_money = left_money.clone();
        } else {
            need_times = div_int(cur_money, &every_cost, false);
            if need_times < 1 {
                return None;
            }
            cost_money = every_cost * BigInt::from(need_times);
        }
    } else {
        if *cur_money < left_money {
            return None;
        }
        cost_money = left_money.clone();
    }

    let new_money = parse_big(&st.money) + &cost_money;
    let full_reward = new_money >= total_cost;
    if full_reward {
        st.money = "0".to_owned();
        if check_cnd {
            st.left_times -= 1;
            st.next_trade_time = now_time + cd;
        }
    } else {
        st.money = new_money.to_string();
    }
    Some(TradeAllOutcome {
        trader_id: st.trader_id,
        cost_money,
        gain: parse_big(&row.every_reward) * BigInt::from(need_times),
        multiple_gain: Vec::new(),
        full_reward,
    })
}

// ---------------------------------------------------------------- 19002 用道具通商

/// `Trader.tradeByItem(int itemNum)` 的结算计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByItemPlan {
    /// 需要消耗的葡萄美酒数量（= `itemNum`）。
    pub item_num: i32,
    /// 需要扣除的银两（`totalCost * item_num`）。
    pub need_money: BigInt,
    /// `needTimes = (totalCost / everyCost) * itemNum`（**整数除法**）。
    pub need_times: i32,
    /// `reward = everyReward * (totalCost / everyCost) * itemNum`。
    pub reward: BigInt,
}

/// `Trader.tradeByItem(int itemNum)` 的前半段：算账 + 校验。
///
/// ```java
/// if (itemNum <= 0) throw TRADE_ERROR_RESOURCES_LIMIT;
/// if (itemNum > ItemDB(37).getUseLimit()) throw COMMON_SERVER_BUSY;
/// needMoney = totalCost * itemNum;
/// needTimes = (totalCost / everyCost).intValue() * itemNum;    // 整数除法
/// reward    = everyReward * (totalCost / everyCost) * itemNum;
/// ```
///
/// 资源校验（酒 + 银两，**全有或全无**）与掉落抽取都在调用方。
///
/// # Errors
/// `2208`（`itemNum <= 0`）或 `2`（`itemNum > useLimit`）。
pub fn by_item_verdict(row: &TraderRow, item_num: i32, use_limit: i32) -> Result<ByItemPlan, i32> {
    if item_num <= 0 {
        return Err(CODE_RESOURCES_LIMIT);
    }
    if item_num > use_limit {
        return Err(CODE_SERVER_BUSY);
    }
    let total_cost = parse_big(&row.total_cost);
    let every_cost = parse_big(&row.every_cost);
    let every_reward = parse_big(&row.every_reward);
    let need_money = total_cost.clone() * BigInt::from(item_num);
    // `new BigInteger(totalCost).divide(new BigInteger(everyCost)).intValue()`
    let ratio = div_int(&total_cost, &every_cost, false);
    let ratio = ratio.saturating_mul(item_num);
    let reward = every_reward * BigInt::from(ratio);
    Ok(ByItemPlan {
        item_num,
        need_money,
        need_times: ratio,
        reward,
    })
}

// ---------------------------------------------------------------- 资源合并

/// `ResourcesJson.mergeResourcesJson(List<ResourcesJson>)` 的忠实复刻。
///
/// ```java
/// HashMap<Integer,Long>    numMap   // 全部条目，Long.parseLong(num) 累加
/// HashMap<Integer,String>  strMap   // 仅 PROPERTY + TypeProperty.MONEY
/// ...
/// for (numMap)  → 输出
/// for (strMap)  → 再输出
/// ```
///
/// ⚠ **银两会输出两次**：`numMap` 一次（数值版）、`strMap` 一次（字符串版）。
/// 这是原版的真实行为，见 AUDIT A25 第 4 条。
///
/// Java 用 `HashMap`（顺序不可依赖）；这里按**首次出现顺序**，
/// 因为客户端只按 id 匹配，顺序不影响语义。
#[must_use]
pub fn merge_resources_json(list: &[(i32, i64)], money_id: i32) -> Vec<(i32, i64)> {
    let mut order: Vec<i32> = Vec::new();
    let mut acc: HashMap<i32, i64> = HashMap::new();
    for (id, num) in list {
        if !acc.contains_key(id) {
            order.push(*id);
        }
        *acc.entry(*id).or_insert(0) += num;
    }
    let mut out: Vec<(i32, i64)> = order
        .into_iter()
        .filter_map(|id| acc.get(&id).map(|n| (id, *n)))
        .collect();
    if let Some(n) = acc.get(&money_id) {
        out.push((money_id, *n));
    }
    out
}

/// 简单合并（按 id 累加、保首次出现顺序），不重复银两。
///
/// 用于 `rewardPond` 这类「原版直接 `mergeResourcesJson` 但列表里没有银两」的位置。
#[must_use]
pub fn merge_plain(list: &[(i32, i64)]) -> Vec<(i32, i64)> {
    let mut order: Vec<i32> = Vec::new();
    let mut acc: HashMap<i32, i64> = HashMap::new();
    for (id, num) in list {
        if !acc.contains_key(id) {
            order.push(*id);
        }
        *acc.entry(*id).or_insert(0) += num;
    }
    order
        .into_iter()
        .filter_map(|id| acc.get(&id).map(|n| (id, *n)))
        .collect()
}

// ---------------------------------------------------------------- 小工具

/// `new BigInteger(s)`；解析失败按 `0`（原版会抛异常，这里交给调用方兜底）。
#[must_use]
pub fn parse_big(s: &str) -> BigInt {
    s.trim()
        .parse::<BigInt>()
        .unwrap_or_else(|_| BigInt::zero())
}

/// `BigDecimal(a).divide(BigDecimal(b), 0, RoundingMode.UP|DOWN).intValue()`。
///
/// `round_up` 为真时按 `UP`（远离零）进位，否则按 `DOWN`（朝零）截断。
/// 转 `i32` 越界时饱和（原版 `intValue()` 会静默截断低 32 位 —— 那是另一个坑，
/// 这里的输入量级远达不到，故只在文档里记一笔）。
#[must_use]
pub fn div_int(a: &BigInt, b: &BigInt, round_up: bool) -> i32 {
    if b.is_zero() {
        return 0;
    }
    let rem = a % b;
    let mut q = a / b;
    if round_up && !rem.is_zero() {
        // 同号才向上（远离零）
        if a.is_negative() == b.is_negative() {
            q += BigInt::one();
        }
    }
    match q.to_string().parse::<i32>() {
        Ok(v) => v,
        Err(_) => {
            if q.is_negative() {
                i32::MIN
            } else {
                i32::MAX
            }
        }
    }
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    /// 1201：`tradeNum=3` / `totalCost=25000000` / `everyCost=500000` / `everyReward=30000`。
    fn row_1201() -> TraderRow {
        TraderRow {
            id: 1201,
            name: "天竺商人".to_owned(),
            unlock_chapter: 6,
            trade_num: 3,
            total_cost: "25000000".to_owned(),
            every_cost: "500000".to_owned(),
            every_reward: "30000".to_owned(),
            drop_id: vec![51],
        }
    }

    fn big(s: &str) -> BigInt {
        BigInt::from(s.parse::<i64>().expect("测试值"))
    }

    #[test]
    fn cd_is_one_hour() {
        // 3600 * 1000 = 3_600_000 ms
        assert_eq!(cd_ms(TRADER_TIME_DEFAULT), 3_600_000);
    }

    #[test]
    fn new_trader_takes_trade_num_and_starts_at_zero_money() {
        let st = TraderState::new(1201, 3, 1_700_000_000_000);
        assert_eq!(st.left_times, 3);
        assert_eq!(st.money, "0");
        assert_eq!(st.next_trade_time, 1_700_000_000_000);
        assert_eq!(st.last_refresh_time, 1_700_000_000_000);
    }

    #[test]
    fn trade_rejects_when_left_times_exhausted() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        st.left_times = 0;
        assert_eq!(
            trade_verdict(&mut st, &row, 0, &big("1000000"), 1_000, 3_600_000),
            Err(CODE_ZERO_TRADE_TIMES)
        );
    }

    #[test]
    fn trade_rejects_when_exactly_on_cd() {
        // ⚠ 原版是 `>=`：nextTradeTime 正好等于 now 也拒。
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 1_000);
        assert_eq!(
            trade_verdict(&mut st, &row, 0, &big("1000000"), 1_000, 3_600_000),
            Err(CODE_CD)
        );
        // 差 1 毫秒就放行
        assert!(
            trade_verdict(&mut st, &row, 0, &big("1000000"), 1_001, 3_600_000).is_ok(),
            "now 比 nextTradeTime 大 1ms 就该过"
        );
    }

    #[test]
    fn trade_single_pays_every_cost_and_accumulates_money() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        let out = trade_verdict(&mut st, &row, 0, &big("1000000"), 1_000, 3_600_000).expect("通商");
        assert_eq!(out.cost_money, big("500000"));
        assert_eq!(out.need_times, 1);
        assert_eq!(out.reward, big("30000"));
        assert!(!out.full_reward, "只投了 50 万，离 2500 万还远");
        assert_eq!(st.money, "500000");
        assert_eq!(st.left_times, 3, "没满额不动次数");
    }

    #[test]
    fn trade_rejects_when_player_has_no_money() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        assert_eq!(
            trade_verdict(&mut st, &row, 0, &big("499999"), 1_000, 3_600_000),
            Err(CODE_MONEY_LIMIT)
        );
    }

    #[test]
    fn trade_full_fills_when_money_reaches_total() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        // 已投 2450 万，差 50 万；玩家有钱 → costMoney 收敛到 leftMoney
        st.money = "24500000".to_owned();
        let out = trade_verdict(&mut st, &row, 0, &big("1000000"), 1_000, 3_600_000).expect("通商");
        assert_eq!(
            out.cost_money,
            big("500000"),
            "leftMoney < everyCost 时只扣差额"
        );
        assert_eq!(out.need_times, 1);
        assert!(out.full_reward);
        assert_eq!(st.money, "0");
        assert_eq!(st.left_times, 2);
        assert_eq!(st.next_trade_time, 1_000 + 3_600_000);
    }

    #[test]
    fn trade_all_uses_ceil_to_reach_total_when_rich() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        // leftMoney = 2500 万，everyCost = 50 万 → ceil = 50 次
        let out =
            trade_verdict(&mut st, &row, 1, &big("25000000"), 1_000, 3_600_000).expect("通商");
        assert_eq!(out.need_times, 50);
        assert_eq!(out.cost_money, big("25000000"));
        assert_eq!(out.reward, big("1500000"), "30000 * 50");
        assert!(out.full_reward);
    }

    #[test]
    fn trade_all_falls_back_to_floor_when_poor() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        // 玩家只有 120 万 → floor(120万/50万) = 2 次，扣 100 万，不满额
        let out = trade_verdict(&mut st, &row, 1, &big("1200000"), 1_000, 3_600_000).expect("通商");
        assert_eq!(out.need_times, 2);
        assert_eq!(out.cost_money, big("1000000"));
        assert!(!out.full_reward);
        assert_eq!(st.money, "1000000");
    }

    #[test]
    fn trade_all_rejects_when_floor_is_zero() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        // 只有 40 万，floor(40万/50万) = 0 → 2207
        assert_eq!(
            trade_verdict(&mut st, &row, 1, &big("400000"), 1_000, 3_600_000),
            Err(CODE_MONEY_LIMIT)
        );
    }

    #[test]
    fn cnd_verdict_uses_strict_greater_than_for_cd() {
        // ⚠ 与 trade(int) 的 `>=` 不同：等于 now 是**过**的。
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 1_000);
        assert!(
            trade_cnd_verdict(&mut st, &row, &big("1000000"), 1_000, true, 3_600_000).is_some(),
            "nextTradeTime == nowTime 时 trade(checkCnd) 放行"
        );
    }

    #[test]
    fn cnd_verdict_skips_decrement_when_not_checking() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        st.money = "24500000".to_owned();
        let out =
            trade_cnd_verdict(&mut st, &row, &big("1000000"), 0, false, 3_600_000).expect("满额");
        assert!(out.full_reward);
        assert_eq!(st.money, "0");
        assert_eq!(st.left_times, 3, "checkCnd 为假时不动次数");
        assert_eq!(st.next_trade_time, 0, "checkCnd 为假时不动 CD");
    }

    #[test]
    fn cnd_verdict_returns_none_instead_of_error() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        assert!(
            trade_cnd_verdict(&mut st, &row, &big("1"), 1_000, true, 3_600_000).is_none(),
            "钱不够时一键通商静默跳过"
        );
        st.left_times = 0;
        assert!(
            trade_cnd_verdict(&mut st, &row, &big("999999999"), 1_000, true, 3_600_000).is_none()
        );
    }

    #[test]
    fn refresh_resets_to_today_zero_not_now() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        st.money = "1234".to_owned();
        st.left_times = 1;
        st.next_trade_time = 500;
        st.last_refresh_time = 999;
        // now = 2000，今天零点 = 1500
        assert!(refresh(&mut st, &row, 2_000, 1_500, 3_600_000));
        assert_eq!(st.left_times, 3, "回满");
        assert_eq!(st.money, "0");
        assert_eq!(st.next_trade_time, 1_500, "⚠ 设为今天零点而非 now");
        assert_eq!(st.last_refresh_time, 2_000);
        // 同一天再刷不动
        assert!(!refresh(&mut st, &row, 2_100, 1_500, 3_600_000));
    }

    #[test]
    fn refresh_collects_yesterday_left_times() {
        let row = row_1201();
        let mut st = TraderState::new(1201, 3, 0);
        st.left_times = 2;
        // nextTradeTime 在昨天零点前 2 个 CD（2 * 3_600_000 = 7_200_000）
        st.next_trade_time = 1_500 - 7_200_000;
        st.last_refresh_time = 0;
        assert!(refresh(&mut st, &row, 2_000, 1_500, 3_600_000));
        assert_eq!(st.yesterday_left_times, 2, "min(leftTimes=2, 2) = 2");
    }

    #[test]
    fn by_item_rejects_non_positive_and_over_limit() {
        let row = row_1201();
        assert_eq!(by_item_verdict(&row, 0, 100), Err(CODE_RESOURCES_LIMIT));
        assert_eq!(by_item_verdict(&row, -1, 100), Err(CODE_RESOURCES_LIMIT));
        assert_eq!(
            by_item_verdict(&row, 101, 100),
            Err(CODE_SERVER_BUSY),
            "超过 useLimit 走 COMMON_SERVER_BUSY"
        );
    }

    #[test]
    fn by_item_matches_java_arithmetic() {
        let row = row_1201();
        let plan = by_item_verdict(&row, 3, 100).expect("3 瓶酒");
        // needMoney = 25_000_000 * 3
        assert_eq!(plan.need_money, big("75000000"));
        // totalCost / everyCost = 25_000_000 / 500_000 = 50（整数除）
        assert_eq!(plan.need_times, 150, "50 * 3");
        // reward = 30_000 * 50 * 3
        assert_eq!(plan.reward, big("4500000"));
    }

    #[test]
    fn merge_duplicates_money_like_original() {
        // 银两会输出两次：numMap 一次、strMap 一次（AUDIT A25 第 4 条）
        let merged = merge_resources_json(&[(2, 500), (2, 700), (3, 1)], MONEY_ID);
        assert_eq!(merged, vec![(2, 1200), (3, 1), (2, 1200)]);
    }

    #[test]
    fn merge_plain_does_not_duplicate() {
        assert_eq!(
            merge_plain(&[(2, 500), (2, 700), (3, 1)]),
            vec![(2, 1200), (3, 1)]
        );
    }

    #[test]
    fn merge_keeps_first_seen_order() {
        assert_eq!(merge_plain(&[(9, 1), (4, 2), (9, 3)]), vec![(9, 4), (4, 2)]);
    }

    #[test]
    fn tables_report_max_id_and_rows() {
        let t = TradeTables::new(vec![
            row_1201(),
            TraderRow {
                id: 1210,
                ..row_1201()
            },
        ]);
        assert_eq!(t.len(), 2);
        assert_eq!(t.max_trader_id(), 1210);
        assert!(t.get(1201).is_some());
        assert!(t.get(1202).is_none());
    }

    #[test]
    fn to_msg_carries_the_four_fields() {
        let mut st = TraderState::new(1201, 3, 777);
        st.money = "123".to_owned();
        st.left_times = 2;
        let m = to_msg_trader(&st);
        assert_eq!(m.id, 1201);
        assert_eq!(m.next_trade_time, 777);
        assert_eq!(m.left_times, 2);
        assert_eq!(m.money, "123");
    }

    #[test]
    fn div_int_rounds_like_bigdecimal() {
        assert_eq!(div_int(&big("10"), &big("4"), true), 3, "UP");
        assert_eq!(div_int(&big("10"), &big("4"), false), 2, "DOWN");
        assert_eq!(div_int(&big("8"), &big("4"), true), 2, "整除不进位");
        assert_eq!(div_int(&big("1"), &big("0"), true), 0, "除零按 0");
    }
}
