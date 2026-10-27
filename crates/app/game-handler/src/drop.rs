//! # 掉落系统（`DropUtils`）
//!
//! 对应 Java：
//! - `bf/game/server/utils/DropUtils.java`（160 行）
//! - `bf/template/db/DropOneDB.java` / `DropTwoDB.java` / `DropSignDB.java`
//! - `bf/common/utils/WeightRandomModelUtils.java`（权重抽取）
//! - `bf/game/server/module/drop/DropTimesCounter.java`（下标计数接口）
//!
//! ## 三层结构
//! ```text
//! DropOneDB（一级）   extractNum 次抽取，每次从 idTwo 里挑一个二级
//!   ↓ 可被 givenDropId 命中（按累计次数强制指定）
//! DropTwoDB（二级）   formulaId 判公式 → 命中则「必出」某个 weight 项
//!   ↓ 没命中公式 → 按 probability 权重抽
//! DropSignDB（下标）  addType / resetType 控制「抽了多少次」的累加与重置
//! ```
//!
//! `DropUtils.rand(player, dropId)` 的完整链路：
//! ```java
//! DropOneDB one = DropOneDBCache.get(dropId);
//! for (int i = 0; i < one.getExtractNum(); i++) {
//!     ResourcesJson r = randOnce(one, modelOne, modelSign);   // 一次抽取
//!     if (r != null) list.add(r);
//! }
//! return mergeResourcesJson(list);                            // 同 id 合并数量
//! ```
//!
//! ## ⚠ `randOnce` 的顺序不能调换
//! ```java
//! DropTwoDB two = null;
//! if (!givenDropId.isEmpty()) {
//!     two = getGivenDropId(one, modelOne);   // 按次数强制指定
//!     modelOne.addTimes(one.getId(), 1);     // ⚠ 无条件 +1，不管有没有命中
//! }
//! if (two == null) two = getDropTwoByFormula(one, modelSign);   // 公式
//! if (two == null) return null;                                  // 一条都不出
//! return two.randItem();
//! ```
//! 注意 `modelOne.addTimes(one.getId(), 1)` 在 `givenDropId` 非空时**无条件累加**，
//! 且累加发生在 `getGivenDropId` **之后**（后者内部用的是 `times + 1`）。
//!
//! ## ⚠ `getGivenDropId` 用 `times + 1` 比对
//! ```java
//! int times = modelOne.getTimes(one.getId()) + 1;
//! for (List<Integer> pair : one.getGivenDropId()) {
//!     if (times == pair.get(0)) return DropTwoDBCache.get(pair.get(1));
//! }
//! return null;
//! ```
//! 也就是「第 N 次抽（1-based）出指定掉落」。
//!
//! ## ⚠ `randItem` 的权重下限
//! ```java
//! new WeightRandomModelUtils(items, 1000000).random()
//! ```
//! 第二个参数是 `minWeightTheSum`。当所有 `probability` 之和 < 1000000 时，
//! 会**补一个 `weight = 1000000 - 实际和`、`t = null` 的哑元** ——
//! 抽到它就返回 `null`（这一抽什么都不出）。
//!
//! 实测 `D-掉落2-DropTwo.csv` 的 762 行里：
//! - **745 行**权重和正好 `1000000`
//! - **17 行**不足（`10101`~`11001` 等是 `500000`，还有 `350000`/`400000`/`450000`/
//!   `600000`/`250000`）→ 这些行有 **50%~75% 的空抽概率**
//!
//! 这是配置本意（保底前的稀有掉落），不是 bug。照抄。
//!
//! ## ⚠ `formulaRand` 的比较是严格小于
//! ```java
//! boolean ret = UtilsTools.random(0, FeastSeatInfo.CANNOT_ATTACT_TIME_OFFSET) < value;
//! ```
//! `CANNOT_ATTACT_TIME_OFFSET` 见 [`CANNOT_ATTACK_TIME_OFFSET`]（实测 `1000000`）。
//! 随机区间是 `[0, 1000000]`（**闭区间，1000001 个值**），`< value` 严格小于。
//!
//! ## ⚠ 两个公式的 VIP 项是**整数除法**
//! ```java
//! float value2 = 1.0f - (player.getVipLv() / formulaValue.get(2));     // int / int → int
//! float value3 = 1.0f - (player.getVipLv() / formulaValue.get(3));
//! ```
//! `getVipLv()` 与 `formulaValue.get(2)` 都是 `int`，所以除完是**整数**再转 `float`。
//! VIP 低于除数时结果是 `1.0f - 0 = 1.0f`（不打折），
//! 高于除数时才会 < 1。已在 [`formula_value`] 里用整数除法照抄。
//!
//! ## `DropSignDB.onRand` 的四种组合
//! ```java
//! if ( randOK && addType == 1) counter.addTimes(id, 1);
//! if (!randOK && addType == 0) counter.addTimes(id, 1);
//! if ( randOK && resetType == 1) counter.resetTimes(id);
//! ```
//! - `addType=1`：**成功**才累加（保底计数）
//! - `addType=0`：**失败**才累加（连败计数）
//! - `resetType=1`：成功时清空

use std::collections::HashMap;

/// `FeastSeatInfo.CANNOT_ATTACT_TIME_OFFSET`（原版拼写就是 `ATTACT`）。
///
/// 实测值 `1000000`：`formulaRand` 的随机上界。
pub const CANNOT_ATTACK_TIME_OFFSET: i64 = 1_000_000;

/// `randItem` 的 `minWeightTheSum`（`DropTwoDB.randItem` 里硬编码）。
pub const MIN_WEIGHT_SUM: i32 = 1_000_000;

/// `DropSignDB`（掉落下标）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DropSign {
    /// `id`。
    pub id: i32,
    /// `signType`：`1` 玩家私有 / `2` 全服共享。
    pub sign_type: i32,
    /// `addType`：`1` 成功累加 / `0` 失败累加。
    pub add_type: i32,
    /// `resetType`：`1` 成功时重置。
    pub reset_type: i32,
    /// `timeReset`：固定时间重置（本次未使用）。
    pub time_reset: i32,
}

/// `DropTwoDB.ItemWithWeight`：`items` 的一行 + 对应权重。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropItem {
    /// 道具 id。
    pub item_id: i32,
    /// 数量下限（`item.get(1)`）。
    pub num_min: i64,
    /// 数量上限（`item.get(2)`）。
    pub num_max: i64,
    /// `probability` 列的值。
    pub weight: i32,
}

impl DropItem {
    /// `ItemWithWeight.getRandNum()`：`UtilsTools.random(num_min, num_max)`（闭区间）。
    ///
    /// 抽到数量由调用方注入的随机源决定，纯函数只暴露区间。
    #[must_use]
    pub const fn num_range(&self) -> (i64, i64) {
        (self.num_min, self.num_max)
    }
}

/// `DropTwoDB`（二级掉落）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropTwo {
    /// `id`。
    pub id: i32,
    /// `formulaId`：`0` 直接抽 / `1` 公式一 / `2` 公式二。
    pub formula_id: i32,
    /// `signId`：关联的下标 id 列表。
    pub sign_id: Vec<i32>,
    /// `formulaValue`：公式系数。
    pub formula_value: Vec<i32>,
    /// `items` + `probability` 配对后的结果。
    pub items: Vec<DropItem>,
}

/// `DropOneDB`（一级掉落）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropOne {
    /// `id`。
    pub id: i32,
    /// `extractNum`：抽几次。
    pub extract_num: i32,
    /// `idTwo`：候选的二级掉落 id。
    pub id_two: Vec<i32>,
    /// `givenDropId`：`(第几次, 二级 id)` 的强制指定。
    pub given_drop: Vec<(i32, i32)>,
}

/// `DropSignModel` / `DBGlobalDropModule` 的计数视图。
///
/// 原版是两层：`signType == 2` 走全服计数，否则走玩家私有。
/// 这里由调用方分别传入两个 map，纯函数不关心来源。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropCounters {
    /// 玩家私有计数（`PrayModel` / `DropSignModel`）。
    pub private: HashMap<i32, i64>,
    /// 全服共享计数（`DBGlobalDropModule`）。
    pub global: HashMap<i32, i64>,
}

impl DropCounters {
    /// `DropUtils.getTimes(id, model)`：`signType == 2` 走全局。
    #[must_use]
    pub fn times(&self, sign: &DropSign) -> i64 {
        let map = if sign.sign_type == 2 {
            &self.global
        } else {
            &self.private
        };
        map.get(&sign.id).copied().unwrap_or(0)
    }
}

/// `DropUtils.getGivenDropId(one, modelOne)`（`:83-93`）。
///
/// 返回命中的二级掉落 id；没命中返回 `None`。
///
/// ⚠ 内部用 `times + 1` 比对（1-based 的「第几次」）。
#[must_use]
pub fn given_drop_id(one: &DropOne, times_done: i64) -> Option<i32> {
    let nth = times_done + 1;
    one.given_drop
        .iter()
        .find(|(n, _)| i64::from(*n) == nth)
        .map(|(_, id)| *id)
}

/// 公式判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaOutcome {
    /// 命中 → 必出（`randItem` 里再按 weight 抽？不 —— 原版是直接返回该 `DropTwoDB`
    /// 然后 `randItem()`，仍然要走权重）。
    Hit,
    /// 未命中。
    Miss,
}

/// `DropUtils.getFormulaValue1`（`:131-146`）与 `getFormulaValue2`（`:148-163`）。
///
/// 返回 `value`；`<= 0` 表示这次必定不命中（原版直接 `return 0L`）。
///
/// ⚠ VIP 项是 **int / int 整数除法**再转 float（见模块文档）。
#[must_use]
pub fn formula_value(
    two: &DropTwo,
    vip_lv: i32,
    times_of_sign: impl Fn(i32) -> i64,
) -> i64 {
    let fv = &two.formula_value;
    if two.formula_id == 1 {
        // `getTimes(signId[0], model) - formulaValue.get(1)`
        let Some(&s0) = two.sign_id.first() else {
            return 0;
        };
        let Some(&c1) = fv.get(1) else { return 0 };
        let v1 = times_of_sign(s0) - i64::from(c1);
        if v1 <= 0 {
            return 0;
        }
        let Some(&c2) = fv.get(2) else { return 0 };
        // `player.getVipLv() / formulaValue.get(2)` —— int / int
        let vip_term = if c2 == 0 { 0 } else { vip_lv / c2 };
        let v2 = 1.0f32 - vip_term as f32;
        if v2 < 0.0 {
            return 0;
        }
        let Some(&c0) = fv.first() else { return 0 };
        (f64::from(c0) * v1 as f64 * f64::from(v2)) as i64
    } else {
        // formulaId != 1 → 走公式二
        let (Some(&s0), Some(&s1)) = (two.sign_id.first(), two.sign_id.get(1)) else {
            return 0;
        };
        let Some(&c1) = fv.get(1) else { return 0 };
        let v1 = times_of_sign(s0) - i64::from(c1);
        if v1 <= 0 {
            return 0;
        }
        let Some(&c2) = fv.get(2) else { return 0 };
        let v2 = times_of_sign(s1) - i64::from(c2);
        if v2 < 0 {
            return 0;
        }
        let Some(&c3) = fv.get(3) else { return 0 };
        let vip_term = if c3 == 0 { 0 } else { vip_lv / c3 };
        let v3 = 1.0f32 - vip_term as f32;
        if v3 < 0.0 {
            return 0;
        }
        let Some(&c0) = fv.first() else { return 0 };
        (f64::from(c0) * v1 as f64 * v2 as f64 * f64::from(v3)) as i64
    }
}

/// `DropUtils.formulaRand(two, model)`（`:95-102`）的判定部分。
///
/// ⚠ 随机区间 `[0, CANNOT_ATTACK_TIME_OFFSET]` 是**闭区间**，比较是 `<`。
#[must_use]
pub fn formula_hit(value: i64, roll: i64) -> bool {
    roll < value
}

/// `WeightRandomModelUtils::random()` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightPick {
    /// 抽到第 `index` 项。
    Hit(usize),
    /// 抽到哑元（`minWeightTheSum` 补齐的那个），本次不出东西。
    Empty,
    /// 权重和为 0（原版 `random(1, 0)` 会抛异常）。
    ZeroWeight,
}

/// `WeightRandomModelUtils(items, minWeightTheSum).random()`。
///
/// `roll` 是 `UtilsTools.random(1, weightTheSum)` 的结果，由调用方注入。
///
/// ⚠ 实现照抄原版：**累计上界**判定 `roll <= probability + weight`。
#[must_use]
pub fn pick_weight(items: &[DropItem], min_weight_sum: i32, roll: i32) -> WeightPick {
    let actual: i32 = items.iter().map(|i| i.weight).sum();
    // `if (var > 0) { weightTheSum = minWeightTheSum; add DEGModel(minWeightTheSum, null) }`
    // ⚠ 原版补的哑元 weight 是 `this.weightTheSum`（**补齐后的总和**），不是差值！
    // 见 `WeightRandomModelUtils.java:56`：`add(new DEGModel(this.weightTheSum, null))`。
    let total = if min_weight_sum - actual > 0 {
        min_weight_sum
    } else {
        actual
    };
    if total <= 0 {
        return WeightPick::ZeroWeight;
    }
    let mut acc = 0i32;
    for (i, item) in items.iter().enumerate() {
        // `if (iRandom <= probability + degModel.getProbability())`
        if roll <= acc + item.weight {
            return WeightPick::Hit(i);
        }
        acc += item.weight;
    }
    // 落到哑元（补齐的部分）
    WeightPick::Empty
}

/// `DropUtils.onRand(sign, player, model, randOK)`（`:165-176`）的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignAction {
    /// 是否 `addTimes(id, 1)`。
    pub add_times: bool,
    /// 是否 `resetTimes(id)`。
    pub reset_times: bool,
}

/// `DropUtils.onRand` 的纯逻辑。
#[must_use]
pub fn sign_action(sign: &DropSign, rand_ok: bool) -> SignAction {
    SignAction {
        add_times: (rand_ok && sign.add_type == 1) || (!rand_ok && sign.add_type == 0),
        reset_times: rand_ok && sign.reset_type == 1,
    }
}

/// 掉落三表的只读视图（由 `game-core` 的 CSV 加载器实现）。
///
/// 原版是三个静态 Cache（`DropOneDBCache.get` 等）。
pub trait DropTables: Send + Sync {
    /// `DropOneDBCache.get(id)`。
    fn one(&self, id: i32) -> Option<DropOne>;
    /// `DropTwoDBCache.get(id)`。
    fn two(&self, id: i32) -> Option<DropTwo>;
    /// `DropSignDBCache.get(id)`。
    fn sign(&self, id: i32) -> Option<DropSign>;
}

static TABLES: std::sync::OnceLock<std::sync::Arc<dyn DropTables>> = std::sync::OnceLock::new();

/// 注入掉落表（只第一次生效）。
pub fn install_tables(t: std::sync::Arc<dyn DropTables>) {
    let _ = TABLES.set(t);
}

/// 取掉落表。
#[must_use]
pub fn tables() -> Option<&'static std::sync::Arc<dyn DropTables>> {
    TABLES.get()
}

/// `ResourcesJson.mergeResourcesJson(list)`：同 id 累加。
#[must_use]
pub fn merge(mut list: Vec<(i32, i64)>) -> Vec<(i32, i64)> {
    let mut order: Vec<i32> = Vec::new();
    let mut acc: HashMap<i32, i64> = HashMap::new();
    for (id, num) in list.drain(..) {
        if !acc.contains_key(&id) {
            order.push(id);
        }
        *acc.entry(id).or_insert(0) += num;
    }
    // 原版 `LinkedHashMap` 保留首次出现顺序
    order
        .into_iter()
        .filter_map(|id| acc.get(&id).map(|n| (id, *n)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(sign_type: i32, add_type: i32, reset_type: i32) -> DropSign {
        DropSign {
            id: 1,
            sign_type,
            add_type,
            reset_type,
            time_reset: 0,
        }
    }

    fn item(id: i32, w: i32) -> DropItem {
        DropItem {
            item_id: id,
            num_min: 1,
            num_max: 1,
            weight: w,
        }
    }

    // ── 权重抽取 ──

    #[test]
    fn pick_first_item_when_roll_small() {
        let items = vec![item(1, 100), item(2, 900)];
        assert_eq!(pick_weight(&items, 1000, 50), WeightPick::Hit(0));
    }

    #[test]
    fn pick_second_item_at_boundary() {
        let items = vec![item(1, 100), item(2, 900)];
        // roll=100 命中第 0（<=100）；roll=101 落到第 1
        assert_eq!(pick_weight(&items, 1000, 100), WeightPick::Hit(0));
        assert_eq!(pick_weight(&items, 1000, 101), WeightPick::Hit(1));
    }

    #[test]
    fn pick_last_item_at_total() {
        let items = vec![item(1, 100), item(2, 900)];
        assert_eq!(pick_weight(&items, 1000, 1000), WeightPick::Hit(1));
    }

    #[test]
    fn insufficient_weight_yields_empty() {
        // 实测 10101 这类行：权重和 500000 < minWeightSum 1000000
        let items = vec![item(3, 200_000), item(202, 150_000), item(201, 150_000)];
        // roll 落在补齐区间（500001..1000000）→ 空
        assert_eq!(
            pick_weight(&items, MIN_WEIGHT_SUM, 700_000),
            WeightPick::Empty,
            "补齐的哑元吃掉 50% 概率"
        );
        // roll 落在实际区间 → 命中
        assert_eq!(
            pick_weight(&items, MIN_WEIGHT_SUM, 100_000),
            WeightPick::Hit(0)
        );
    }

    #[test]
    fn exact_weight_sum_has_no_dummy() {
        let items = vec![item(1, 400_000), item(2, 600_000)];
        // 权重和正好 1000000 → 没有哑元，roll=1000000 落在最后一项
        assert_eq!(
            pick_weight(&items, MIN_WEIGHT_SUM, 1_000_000),
            WeightPick::Hit(1)
        );
    }

    #[test]
    fn zero_weight_is_reported() {
        assert_eq!(pick_weight(&[], MIN_WEIGHT_SUM, 1), WeightPick::Empty);
        let zero = vec![item(1, 0)];
        // 实际和 0、minWeightSum 1000000 → total=1000000，哑元吃掉全部
        assert_eq!(pick_weight(&zero, MIN_WEIGHT_SUM, 500_000), WeightPick::Empty);
    }

    // ── 强制掉落 ──

    #[test]
    fn given_drop_matches_nth_pull() {
        let one = DropOne {
            id: 1,
            extract_num: 1,
            id_two: vec![101],
            given_drop: vec![(3, 999)],
        };
        // times_done=2 → nth=3 → 命中
        assert_eq!(given_drop_id(&one, 2), Some(999));
        assert_eq!(given_drop_id(&one, 1), None);
        assert_eq!(given_drop_id(&one, 3), None);
    }

    #[test]
    fn given_drop_empty_never_matches() {
        let one = DropOne {
            id: 1,
            extract_num: 1,
            id_two: vec![101],
            given_drop: vec![],
        };
        assert_eq!(given_drop_id(&one, 0), None);
    }

    // ── 公式 ──

    #[test]
    fn formula1_zero_when_times_not_reached() {
        let two = DropTwo {
            formula_id: 1,
            sign_id: vec![1],
            formula_value: vec![100, 5, 100],
            ..Default::default()
        };
        // times=5, c1=5 → v1=0 → 返回 0
        assert_eq!(formula_value(&two, 0, |_| 5), 0);
        assert_eq!(formula_value(&two, 0, |_| 4), 0);
    }

    #[test]
    fn formula1_computes_with_integer_vip_division() {
        // c0=100, c1=5, c2=10；times=15 → v1=10；vip=5 → 5/10=0 → v2=1.0 → 100*10*1=1000
        let two = DropTwo {
            formula_id: 1,
            sign_id: vec![1],
            formula_value: vec![100, 5, 10],
            ..Default::default()
        };
        assert_eq!(formula_value(&two, 5, |_| 15), 1_000);
        // vip=15 → 15/10=1 → v2=0.0 → 0
        assert_eq!(formula_value(&two, 15, |_| 15), 0);
        // vip=20 → 20/10=2 → v2=-1 → <0 → 0
        assert_eq!(formula_value(&two, 20, |_| 15), 0);
    }

    #[test]
    fn formula2_needs_two_signs() {
        let two = DropTwo {
            formula_id: 2,
            sign_id: vec![1, 2],
            formula_value: vec![10, 1, 1, 100],
            ..Default::default()
        };
        // times[1]=5 → v1=4；times[2]=5 → v2=4；vip=0 → v3=1 → 10*4*4*1=160
        let mut t = HashMap::new();
        t.insert(1, 5);
        t.insert(2, 5);
        assert_eq!(formula_value(&two, 0, |id| *t.get(&id).unwrap_or(&0)), 160);
    }

    #[test]
    fn formula2_zero_when_second_sign_not_reached() {
        let two = DropTwo {
            formula_id: 2,
            sign_id: vec![1, 2],
            formula_value: vec![10, 1, 5, 100],
            ..Default::default()
        };
        let mut t = HashMap::new();
        t.insert(1, 5);
        t.insert(2, 3); // 3 - 5 = -2 < 0 → 0
        assert_eq!(formula_value(&two, 0, |id| *t.get(&id).unwrap_or(&0)), 0);
    }

    #[test]
    fn formula_hit_is_strict_less_than() {
        assert!(formula_hit(1000, 999));
        assert!(!formula_hit(1000, 1000), "等于不算命中");
        assert!(!formula_hit(1000, 1001));
        assert!(!formula_hit(0, 0), "value=0 时永不命中");
    }

    // ── 下标动作 ──

    #[test]
    fn add_type_1_accumulates_on_success() {
        let a = sign_action(&sign(1, 1, 0), true);
        assert!(a.add_times);
        assert!(!a.reset_times);
        let a = sign_action(&sign(1, 1, 0), false);
        assert!(!a.add_times, "addType=1 失败不累加");
    }

    #[test]
    fn add_type_0_accumulates_on_failure() {
        let a = sign_action(&sign(1, 0, 0), false);
        assert!(a.add_times);
        let a = sign_action(&sign(1, 0, 0), true);
        assert!(!a.add_times, "addType=0 成功不累加");
    }

    #[test]
    fn reset_type_1_resets_on_success_only() {
        let a = sign_action(&sign(1, 0, 1), true);
        assert!(a.reset_times);
        let a = sign_action(&sign(1, 0, 1), false);
        assert!(!a.reset_times);
    }

    // ── 计数视图 ──

    #[test]
    fn sign_type_2_uses_global_counter() {
        let mut c = DropCounters::default();
        c.private.insert(1, 10);
        c.global.insert(1, 99);
        assert_eq!(c.times(&sign(1, 0, 0)), 10);
        assert_eq!(c.times(&sign(2, 0, 0)), 99);
    }

    #[test]
    fn missing_counter_is_zero() {
        let c = DropCounters::default();
        assert_eq!(c.times(&sign(1, 0, 0)), 0);
    }

    // ── 合并 ──

    #[test]
    fn merge_sums_same_id() {
        assert_eq!(
            merge(vec![(1, 10), (2, 5), (1, 3)]),
            vec![(1, 13), (2, 5)],
            "同 id 累加，保留首次出现顺序"
        );
    }

    #[test]
    fn merge_empty() {
        assert!(merge(vec![]).is_empty());
    }

    // ── 常量 ──

    #[test]
    fn constants_match_original() {
        assert_eq!(CANNOT_ATTACK_TIME_OFFSET, 1_000_000);
        assert_eq!(MIN_WEIGHT_SUM, 1_000_000);
    }
}
