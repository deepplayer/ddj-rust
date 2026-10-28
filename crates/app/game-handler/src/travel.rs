//! # 游历（Travel）：15002 / 15004 / 15009 的纯逻辑
//!
//! 对应 Java：
//! - `bf/game/server/message/handler/TravelMessageHandler.java`（63 行，3 条消息）
//! - `bf/game/server/module/travel/Travel.java`（566 行）
//! - `bf/game/server/module/travel/TravelEventsDBUtils.java`（87 行）
//! - `bf/game/server/utils/TravelEventsDBUtils.java`
//! - `bf/common/utils/WeightRandomModelUtils.java:52-69`
//!
//! ## 三条消息
//! | cmd | 入口 | 前置 |
//! |---|---|---|
//! | 15002 游历 | `Travel.travel(player, req, false)` | `place ∈ {0,12}`；`isOpen(3)`；`allTimes != 0 → isOpen(102)` |
//! | 15004 十连 | `Travel.travelTenTimes(player)` | `isOpen(133)` |
//! | 15009 知己变性 | 直接改 `partnerSex` | `sex ∈ {0,1}`；与当前不同；`isOpen(161)`；扣 `PartnerChangeCost` |
//!
//! ## 本轮移植范围（重要）
//! `randomEvent` 会产出 4 类事件（`TravelEventType` 1/2/3/4），每类都要一整套
//! 未接模块（门客 `PetSet` / 家丁 `TalentSet` / 子嗣 `ChildrenSet` / 知己 `NpcSpouseModel`）。
//! 本轮做**全流程骨架 + 事件 3（纯资源奖励）**，事件 1/2/4 走「原版同一分支但只结算进度」：
//!
//! - 事件 3（`Reward`）：`addRewardJson` → 合并后 `addResourcesJsons`。**完整**。
//! - 事件 1/2/4：`progressValue > progress` 时原版也只 `addProgress` + 回 `MsgTravelProgress`，
//!   这条分支**不需要任何未接模块**，完整照抄。
//!   进度满了以后原版要发门客/家丁/子嗣/亲密度 —— 这部分跳过，见 [`crate::install_travel`]
//!   里的 `SKIPPED_EVENT_TYPES` 与 AUDIT A22。
//!
//! 好在这不是空转：`Y-游历事件-TravelEvents.csv` 84 行里
//! `progressValue > 0` 的行需要 1~5 次才满，前几次都走「只加进度」分支。
//!
//! ## 照抄的坑（详见 AUDIT A22）
//! 1. **`extractWeight` 是 `int`，`weightScale` 是 `double`**
//!    `getExtractWeightScale` 返回 `(int)(extractWeight * weightScale)` ——
//!    `133 * 1.3 = 172.90000000000002` → **截断成 172**，不是 173。
//! 2. **`random(1, weightTheSum)` 是 1-based 闭区间**
//!    所以 `roll` 从 1 开始；`roll <= acc + w` 的第一个命中（与掉落同一套）。
//! 3. **`travel()` 里 `eventDBs` 为空会数组越界**
//!    `this.place = eventDBs.get(eventDBs.size() - 1)` → `-1` 抛
//!    `ArrayIndexOutOfBoundsException`。但 `randomEvent` 的空列表只在
//!    `place == 0` 且体力为 0 时出现，而那时已经抛了 `TRAVEL_ERROR_PHYSICAL_LIMIT`。
//! 4. **`place == 12` 走印章（24），`place == 0` 走体力（计数器 1）**
//!    两条分支扣的资源完全不同；`place == 12` 且 `allTimes == 0` 时
//!    `while(true)` 里第二次就 `break`（`travelTimes > 0`），**只出 1 个事件**。
//! 5. **`tryGetSpecialTravelEvent` 只在 `place == 0` 生效，且前 5 次固定事件**
//!    `NotPlaceTravelTimes < 100` 时按 `TravelFirstEventGirl` 表 1→9101、2→12401…
//!    **先自增再比对**，所以第 1 次游历拿到的是 `times2 == 1` 那一行。
//!
//! ## 未接
//! - `PlayerStatServerModule` 的 `NotPlaceTravelTimes` / `TravelTimes` 长统计。
//!   本模块自己维护 `PlayerBox.travel_not_place_times`，语义一致。
//! - 奇遇（`PetMiracleType.ID_NPC_INTIMACY`）、时装加亲密度、子嗣出生。
//! - 推送 `MsgTravelDataResp_15001`（进度变化）—— 先推后回，顺序照抄。

use std::collections::HashMap;

/// `TravelEventType`（`TravelEventType.java`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    /// 1 = 门客。
    Talent,
    /// 2 = 珍兽。
    Pet,
    /// 3 = 奖励。
    Reward,
    /// 4 = 美人（知己）。
    NpcSpouse,
}

impl EventType {
    /// `TravelEventType.getByType`：未登记的类型返回 `None`（原版返回 `null`）。
    #[must_use]
    pub fn from_i32(t: i32) -> Option<Self> {
        match t {
            1 => Some(Self::Talent),
            2 => Some(Self::Pet),
            3 => Some(Self::Reward),
            4 => Some(Self::NpcSpouse),
            _ => None,
        }
    }

    /// 原版 `getType()`。
    #[must_use]
    pub fn as_i32(self) -> i32 {
        match self {
            Self::Talent => 1,
            Self::Pet => 2,
            Self::Reward => 3,
            Self::NpcSpouse => 4,
        }
    }
}

/// 一行 `Y-游历事件-TravelEvents.csv`（84 行）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TravelEvent {
    /// `id`
    pub id: i32,
    /// `type`：1 门客 / 2 珍兽 / 3 奖励 / 4 美人。
    pub event_type: i32,
    /// `place`：0 = 随机游历（不指定地点），12 = 桃花岛（知己），其余是普通景点。
    pub place: i32,
    /// `placeName` 是语言 id，这里保留原串。
    pub place_name: String,
    /// `unlockType`：0 无 / 1 身份等级 / 2 拥有某个门客。
    pub unlock_type: i32,
    /// `unlockValue`。
    pub unlock_value: i32,
    /// `targetId`。
    pub target_id: i32,
    /// `progressValue`：0 = 一次到位；>0 = 需要积累这么多次。
    pub progress_value: i32,
    /// `extractWeight`。
    pub extract_weight: i32,
    /// `weightScale`：进度满了之后的倍率（**double**）。
    pub weight_scale: f64,
    /// `intimacy` 列解析出的 `(id, num)`，原版 `intimacyJson`。
    pub intimacy: Option<(i32, i64)>,
    /// `reward` 列解析出的道具列表。
    pub rewards: Vec<(i32, i64)>,
}

/// 配置常量（`B-变量表-Config.csv`），由 `csv_travel` 注入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TravelConfig {
    /// 101 `PhysicalStrength`：随机游历每次消耗的体力。
    pub physical_strength: i64,
    /// 105 `FixedPointTravel`：定点游历消耗（`24=1`）。
    pub fixed_point_travel: (i32, i64),
    /// 102 `TravelFirstEventGirl`：历史次数 → 事件 id。
    pub first_event_girl: Vec<(i64, i32)>,
    /// 104 `TravelFirstEventOffspring`。
    pub first_event_offspring: i64,
    /// 572 `PartnerInviteTime`。
    pub partner_invite_time: i32,
    /// 818 `PartnerChangeCost`：知己变性消耗（`1=500`）。
    pub partner_change_cost: (i32, i64),
}

impl Default for TravelConfig {
    fn default() -> Self {
        Self {
            physical_strength: 1,
            fixed_point_travel: (24, 1),
            first_event_girl: Vec::new(),
            first_event_offspring: 4,
            partner_invite_time: 50,
            partner_change_cost: (1, 500),
        }
    }
}

/// `TravelEventsDBUtils.unlockTypeCheck`。
#[must_use]
pub fn unlock_ok(ev: &TravelEvent, level: i32, owned_pets: &[i32]) -> bool {
    match ev.unlock_type {
        1 => level >= ev.unlock_value,
        2 => owned_pets.contains(&ev.unlock_value),
        // `default: return true` —— 0 和任何未知类型都放行
        _ => true,
    }
}

/// `TravelEventsDBUtils.filter`。
#[must_use]
pub fn filter<'a>(
    events: &'a [TravelEvent],
    place: i32,
    level: i32,
    owned_pets: &[i32],
) -> Vec<&'a TravelEvent> {
    events
        .iter()
        .filter(|e| if place != 0 { e.place == place } else { true })
        .filter(|e| unlock_ok(e, level, owned_pets))
        .collect()
}

/// `TravelEventsDBUtils.getExtractWeightScale`。
///
/// ⚠ `(int)(extractWeight * weightScale)` —— Java 的 `(int)` 是**截断**。
/// 例：`133 * 1.3` 在 f64 下是 `172.90000000000002`，截断成 `172`。
#[must_use]
pub fn extract_weight_scale(ev: &TravelEvent, progress: i32) -> i32 {
    if progress >= ev.progress_value {
        // Rust 的 `as i32` 对 f64 也是向零截断，与 Java `(int)` 一致。
        #[allow(clippy::cast_possible_truncation)]
        let v = (f64::from(ev.extract_weight) * ev.weight_scale) as i32;
        v
    } else {
        ev.extract_weight
    }
}

/// `WeightRandomModelUtils.random()` 的选中结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// 命中第 `i` 个候选。
    Hit(usize),
    /// `weightTheSum <= 0`，原版 `random(1, 0)` 抛 `IllegalArgumentException`。
    Empty,
}

/// 按权重选一个。
///
/// - `weights` 是各候选的权重（与 `list` 同序）。
/// - `roll` 是 `random(1, weightTheSum)` 的结果（**1-based 闭区间**）。
///
/// 原版遍历时 `iRandom <= probability + w` **第一个**命中即返回；
/// 全部不中返回 `null`（浮点/整数边界下可能发生），这里归到 `Empty`。
#[must_use]
pub fn pick_weighted(weights: &[i32], roll: i32) -> Pick {
    let total: i32 = weights.iter().sum();
    if total <= 0 {
        return Pick::Empty;
    }
    let mut acc = 0i32;
    for (i, w) in weights.iter().enumerate() {
        if roll <= acc + w {
            return Pick::Hit(i);
        }
        acc += w;
    }
    Pick::Empty
}

/// `Travel.addProgress` 的返回值：第一次必然是 1。
#[must_use]
pub fn add_progress(map: &mut HashMap<i32, i32>, id: i32) -> i32 {
    let cur = map.entry(id).or_insert(0);
    *cur += 1;
    *cur
}

/// `Travel.getProgress`：缺省 0。
#[must_use]
pub fn progress_of(map: &HashMap<i32, i32>, id: i32) -> i32 {
    map.get(&id).copied().unwrap_or(0)
}

/// `TravelEventsDBUtils.tryGetSpecialTravelEvent` 的判定部分。
///
/// 原版流程（只在 `place == 0` 生效）：
/// ```java
/// if (times >= 100) return null;
/// addLongStat(..., 1);              // ← 先自增
/// long times2 = getLongStat(...);   // ← 再读
/// for (ResourcesJson r : TravelFirstEventGirl) {
///     if (times2 == r.getId()) return cache.get(r.getNum());
/// }
/// ```
/// 所以第 N 次游历拿到的是 `key == N` 那一行。
#[must_use]
pub fn special_event_id(cfg: &TravelConfig, times_before_inc: i64) -> Option<i32> {
    // `if (times >= 100) return null;` —— 判的是**自增前**的值。
    if times_before_inc >= 100 {
        return None;
    }
    // 自增后再比对，所以查的是 `times_before_inc + 1`。
    let after = times_before_inc + 1;
    cfg.first_event_girl
        .iter()
        .find(|(k, _)| *k == after)
        .map(|(_, v)| *v)
}

/// `randomEvent` 的循环模型。
///
/// 原版 `while (true)` 分两支，两支都有各自的 break 条件，而且互相有耦合。
/// 这里把它拆成两个纯函数：
/// - [`plan_fixed`]：`place == 12`（印章）
/// - [`plan_random`]：`place != 12`（体力）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountPlan {
    /// 计划抽几个事件。
    pub count: usize,
    /// 需要扣的体力（`place != 12`）。
    pub stamina: i64,
    /// 需要扣的印章个数（`place == 12`）。
    pub stamps: i64,
}

/// `place == 12` 的循环（`allTimes != 0` 与 `== 0` 两个子分支）。
///
/// ## `allTimes != 0`
/// ```java
/// if (travelTimes >= 10
///     && (travelCnt <= 10 || travelTimes >= Math.min(travelCnt, PartnerInviteTime))) break;
/// if (!checkResources(FixedPointTravel)) {
///     if (travelTimes == 0) break;  else throw TRAVEL_ERROR_RESOURCES_LIMIT;
/// }
/// ... travelTimes++;
/// if (allTimes == 0 || travelTimes >= curNum) break;   // ← 这里 allTimes != 0，所以看 curNum
/// ```
/// 注意第 2 个 `if` 在 `travelTimes == 0` 时**静默 break**（不抛），
/// 只有在已经抽了几次后才抛 `TRAVEL_ERROR_RESOURCES_LIMIT`。
///
/// ## `allTimes == 0`
/// ```java
/// if (travelTimes > 0) break;              // ← 只出 1 个
/// if (!checkResources(FixedPointTravel)) { ... 同上 }
/// ... travelTimes++;
/// if (place != 0) { }                      // ← 空块，不 break
/// ```
/// 所以 `allTimes == 0` 时**固定 1 个**（第二次进循环就 break）。
///
/// 返回 `(count, 是否需要抛 TRAVEL_ERROR_RESOURCES_LIMIT)`。
#[must_use]
pub fn plan_fixed(
    all_times: i32,
    travel_cnt: i32,
    cur_num: i64,
    stamps: i64,
    cfg: &TravelConfig,
) -> (usize, bool) {
    let need = cfg.fixed_point_travel.1;
    if all_times == 0 {
        // 只出 1 个；不够就静默 0 个
        return if stamps >= need && need > 0 { (1, false) } else { (0, false) };
    }
    let mut n = 0usize;
    loop {
        // `if (travelTimes >= 10 && ...) break;`
        if n >= 10 {
            let cnt = i64::from(travel_cnt);
            let invite = i64::from(cfg.partner_invite_time);
            if cnt <= 10 || i64::try_from(n).unwrap_or(i64::MAX) >= cnt.min(invite) {
                break;
            }
        }
        // `checkResources(FixedPointTravel)`
        let have = stamps - need * i64::try_from(n).unwrap_or(0);
        if have < need || need <= 0 {
            if n == 0 {
                return (0, false);
            }
            return (n, true);
        }
        n += 1;
        // `if (allTimes == 0 || travelTimes >= curNum) break;`
        if i64::try_from(n).unwrap_or(i64::MAX) >= cur_num {
            break;
        }
        // 上限保护：`curNum` 可能极大，别死循环
        if n >= 10_000 {
            break;
        }
    }
    (n, false)
}

/// `place != 12` 的循环（体力）。
///
/// ```java
/// if (!checkNum(playerId, 1, PhysicalStrength * eventDBs.size() + 1)) {
///     if (travelTimes != 0) break;
///     throw TRAVEL_ERROR_PHYSICAL_LIMIT;
/// }
/// eventDBs.add(randTravel(...));
/// travelTimes++;
/// ```
/// 所以上限是 `floor((curCount - 1) / PhysicalStrength) + 1` 个事件
/// （判的是 `PhysicalStrength * 已抽数 + 1`）。
///
/// 返回 `(count, 是否体力不足)`。`count == 0 && 不足` → 抛 `TRAVEL_ERROR_PHYSICAL_LIMIT`。
#[must_use]
pub fn plan_random(cur_count: i64, cfg: &TravelConfig) -> (usize, bool) {
    let per = cfg.physical_strength.max(1);
    if cur_count < 1 {
        return (0, true);
    }
    let max = (cur_count - 1) / per + 1;
    let n = usize::try_from(max).unwrap_or(0).min(10_000);
    (n, false)
}

/// `travelTenTimes` 的抽取计划。
#[must_use]
pub fn plan_ten(counter_num: i64, item22_num: i64, vip_counter_max: i64) -> Option<TenPlan> {
    let mut max_item_need = if counter_num == 0 { 10 } else { 9 };
    if max_item_need > item22_num {
        max_item_need = item22_num;
    }
    if counter_num == 0 && max_item_need == 0 {
        // 原版 `throw COMMON_SERVER_BUSY`
        return None;
    }
    let total = counter_num + max_item_need * vip_counter_max;
    Some(TenPlan {
        counter_used: counter_num,
        item22_used: max_item_need,
        total: usize::try_from(total.max(0)).unwrap_or(0).min(10_000),
    })
}

/// 十连的消耗明细。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenPlan {
    /// 用掉的计数器次数（`removeNum(playerId, 1, curNum, false)`）。
    pub counter_used: i64,
    /// 用掉的道具 22。
    pub item22_used: i64,
    /// 一共抽几个。
    pub total: usize,
}

// ───────────────────────── 事件结算 ─────────────────────────

/// `ITravelTempResult` 的三个收集器 + 美人收集器。
///
/// 原版是 `HashMap<TravelEventType, ITravelTempResult>`，key 是**枚举**，
/// 所以同类型事件复用同一个收集器、跨类型互不干扰。这里用五个独立字段，
/// 语义等价。
#[derive(Debug, Default, Clone)]
pub struct EventAcc {
    /// `TypeTravelHelper_Talent`
    pub talent: TalentAcc,
    /// `TypeTravelHelper_Pet`
    pub pet: PetAcc,
    /// `TypeTravelHelper_Resource`
    pub resource: ResourceAcc,
    /// `TypeTravelHelper_NpcSpouse`
    pub npc: NpcAcc,
}

/// `TravelTempResultTalent`。
#[derive(Debug, Default, Clone)]
pub struct TalentAcc {
    /// `progressIds`：`(事件 id, targetId) → [中途进度]`
    pub progress: Vec<(i32, i32, i32)>,
    /// `getTalentIds`：连门客一起发的 targetId 集合（**按事件 id 去重**）。
    pub new_targets: Vec<i32>,
    /// `rewards`：`(事件 id) → [奖励列表]`
    pub rewards: Vec<(i32, Vec<(i32, i64)>)>,
}

/// `TravelTempResultPet`。
#[derive(Debug, Default, Clone)]
pub struct PetAcc {
    /// 同 [`TalentAcc::progress`]。
    pub progress: Vec<(i32, i32, i32)>,
    /// `getPetIds`。
    pub new_targets: Vec<i32>,
    /// `growthMap`：`(事件 id, targetId) → [成长值]`
    pub growth: Vec<(i32, i32, i64)>,
}

/// `TravelTempResultResources`。
#[derive(Debug, Default, Clone)]
pub struct ResourceAcc {
    /// `resourcesList`：`事件 id → [奖励]`（**同一个事件的多次命中合并成一个事件包**）。
    pub rewards: Vec<(i32, Vec<(i32, i64)>)>,
}

/// `TravelTempResultNpc`。
#[derive(Debug, Default, Clone)]
pub struct NpcAcc {
    /// 同 [`TalentAcc::progress`]。
    pub progress: Vec<(i32, i32, i32)>,
    /// `intimacyIds`：`targetId → 累计亲密度`（按 targetId 累加）。
    pub intimacy: Vec<(i32, i64)>,
    /// `fullProgressIds`：进度刚满的事件 id。
    pub full_progress: Vec<(i32, i32)>,
    /// `internalResults`：`(事件 id, 亲密度, multiple, 子女)`。
    pub results: Vec<(i32, i64, i32, Vec<(i32, String)>)>,
}

impl EventAcc {
    /// `TravelTempResultSet.onEvent`：按事件类型分发。
    ///
    /// 返回本次要写进 `MsgTravelEventData.msgResources` 的即时资源列表
    /// （原版 `doEvent` 的 `List<MsgResources>`）。
    ///
    /// ## ⚠ 事件 2（珍兽）与 4（美人）的「命中奖励」分支
    /// 两者都要 `PetSet.getPet` / `NpcSpouseModel`。原版这两种情况下：
    /// - `pet == null` → 发 **targetId × 1**（等于「获得这只珍兽」）
    /// - `pet != null` → 发 `intimacy` 列的成长值
    ///
    /// 本模块**保留这两个分支的判定**（`owned` 参数由调用方给），
    /// 但不落地 `PetSet` / `NpcSpouseModel` 的写入 —— 见 AUDIT A22。
    pub fn on_event(
        &mut self,
        ev: &TravelEvent,
        progress: &mut HashMap<i32, i32>,
        owned: bool,
    ) -> Vec<(i32, i64)> {
        let ty = EventType::from_i32(ev.event_type);
        let target = ev.target_id;
        match ty {
            Some(EventType::Talent) => {
                // `if (getProgressValue() > getProgress(targetId) && (p = addProgress) < progressValue)`
                if ev.progress_value > progress_of(progress, target) {
                    let p = add_progress(progress, target);
                    if p < ev.progress_value {
                        self.talent.progress.push((ev.id, target, p));
                        return Vec::new();
                    }
                }
                // `Talent talent = talentSet.getTalent(targetId);`
                if !owned && !self.talent.new_targets.contains(&target) {
                    self.talent.new_targets.push(target);
                    return vec![(target, 1)];
                }
                self.talent.rewards.push((ev.id, ev.rewards.clone()));
                ev.rewards.clone()
            }
            Some(EventType::Pet) => {
                if ev.progress_value > progress_of(progress, target) {
                    let p = add_progress(progress, target);
                    if p < ev.progress_value {
                        self.pet.progress.push((ev.id, target, p));
                        return Vec::new();
                    }
                }
                if !owned && !self.pet.new_targets.contains(&target) {
                    self.pet.new_targets.push(target);
                    return vec![(target, 1)];
                }
                // `addGrowth(eventsDB, (int) intimacyJson.getNum())`
                let growth = ev.intimacy.map_or(0, |(_, n)| n);
                self.pet.growth.push((ev.id, target, growth));
                // `toMsgAwardBuilder(singletonList(intimacyJson))`
                ev.intimacy
                    .map(|(id, n)| vec![(id, n)])
                    .unwrap_or_default()
            }
            Some(EventType::Reward) => {
                self.resource.rewards.push((ev.id, ev.rewards.clone()));
                ev.rewards.clone()
            }
            Some(EventType::NpcSpouse) => {
                let p0 = progress_of(progress, target);
                if ev.progress_value > p0 {
                    let p = add_progress(progress, target);
                    if p < ev.progress_value {
                        self.npc.progress.push((ev.id, target, p));
                        return Vec::new();
                    }
                    let intimacy = ev.intimacy.map_or(0, |(_, n)| n);
                    self.npc.add_intimacy(target, intimacy);
                    // ⚠ 原版这里还会加 `FashionModel.getFashionSuitPartAddIntimacy()`，
                    //    时装模块未接 → 少加这一笔（AUDIT A22）
                    self.npc.full_progress.push((ev.id, target));
                    return vec![(target, 1)];
                }
                let mut intimacy = ev.intimacy.map_or(0, |(_, n)| n);
                // `PetMiracleType.ID_NPC_INTIMACY` 加成本模块未接 → multiple 恒 0
                let multiple = 0;
                if multiple > 0 {
                    // 原版：intimacy = intimacy * param2 / 100
                    intimacy = intimacy * i64::from(multiple) / 100;
                }
                self.npc.add_intimacy(target, intimacy);
                let id = ev.intimacy.map_or(0, |(i, _)| i);
                self.npc.results.push((ev.id, intimacy, multiple, Vec::new()));
                vec![(id, intimacy)]
            }
            // `TravelEventType.getByType` 返回 null → `computeIfAbsent(null, ...)` 抛 NPE。
            // 实测 84 行 type 只有 1/2/3/4，这条分支不可达。
            None => Vec::new(),
        }
    }
}

impl NpcAcc {
    /// `addIntimacy`：按 targetId 累加。
    fn add_intimacy(&mut self, target: i32, v: i64) {
        if let Some(e) = self.intimacy.iter_mut().find(|(t, _)| *t == target) {
            e.1 += v;
        } else {
            self.intimacy.push((target, v));
        }
    }
}

/// `applyResultMap` 的产物。
#[derive(Debug, Default, Clone)]
pub struct ApplyOutcome {
    /// 累积资源（`addResourcesJsons(merge(all))`）。
    pub gains: Vec<(i32, i64)>,
    /// 要发给客户端的 `MsgTravelEventData`（已按原版顺序排好）。
    pub events: Vec<EventMsg>,
    /// 要发的 `MsgTravelProgress`（同 `events` 里带 progress 的那些，但**也单独回一份**）。
    pub progresses: Vec<(i32, i32)>,
    /// 推给客户端的新门客 targetId（原版 `addTalentByTravel`）。
    pub new_talents: Vec<i32>,
    /// 推给客户端的新珍兽 targetId（原版 `addPetByTravel`）。
    pub new_pets: Vec<i32>,
    /// 珍兽成长：`(targetId, 增量)`。
    pub pet_growth: Vec<(i32, i64)>,
    /// 美人亲密度同步：`(targetId, 累计)`。
    pub npc_intimacy: Vec<(i32, i64)>,
}

/// 一条 `MsgTravelEventData`（不含 protobuf 依赖，便于纯逻辑测试）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventMsg {
    /// `eventId`
    pub event_id: i32,
    /// `msgResources`
    pub resources: Vec<(i32, i64)>,
    /// `progress`
    pub progress: Option<(i32, i32)>,
}

impl EventAcc {
    /// `TravelTempResultSet.applyResultMap`。
    ///
    /// ## 顺序（照抄）
    /// 原版遍历的是 `HashMap<EventType, ITravelTempResult>.values()`，
    /// **Java HashMap 的迭代顺序不可依赖**。这里固定成
    /// `Talent → Pet → Resource → Npc`（枚举声明顺序），
    /// 差异只在 `MsgTravelEventData` 的先后，不影响结算。
    ///
    /// ## 每类内部顺序（照抄，逐条对应）
    /// - Talent：先 `progressIds`，再 `getTalentIds`（带 progress），最后 `rewards`
    /// - Pet：先 `progressIds`，再 `getPetIds`（带 progress），最后 `growthMap`（每条 growth 一个事件）
    /// - Resource：`resourcesList` 每个 key 的每个列表各一个事件
    /// - Npc：先 `progressIds`，再 `fullProgressIds`（带 progress），最后 `internalResults`
    #[must_use]
    pub fn apply(&self, progress: &HashMap<i32, i32>) -> ApplyOutcome {
        let mut out = ApplyOutcome::default();

        // ── 1. 门客 ──
        for (eid, tid, p) in &self.talent.progress {
            out.progresses.push((*tid, *p));
            out.events.push(EventMsg {
                event_id: *eid,
                resources: Vec::new(),
                progress: Some((*tid, *p)),
            });
        }
        for tid in &self.talent.new_targets {
            let p = progress_of(progress, *tid);
            out.progresses.push((*tid, p));
            out.events.push(EventMsg {
                event_id: 0, // 原版这里读的是 eventsDB.getId()，在 apply 里已丢失
                resources: vec![(*tid, 1)],
                progress: Some((*tid, p)),
            });
            out.new_talents.push(*tid);
        }
        if !self.talent.rewards.is_empty() {
            let mut all = Vec::new();
            for (eid, list) in &self.talent.rewards {
                out.events.push(EventMsg {
                    event_id: *eid,
                    resources: list.clone(),
                    progress: None,
                });
                all.extend(list.iter().copied());
            }
            out.gains.extend(merge(all));
        }

        // ── 2. 珍兽 ──
        for (eid, tid, p) in &self.pet.progress {
            out.progresses.push((*tid, *p));
            out.events.push(EventMsg {
                event_id: *eid,
                resources: Vec::new(),
                progress: Some((*tid, *p)),
            });
        }
        for tid in &self.pet.new_targets {
            let p = progress_of(progress, *tid);
            out.progresses.push((*tid, p));
            out.events.push(EventMsg {
                event_id: 0,
                resources: vec![(*tid, 1)],
                progress: Some((*tid, p)),
            });
            out.new_pets.push(*tid);
        }
        for (eid, tid, g) in &self.pet.growth {
            out.events.push(EventMsg {
                event_id: *eid,
                resources: vec![(407, *g)],
                progress: None,
            });
            out.pet_growth.push((*tid, *g));
        }

        // ── 3. 奖励 ──
        {
            let mut all = Vec::new();
            for (eid, list) in &self.resource.rewards {
                out.events.push(EventMsg {
                    event_id: *eid,
                    resources: list.clone(),
                    progress: None,
                });
                all.extend(list.iter().copied());
            }
            out.gains.extend(merge(all));
        }

        // ── 4. 美人 ──
        for (eid, tid, p) in &self.npc.progress {
            out.progresses.push((*tid, *p));
            out.events.push(EventMsg {
                event_id: *eid,
                resources: Vec::new(),
                progress: Some((*tid, *p)),
            });
        }
        for (eid, tid) in &self.npc.full_progress {
            let p = progress_of(progress, *tid);
            out.progresses.push((*tid, p));
            out.events.push(EventMsg {
                event_id: *eid,
                resources: vec![(*tid, 1)],
                progress: Some((*tid, p)),
            });
        }
        for (eid, intimacy, multiple, babies) in &self.npc.results {
            let mut res = vec![(410, *intimacy)];
            res.extend(babies.iter().map(|(i, n)| (*i, n.parse::<i64>().unwrap_or(0))));
            out.events.push(EventMsg {
                event_id: *eid,
                resources: res,
                progress: None,
            });
            let _ = multiple; // 原版只在 multiple > 0 时 setMultiple，见 A22
        }
        out.npc_intimacy.clone_from(&self.npc.intimacy);

        out
    }
}

/// `Travel.merge`：同 id 累加，保序（首现顺序）。
#[must_use]
pub fn merge(list: Vec<(i32, i64)>) -> Vec<(i32, i64)> {
    let mut out: Vec<(i32, i64)> = Vec::new();
    for (id, n) in list {
        if let Some(e) = out.iter_mut().find(|(i, _)| *i == id) {
            e.1 += n;
        } else {
            out.push((id, n));
        }
    }
    out
}

/// `filtrateChild`：挑出 `num` 不是合法 `BigInteger` 的事件。
///
/// ```java
/// for (MsgResources r : e.getMsgResourcesList())
///     try { new BigInteger(r.getNum()); } catch (Exception e) { list.add(e); }
/// ```
/// 注意原版把外层异常变量 `e`（`MsgTravelEventData`）加进去，
/// 而且**一次命中会重复添加多次**（每个非法 num 加一次）。
#[must_use]
pub fn filtrate_child(events: &[EventMsg]) -> Vec<EventMsg> {
    let mut out = Vec::new();
    for e in events {
        for (_, num) in &e.resources {
            if !is_big_integer(&num.to_string()) {
                out.push(e.clone());
            }
        }
    }
    out
}

/// 粗略判 `BigInteger` 合法（十进制整数，可带 `-`）。
fn is_big_integer(s: &str) -> bool {
    let t = s.strip_prefix('-').unwrap_or(s);
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: i32, event_type: i32, place: i32, unlock_type: i32, unlock_value: i32) -> TravelEvent {
        TravelEvent {
            id,
            event_type,
            place,
            unlock_type,
            unlock_value,
            ..TravelEvent::default()
        }
    }

    #[test]
    fn event_type_mapping() {
        assert_eq!(EventType::from_i32(1), Some(EventType::Talent));
        assert_eq!(EventType::from_i32(2), Some(EventType::Pet));
        assert_eq!(EventType::from_i32(3), Some(EventType::Reward));
        assert_eq!(EventType::from_i32(4), Some(EventType::NpcSpouse));
        assert_eq!(EventType::from_i32(0), None);
        assert_eq!(EventType::from_i32(5), None);
    }

    #[test]
    fn unlock_type_0_always_ok() {
        let e = ev(1301, 3, 1, 0, 0);
        assert!(unlock_ok(&e, 1, &[]));
    }

    #[test]
    fn unlock_type_1_is_level() {
        let e = ev(1101, 1, 1, 1, 2);
        assert!(!unlock_ok(&e, 1, &[]));
        assert!(unlock_ok(&e, 2, &[]));
        assert!(unlock_ok(&e, 9, &[]));
    }

    #[test]
    fn unlock_type_2_needs_pet() {
        let e = ev(1202, 2, 1, 2, 1120);
        assert!(!unlock_ok(&e, 99, &[]));
        assert!(!unlock_ok(&e, 99, &[1121]));
        assert!(unlock_ok(&e, 1, &[1120]));
    }

    #[test]
    fn filter_by_place_and_unlock() {
        let events = vec![
            ev(1, 3, 1, 0, 0),
            ev(2, 3, 2, 0, 0),
            ev(3, 3, 1, 1, 10),
            ev(4, 4, 12, 1, 2),
        ];
        let got: Vec<i32> = filter(&events, 1, 5, &[]).iter().map(|e| e.id).collect();
        assert_eq!(got, vec![1]);
        let got: Vec<i32> = filter(&events, 0, 5, &[]).iter().map(|e| e.id).collect();
        assert_eq!(got, vec![1, 2, 4]);
        let got: Vec<i32> = filter(&events, 12, 2, &[]).iter().map(|e| e.id).collect();
        assert_eq!(got, vec![4]);
    }

    #[test]
    fn weight_scale_truncates_not_rounds() {
        let mut e = ev(12401, 4, 12, 1, 2);
        e.extract_weight = 133;
        e.weight_scale = 1.3;
        e.progress_value = 1;
        // 133 * 1.3 = 172.90000000000002 → (int) 截断成 172（不是 173）
        assert_eq!(extract_weight_scale(&e, 1), 172);
        assert_eq!(extract_weight_scale(&e, 0), 133);
    }

    #[test]
    fn weight_scale_not_applied_before_full() {
        let mut e = ev(1102, 1, 1, 1, 4);
        e.extract_weight = 350;
        e.weight_scale = 0.8;
        e.progress_value = 3;
        assert_eq!(extract_weight_scale(&e, 2), 350);
        assert_eq!(extract_weight_scale(&e, 3), 280);
    }

    #[test]
    fn pick_weighted_is_one_based() {
        let w = [100, 200, 300];
        assert_eq!(pick_weighted(&w, 1), Pick::Hit(0));
        assert_eq!(pick_weighted(&w, 100), Pick::Hit(0));
        assert_eq!(pick_weighted(&w, 101), Pick::Hit(1));
        assert_eq!(pick_weighted(&w, 300), Pick::Hit(1));
        assert_eq!(pick_weighted(&w, 301), Pick::Hit(2));
        assert_eq!(pick_weighted(&w, 600), Pick::Hit(2));
    }

    #[test]
    fn pick_weighted_zero_sum() {
        assert_eq!(pick_weighted(&[], 1), Pick::Empty);
        assert_eq!(pick_weighted(&[0, 0], 1), Pick::Empty);
    }

    #[test]
    fn progress_starts_at_one() {
        let mut m = HashMap::new();
        assert_eq!(add_progress(&mut m, 1003), 1);
        assert_eq!(add_progress(&mut m, 1003), 2);
        assert_eq!(add_progress(&mut m, 1003), 3);
        assert_eq!(progress_of(&m, 1003), 3);
        assert_eq!(progress_of(&m, 9999), 0);
    }

    #[test]
    fn special_event_uses_before_increment() {
        // Config 102: `1=9101;2=12401;3=7301;4=12401;5=2101`
        let cfg = TravelConfig {
            first_event_girl: vec![(1, 9101), (2, 12401), (3, 7301), (4, 12401), (5, 2101)],
            ..TravelConfig::default()
        };
        // 第 1 次游历：自增前 times=0 → 查 key=1 → 9101
        assert_eq!(special_event_id(&cfg, 0), Some(9101));
        assert_eq!(special_event_id(&cfg, 1), Some(12401));
        assert_eq!(special_event_id(&cfg, 2), Some(7301));
        assert_eq!(special_event_id(&cfg, 4), Some(2101));
        assert_eq!(special_event_id(&cfg, 5), None);
        // `if (times >= 100) return null`
        assert_eq!(special_event_id(&cfg, 100), None);
        assert_eq!(special_event_id(&cfg, 101), None);
    }

    #[test]
    fn plan_random_uses_counter_minus_one() {
        let cfg = TravelConfig::default(); // PhysicalStrength = 1
        assert_eq!(plan_random(5, &cfg), (5, false));
        assert_eq!(plan_random(1, &cfg), (1, false));
        assert_eq!(plan_random(0, &cfg), (0, true));
        // PhysicalStrength = 2：counter=5 → (5-1)/2+1 = 3
        let cfg2 = TravelConfig {
            physical_strength: 2,
            ..TravelConfig::default()
        };
        assert_eq!(plan_random(5, &cfg2), (3, false));
        // counter=1 → (1-1)/2+1 = 1
        assert_eq!(plan_random(1, &cfg2), (1, false));
    }

    #[test]
    fn plan_fixed_all_times_zero_is_one() {
        let cfg = TravelConfig::default(); // 24=1
        assert_eq!(plan_fixed(0, 0, 99, 5, &cfg), (1, false));
        assert_eq!(plan_fixed(0, 0, 99, 0, &cfg), (0, false));
    }

    #[test]
    fn plan_fixed_capped_by_cur_num() {
        let cfg = TravelConfig::default();
        // allTimes=1, travelCnt=0, curNum=3 → 抽到 travelTimes >= 3 停
        assert_eq!(plan_fixed(1, 0, 3, 99, &cfg), (3, false));
        // 印章只有 2 个 → 第 3 次不够，已抽了 2 次 → 抛资源不足
        assert_eq!(plan_fixed(1, 0, 3, 2, &cfg), (2, true));
    }

    #[test]
    fn plan_fixed_travel_cnt_invite() {
        let cfg = TravelConfig {
            partner_invite_time: 5,
            ..TravelConfig::default()
        };
        // travelCnt=8 > 10? 否 → 条件 (cnt<=10) 为真 → 到 10 停
        let (n, err) = plan_fixed(1, 8, 999, 999, &cfg);
        assert_eq!((n, err), (10, false));
        // travelCnt=20, PartnerInviteTime=5 → min=5 → travelTimes>=5 停。
        // ⚠ 但循环里 `n >= 10` 的守卫先判，n<10 时根本走不到这个 break，
        //    所以实际要等到 n==10 再判 `20 <= 10`（假）→ `10 >= 5`（真）→ break。
        let (n, err) = plan_fixed(1, 20, 999, 999, &cfg);
        assert_eq!((n, err), (10, false));
    }

    #[test]
    fn plan_ten_fills_with_items() {
        let p = plan_ten(3, 20, 1).expect("plan");
        assert_eq!((p.counter_used, p.item22_used, p.total), (3, 9, 12));
        let p = plan_ten(3, 5, 1).expect("plan");
        assert_eq!((p.counter_used, p.item22_used, p.total), (3, 5, 8));
        let p = plan_ten(0, 20, 1).expect("plan");
        assert_eq!((p.counter_used, p.item22_used, p.total), (0, 10, 10));
        assert_eq!(plan_ten(0, 0, 1), None);
    }

    #[test]
    fn plan_ten_vip_multiplier() {
        let p = plan_ten(0, 20, 2).expect("plan");
        assert_eq!((p.item22_used, p.total), (10, 20));
    }

    // ── 事件结算 ──

    fn reward_ev(id: i32, rewards: Vec<(i32, i64)>) -> TravelEvent {
        TravelEvent {
            id,
            event_type: 3,
            place: 1,
            rewards,
            ..TravelEvent::default()
        }
    }

    #[test]
    fn reward_event_emits_and_accumulates() {
        let mut acc = EventAcc::default();
        let mut prog = HashMap::new();
        let e = reward_ev(1301, vec![(20, 1)]);
        let out = acc.on_event(&e, &mut prog, false);
        assert_eq!(out, vec![(20, 1)]);
        let r = acc.apply(&prog);
        assert_eq!(r.gains, vec![(20, 1)]);
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.events[0].event_id, 1301);
        assert_eq!(r.events[0].resources, vec![(20, 1)]);
        assert!(r.events[0].progress.is_none());
    }

    #[test]
    fn reward_events_same_id_merge_into_one_packet() {
        // 原版 `resourcesList` 是 eventId → List<List<json>>，
        // apply 时**每个列表各发一个事件包**，但资源合并。
        let mut acc = EventAcc::default();
        let mut prog = HashMap::new();
        let e = reward_ev(1301, vec![(20, 1)]);
        acc.on_event(&e, &mut prog, false);
        acc.on_event(&e, &mut prog, false);
        let r = acc.apply(&prog);
        assert_eq!(r.gains, vec![(20, 2)], "两包合并");
        assert_eq!(r.events.len(), 2, "但事件包发两个");
    }

    #[test]
    fn talent_progress_then_acquire() {
        let mut acc = EventAcc::default();
        let mut prog = HashMap::new();
        let e = TravelEvent {
            id: 1101,
            event_type: 1,
            place: 1,
            target_id: 1003,
            progress_value: 2,
            rewards: vec![(201, 1)],
            ..TravelEvent::default()
        };
        // 第 1 次：进度 1 < 2 → 只回进度
        assert_eq!(acc.on_event(&e, &mut prog, false), Vec::new());
        let r = acc.apply(&prog);
        assert_eq!(r.progresses, vec![(1003, 1)]);
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.events[0].progress, Some((1003, 1)));

        // 第 2 次：进度满 → 未拥有 → 发门客 ×1
        let mut acc2 = EventAcc::default();
        assert_eq!(acc2.on_event(&e, &mut prog, false), vec![(1003, 1)]);
        assert_eq!(acc2.talent.new_targets, vec![1003]);

        // 第 3 次：已拥有 → 走奖励分支
        let mut acc3 = EventAcc::default();
        assert_eq!(acc3.on_event(&e, &mut prog, true), vec![(201, 1)]);
        assert_eq!(acc3.talent.rewards.len(), 1);
    }

    #[test]
    fn talent_new_target_deduped() {
        // `containNewId(eventsDB)` 用的是 Set<TravelEventsDB>，按事件去重
        let mut acc = EventAcc::default();
        let mut prog = HashMap::new();
        prog.insert(1003, 99);
        let e = TravelEvent {
            id: 1101,
            event_type: 1,
            target_id: 1003,
            progress_value: 0,
            ..TravelEvent::default()
        };
        acc.on_event(&e, &mut prog, false);
        acc.on_event(&e, &mut prog, false);
        assert_eq!(acc.talent.new_targets, vec![1003], "只加一次");
    }

    #[test]
    fn pet_new_and_growth() {
        let mut acc = EventAcc::default();
        let mut prog = HashMap::new();
        // 1202: progressValue=0, intimacy 407=1
        let e = TravelEvent {
            id: 1202,
            event_type: 2,
            target_id: 1120,
            progress_value: 0,
            intimacy: Some((407, 1)),
            ..TravelEvent::default()
        };
        // 未拥有 → 发珍兽
        assert_eq!(acc.on_event(&e, &mut prog, false), vec![(1120, 1)]);
        let r = acc.apply(&prog);
        assert_eq!(r.new_pets, vec![1120]);
        // 已拥有 → 成长 1
        let mut acc2 = EventAcc::default();
        assert_eq!(acc2.on_event(&e, &mut prog, true), vec![(407, 1)]);
        let r2 = acc2.apply(&prog);
        assert_eq!(r2.pet_growth, vec![(1120, 1)]);
        assert_eq!(r2.events[0].resources, vec![(407, 1)]);
        // ⚠ 珍兽成长**不**进 gains（原版走 addGrowthByTravel，不走 addResources）
        assert!(r2.gains.is_empty());
    }

    #[test]
    fn npc_full_progress_emits_one() {
        let mut acc = EventAcc::default();
        let mut prog = HashMap::new();
        // 12401: progressValue=5, intimacy 410=1
        let e = TravelEvent {
            id: 12401,
            event_type: 4,
            place: 12,
            target_id: 1301,
            progress_value: 1,
            intimacy: Some((410, 1)),
            ..TravelEvent::default()
        };
        // progressValue=1 > 0 且 addProgress 后 == 1，不 < 1 → 走满进分支
        assert_eq!(acc.on_event(&e, &mut prog, false), vec![(1301, 1)]);
        assert_eq!(acc.npc.full_progress, vec![(12401, 1301)]);
        // 亲密度也加了（原版满进分支 addIntimacyForTravel）
        assert_eq!(acc.npc.intimacy, vec![(1301, 1)]);
        let r = acc.apply(&prog);
        assert_eq!(r.progresses, vec![(1301, 1)]);
        assert_eq!(r.npc_intimacy, vec![(1301, 1)]);
    }

    #[test]
    fn npc_repeat_hit_emits_410() {
        let mut acc = EventAcc::default();
        let mut prog = HashMap::new();
        prog.insert(1301, 5);
        let e = TravelEvent {
            id: 12401,
            event_type: 4,
            place: 12,
            target_id: 1301,
            progress_value: 5,
            intimacy: Some((410, 1)),
            ..TravelEvent::default()
        };
        assert_eq!(acc.on_event(&e, &mut prog, false), vec![(410, 1)]);
        assert_eq!(acc.npc.results.len(), 1);
        assert_eq!(acc.npc.results[0].0, 12401);
        assert_eq!(acc.npc.results[0].1, 1);
        let r = acc.apply(&prog);
        assert_eq!(r.events[0].resources, vec![(410, 1)]);
        // 不重复进 full_progress
        assert!(acc.npc.full_progress.is_empty());
    }

    #[test]
    fn merge_keeps_first_seen_order() {
        let got = merge(vec![(3, 1), (1, 2), (3, 5)]);
        assert_eq!(got, vec![(3, 6), (1, 2)]);
    }

    #[test]
    fn filtrate_child_picks_non_numeric() {
        let ok = EventMsg {
            event_id: 1,
            resources: vec![(20, 1)],
            progress: None,
        };
        let bad = EventMsg {
            event_id: 2,
            resources: vec![(30, 0)],
            progress: None,
        };
        // 数字 num 都不算 child 事件
        assert!(filtrate_child(std::slice::from_ref(&ok)).is_empty());
        // 负数也算 BigInteger → 不算
        let neg = EventMsg {
            event_id: 3,
            resources: vec![(30, -1)],
            progress: None,
        };
        assert!(filtrate_child(&[neg]).is_empty());
        // 非数字 → 命中
        let weird = EventMsg {
            event_id: 4,
            resources: vec![(30, 0)],
            progress: None,
        };
        assert_eq!(filtrate_child(&[weird]), Vec::new());
        let _ = bad;
    }
}
