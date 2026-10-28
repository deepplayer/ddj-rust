//! 远洋贸易（`41001` / `41002` / `41003`）。
//!
//! 一一对应 `data_d/game_7010544/sources/bf/game/server/module/oceanTrade/`：
//!
//! | 这里的项 | 原版 |
//! |---|---|
//! | [`OceanTradeTime`] | `.../oceanTrade/OceanTradeTime.java` |
//! | [`OceanTradeCommonData`] | `.../oceanTrade/OceanTradeCommonData.java` |
//! | [`CheckpointRow`] / [`CheckpointTables`] | `.../entity/OceanTradeDB` + `OceanTradeDBCache` |
//! | [`OceanTradeState`] | `.../entity/OceanTradePlayer`（`ocean_trade_player` 表一行） |
//! | [`NegotiationCalc`] | `OceanTradePlayer.OceanTradeNegotiationCalculator`（内部类） |
//! | [`negotiate_verdict`] | `OceanTradePlayer.negotiate` |
//!
//! 结算公式在原版是 `BigInteger` / `BigDecimal` + `RoundingMode`，这里用
//! `num_bigint::BigInt` 与自写的定点除法复刻同样的截断方向（`DOWN` / `UP`），
//! 因为原版 `getRate()` 用 `divide(..., 5, RoundingMode.DOWN)` 保留 5 位小数后再参与
//! 乘法，直接换成 `f64` 会在 `4.68e18` 量级上丢精度。
//!
//! 见 `rust/docs/AUDIT.md` 的 A24。

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::{One, Signed, Zero};

/// `TypeActivity` 无关；远洋贸易靠 `SystemOpenUtils.isOpen(playerId, 19)` 开门。
pub const SYSTEM_OPEN_ID: i32 = 19;
/// 一键谈判的系统开放 id（`doNegotiateQuickly` 里的 `isOpen(playerId, 112)`）。
pub const QUICKLY_OPEN_ID: i32 = 112;

/// `LanguageUtils` 码值（`data_d/libs/jb3.common.v1/.../LanguageUtils.java:575-578`）。
pub const CODE_NOT_OPEN: i32 = 3001;
/// `OCEAN_TRADE_NUM_ILLEGAL(3002)`。
pub const CODE_NUM_ILLEGAL: i32 = 3002;
/// `OCEAN_TRADE_NEGOTIATION_QUICKLY_BE_LOCKED(3003)`。
pub const CODE_QUICKLY_LOCKED: i32 = 3003;
/// `OCEAN_TRADE_SEASON_ID_ILLEGAL(3004)`。
pub const CODE_SEASON_ID_ILLEGAL: i32 = 3004;

/// `OceanTradeService.MAX_INDEX` —— 原版用来做 `num` 的粗上界。
pub const MAX_INDEX: i32 = 1000;
/// `OceanTradeService.NOT_IN_RANKING`：`getRankingIndex` 查不到时的下标。
pub const NOT_IN_RANKING: i32 = -1;
/// `OceanTradeService.TOP_PLAYER_SIZE`：`MsgOceanTradeData.top_three` 最多几条。
pub const TOP_PLAYER_SIZE: usize = 3;
/// 一天毫秒数（`OceanTradeCommonData` 里 `*86400000` 的换算用）。
pub const DAY_MS: i64 = 86_400_000;

// ---------------------------------------------------------------- 赛季时间

/// `OceanTradeTime.java`：一个赛季的区间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OceanTradeTime {
    /// `id`：赛季 id（`seasonIdGenerator.generate()` 发号）。
    pub id: i64,
    /// `startTime`：赛季起点（毫秒）。
    pub start_time: i64,
    /// `endTime`：赛季终点（毫秒）。
    pub end_time: i64,
    /// `end`：`isEnd()` 直接返回它（不是算出来的）。
    pub end: bool,
}

impl OceanTradeTime {
    /// 新赛季。
    #[must_use]
    pub const fn new(id: i64, start_time: i64, end_time: i64) -> Self {
        Self {
            id,
            start_time,
            end_time,
            end: false,
        }
    }

    /// `isEnd()`。
    #[must_use]
    pub const fn is_end(&self) -> bool {
        self.end
    }

    /// `getStartTime()`。
    #[must_use]
    pub const fn start_time(&self) -> i64 {
        self.start_time
    }

    /// `getEndTime()`。
    #[must_use]
    pub const fn end_time(&self) -> i64 {
        self.end_time
    }
}

/// `OceanTradeCommonData` 的两个赛季槽位 + 赛季 id 发号器。
///
/// 原版还有 `seasonIdGenerator`（一个 `IdGenerator`，DB 表 `id_generator`）。
/// 这里把「当前最大 id」也放进结构体，由调用方持久化。
#[derive(Debug, Clone, Default)]
pub struct OceanTradeCommonData {
    /// `currentOceanTrade`。
    pub current: Option<OceanTradeTime>,
    /// `lastOceanTrade`。
    pub last: Option<OceanTradeTime>,
    /// `seasonIdGenerator.getCurrId()`。
    pub season_id: i64,
}

impl OceanTradeCommonData {
    /// `createFirstStartTime()`：算出第一个赛季，从「今天零点」起按
    /// `season_days` 天一段往后推，直到 `now < endTime`。
    ///
    /// 原版循环 1000 次后抛 `IllegalStateException`，这里返回 `None`
    /// —— 与 `season_days <= 0` 无法推进是同一种失败，由调用方决定是否记日志。
    #[must_use]
    pub fn create_first_start_time(
        now: i64,
        today_zero: i64,
        season_days: i64,
        season_id: i64,
    ) -> Option<OceanTradeTime> {
        if season_days <= 0 {
            return None;
        }
        let mut start_time = today_zero;
        for _ in 0..1000 {
            let end_time = today_zero + (start_time - today_zero) + season_days * DAY_MS;
            if now < end_time {
                return Some(OceanTradeTime::new(season_id, start_time, end_time));
            }
            start_time = end_time;
        }
        None
    }

    /// `getNextOceanTradeTime(nowTime)`：从 `current.endTime` 往后逐段找。
    ///
    /// 注意原版写的是 `startTime = current.endTime`（**不推进**），而
    /// `endTime = current.endTime + i * season_days * DAY_MS`，`i` 从 1 开始。
    /// 所以第 `i` 段的长度是 `i * season_days` 天 —— 这是个显而易见的 bug，
    /// 但线上 `oceanTradeSeasonTime = 7`、且 `now <= endTime` 第一次就命中，
    /// 实际等价于「下一段 7 天」。照抄。
    #[must_use]
    pub fn next_time(&self, now: i64, season_days: i64) -> Option<OceanTradeTime> {
        let current = self.current?;
        if season_days <= 0 {
            return None;
        }
        for i in 1..1000 {
            let start_time = current.end_time;
            let end_time = current.end_time + i * season_days * DAY_MS;
            if now <= end_time {
                return Some(OceanTradeTime::new(0, start_time, end_time));
            }
        }
        None
    }

    /// `handleNewOceanTradeSeason(newTime)`：`last = copy(current)`，
    /// `current = newTime`（`id` 重新发号），然后落库。
    ///
    /// ⚠ 原版 `copyOceanTradeTime` **逐字段拷贝含 `end`**，所以旧赛季的 `end`
    /// 会原样带进 `last`。
    pub fn handle_new_season(&mut self, new_time: OceanTradeTime) {
        self.last = self.current;
        self.season_id += 1;
        self.current = Some(OceanTradeTime {
            id: self.season_id,
            ..new_time
        });
    }

    /// `service.checkSeasonEndOrNextSeason(seasonId)`：
    /// `current.isEnd() || seasonId != current.id`。
    ///
    /// `current == None`（服务还没 init）时原版会 NPE；这里按「不合法」处理，
    /// 因为调用方拿到的都是 `Err(CODE_SEASON_ID_ILLEGAL)`，与 NPE 被框架兜底
    /// 后的表现一致。
    #[must_use]
    pub fn season_invalid(&self, season_id: i64) -> bool {
        match &self.current {
            None => true,
            Some(t) => t.is_end() || season_id != t.id,
        }
    }
}

// ---------------------------------------------------------------- 关卡模板

/// `OceanTradeDB.java`：一个关卡（`Y-远洋贸易-OceanTrade.csv` 一行）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckpointRow {
    /// `id`：关卡号（1..=400）。
    pub id: i32,
    /// `power`：NPC 战力（`BigInteger` 十进制串）。
    pub power: String,
    /// `cost`：NPC 血量（`BigInteger` 十进制串）。原版字段名叫 `cost`，
    /// 但语义是「打光这一条要付多少银两」，见 `getHpLeft()`。
    pub cost: String,
    /// `dropId`：额外掉落组。
    pub drop_id: i32,
}

/// 关卡表：`OceanTradeDBCache` 的 `HashMap<Integer, OceanTradeDB>` + `TreeSet<Integer>`。
#[derive(Debug, Clone, Default)]
pub struct CheckpointTables {
    rows: HashMap<i32, CheckpointRow>,
    ids: BTreeSet<i32>,
}

impl CheckpointTables {
    /// 从行列表建表（原版 `load()` 里 `idSet.add(db.getId())`）。
    #[must_use]
    pub fn new(rows: Vec<CheckpointRow>) -> Self {
        let mut ids = BTreeSet::new();
        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            ids.insert(row.id);
            map.insert(row.id, row);
        }
        Self { rows: map, ids }
    }

    /// `checkpointSize()`。
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// 表空（模板没装载）。原版 `checkpointSize() <= 0` 在 `doNegotiateQuickly` 里
    /// 直接抛 `OCEAN_TRADE_NUM_ILLEGAL`。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// `get(id)`：取一行。
    #[must_use]
    pub fn get(&self, id: i32) -> Option<&CheckpointRow> {
        self.rows.get(&id)
    }

    /// `getFirstCheckpointId()` = `idSet.first()`。
    #[must_use]
    pub fn first_id(&self) -> i32 {
        self.ids.first().copied().unwrap_or(0)
    }

    /// `getLastCheckpointId()` = `idSet.last()`。
    #[must_use]
    pub fn last_id(&self) -> i32 {
        self.ids.last().copied().unwrap_or(0)
    }

    /// `getNextCheckpointId(cp)` = `idSet.ceiling(cp + 1)`，没有则返回 `cp`。
    #[must_use]
    pub fn next_id(&self, cp: i32) -> i32 {
        self.ids
            .range((cp + 1)..)
            .next()
            .copied()
            .unwrap_or(cp)
    }

    /// `getPrevCheckpointId(cp)` = `idSet.floor(cp - 1)`，没有则返回 `0`。
    #[must_use]
    pub fn prev_id(&self, cp: i32) -> i32 {
        self.ids
            .range(..cp)
            .next_back()
            .copied()
            .unwrap_or(0)
    }

    /// `isLastCheckpointId(cp)` = `idSet.last() == cp`。
    #[must_use]
    pub fn is_last_id(&self, cp: i32) -> bool {
        self.last_id() == cp
    }

    /// `getMaxCheckpointQuicklySize()` = `checkpointSize() - getPrevCheckpointId(cp)`。
    ///
    /// 注意这是 **`usize` 减 `i32`** 的混合语义：原版 `prevCheckpointId` 可能是 0，
    /// 所以结果就是「从当前关卡到最后关卡还剩多少条」。若 `cp` 已经越界
    /// （`prev_id == last_id`），结果是 `size - last_id` 而不是 0 —— 照抄。
    #[must_use]
    pub fn max_quickly_size(&self, cp: i32) -> i32 {
        i32::try_from(self.len()).unwrap_or(i32::MAX) - self.prev_id(cp)
    }
}

// ---------------------------------------------------------------- 持久化状态

/// `OceanTradePlayer.java`：`ocean_trade_player` 表一行。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OceanTradeState {
    /// `curSeasonId`：当前赛季（`0` = 还没进过）。
    pub cur_season_id: i64,
    /// `checkpointId`：当前待挑战关卡（`0` = 未初始化）。
    pub checkpoint_id: i32,
    /// `hpCost`：当前关卡已消耗的银两（`BigInteger` 十进制串）。
    pub hp_cost: String,
    /// `endGame`：是否已通关。
    pub end_game: bool,
    /// `progress`：累计通关数。
    pub progress: i32,
    /// `lastProgress`：上一赛季的 `progress`。
    pub last_progress: i32,
    /// `seasonStartTime`：本次赛季的起点。
    pub season_start_time: i64,
}

impl OceanTradeState {
    /// 新赛季起点：`setFirstCheckpointId()`。
    ///
    /// ```java
    /// checkpointId = cache.getFirstCheckpointId();
    /// hpCost = BigInteger.ZERO.toString();
    /// progress = 0;
    /// seasonStartTime = service.getCurrentSeasonStartTime();
    /// ```
    pub fn set_first_checkpoint(&mut self, tables: &CheckpointTables, season_start_time: i64) {
        self.checkpoint_id = tables.first_id();
        self.hp_cost = "0".to_owned();
        self.progress = 0;
        self.season_start_time = season_start_time;
    }

    /// `getNpcHpLeft()`：`checkpointId <= 0 → 0`；否则 `cost - hpCost`。
    #[must_use]
    pub fn npc_hp_left(&self, tables: &CheckpointTables) -> BigInt {
        if self.checkpoint_id <= 0 {
            return BigInt::zero();
        }
        let Some(row) = tables.get(self.checkpoint_id) else {
            return BigInt::zero();
        };
        let cost = parse_big(&row.cost);
        cost - parse_big(&self.hp_cost)
    }

    /// `enterOceanTrade(builder)`：只覆盖 `checkpointId` / `seasonId` / `endGame`。
    pub fn enter(&mut self, tables: &CheckpointTables, season_id: i64) {
        if self.checkpoint_id == 0 {
            self.checkpoint_id = tables.first_id();
        }
        self.cur_season_id = season_id;
        self.end_game = false;
    }

    /// `refresh()`：进消息前跑一次。
    ///
    /// ```java
    /// if (endGame) {
    ///     lastCheckpointId = cache.getLastCheckpointId();
    ///     if (lastCheckpointId != checkpointId) {
    ///         checkpointId = cache.getNextCheckpointId(checkpointId);
    ///         endGame = false;
    ///     }
    /// }
    /// if (currentSeasonId != curSeasonId && seasonStartTime != currentSeasonStartTime) {
    ///     if (seasonStartTime > 0) {
    ///         intervalDay = UtilsTools.getIntervalDay(now, seasonStartTime);
    ///         if (intervalDay / ConfigDBCache.OceanTradeSeasonTime > 1) progress = 0;
    ///     }
    ///     curSeasonId = currentSeasonId;
    ///     lastProgress = progress;
    ///     endGame = false;
    ///     setFirstCheckpointId();   // 这里面把 progress 也清 0
    ///     save();
    /// }
    /// ```
    ///
    /// ⚠ `intervalDay / OceanTradeSeasonTime > 1` —— 是**整除后**再比大于 1，
    /// 即 `intervalDay >= 2 * season_days` 才清 `progress`；但紧接着
    /// `setFirstCheckpointId()` 又把 `progress` 置 0 —— 所以那句 `progress = 0`
    /// 其实是死代码（原版 bug，照抄）。
    ///
    /// ⚠ `season_invalid` 为真（`current == None` 或 `current.is_end()`）时，
    /// 原版 `getCurrentSeasonId()` / `getCurrentSeasonStartTime()` 都返回 0，
    /// 于是 `currentSeasonId != curSeasonId` 成立但不满足 `seasonStartTime != currentSeasonStartTime`
    /// （仅当 `seasonStartTime == 0` 时两者相等）—— 这里按同口径计算。
    pub fn refresh(
        &mut self,
        tables: &CheckpointTables,
        now: i64,
        season_days: i64,
        season_id: i64,
        season_start_time: i64,
    ) {
        if self.end_game {
            let last = tables.last_id();
            if last != self.checkpoint_id {
                self.checkpoint_id = tables.next_id(self.checkpoint_id);
                self.end_game = false;
            }
        }
        if season_id != self.cur_season_id && self.season_start_time != season_start_time {
            if self.season_start_time > 0 {
                let interval_day = interval_day(now, self.season_start_time);
                if season_days > 0 && interval_day / season_days > 1 {
                    self.progress = 0;
                }
            }
            self.cur_season_id = season_id;
            self.last_progress = self.progress;
            self.end_game = false;
            self.set_first_checkpoint(tables, season_start_time);
        }
    }

    /// `dbInit()`：`seasonStartTime <= 0 && curSeasonId > 0` 时补赛季起点。
    pub fn db_init(&mut self, season_start_time: i64) {
        if self.season_start_time <= 0 && self.cur_season_id > 0 {
            self.season_start_time = season_start_time;
        }
    }

    /// `modifyWhenComeNewServer()`。
    pub fn modify_when_come_new_server(&mut self, season_id: i64, season_start_time: i64) {
        self.cur_season_id = season_id;
        self.season_start_time = season_start_time;
    }

    /// `getCurrentProgress()`：排行榜里没自己时用它回落。
    #[must_use]
    pub const fn current_progress(&self) -> i32 {
        self.checkpoint_id
    }
}

/// `UtilsTools.getIntervalDay(now, from)`：按 24h 整除。
///
/// 原版实现是 `(now - from) / (24*3600*1000)`（`long` 除法，向零截断）。
#[must_use]
pub const fn interval_day(now: i64, from: i64) -> i64 {
    (now - from) / DAY_MS
}

// ---------------------------------------------------------------- 大数工具

/// 把十进制串解析成 `BigInt`；空串 / 非法串按 `0`。
///
/// 原版 `new BigInteger("")` 会抛 `NumberFormatException`；线上 `hpCost` 恒为
/// 合法串（`setFirstCheckpointId` 写 `BigInteger.ZERO.toString()`），
/// 唯一可能为空的场景是「玩家行刚建、还没走过 `setFirstCheckpointId`」，
/// 那时 `checkpointId == 0` 已经在 `npc_hp_left` 里短路了。
fn parse_big(s: &str) -> BigInt {
    s.trim().parse::<BigInt>().unwrap_or_else(|_| BigInt::zero())
}

/// `BigDecimal.divide(..., scale, RoundingMode)` 的复刻。
///
/// `num_bigint` 只有整数，所以手写「移小数点 → 整除 / 进位」：
/// `a / b` 保留 `scale` 位，`round_up` 为真时按 `UP`（远离零）进位，否则
/// 按 `DOWN`（朝零）截断 —— 与原版 `RoundingMode.DOWN` / `RoundingMode.UP` 一致。
///
/// 返回的是 `BigDecimal` 的 **scaled integer**：真实值 = 返回值 / 10^scale。
#[must_use]
pub fn div_scaled(a: &BigInt, b: &BigInt, scale: u32, round_up: bool) -> BigInt {
    if b.is_zero() {
        return BigInt::zero();
    }
    let factor = pow10(scale);
    let numer = a * factor;
    let quot = &numer / b;
    let rem = &numer % b;
    if round_up && !rem.is_zero() {
        // 原版 BigDecimal 在余数非 0 且被除数与被除数同号时向上，异号时向下。
        let same_sign = (numer.is_negative()) == (b.is_negative());
        if same_sign {
            quot + BigInt::one()
        } else {
            quot
        }
    } else {
        quot
    }
}

/// `10^n`。
#[must_use]
pub fn pow10(n: u32) -> BigInt {
    let mut v = BigInt::one();
    let ten = BigInt::from(10_u8);
    for _ in 0..n {
        v *= &ten;
    }
    v
}

/// 把「保留 `scale` 位的整数」还原成 `BigInt`（`RoundingMode.DOWN`）。
#[must_use]
pub fn descale(v: &BigInt, scale: u32) -> BigInt {
    if scale == 0 {
        return v.clone();
    }
    v / pow10(scale)
}

// ---------------------------------------------------------------- 谈判计算

/// `OceanTradeNegotiationCalculator`：一次谈判的可变中间量。
#[derive(Debug, Clone)]
pub struct NegotiationCalc {
    /// `checkpointId`（会被 `toNextCheckpoint` 推进）。
    pub checkpoint_id: i32,
    /// `endGame`。
    pub end_game: bool,
    /// `hpCost`（会被 `calNewCost` / `toNextCheckpoint` 改写）。
    pub hp_cost: BigInt,
    /// `progress`。
    pub progress: i32,
    /// `npcPower`（`settleBefore` 里从模板读）。
    pub npc_power: BigInt,
    /// `npcCost`（`settleBefore` 里从模板读）。
    pub npc_cost: BigInt,
    /// `preMoney`（`getDamage()`，即玩家当前银两）。
    pub pre_money: BigInt,
    /// `needMoney`（`getDamageForKill()`）。
    pub need_money: BigInt,
    /// `costMoney`：本次实际花掉的银两。
    pub cost_money: BigInt,
    /// `subtractCost`：本次扣掉的 HP。
    pub subtract_cost: BigInt,
    /// `hpLeftForLog`：结算后的剩余 HP（日志用）。
    pub hp_left_for_log: BigInt,
    /// `playerConstMoney`：累计花费（`getCostItem()` 用）。
    pub player_const_money: BigInt,
    /// `finalCostMoney`：累计花费（不扣钱版本，即 builder 上的 `costMoney`）。
    pub final_cost_money: BigInt,
    /// `totalMoney`：玩家总财力（`getRate()` 的分母）。
    pub total_money: BigInt,
    /// `win`。
    pub win: bool,
}

/// `ConfigDBCache.CheckpointCostInterval` 的第 0 项 —— 实测 `0.2`
/// （`B-变量表-Config.csv:41` → `0.2;0.5;0.8;1.2;2`，原版只取 `get(0)`）。
/// 已按 [`RATE_SCALE`] 放大，即 `0.2 * 10^5 = 20000`。
pub const MIN_RATE_SCALED: i64 = 20_000;
/// `getRate()` 保留的小数位数。
pub const RATE_SCALE: u32 = 5;

impl NegotiationCalc {
    /// 新计算器。
    #[must_use]
    pub fn new(
        checkpoint_id: i32,
        end_game: bool,
        hp_cost: BigInt,
        progress: i32,
        total_money: BigInt,
    ) -> Self {
        Self {
            checkpoint_id,
            end_game,
            hp_cost,
            progress,
            npc_power: BigInt::zero(),
            npc_cost: BigInt::zero(),
            pre_money: BigInt::zero(),
            need_money: BigInt::zero(),
            cost_money: BigInt::zero(),
            subtract_cost: BigInt::zero(),
            hp_left_for_log: BigInt::zero(),
            player_const_money: BigInt::zero(),
            final_cost_money: BigInt::zero(),
            total_money,
            win: false,
        }
    }

    /// `settleBefore()`：从模板装载 `npcPower` / `npcCost`。
    pub fn settle_before(&mut self, row: &CheckpointRow) {
        self.npc_power = parse_big(&row.power);
        self.npc_cost = parse_big(&row.cost);
    }

    /// `getDamage()`：`new BigInteger(player.getMoney())` —— **当前银两**，不是总财力。
    ///
    /// ⚠ 原版这个方法直接读 `Player.money`，**不读** `calc` 自己的字段。
    /// 调用方必须在 `settle_before()` 之后先 `calc.pre_money = player.money`，
    /// 再调本方法（见 `negotiate_once` / `negotiate_quickly`）。
    #[must_use]
    pub fn damage(&self) -> BigInt {
        self.pre_money.clone()
    }

    /// `getHpLeft()`：`npcCost - hpCost`。
    #[must_use]
    pub fn hp_left(&self) -> BigInt {
        &self.npc_cost - &self.hp_cost
    }

    /// `getRate()`：
    ///
    /// ```java
    /// rate = new BigDecimal(npcPower).divide(player.getTotalMoney(), 5, RoundingMode.DOWN);
    /// minRate = (BigDecimal) ConfigDBCache.CheckpointCostInterval.get(0);
    /// return rate.compareTo(minRate) < 0 ? minRate : rate;
    /// ```
    ///
    /// 返回 scaled 值（`* 10^5`）以免小数。
    #[must_use]
    pub fn rate_scaled(&self) -> BigInt {
        let min = BigInt::from(MIN_RATE_SCALED);
        if self.total_money.is_zero() {
            // 原版除零抛 ArithmeticException → `negotiate` 冒泡 → 框架兜底。
            // 这里退化为 minRate，保证「总财力为 0 也至少按 0.2 打」。
            return min;
        }
        let rate = div_scaled(&self.npc_power, &self.total_money, RATE_SCALE, false);
        if rate < min {
            min
        } else {
            rate
        }
    }

    /// `getDamageForKill()` = `ceil(hpLeft * rate)`。
    #[must_use]
    pub fn damage_for_kill(&self) -> BigInt {
        // hpLeft * (rate_scaled / 10^5)，向上取整
        let numer = &self.hp_left() * self.rate_scaled();
        div_scaled(&numer, &pow10(RATE_SCALE), 0, true)
    }

    /// `toNextCheckpoint()`：
    ///
    /// ```java
    /// int curId = checkpointId;
    /// checkpointId = cache.getNextCheckpointId(curId);
    /// if (curId == cache.getLastCheckpointId()) endGame = true;   // 先比较再用 next
    /// hpCost = "0"; progress++;
    /// ```
    pub fn to_next_checkpoint(&mut self, tables: &CheckpointTables) {
        let cur_id = self.checkpoint_id;
        self.checkpoint_id = tables.next_id(cur_id);
        if cur_id == tables.last_id() {
            self.end_game = true;
        }
        self.hp_cost = BigInt::zero();
        self.progress += 1;
    }

    /// `calNewCost(preMoney, needMoney)`（普通模式，会扣钱）：
    ///
    /// ```java
    /// rate = getRate();
    /// subtractCost = new BigDecimal(preMoney).divide(rate, 0, RoundingMode.DOWN).toBigInteger();
    /// costMoney = new BigDecimal(subtractCost).multiply(rate).toBigInteger();
    /// if (costMoney < preMoney) { costMoney = preMoney; subtractCost = subtractCost.add(ONE); }
    /// builder.setDamageHp(subtractCost.toString());
    /// player.removeMoney(costMoney);
    /// hpCost = npcCost.subtract(getHpLeft().subtract(subtractCost)).toString();
    /// hpLeftForLog = getHpLeft();
    /// finalCostMoney = getCurCostMoney().add(costMoney).toString();
    /// builder.setCostMoney(finalCostMoney);
    /// if (getHpLeft() <= 0) toNextCheckpoint();
    /// ```
    ///
    /// 返回 `true` 表示 HP 打空并推进到下一关。
    pub fn cal_new_cost(&mut self, builder_cost_money: &BigInt, tables: &CheckpointTables) -> bool {
        let rate_scaled = self.rate_scaled();
        // preMoney / rate → 整数（DOWN）
        self.subtract_cost = div_scaled(&self.pre_money, &rate_scaled, RATE_SCALE, false);
        // subtractCost * rate → 整数（BigDecimal.multiply().toBigInteger() 即 DOWN）
        self.cost_money = descale(&(&self.subtract_cost * &rate_scaled), RATE_SCALE);
        if self.cost_money < self.pre_money {
            self.cost_money.clone_from(&self.pre_money);
            self.subtract_cost += BigInt::one();
        }
        // hpCost = npcCost - (hpLeft - subtractCost)
        self.hp_cost = &self.npc_cost - (&self.hp_left() - &self.subtract_cost);
        self.hp_left_for_log = self.hp_left();
        self.final_cost_money = builder_cost_money + &self.cost_money;
        if self.hp_left() <= BigInt::zero() {
            self.to_next_checkpoint(tables);
            return true;
        }
        false
    }

    /// `calNewCostQuickly(...)`：与 [`Self::cal_new_cost`] 完全一致，只是
    /// **不扣钱、不写 `hpLeftForLog`**，返回本次 `costMoney`。
    pub fn cal_new_cost_quickly(
        &mut self,
        builder_cost_money: &BigInt,
        tables: &CheckpointTables,
    ) -> BigInt {
        let rate_scaled = self.rate_scaled();
        self.subtract_cost = div_scaled(&self.pre_money, &rate_scaled, RATE_SCALE, false);
        self.cost_money = descale(&(&self.subtract_cost * &rate_scaled), RATE_SCALE);
        if self.cost_money < self.pre_money {
            self.cost_money.clone_from(&self.pre_money);
            self.subtract_cost += BigInt::one();
        }
        self.hp_cost = &self.npc_cost - (&self.hp_left() - &self.subtract_cost);
        self.final_cost_money = builder_cost_money + &self.cost_money;
        if self.hp_left() <= BigInt::zero() {
            self.to_next_checkpoint(tables);
        }
        self.cost_money.clone()
    }
}

// ---------------------------------------------------------------- 谈判入口

/// 一次谈判请求的输入。
#[derive(Debug, Clone)]
pub struct NegotiateInput<'a> {
    /// 赛季 id（客户端上送，用来校验 `checkSeasonEndOrNextSeason`）。
    pub season_id: i64,
    /// `negotiateType`：`0` 普通，非 `0` 一键。
    pub negotiate_type: i32,
    /// `num`：一键谈判的目标关卡数。
    pub num: i32,
    /// 玩家当前银两。
    pub money: BigInt,
    /// 玩家总财力（`getRate()` 分母）。
    pub total_money: BigInt,
    /// 是否开通系统开放 `19`。
    pub open_19: bool,
    /// 是否开通系统开放 `112`（一键谈判）。
    pub open_112: bool,
    /// 关卡表。
    pub tables: &'a CheckpointTables,
    /// 当前赛季是否已结束 / 不匹配。
    pub season_invalid: bool,
}

/// 一次谈判请求的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NegotiateOutcome {
    /// 打掉几关（`builder.setWinNum`）。
    pub win_num: i32,
    /// 单个关卡打掉的血量（`builder.setDamageHp`）。
    pub damage_hp: BigInt,
    /// 累计花费（`builder.setCostMoney`）。
    pub cost_money: BigInt,
    /// 战斗前的银两（`builder.setPreMoney`）。
    pub pre_money: BigInt,
    /// 剩余 NPC 血量（`builder.setNpcHp`）。
    pub npc_hp: BigInt,
    /// 推进后的关卡号（`builder.setCheckpointId`）。
    pub checkpoint_id: i32,
    /// 是否通关（`builder.setEndGame`）。
    pub end_game: bool,
    /// 本次拿到的奖励（`(资源 id, 数量)`，已合并）。
    pub resources: Vec<(i32, i64)>,
    /// 从上一关 HP 归零到这次推进之间，本关 `progress` 的增量（原版是 `progress++`）。
    pub progress_gain: i32,
    /// 结算后被改写的 `hp_cost`（写回 `OceanTradeState`）。
    pub hp_cost: BigInt,
}

/// `OceanTradePlayer.negotiate` 的纯逻辑版。
///
/// 顺序严格照抄：
/// 1. `checkSeasonEndOrNextSeason` → `Err(3004)`
/// 2. `builder.setPreMoney(player.getMoney())`
/// 3. `oldCheckpointId = checkpointId`
/// 4. `doNegotiate` / `doNegotiateQuickly`
/// 5. `checkpointId >= oldCheckpointId && progress > 0` → `addOrUpdate`（排行榜，搬运在外层）
/// 6. `save()`
/// 7. 填 `npcHp` / `checkpointId` / `endGame` / `ret=0`
///
/// # Errors
/// `3004` 赛季不匹配 / `3003` 一键未解锁 / `3002` 关卡数非法。
pub fn negotiate_verdict(
    state: &mut OceanTradeState,
    input: &NegotiateInput<'_>,
    // 每关奖励表：`checkpoint_id → 资源列表`。原版是 `OceanTradeDB.getRewardJson()`；
    // 掉落组（`dropId`）需要 `DropUtils.rand`，这里由调用方拼好一起传进来。
    rewards: &dyn Fn(i32) -> Vec<(i32, i64)>,
) -> Result<NegotiateOutcome, i32> {
    if input.season_invalid {
        return Err(CODE_SEASON_ID_ILLEGAL);
    }
    let _ = input.open_19;
    let pre_money = input.money.clone();
    let old_checkpoint_id = state.checkpoint_id;

    let mut out = NegotiateOutcome {
        pre_money: pre_money.clone(),
        ..NegotiateOutcome::default()
    };

    if input.negotiate_type == 0 {
        negotiate_once(state, input, rewards, &mut out)?;
    } else {
        negotiate_quickly(state, input, rewards, &mut out)?;
    }

    if state.checkpoint_id >= old_checkpoint_id && state.progress > 0 {
        // 排行榜更新在外层（需要 playerId + 名字/头像），这里只保证
        // `progress` 已就位。见 `install_ocean_trade.rs`。
    }

    out.npc_hp = state.npc_hp_left(input.tables);
    out.checkpoint_id = state.checkpoint_id;
    out.end_game = state.end_game;
    out.hp_cost = parse_big(&state.hp_cost);
    Ok(out)
}

/// `doNegotiate`（`negotiateType == 0`）。
fn negotiate_once(
    state: &mut OceanTradeState,
    input: &NegotiateInput<'_>,
    rewards: &dyn Fn(i32) -> Vec<(i32, i64)>,
    out: &mut NegotiateOutcome,
) -> Result<(), i32> {
    let checkpoint_id = state.checkpoint_id;
    let Some(row) = input.tables.get(checkpoint_id) else {
        // 原版 `cache.get(checkpointId)` 为 null → `settleBefore()` NPE。
        // 线上不可能（`checkpointId` 恒为表里的 id），按 3002 兜底。
        return Err(CODE_NUM_ILLEGAL);
    };
    let mut calc = NegotiationCalc::new(
        state.checkpoint_id,
        state.end_game,
        parse_big(&state.hp_cost),
        state.progress,
        input.total_money.clone(),
    );
    calc.settle_before(row);
    // ⚠ 原版 `settleBefore()` **不设** `preMoney`；`getDamage()` 返回的是
    // `new BigInteger(player.getMoney())`（玩家当前银两），而不是 `calc.preMoney`（初值 0）。
    // 所以这里必须显式用 `input.money` 赋给 `pre_money`，否则会走「0 银两」分支。
    calc.pre_money = input.money.clone();
    calc.need_money = calc.damage_for_kill();

    if calc.pre_money >= calc.need_money {
        // settle(): onWin(needMoney)
        out.damage_hp = calc.hp_left();
        let need_money = calc.need_money.clone();
        let mut list = rewards(checkpoint_id);
        // onWin: costMoney = getCurCostMoney().add(needMoney)
        let cur = if out.cost_money.is_zero() {
            BigInt::zero()
        } else {
            out.cost_money.clone()
        };
        calc.cost_money = cur + &need_money;
        calc.player_const_money = calc.cost_money.clone();
        calc.hp_left_for_log = BigInt::zero();
        out.cost_money = calc.cost_money.clone();
        calc.win = true;
        let before_progress = calc.progress;
        calc.to_next_checkpoint(input.tables);
        out.progress_gain = calc.progress - before_progress;
        // 奖励：原版 `addResourcesJsons(player, list, ...)` 是发给玩家背包，
        // 回包里也 `builder.addAllMsgResources(toMsgAwardBuilder(list))`。
        list = merge_resources(list);
        out.resources = list;
        out.win_num = 1;
    } else {
        let builder_cost_money = out.cost_money.clone();
        calc.cal_new_cost(&builder_cost_money, input.tables);
        out.damage_hp = calc.subtract_cost.clone();
        out.cost_money = calc.final_cost_money.clone();
        calc.win = false;
        out.win_num = 0;
    }

    // 写回
    state.checkpoint_id = calc.checkpoint_id;
    state.end_game = calc.end_game;
    state.progress = calc.progress;
    state.hp_cost = calc.hp_cost.to_string();
    // 扣钱：`settle()` 里 `onWin` 走 `removeMoney(needMoney)`，
    // `calNewCost` 里走 `removeMoney(costMoney)`。两条路径互斥。
    let spent = if calc.win {
        calc.player_const_money.clone()
    } else {
        calc.cost_money.clone()
    };
    out.cost_money = spent;
    Ok(())
}

/// `doNegotiateQuickly`（`negotiateType != 0`）。
fn negotiate_quickly(
    state: &mut OceanTradeState,
    input: &NegotiateInput<'_>,
    rewards: &dyn Fn(i32) -> Vec<(i32, i64)>,
    out: &mut NegotiateOutcome,
) -> Result<(), i32> {
    if !input.open_112 {
        return Err(CODE_QUICKLY_LOCKED);
    }
    let checkpoint_size = input.num;
    if checkpoint_size <= 0 || checkpoint_size > input.tables.max_quickly_size(state.checkpoint_id)
    {
        return Err(CODE_NUM_ILLEGAL);
    }

    let mut calc = NegotiationCalc::new(
        state.checkpoint_id,
        state.end_game,
        parse_big(&state.hp_cost),
        state.progress,
        input.total_money.clone(),
    );
    let mut acc_cost = BigInt::zero();
    let mut all = Vec::new();
    let mut i = 0;
    let mut builder_cost_money = BigInt::zero();
    while i < checkpoint_size {
        if input.season_invalid {
            break;
        }
        let Some(row) = input.tables.get(calc.checkpoint_id) else {
            break;
        };
        calc.settle_before(row);
        // 同 `negotiate_once`：`getDamage()` 读的是 `player.getMoney()`。
        calc.pre_money = input.money.clone();
        calc.need_money = calc.damage_for_kill();
        if calc.pre_money >= calc.need_money {
            // onWinQuickly：不扣钱、不发奖励（奖励攒起来最后一起给）
            acc_cost += calc.need_money.clone();
            let mut list = rewards(calc.checkpoint_id);
            let cur = builder_cost_money.clone();
            calc.cost_money = cur + &calc.need_money;
            builder_cost_money = calc.cost_money.clone();
            calc.to_next_checkpoint(input.tables);
            all.append(&mut list);
            i += 1;
        } else {
            let spend = calc.cal_new_cost_quickly(&builder_cost_money, input.tables);
            acc_cost += spend;
            builder_cost_money = calc.final_cost_money.clone();
            break;
        }
    }

    out.win_num = i;
    out.damage_hp = calc.subtract_cost.clone();
    out.cost_money = builder_cost_money;
    out.resources = merge_resources(all);
    out.progress_gain = calc.progress - state.progress;

    state.checkpoint_id = calc.checkpoint_id;
    state.end_game = calc.end_game;
    state.progress = calc.progress;
    state.hp_cost = calc.hp_cost.to_string();
    let _ = acc_cost;
    Ok(())
}

/// `ResourcesJson.mergeResourcesJson`：按 id 累加，保持首次出现顺序。
#[must_use]
pub fn merge_resources(list: Vec<(i32, i64)>) -> Vec<(i32, i64)> {
    let mut order: Vec<i32> = Vec::new();
    let mut acc: HashMap<i32, i64> = HashMap::new();
    for (id, num) in list {
        if !acc.contains_key(&id) {
            order.push(id);
        }
        *acc.entry(id).or_insert(0) += num;
    }
    order
        .into_iter()
        .filter_map(|id| acc.remove(&id).map(|num| (id, num)))
        .collect()
}

// ---------------------------------------------------------------- 排行榜

/// 排行榜一格（`OceanTradeRank` 实体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankEntry {
    /// 玩家 id。
    pub player_id: String,
    /// `checkpointId`。
    pub checkpoint_id: i32,
    /// `updateTime`（排序次级键）。
    pub update_time: i64,
}

/// `OceanTradeService.rankingComparator`：
/// `checkpointId` **降序**；相同则 `updateTime` **升序**。
#[must_use]
pub fn rank_cmp(a: &RankEntry, b: &RankEntry) -> std::cmp::Ordering {
    b.checkpoint_id
        .cmp(&a.checkpoint_id)
        .then_with(|| a.update_time.cmp(&b.update_time))
}

// ---------------------------------------------------------------- 模板注入

/// 模板提供者：`game-core` 用真实 CSV 实现，`game-handler` 只认这个 trait。
///
/// 一一对应原版 `OceanTradeDBCache`（关卡表 + 奖励）与 `ConfigDBCache`
/// （赛季时长 / 最低倍率 / 榜单长度）。
pub trait OceanTradeTables: Send + Sync {
    /// 关卡表（`OceanTradeDBCache` 的 `idSet` 部分）。
    fn checkpoints(&self) -> &CheckpointTables;
    /// 某关的 `reward` 列（`OceanTradeDB.getRewardJson()`）。
    fn reward(&self, checkpoint_id: i32) -> Vec<(i32, i64)>;
    /// 某关的 `dropId`；`0` 表示无掉落。
    fn drop_id(&self, checkpoint_id: i32) -> i32;
    /// `OceanTradeSeasonTime`（天）。
    fn season_days(&self) -> i64 {
        7
    }
    /// `CheckpointCostInterval.get(0)` 放大后的值。
    fn min_rate_scaled(&self) -> i64 {
        MIN_RATE_SCALED
    }
    /// `OceanTradeRankListDisplayQuantity`。
    fn rank_display(&self) -> usize {
        500
    }
    /// `OceanTradeRankListActualNumber`。
    fn rank_actual(&self) -> usize {
        2000
    }
}

/// 装配：[`OceanTradeTables`] 的实现。
static TABLES: std::sync::OnceLock<Arc<dyn OceanTradeTables>> = std::sync::OnceLock::new();

/// 注入模板（启动时调用一次）。
pub fn install_tables(t: Arc<dyn OceanTradeTables>) {
    let _ = TABLES.set(t);
}

/// 取模板；未注入时返回 `None`，调用方按「关卡通不了」处理。
#[must_use]
pub fn tables() -> Option<&'static Arc<dyn OceanTradeTables>> {
    TABLES.get()
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> CheckpointTables {
        CheckpointTables::new(vec![
            CheckpointRow {
                id: 1,
                power: "2000000".to_owned(),
                cost: "12250000000".to_owned(),
                drop_id: 121,
            },
            CheckpointRow {
                id: 2,
                power: "4000000".to_owned(),
                cost: "24500000000".to_owned(),
                drop_id: 121,
            },
            CheckpointRow {
                id: 3,
                power: "6000000".to_owned(),
                cost: "49000000000".to_owned(),
                drop_id: 122,
            },
        ])
    }

    #[test]
    fn next_and_prev_id_match_java_ceiling_floor() {
        let t = tables();
        assert_eq!(t.first_id(), 1);
        assert_eq!(t.last_id(), 3);
        assert_eq!(t.next_id(1), 2);
        assert_eq!(t.next_id(3), 3, "ceiling 没有则原样返回");
        assert_eq!(t.prev_id(2), 1);
        assert_eq!(t.prev_id(1), 0, "floor 没有则返回 0");
        assert_eq!(t.prev_id(3), 2);
        assert!(t.is_last_id(3));
        assert!(!t.is_last_id(2));
    }

    #[test]
    fn max_quickly_size_uses_prev_not_count() {
        let t = tables();
        // size=3, prev(1)=0 → 3
        assert_eq!(t.max_quickly_size(1), 3);
        // size=3, prev(2)=1 → 2
        assert_eq!(t.max_quickly_size(2), 2);
        // size=3, prev(3)=2 → 1
        assert_eq!(t.max_quickly_size(3), 1);
    }

    #[test]
    fn rate_is_capped_at_min_and_scaled() {
        let mut c = NegotiationCalc::new(1, false, BigInt::zero(), 0, BigInt::zero());
        c.npc_power = BigInt::from(2_000_000);
        // 总财力 10_000_000 → rate = 0.2 → scaled 20000，正好等于 minRate
        c.total_money = BigInt::from(10_000_000);
        assert_eq!(c.rate_scaled(), BigInt::from(MIN_RATE_SCALED));
        // 总财力极小 → rate 反而大，取计算值
        c.total_money = BigInt::from(1_000_000);
        // 2_000_000 / 1_000_000 = 2.00000 → 2 * 10^5 = 200000
        assert_eq!(c.rate_scaled(), BigInt::from(200_000));
        // 总财力极大 → 计算值小于 minRate → 取 minRate
        c.total_money = BigInt::from(1_000_000_000);
        // 2_000_000 / 1e9 = 0.002 → 小于 0.2
        assert_eq!(c.rate_scaled(), BigInt::from(MIN_RATE_SCALED));
    }

    #[test]
    fn damage_for_kill_rounds_up() {
        let mut c = NegotiationCalc::new(1, false, BigInt::zero(), 0, BigInt::from(1_000_000));
        c.npc_power = BigInt::from(2_000_000); // rate = 2.0
        c.npc_cost = BigInt::from(1001);
        c.hp_cost = BigInt::zero();
        // hpLeft 1001 * 2.0 = 2002
        assert_eq!(c.damage_for_kill(), BigInt::from(2002));
        c.npc_cost = BigInt::from(1001);
        c.hp_cost = BigInt::from(1000);
        // hpLeft 1 * 2.0 = 2
        assert_eq!(c.damage_for_kill(), BigInt::from(2));
    }

    #[test]
    fn to_next_checkpoint_checks_old_id_for_end_game() {
        let t = tables();
        let mut c = NegotiationCalc::new(3, false, BigInt::from(5), 0, BigInt::zero());
        c.to_next_checkpoint(&t);
        // 3 是最后一关 → endGame 置 true；checkpointId 因 next_id 无下一项而保持 3
        assert!(c.end_game);
        assert_eq!(c.checkpoint_id, 3);
        assert_eq!(c.progress, 1);
        assert!(c.hp_cost.is_zero());
    }

    #[test]
    fn to_next_checkpoint_moves_forward_normally() {
        let t = tables();
        let mut c = NegotiationCalc::new(1, false, BigInt::from(5), 0, BigInt::zero());
        c.to_next_checkpoint(&t);
        assert!(!c.end_game);
        assert_eq!(c.checkpoint_id, 2);
        assert_eq!(c.progress, 1);
    }

    #[test]
    fn cal_new_cost_zero_money_floors_to_one_damage() {
        let t = tables();
        let mut c = NegotiationCalc::new(1, false, BigInt::zero(), 0, BigInt::from(1_000_000));
        c.settle_before(t.get(1).expect("关卡 1"));
        c.pre_money = BigInt::zero();
        // 银两为 0 → subtractCost 0 → costMoney 0 < preMoney(0) 不成立，
        // 但 `costMoney < preMoney` 判的是 `<`，0 < 0 为假，所以不减。
        let advanced = c.cal_new_cost(&BigInt::zero(), &t);
        assert!(!advanced);
        assert!(c.cost_money.is_zero());
        assert!(c.subtract_cost.is_zero());
    }

    #[test]
    fn negotiate_once_uses_player_money_not_calc_field() {
        // 回归：`settleBefore()` 不设 `preMoney`，`getDamage()` 直接读
        // `player.getMoney()`。早期实现漏了这一步，`preMoney` 恒为 0 →
        // `damageForKill()` 也恒为 0 → 走「0 银两」分支，回包 `damageHp=0`。
        let t = tables();
        let mut st = OceanTradeState {
            checkpoint_id: 1,
            ..OceanTradeState::default()
        };
        let input = NegotiateInput {
            season_id: 1,
            negotiate_type: 0,
            num: 1,
            money: BigInt::from(20000),
            total_money: BigInt::zero(),
            open_19: true,
            open_112: false,
            tables: &t,
            season_invalid: false,
        };
        let out = negotiate_verdict(&mut st, &input, &|_| Vec::new()).expect("应成功");
        // rate = max(0.2, npcPower/totalMoney)；totalMoney=0 走兜底 0.2
        //   subtractCost = 20000 / 0.2 = 100000
        //   costMoney    = 100000 * 0.2 = 20000，不小于 preMoney → 不兜底
        assert_eq!(out.pre_money, BigInt::from(20000));
        assert_eq!(out.damage_hp, BigInt::from(100_000));
        assert_eq!(out.cost_money, BigInt::from(20000));
        assert_eq!(out.win_num, 0);
        assert!(!out.end_game);
        // npcCost 12250000000 - 100000
        assert_eq!(out.npc_hp, BigInt::from(12_249_900_000_i64));
        assert_eq!(parse_big(&st.hp_cost), BigInt::from(100_000));
    }

    #[test]
    fn cal_new_cost_small_money_rounds_cost_up_to_pre_money() {
        let t = tables();
        let mut c = NegotiationCalc::new(1, false, BigInt::zero(), 0, BigInt::from(1_000_000));
        c.settle_before(t.get(1).expect("关卡 1"));
        c.pre_money = BigInt::from(1);
        // rate = 2.0；subtractCost = 1 / 2 = 0；costMoney = 0 * 2 = 0 < 1
        // → costMoney = 1, subtractCost = 1
        let advanced = c.cal_new_cost(&BigInt::zero(), &t);
        assert!(!advanced);
        assert_eq!(c.cost_money, BigInt::from(1));
        assert_eq!(c.subtract_cost, BigInt::from(1));
        // hpCost = npcCost - (hpLeft - subtractCost)
        //        = 12250000000 - (12250000000 - 1) = 1
        assert_eq!(c.hp_cost, BigInt::from(1));
    }

    #[test]
    fn cal_new_cost_kills_and_advances() {
        let t = tables();
        // 关卡 1 cost = 12250000000，hpCost = 12249999999 → 剩 1 HP
        let mut c = NegotiationCalc::new(
            1,
            false,
            BigInt::from(12_249_999_999_i64),
            0,
            BigInt::from(1_000_000_000_i64),
        );
        c.settle_before(t.get(1).expect("关卡 1"));
        c.pre_money = BigInt::from(1_000_000_000_i64);
        let advanced = c.cal_new_cost(&BigInt::zero(), &t);
        assert!(advanced, "HP 打到 0 应该推进");
        assert_eq!(c.checkpoint_id, 2);
        assert_eq!(c.progress, 1);
    }

    #[test]
    fn negotiate_once_blocks_when_season_invalid() {
        let t = tables();
        let mut st = OceanTradeState::default();
        let input = NegotiateInput {
            season_id: 1,
            negotiate_type: 0,
            num: 0,
            money: BigInt::zero(),
            total_money: BigInt::zero(),
            open_19: true,
            open_112: true,
            tables: &t,
            season_invalid: true,
        };
        let r = negotiate_verdict(&mut st, &input, &|_| Vec::new());
        assert_eq!(r, Err(CODE_SEASON_ID_ILLEGAL));
    }

    #[test]
    fn negotiate_quickly_requires_open_112() {
        let t = tables();
        let mut st = OceanTradeState::default();
        let input = NegotiateInput {
            season_id: 1,
            negotiate_type: 1,
            num: 1,
            money: BigInt::from(1_000_000),
            total_money: BigInt::from(1_000_000),
            open_19: true,
            open_112: false,
            tables: &t,
            season_invalid: false,
        };
        let r = negotiate_verdict(&mut st, &input, &|_| Vec::new());
        assert_eq!(r, Err(CODE_QUICKLY_LOCKED));
    }

    #[test]
    fn negotiate_quickly_rejects_num_over_max() {
        let t = tables();
        let mut st = OceanTradeState::default();
        let input = NegotiateInput {
            season_id: 1,
            negotiate_type: 1,
            num: 99,
            money: BigInt::from(1_000_000),
            total_money: BigInt::from(1_000_000),
            open_19: true,
            open_112: true,
            tables: &t,
            season_invalid: false,
        };
        let r = negotiate_verdict(&mut st, &input, &|_| Vec::new());
        assert_eq!(r, Err(CODE_NUM_ILLEGAL));
    }

    #[test]
    fn merge_resources_sums_and_keeps_order() {
        let got = merge_resources(vec![(4, 5), (3, 300_000), (4, 1), (1, 2)]);
        assert_eq!(got, vec![(4, 6), (3, 300_000), (1, 2)]);
    }

    #[test]
    fn refresh_advances_end_game_to_next() {
        let t = tables();
        let mut st = OceanTradeState {
            checkpoint_id: 1,
            end_game: true,
            ..OceanTradeState::default()
        };
        st.refresh(&t, 0, 7, 0, 0);
        assert_eq!(st.checkpoint_id, 2);
        assert!(!st.end_game);
    }

    #[test]
    fn refresh_keeps_end_game_at_last_checkpoint() {
        let t = tables();
        let mut st = OceanTradeState {
            checkpoint_id: 3,
            end_game: true,
            ..OceanTradeState::default()
        };
        st.refresh(&t, 0, 7, 0, 0);
        assert_eq!(st.checkpoint_id, 3);
        assert!(st.end_game, "最后一关不再推进，endGame 保留");
    }

    #[test]
    fn refresh_new_season_resets_progress() {
        let t = tables();
        let mut st = OceanTradeState {
            cur_season_id: 1,
            checkpoint_id: 3,
            progress: 42,
            season_start_time: 1_000_000,
            ..OceanTradeState::default()
        };
        // 新赛季 id=2、起点不同 → 走 reset 分支
        st.refresh(&t, 2 * DAY_MS, 7, 2, 2_000_000);
        assert_eq!(st.cur_season_id, 2);
        assert_eq!(st.progress, 0);
        assert_eq!(st.last_progress, 42);
        assert_eq!(st.checkpoint_id, 1, "setFirstCheckpointId");
        assert_eq!(st.season_start_time, 2_000_000);
        assert_eq!(st.hp_cost, "0");
    }

    #[test]
    fn refresh_same_season_start_does_not_reset() {
        let t = tables();
        let mut st = OceanTradeState {
            cur_season_id: 1,
            checkpoint_id: 2,
            progress: 42,
            season_start_time: 1_000_000,
            ..OceanTradeState::default()
        };
        // 赛季 id 变了但起点相同 → 不进分支（原版 `&&` 两个条件）
        st.refresh(&t, 0, 7, 2, 1_000_000);
        assert_eq!(st.cur_season_id, 1);
        assert_eq!(st.progress, 42);
    }

    #[test]
    fn npc_hp_left_is_cost_minus_spent() {
        let t = tables();
        let mut st = OceanTradeState {
            checkpoint_id: 1,
            hp_cost: "5000000000".to_owned(),
            ..OceanTradeState::default()
        };
        assert_eq!(st.npc_hp_left(&t), BigInt::from(12_250_000_000_i64 - 5_000_000_000_i64));
        st.checkpoint_id = 0;
        assert!(st.npc_hp_left(&t).is_zero(), "checkpointId<=0 直接返回 0");
    }

    #[test]
    fn season_invalid_when_no_current() {
        let d = OceanTradeCommonData::default();
        assert!(d.season_invalid(0));
    }

    #[test]
    fn season_invalid_on_end_or_mismatch() {
        let d = OceanTradeCommonData {
            current: Some(OceanTradeTime::new(7, 0, 100)),
            ..OceanTradeCommonData::default()
        };
        assert!(!d.season_invalid(7));
        assert!(d.season_invalid(8));
        let d2 = OceanTradeCommonData {
            current: Some(OceanTradeTime {
                end: true,
                ..OceanTradeTime::new(7, 0, 100)
            }),
            ..OceanTradeCommonData::default()
        };
        assert!(d2.season_invalid(7), "isEnd() 优先");
    }

    #[test]
    fn create_first_start_time_returns_first_window_containing_now() {
        // 今天零点 = 0；赛季 7 天；now = 第 1 天
        let t = OceanTradeCommonData::create_first_start_time(DAY_MS, 0, 7, 1).expect("有解");
        assert_eq!(t.start_time, 0);
        assert_eq!(t.end_time, 7 * DAY_MS);
        assert_eq!(t.id, 1);
    }

    #[test]
    fn create_first_start_time_rejects_non_positive_days() {
        assert!(OceanTradeCommonData::create_first_start_time(0, 0, 0, 1).is_none());
        assert!(OceanTradeCommonData::create_first_start_time(0, 0, -7, 1).is_none());
    }

    #[test]
    fn next_time_starts_from_current_end() {
        let d = OceanTradeCommonData {
            current: Some(OceanTradeTime::new(1, 0, 7 * DAY_MS)),
            ..OceanTradeCommonData::default()
        };
        let next = d.next_time(7 * DAY_MS, 7).expect("有解");
        // startTime = current.endTime = 7d；endTime = 7d + 1*7d = 14d；now(7d) <= 14d 命中
        assert_eq!(next.start_time, 7 * DAY_MS);
        assert_eq!(next.end_time, 14 * DAY_MS);
    }

    #[test]
    fn handle_new_season_moves_current_to_last() {
        let mut d = OceanTradeCommonData {
            current: Some(OceanTradeTime::new(5, 0, 100)),
            season_id: 5,
            ..OceanTradeCommonData::default()
        };
        d.handle_new_season(OceanTradeTime::new(0, 100, 200));
        assert_eq!(d.season_id, 6);
        assert_eq!(d.current.expect("有 current").id, 6);
        assert_eq!(d.last.expect("有 last").id, 5);
    }

    #[test]
    fn div_scaled_down_truncates_toward_zero() {
        assert_eq!(
            div_scaled(&BigInt::from(1), &BigInt::from(3), 2, false),
            BigInt::from(33)
        );
        assert_eq!(
            div_scaled(&BigInt::from(-1), &BigInt::from(3), 2, false),
            BigInt::from(-33)
        );
    }

    #[test]
    fn div_scaled_up_rounds_away_from_zero() {
        assert_eq!(
            div_scaled(&BigInt::from(1), &BigInt::from(3), 2, true),
            BigInt::from(34)
        );
    }

    #[test]
    fn div_scaled_by_zero_is_zero() {
        assert!(div_scaled(&BigInt::from(5), &BigInt::zero(), 3, true).is_zero());
    }
}
