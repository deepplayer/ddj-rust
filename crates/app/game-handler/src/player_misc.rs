//! 角色周边小功能：引导存档 / 客户端缓存 / 定位 / 礼包码 / 系统开启 / 改性别。
//!
//! 对应 `PlayerMessageHandler` 里的 7 个方法，全部按字节码逐分支还原：
//!
//! | cmdId | 方法 | 回包 |
//! |---|---|---|
//! | 1103 | `playerSaveGuideIdReq` | 无（catch 只打日志） |
//! | 1111 | `PlayerSaveCacheDataReq` | 无（连 catch 都没有） |
//! | 1112 | `GetPlayerCacheDataReq` | `GetPlayerCacheDataResp_1112` |
//! | 1009 | `playerUseGiftKeyReq` | 失败时 `PlayerUseGiftKeyResp_1009` |
//! | 1010 | `SetLocationReq`(由 handler 调用) | `SetLocationResp_1010` |
//! | 1015 | `systemOpenReq_1015` | `systemOpenResp_1015` |
//! | 1024 | `msgUpdateSexReq` | `MsgUpdateSexResp_1024`（返回值，不是主动推） |
//!
//! 两个**原版缺陷**照抄，不要"修正"：
//! 1. `SetLocationReq` 的 catch 块构造的是 `PlayerUseGiftKeyResp_1009{ret=2}` —— 复制粘贴事故，
//!    客户端收到的是错误的响应类型，会解析失败。
//! 2. `1111` 完全没有 try/catch，`1112` 也没有 —— 异常直接抛给框架。

use ddj_pb::RetCode;

// ---------------------------------------------------------------------------
// 1103 引导存档
// ---------------------------------------------------------------------------

/// `Player.playerGuideInfo` 的落地形态。
///
/// 原版是 `ConcurrentHashMap<String,Long>`，键只有 `id` / `time` / `nextId` 三个，
/// 每次写入都先 `clear()` —— 不是合并，是整体替换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GuideInfo {
    /// 存档时取的是 `player.dbId`，不是玩家 id。
    pub db_id: i64,
    /// 客户端上报的下一节点。
    pub next_id: i64,
    /// 写入时刻。
    pub time: i64,
}

/// `EventLogSendRecord.guide(player, isPast, guideId, nextId)` 的入参。
///
/// 注意实参顺序与原方法的形参顺序**不一致**：`setPlayerGuideInfo(guideId, nextId, isPast)`
/// 调用点是把 `isPast` 放第一个传下去的（字节码 9-12：`iload_3, iload_1, iload_2`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuideEvent {
    /// 是否已过引导（原 `isPastt`，字段名拼写就是错的）。
    pub is_past: bool,
    /// 客户端上报的当前引导节点。
    pub guide_id: i32,
    /// 客户端上报的下一节点。
    pub next_id: i32,
}

/// 1103 的结果：事件 + 要落库的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuideSave {
    /// 要投递的埋点事件。
    pub event: GuideEvent,
    /// 要写回 `playerGuideInfo` 的快照。
    pub info: GuideInfo,
}

/// 还原 `Player.setPlayerGuideInfo(guideId, nextId, isPast)`。
#[must_use]
pub fn save_guide_info(
    guide_id: i32,
    next_id: i32,
    is_past: bool,
    db_id: i64,
    now: i64,
) -> GuideSave {
    GuideSave {
        event: GuideEvent {
            is_past,
            guide_id,
            next_id,
        },
        info: GuideInfo {
            db_id,
            next_id: i64::from(next_id),
            time: now,
        },
    }
}

// ---------------------------------------------------------------------------
// 1111 / 1112 客户端缓存
// ---------------------------------------------------------------------------

/// `PlayerCommon.clientCacheMap` 的行为面（`ConcurrentHashMap<Integer,String>`）。
pub trait ClientCache {
    /// 逐键 `put`（**不是**整表替换）。
    fn put(&mut self, key: i32, value: String);
    /// 遍历 `entrySet()` —— 原版迭代 `ConcurrentHashMap`，顺序不保证。
    fn all(&self) -> Vec<(i32, String)>;
    /// `getOrDefault(key, "")` —— 缺失时给空串，而不是跳过。
    fn get_or_default(&self, key: i32) -> String;
}

/// 还原 `PlayerCommon.saveClientCache(list)`。
///
/// 返回实际写入的条数。原版随后调 `save()`，这里由调用方负责持久化。
pub fn save_client_cache(cache: &mut dyn ClientCache, items: &[(i32, String)]) -> usize {
    for (k, v) in items {
        cache.put(*k, v.clone());
    }
    items.len()
}

/// 还原 `PlayerCommon.getClientCache()` 与 `getClientCache(List)` 两条路径。
///
/// `keys` 为空 = `getKeyCount()==0` = 返回整张表；否则按键逐个取，
/// 缺失的键**仍然会出现在结果里**，值为空串。
pub fn get_client_cache(cache: &dyn ClientCache, keys: &[i32]) -> Vec<(i32, String)> {
    if keys.is_empty() {
        return cache.all();
    }
    keys.iter()
        .map(|k| (*k, cache.get_or_default(*k)))
        .collect()
}

// ---------------------------------------------------------------------------
// 1010 定位
// ---------------------------------------------------------------------------

/// `Player.resetAddress(cityId, type)` 之前能拿到的玩家侧状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocationCtx {
    /// 当前城市 id。
    pub city_id: i32,
    /// `resetAddressTimes`（已改定位次数）。
    pub reset_times: i32,
    /// `LocationUtils.getAddressInfo(newCityId) != null`。
    pub address_exists: bool,
}

/// 两种不同的扣费路径，分开注入便于测试。
pub trait LocationCost {
    /// `revmoeResources([ConfigDBCache.PositioningConsumption], TYPE_1016)`。
    fn charge_positioning(&mut self) -> bool;
    /// `revmoeResources([ResourcesJson(54,"1")], TYPE_1016)`。
    fn charge_item_54(&mut self) -> bool;
}

/// 1010 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetLocationOutcome {
    /// 直接回 `SetLocationResp_1010{ret}`。
    Reject(RetCode),
    /// 成功：写新城市、次数 +1、推属性 24/25。
    Accept {
        /// 写入 `Player.cityId` 的新值。
        new_city_id: i32,
        /// 递增后的 `resetAddressTimes`。
        reset_times: i32,
    },
}

/// 成功时 `sendPlayerAbUpdate(player, true, {24, 25})` 推送的属性号。
pub const SET_LOCATION_PUSH_ATTRS: [i32; 2] = [24, 25];

/// 还原 `Player.resetAddress(cityId, type)`（384 字节，六个分支）。
///
/// `kind`：1 = 免费改定位（仅当还没改过）、2 = 扣 `PositioningConsumption`、
/// 3 = 扣物品 54；其它值不校验、不扣费，直接落地。
#[must_use]
pub fn set_location(
    new_city_id: i32,
    kind: i32,
    ctx: &LocationCtx,
    cost: &mut dyn LocationCost,
) -> SetLocationOutcome {
    if !ctx.address_exists {
        return SetLocationOutcome::Reject(RetCode::PlayerError16);
    }
    if ctx.city_id == new_city_id {
        return SetLocationOutcome::Reject(RetCode::PlayerError22);
    }
    match kind {
        1 => {
            if ctx.reset_times > 0 {
                return SetLocationOutcome::Reject(RetCode::PlayerError21);
            }
        }
        2 => {
            if ctx.reset_times == 0 {
                return SetLocationOutcome::Reject(RetCode::PlayerError15);
            }
            if !cost.charge_positioning() {
                return SetLocationOutcome::Reject(RetCode::JewelNo);
            }
        }
        3 => {
            if ctx.reset_times == 0 {
                return SetLocationOutcome::Reject(RetCode::PlayerError15);
            }
            if !cost.charge_item_54() {
                return SetLocationOutcome::Reject(RetCode::ResourcesNo);
            }
        }
        _ => {}
    }
    SetLocationOutcome::Accept {
        new_city_id,
        reset_times: ctx.reset_times + 1,
    }
}

// ---------------------------------------------------------------------------
// 1009 礼包码
// ---------------------------------------------------------------------------

/// 1009 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiftKeyOutcome {
    /// `GsGiftKey == null` → `PLAYER_ERROR_10(110)`。
    Reject(RetCode),
    /// 交给 `GsGiftKey.use(player)` 自己回包。
    Accept,
}

/// 还原 `playerUseGiftKeyReq` 的查表部分：先 `toLowerCase()` 再去 `GsGiftManager` 找。
#[must_use]
pub fn use_gift_key(gift_key: &str, exists: bool) -> GiftKeyOutcome {
    let _normalized = gift_key.to_lowercase();
    if exists {
        GiftKeyOutcome::Accept
    } else {
        GiftKeyOutcome::Reject(RetCode::PlayerError10)
    }
}

// ---------------------------------------------------------------------------
// 1015 系统开启
// ---------------------------------------------------------------------------

/// 还原 `LogicUtils.getDescribeName`：按 `_` 切分取**最后一段**，`parseInt` 当语言 id。
///
/// 末段不是数字时原版抛 `NumberFormatException` → handler catch → `ret=2`，这里回 `None`。
#[must_use]
pub fn describe_lang_id(name: &str) -> Option<i32> {
    name.rsplit('_').next()?.parse::<i32>().ok()
}

/// 1015 的结果：要么拿到语言文案并 `ret=0`，要么按 `ret=2` 兜底。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemOpenOutcome {
    /// 查到模板、解出语言 id，回 `ret=0`。
    Accept {
        /// 请求里的系统 id，原样带进埋点。
        system_id: i32,
        /// 从模板名末段解出来的语言表 id。
        lang_id: i32,
    },
    /// `getSystemOpenDB(id)` 为 null 或名字末段不是数字 —— 两种情况原版都是 NPE/异常 → `ret=2`。
    Reject(RetCode),
}

/// 还原 `systemOpenReq_1015`：查模板名 → 取名末段语言 id → 记事件日志 → 回 `ret=0`。
#[must_use]
pub fn system_open(system_id: i32, template_name: Option<&str>) -> SystemOpenOutcome {
    let Some(name) = template_name else {
        return SystemOpenOutcome::Reject(RetCode::ServerBusy);
    };
    match describe_lang_id(name) {
        Some(lang_id) => SystemOpenOutcome::Accept { system_id, lang_id },
        None => SystemOpenOutcome::Reject(RetCode::ServerBusy),
    }
}

// ---------------------------------------------------------------------------
// 1024 改性别
// ---------------------------------------------------------------------------

/// `MsgUpdateSexReq_1024` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateSexOutcome {
    /// 扣费失败 → `COMMON_SERVER_JEWEL_NO(7)`。
    Reject(RetCode),
    /// 扣费成功，写入新性别。
    Accept {
        /// 请求里的性别值。
        sex: i32,
    },
}

/// 还原 `msgUpdateSexReq`：先扣 `ConfigDBCache.GenderChangeMoney`（资源 1 = 元宝），
/// 扣不动回 `COMMON_SERVER_JEWEL_NO(7)`，扣成功才 `setSex(sex)` 并回 `ret=0`。
#[must_use]
pub fn update_sex(sex: i32, cost: &mut dyn FnMut() -> bool) -> UpdateSexOutcome {
    if !cost() {
        return UpdateSexOutcome::Reject(RetCode::JewelNo);
    }
    UpdateSexOutcome::Accept { sex }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // ---- 1103 ----
    #[test]
    fn guide_info_uses_player_db_id_not_player_id() {
        let r = save_guide_info(7, 9, true, 123_456, 1_000);
        assert_eq!(r.info.db_id, 123_456);
        assert_eq!(r.info.next_id, 9);
        assert_eq!(r.info.time, 1_000);
    }

    #[test]
    fn guide_event_arg_order_is_swapped() {
        // 字节码 9-12 是 iload_3(isPast), iload_1(guideId), iload_2(nextId)
        let r = save_guide_info(7, 9, false, 1, 0);
        assert_eq!(
            r.event,
            GuideEvent {
                is_past: false,
                guide_id: 7,
                next_id: 9
            }
        );
    }

    // ---- 1111 / 1112 ----
    #[derive(Default)]
    struct FakeCache(BTreeMap<i32, String>);

    impl ClientCache for FakeCache {
        fn put(&mut self, key: i32, value: String) {
            self.0.insert(key, value);
        }
        fn all(&self) -> Vec<(i32, String)> {
            self.0.iter().map(|(k, v)| (*k, v.clone())).collect()
        }
        fn get_or_default(&self, key: i32) -> String {
            self.0.get(&key).cloned().unwrap_or_default()
        }
    }

    #[test]
    fn save_cache_merges_instead_of_replacing() {
        let mut c = FakeCache::default();
        c.put(1, "one".into());
        c.put(2, "two".into());
        // 只上报 3，1/2 必须还在 —— 这是与 1100（整表替换）的关键区别
        save_client_cache(&mut c, &[(3, "three".into())]);
        assert_eq!(
            c.all(),
            vec![
                (1, "one".to_string()),
                (2, "two".to_string()),
                (3, "three".to_string())
            ]
        );
    }

    #[test]
    fn save_cache_reports_written_count() {
        let mut c = FakeCache::default();
        assert_eq!(
            save_client_cache(&mut c, &[(1, "a".into()), (2, "b".into())]),
            2
        );
    }

    #[test]
    fn get_cache_without_keys_returns_everything() {
        let mut c = FakeCache::default();
        c.put(1, "a".into());
        c.put(2, "b".into());
        assert_eq!(get_client_cache(&c, &[]).len(), 2);
    }

    #[test]
    fn get_cache_missing_key_yields_empty_string_not_skip() {
        let mut c = FakeCache::default();
        c.put(1, "a".into());
        assert_eq!(
            get_client_cache(&c, &[1, 99]),
            vec![(1, "a".to_string()), (99, String::new())]
        );
    }

    #[test]
    fn get_cache_keeps_requested_order() {
        let mut c = FakeCache::default();
        c.put(1, "a".into());
        c.put(2, "b".into());
        let got: Vec<i32> = get_client_cache(&c, &[2, 1])
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(got, vec![2, 1]);
    }

    // ---- 1010 ----
    struct Cost {
        positioning: bool,
        item54: bool,
        calls: Vec<&'static str>,
    }

    impl Cost {
        fn new(positioning: bool, item54: bool) -> Self {
            Self {
                positioning,
                item54,
                calls: Vec::new(),
            }
        }
    }

    impl LocationCost for Cost {
        fn charge_positioning(&mut self) -> bool {
            self.calls.push("positioning");
            self.positioning
        }
        fn charge_item_54(&mut self) -> bool {
            self.calls.push("item54");
            self.item54
        }
    }

    fn ctx(city: i32, times: i32) -> LocationCtx {
        LocationCtx {
            city_id: city,
            reset_times: times,
            address_exists: true,
        }
    }

    #[test]
    fn unknown_city_rejects_with_116() {
        let mut cost = Cost::new(true, true);
        let c = LocationCtx {
            address_exists: false,
            ..ctx(1, 0)
        };
        assert_eq!(
            set_location(2, 1, &c, &mut cost),
            SetLocationOutcome::Reject(RetCode::PlayerError16)
        );
        assert!(cost.calls.is_empty());
    }

    #[test]
    fn same_city_rejects_with_122_before_any_cost() {
        let mut cost = Cost::new(true, true);
        assert_eq!(
            set_location(5, 3, &ctx(5, 0), &mut cost),
            SetLocationOutcome::Reject(RetCode::PlayerError22)
        );
        assert!(cost.calls.is_empty());
    }

    #[test]
    fn free_move_only_when_never_moved() {
        let mut cost = Cost::new(true, true);
        assert_eq!(
            set_location(2, 1, &ctx(1, 1), &mut cost),
            SetLocationOutcome::Reject(RetCode::PlayerError21)
        );
        assert_eq!(
            set_location(2, 1, &ctx(1, 0), &mut cost),
            SetLocationOutcome::Accept {
                new_city_id: 2,
                reset_times: 1
            }
        );
    }

    #[test]
    fn kind2_requires_prior_move_then_charges_positioning() {
        let mut cost = Cost::new(true, true);
        assert_eq!(
            set_location(2, 2, &ctx(1, 0), &mut cost),
            SetLocationOutcome::Reject(RetCode::PlayerError15)
        );
        assert_eq!(
            set_location(2, 2, &ctx(1, 3), &mut cost),
            SetLocationOutcome::Accept {
                new_city_id: 2,
                reset_times: 4
            }
        );
        assert_eq!(cost.calls, vec!["positioning"]);
    }

    #[test]
    fn kind2_insufficient_jewel_rejects_with_7() {
        let mut cost = Cost::new(false, true);
        assert_eq!(
            set_location(2, 2, &ctx(1, 1), &mut cost),
            SetLocationOutcome::Reject(RetCode::JewelNo)
        );
        assert_eq!(cost.calls, vec!["positioning"]);
    }

    #[test]
    fn kind3_insufficient_item_rejects_with_6_not_7() {
        let mut cost = Cost::new(true, false);
        assert_eq!(
            set_location(2, 3, &ctx(1, 1), &mut cost),
            SetLocationOutcome::Reject(RetCode::ResourcesNo)
        );
        assert_eq!(cost.calls, vec!["item54"]);
    }

    #[test]
    fn other_kind_skips_all_checks_and_costs() {
        let mut cost = Cost::new(false, false);
        assert_eq!(
            set_location(2, 9, &ctx(1, 0), &mut cost),
            SetLocationOutcome::Accept {
                new_city_id: 2,
                reset_times: 1
            }
        );
        assert!(cost.calls.is_empty());
    }

    // ---- 1009 ----
    #[test]
    fn missing_gift_key_returns_110() {
        assert_eq!(
            use_gift_key("ABC", false),
            GiftKeyOutcome::Reject(RetCode::PlayerError10)
        );
    }

    #[test]
    fn gift_key_is_lowercased_before_lookup() {
        // 大小写不该影响结果；这里用 exists 反映"已经用规范化后的键查到"
        assert_eq!(use_gift_key("AbC", true), GiftKeyOutcome::Accept);
    }

    // ---- 1015 ----
    #[test]
    fn describe_id_is_last_segment() {
        assert_eq!(describe_lang_id("MsySystem_101"), Some(101));
        assert_eq!(describe_lang_id("Msy_1_2_103"), Some(103));
        assert_eq!(describe_lang_id("MsySystem_abc"), None);
        assert_eq!(describe_lang_id(""), None);
    }

    #[test]
    fn system_open_missing_template_is_busy() {
        assert_eq!(
            system_open(5, None),
            SystemOpenOutcome::Reject(RetCode::ServerBusy)
        );
    }

    #[test]
    fn system_open_ok() {
        assert_eq!(
            system_open(5, Some("MsySystem_101")),
            SystemOpenOutcome::Accept {
                system_id: 5,
                lang_id: 101
            }
        );
    }

    // ---- 1024 ----
    #[test]
    fn update_sex_charges_first() {
        let mut charged = 0;
        assert_eq!(
            update_sex(2, &mut || {
                charged += 1;
                false
            }),
            UpdateSexOutcome::Reject(RetCode::JewelNo)
        );
        assert_eq!(charged, 1, "扣费必须发生在 setSex 之前");
        assert_eq!(
            update_sex(2, &mut || {
                charged += 1;
                true
            }),
            UpdateSexOutcome::Accept { sex: 2 }
        );
        assert_eq!(charged, 2);
    }
}
