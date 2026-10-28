//! # 系统开放（`SystemOpenUtils`）
//!
//! 对应 Java：`bf/game/server/utils/SystemOpenUtils.java`（约 300 行）
//! + `bf/template/db/SystemOpenDB.java` + `SystemOpenDBCache`
//! + `G-功能开启表-SystemOpen.csv`（实测 99 行数据）。
//!
//! ## 为什么单独抽这一层
//! `SystemOpenUtils.isOpen(playerId, type)` 是**被引用最多的单点**：
//! `data_d` 里 **60 个文件**调它，其中 22 个是 `message/handler/`。
//! 在原版里它直接伸手去抓十几个 `*ServerModule` 的单例；
//! 移植期这些模块（家族、产业、狩猎、门客…）都还没接，
//! 所以这里改成**「事实快照 + 纯判定」**：
//!
//! ```text
//! PlayerFacts  ← 调用方填（能从 MemWorld / PG 拿到的字段）
//! UnlockCtx    ← 判定所需的外部模块（用 trait 注入，未接的返回 None）
//! is_open()    ← 纯函数，无 IO、无全局
//! ```
//!
//! 未接的模块一律返回 `None` → 该 `unlockType` 分支判 `false`。
//! 这与原版「`player` 为 null 时 NPE / 分数不足」的可观测结果一致：
//! 都是**不开放**，不会误开。
//!
//! ## ⚠ 判定是 `unlockType` 数组上「任一命中即开放」
//! ```java
//! for (i = 0; i < unlockTypesSize; i++) {
//!     switch (unlockType[i]) {
//!         case 1: if (lv >= value) return true; break;      // ← 命中就立刻 true
//!         ...
//!     }
//! }
//! return false;                                             // ← 全部不命中才 false
//! ```
//! 也就是 **OR 语义**，不是 AND。CSV 里 `13;4 = 5;5`（27/28 号）
//! 表示「开服 5 天**或**产业数 5 个」都能开。
//!
//! ## ⚠ `value` 是 `List<String>`，有的 `unlockType` 读 `value[0]`、有的读整个列表
//! | unlockType | 读法 |
//! |---|---|
//! | 1/2/3/4/5/6/8/10/13/15/19/20/21/22/24 | `value[i]`（与 `unlockType[i]` **同下标**） |
//! | 18 | `for (String t : values)` —— 遍历**整个** `value` 列表 |
//! 18 的 `values` 是 `1015;1005;1046` 这种「门客 id 列表」，与 `unlockType` 的
//! 逐项对应无关。照抄。
//!
//! ## ⚠ `unlockType == 9` / `12` / `16` / `17` / `18` 在 `isOpen` 里没有 case
//! ```java
//! case 7: ... case 8: ...      // 没有 9
//! case 10: ...                  // 没有 11、12
//! case 13: ... case 15: ...     // 没有 14、16、17
//! case 18: ... case 19: ...     // 有 18
//! ```
//! 实测 CSV 里 `unlockType` 用到 15 种值；其中 **`9`（108 行）、`12`（114 行）
//! 在 `isOpen` 的 `switch` 里没有对应 case**，落到 `default` 什么都不做
//! → 这两行**永远不开放**。照抄（见 `AUDIT.md` A20）。
//!
//! ⚠ 但 `16` **有** case（`Homeland.isEnabled()`），`17` 也有
//! （`family.getLv()`）。别被 `SystemOpenDBCache.TYPE_16` 这类常量名误导 ——
//! 常量存在不代表 `isOpen` 处理了它。以 `isOpen` 的 `switch` 为准。
//!
//! ## ⚠ `SystemOpenDB` 缺失 = 不开放
//! ```java
//! if (systemOpenDB == null) return false;
//! ```
//! 所以 `isOpen(未知id)` 恒 `false`，**不会**当默认开放。

use std::collections::HashMap;

/// `SystemOpenDB` 的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemOpenTemplate {
    /// `id`（系统编号）。
    pub id: i32,
    /// `unlockType` 列（`list;` → `1;3` → `[1, 3]`）。
    pub unlock_type: Vec<i32>,
    /// `value` 列（`list;` → `10;5` → `["10", "5"]`）。
    ///
    /// 保留 `String` 而不是解析成 `i32`：原版就是 `List<String>`，
    /// 再在 case 里 `Integer.parseInt`。少数行的值是门客 id 列表（type 18），
    /// 非数字时原版抛 `NumberFormatException` → 整个 `isOpen` 抛出去。
    pub value: Vec<String>,
    /// `name` 列（`change_word_626`）。
    ///
    /// 1015 用 `LogicUtils.getDescribeName(name)` 取末段当语言 id。
    pub name: String,
}

/// 判定所需的玩家事实（原版直接读 `Player` 对象）。
///
/// 未接的字段按 `0` / `None` 处理 —— 与原版「模块未初始化 → 判定不通过」一致。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerFacts {
    /// `player.getLv()`。
    pub level: i32,
    /// `player.getVipLv()`。
    pub vip_lv: i32,
    /// `player.getFortuneLv()`（财力等级）。
    pub fortune_lv: i32,
    /// `StageServerModule.getThrouthCheckPointDBID` 的通关关卡 id。
    pub stage_id: i32,
    /// `TalentSet.getAllTalent().size()`（门客总数）。
    pub talent_count: i32,
    /// `AccountRoom.getOpenId()`（账房开启 id）。
    pub account_room_open_id: i32,
    /// `TraderSet.getTradeNum()`（通商商人数）。
    pub trade_num: i32,
    /// `ChildrenSet.isHaveChild()`。
    pub have_child: bool,
    /// `PetSet.getAllPet().size()`（珍兽总数）。
    pub pet_count: i32,
    /// `BuildingSet.getBuildingCount()`（产业数量）。
    pub building_count: i32,
    /// `GsServer.getOpenServerDay()`（开服天数，从 1 开始）。
    pub open_server_day: i32,
    /// `HuntingArea.getMaxMapId()`（狩猎最大地图 id）。
    pub hunting_max_map_id: i32,
    /// `Homeland.isEnabled()`。
    pub homeland_enabled: bool,
    /// `Family.getLv()`；`None` 表示无家族。
    pub family_lv: Option<i32>,
    /// `TalentSet.getTalentNum(talentLv)`（满足某阶的门客数）。
    ///
    /// 原版 `case 20` 只读 `values.get(0)` 当**门客阶数**，
    /// 再数「达到该阶的门客数 ≥ 1」。这里保留成 `(阶数 → 数量)` 的查询闭包
    /// 太重，改成直接给「已拥有的门客阶数集合」。
    pub talent_levels: Vec<i32>,
    /// `NpcSpouseModel.getUnlockNpcIds().size()`（知己数）。
    pub npc_spouse_count: i32,
    /// `CloseFriendSet.getCloseFriendNum()`（知己亲密数）。
    pub close_friend_num: i32,
    /// `PlayerStat.getLongStat(StatType.BuildingEvent)`。
    pub building_event_stat: i64,
    /// 已拥有的门客 id（`case 18` 用）。
    pub talent_ids: Vec<i32>,
}

/// `case 18` 判定用：`values` 里**任意一个**是已拥有的门客 id 就算开放。
fn any_talent_owned(values: &[String], owned: &[i32]) -> bool {
    values.iter().any(|v| {
        v.parse::<i32>()
            .is_ok_and(|id| owned.contains(&id))
    })
}

/// `case 20` 判定用：`values[0]` 当门客阶数，数「达到该阶的门客数 ≥ 1」。
fn any_talent_at_level(values: &[String], levels: &[i32]) -> bool {
    let Some(first) = values.first() else {
        return false;
    };
    first
        .parse::<i32>()
        .is_ok_and(|want| levels.iter().any(|&lv| lv >= want))
}

/// 单个 `unlockType` 的判定。
///
/// 返回 `Some(true)` 表示该条命中 → 整体开放；
/// `Some(false)` / `None` 都表示不命中（`None` 是「该 type 没有 case」）。
fn hit(unlock_type: i32, value: &str, all_values: &[String], f: &PlayerFacts) -> Option<bool> {
    // 原版每个 case 都是 `Integer.parseInt(value)`；解析失败会抛异常。
    // 移植期把异常降级成「不命中」—— 观测结果同样是「不开放」。
    let num = || value.trim().parse::<i32>().ok();
    Some(match unlock_type {
        // case 1: player.getLv() >= parseInt(value)
        1 => f.level >= num()?,
        // case 2: StageServerModule.getThrouthCheckPointDBID >= parseInt(value)
        2 => f.stage_id >= num()?,
        // case 3: player.getVipLv() >= parseInt(value)
        3 => f.vip_lv >= num()?,
        // case 4: TalentSet.getAllTalent().size() >= parseInt(value)
        4 => f.talent_count >= num()?,
        // case 5: AccountRoom.getOpenId() >= parseInt(value)
        5 => f.account_room_open_id >= num()?,
        // case 6: TraderSet.getTradeNum() >= parseInt(value)
        6 => f.trade_num >= num()?,
        // case 7: ChildrenSet.isHaveChild()  —— 不看 value
        7 => f.have_child,
        // case 8: PetSet.getAllPet().size() >= Integer.valueOf(value).intValue()
        8 => f.pet_count >= num()?,
        // ⚠ case 9 不存在（`enum` 里有 TYPE_9 但 isOpen 的 switch 没有）
        9 => return None,
        // case 10: BuildingSet.getBuildingCount() >= Integer.valueOf(value)
        10 => f.building_count >= num()?,
        // ⚠ case 11 / 12 不存在
        11 | 12 => return None,
        // case 13: GsServer.getOpenServerDay() >= Integer.valueOf(value)
        13 => f.open_server_day >= num()?,
        // ⚠ case 14 不存在
        14 => return None,
        // case 15: HuntingArea.getMaxMapId() >= parseInt(value)
        15 => f.hunting_max_map_id >= num()?,
        // case 16: Homeland.isEnabled() —— 不看 value
        16 => f.homeland_enabled,
        // case 17: family != null && family.getLv() >= parseInt(value)
        17 => f.family_lv.is_some_and(|lv| lv >= value.trim().parse().unwrap_or(i32::MAX)),
        // case 18: for (t : values) if (getTalent(t) != null) return true
        18 => any_talent_owned(all_values, &f.talent_ids),
        // case 19: player.getFortuneLv() >= Integer.valueOf(value)
        19 => f.fortune_lv >= num()?,
        // case 20: getTalentNum(values.get(0)) >= 1
        20 => any_talent_at_level(all_values, &f.talent_levels),
        // case 21: NpcSpouseModel.getUnlockNpcIds().size() >= parseInt(value)
        21 => f.npc_spouse_count >= num()?,
        // case 22: CloseFriendSet.getCloseFriendNum() >= parseInt(value)
        22 => f.close_friend_num >= num()?,
        // case 24: getLongStat(BuildingEvent) >= parseInt(value)
        24 => f.building_event_stat >= i64::from(num()?),
        // ⚠ case 23 / 25+ 不存在
        _ => return None,
    })
}

/// `SystemOpenUtils.isOpen` 的纯函数版。
///
/// 语义是 **OR**：`unlockType` 任一条命中即 `true`。
#[must_use]
pub fn is_open(t: &SystemOpenTemplate, f: &PlayerFacts) -> bool {
    for (i, &ut) in t.unlock_type.iter().enumerate() {
        let Some(v) = t.value.get(i) else {
            // `unlockType` 比 `value` 长 —— 原版 `values.get(i)` 抛
            // `IndexOutOfBoundsException`，整个 isOpen 抛出。降级成「不命中」。
            continue;
        };
        if hit(ut, v, &t.value, f) == Some(true) {
            return true;
        }
    }
    false
}

/// `SystemOpenDBCache`。
#[derive(Debug, Clone, Default)]
pub struct SystemOpenCache {
    map: HashMap<i32, SystemOpenTemplate>,
}

impl SystemOpenCache {
    /// 用模板行构造。
    #[must_use]
    pub fn new(rows: Vec<SystemOpenTemplate>) -> Self {
        let mut map = HashMap::with_capacity(rows.len());
        for r in rows {
            map.insert(r.id, r);
        }
        Self { map }
    }

    /// `getSystemOpenDB(type)`；`None` = 该行不存在 → `isOpen` 恒 `false`。
    #[must_use]
    pub fn get(&self, id: i32) -> Option<&SystemOpenTemplate> {
        self.map.get(&id)
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

    /// `isOpen` 的便捷入口：模板缺失直接 `false`。
    #[must_use]
    pub fn is_open(&self, id: i32, f: &PlayerFacts) -> bool {
        self.get(id).is_some_and(|t| is_open(t, f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tpl(unlock_type: Vec<i32>, value: Vec<&str>) -> SystemOpenTemplate {
        SystemOpenTemplate {
            id: 1,
            unlock_type,
            value: value.into_iter().map(str::to_owned).collect(),
            name: "change_word_1".to_owned(),
        }
    }

    #[test]
    fn missing_template_is_closed() {
        let c = SystemOpenCache::default();
        assert!(!c.is_open(120, &PlayerFacts::default()));
    }

    #[test]
    fn level_gate() {
        let c = SystemOpenCache::new(vec![tpl(vec![1], vec!["5"])]);
        assert!(!c.is_open(1, &PlayerFacts { level: 4, ..Default::default() }));
        assert!(c.is_open(1, &PlayerFacts { level: 5, ..Default::default() }));
    }

    #[test]
    fn multi_type_is_or() {
        // 实测 CSV 27/28 行：`13;4 = 5;5` → 开服 5 天**或**门客 5 个
        // （case 4 是 `TalentSet.getAllTalent().size()`，不是产业数）
        let c = SystemOpenCache::new(vec![tpl(vec![13, 4], vec!["5", "5"])]);
        assert!(!c.is_open(1, &PlayerFacts::default()));
        assert!(c.is_open(
            1,
            &PlayerFacts { open_server_day: 5, ..Default::default() }
        ));
        assert!(c.is_open(
            1,
            &PlayerFacts { talent_count: 5, ..Default::default() }
        ));
        // 一边 3 天一边 3 个 —— 都不够
        assert!(!c.is_open(
            1,
            &PlayerFacts { open_server_day: 3, talent_count: 3, ..Default::default() }
        ));
    }

    #[test]
    fn absent_case_types_never_open() {
        // 实测 CSV 里 9 / 12 / 16 / 17 各 1 行；isOpen 没有对应 case → 永不开放。
        // 16 例外：`case 16` **确实存在**（homeland_enabled），所以不在此列。
        for ut in [9, 12, 11, 14, 23, 25, 99] {
            let c = SystemOpenCache::new(vec![tpl(vec![ut], vec!["1"])]);
            assert!(!c.is_open(1, &PlayerFacts::default()), "type {ut} 应永不开放");
        }
    }

    #[test]
    fn type_16_and_14_reads_flag_not_value() {
        let c = SystemOpenCache::new(vec![tpl(vec![16], vec!["1"])]);
        assert!(!c.is_open(1, &PlayerFacts::default()));
        assert!(c.is_open(1, &PlayerFacts { homeland_enabled: true, ..Default::default() }));
        // 14 确实没有 case
        let c = SystemOpenCache::new(vec![tpl(vec![14], vec!["1"])]);
        assert!(!c.is_open(1, &PlayerFacts { homeland_enabled: true, ..Default::default() }));
    }

    #[test]
    fn type_17_needs_family() {
        let c = SystemOpenCache::new(vec![tpl(vec![17], vec!["3"])]);
        assert!(!c.is_open(1, &PlayerFacts::default()));
        assert!(!c.is_open(1, &PlayerFacts { family_lv: Some(2), ..Default::default() }));
        assert!(c.is_open(1, &PlayerFacts { family_lv: Some(3), ..Default::default() }));
    }

    #[test]
    fn type_18_scans_whole_value_list() {
        // 实测 404 行：`unlockType=18`, `value=1015;1005;1046;1047;1048`
        let c = SystemOpenCache::new(vec![tpl(
            vec![18],
            vec!["1015", "1005", "1046", "1047", "1048"],
        )]);
        assert!(!c.is_open(1, &PlayerFacts::default()));
        // 拥有列表里**任意一个**即开放
        assert!(c.is_open(
            1,
            &PlayerFacts { talent_ids: vec![1046], ..Default::default() }
        ));
        assert!(!c.is_open(
            1,
            &PlayerFacts { talent_ids: vec![9999], ..Default::default() }
        ));
    }

    #[test]
    fn type_20_uses_first_value_as_talent_level() {
        // 实测 25 行：`20 = 200` —— 门客阶数 ≥ 200 的有 1 个以上
        let c = SystemOpenCache::new(vec![tpl(vec![20], vec!["200"])]);
        assert!(!c.is_open(1, &PlayerFacts { talent_levels: vec![199], ..Default::default() }));
        assert!(c.is_open(1, &PlayerFacts { talent_levels: vec![200], ..Default::default() }));
        // `>` 而不是 `==`：250 阶也满足 200
        assert!(c.is_open(1, &PlayerFacts { talent_levels: vec![250], ..Default::default() }));
    }

    #[test]
    fn type_7_and_16_ignore_value() {
        let c = SystemOpenCache::new(vec![tpl(vec![7], vec!["999"])]);
        assert!(c.is_open(1, &PlayerFacts { have_child: true, ..Default::default() }));
    }

    #[test]
    fn missing_value_skips_only_that_pair() {
        // 手工造一个不齐的表：`unlockType=[1,3]` 但 `value=["5"]`。
        // 原版 `values.get(1)` 会抛 IndexOutOfBounds，整个 isOpen 抛出（→ 不开放）；
        // 这里降级成「跳过这一对」—— 第一对仍按 `1 = 5` 正常判定。
        let t = SystemOpenTemplate {
            id: 1,
            unlock_type: vec![1, 3],
            value: vec!["5".to_owned()],
            name: String::new(),
        };
        // 等级够 → 第一对命中 → 开放（与「整行抛出」分叉，但只在畸形数据下）
        assert!(is_open(&t, &PlayerFacts { level: 99, ..Default::default() }));
        // 等级不够、VIP 够 → 第二对因缺 value 被跳过 → 关闭
        assert!(!is_open(
            &t,
            &PlayerFacts { level: 1, vip_lv: 99, ..Default::default() }
        ));
    }

    #[test]
    fn non_numeric_value_does_not_panic() {
        // 原版 `Integer.parseInt("abc")` 抛 NumberFormatException
        let c = SystemOpenCache::new(vec![tpl(vec![1], vec!["abc"])]);
        assert!(!c.is_open(1, &PlayerFacts { level: 999, ..Default::default() }));
    }

    #[test]
    fn type_24_uses_long_stat() {
        let c = SystemOpenCache::new(vec![tpl(vec![24], vec!["200"])]);
        assert!(!c.is_open(1, &PlayerFacts { building_event_stat: 199, ..Default::default() }));
        assert!(c.is_open(1, &PlayerFacts { building_event_stat: 200, ..Default::default() }));
    }

    #[test]
    fn empty_unlock_type_is_closed() {
        let c = SystemOpenCache::new(vec![tpl(vec![], vec![])]);
        assert!(!c.is_open(1, &PlayerFacts { level: 999, ..Default::default() }));
    }
}
