//! 系统开放（`SystemOpenUtils.isOpen`）的接线 + 注入点。
//!
//! 纯逻辑在 [`crate::system_open`]，本文件负责：
//! 1. 持有 `SystemOpenDBCache`（`OnceLock`，启动时注入一次）；
//! 2. 把 [`MemWorld`] 的玩家态 + 服级信息拼成 [`PlayerFacts`]。
//!
//! ## 为什么 Facts 要单独抽
//! 原版 `isOpen` 直接 `PlayerServerModule.getInstance().getPlayerById(...)`
//! 然后满世界抓单例。移植期这些模块大多没接，所以这里把「能拿到的事实」
//! 显式列出来，拿不到的填默认值 —— 默认值是「不满足」，只会让功能**少开**，
//! 不会误开。
//!
//! ## ⚠ 过渡期只有两个事实是真的
//! | 事实 | 来源 | 状态 |
//! |---|---|---|
//! | `level` | `PlayerBox.level`（登录时从 PG 读） | ✅ 真实 |
//! | `open_server_day` | PG `bf_server.open_time`（启动时注入 `ServerFacts`） | ✅ 真实 |
//! | 其余 17 项 | 对应模块（家族 / 产业 / 狩猎 / 门客 / 珍兽 …）未接 | ⬜ 恒默认 |
//!
//! **不要**用别的字段「近似」未接的语义。例如 `unlockType=20` 要的是
//! 「达到某阶的门客数」，`PlayerBox.pet_skill_class` 是**技能阶级**不是**门客阶数**，
//! 拿它近似会让本该关闭的功能误开。宁可填 `0`。
//! 已记入 `docs/AUDIT.md` A20。
//!
//! ## ⚠ 开服天数来自 PG `bf_server.open_time`
//! `GsServer.getOpenServerDay()`：`now < open_time` 时返回 `0`，
//! 否则 `getIntervalDay(open_time, now) + 1`（**从 1 开始**）。
//! `getIntervalDay` 是「零点对齐（东八区）后的天数差」，同一天返回 `0`。

use std::sync::OnceLock;

use ddj_core::{Clock, PlayerId};

use crate::runtime::{MemWorld, PlayerBox};
use crate::system_open::{PlayerFacts, SystemOpenCache};

/// 东八区偏移（毫秒）。与原版部署环境 `Asia/Shanghai` 一致。
const TZ_MS: i64 = 8 * 3600 * 1000;

/// `SystemOpenDBCache` 的注入点。
static CACHE: OnceLock<SystemOpenCache> = OnceLock::new();

/// 注入系统开放表（只第一次生效）。
pub fn install(c: SystemOpenCache) {
    let _ = CACHE.set(c);
}

/// 当前缓存；未注入时是空表 → `is_open` 恒 `false`（与原版「表未加载」一致）。
pub fn cache() -> &'static SystemOpenCache {
    CACHE.get_or_init(SystemOpenCache::default)
}

/// `GsServer.getOpenServerDay()`：`now < open_time` → `0`，否则天数差 + 1。
#[must_use]
pub fn open_server_day(open_time: i64, now: i64) -> i32 {
    if now < open_time {
        return 0;
    }
    // `UtilsTools.getIntervalDay`：同一天 → 0；否则零点差 / 86400000
    let day_zero = |t: i64| (t + TZ_MS).div_euclid(86_400_000);
    let diff = (day_zero(now) - day_zero(open_time)).unsigned_abs();
    i32::try_from(diff).unwrap_or(i32::MAX).saturating_add(1)
}

/// 服级信息（`GsServer` 里 `isOpen` 会读到的部分）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerFacts {
    /// `bf_server.open_time`（毫秒）。
    pub open_time: i64,
}

/// 服级事实的注入点（启动时从 PG `bf_server` 读一次）。
static SERVER: OnceLock<ServerFacts> = OnceLock::new();

/// 注入服级事实（只第一次生效）。
pub fn install_server(f: ServerFacts) {
    let _ = SERVER.set(f);
}

/// 当前服级事实；未注入时 `open_time = 0` → `open_day` 恒为 1（视为开服当天）。
pub fn server() -> ServerFacts {
    SERVER.get().copied().unwrap_or_default()
}

/// 由 `PlayerBox` + 服级信息拼出判定事实。
///
/// 只有 `level` 与 `open_server_day` 是真实值；其余保持默认（恒「不满足」）。
#[must_use]
pub fn facts_of(b: &PlayerBox, server: ServerFacts, now: i64) -> PlayerFacts {
    PlayerFacts {
        level: b.level,
        open_server_day: open_server_day(server.open_time, now),
        // 以下 17 项依赖未接模块，见模块文档表格。不要拿近似字段顶替。
        ..PlayerFacts::default()
    }
}

/// 便捷入口：按玩家 id 判定（读 `MemWorld` 的当前状态）。
#[must_use]
pub fn is_open(
    world: &MemWorld,
    player: PlayerId,
    clock: &dyn Clock,
    server: ServerFacts,
    id: i32,
) -> bool {
    let now = clock.now_millis();
    let facts = world.peek(player, |b| facts_of(b, server, now));
    cache().is_open(id, &facts)
}

/// 便捷入口：直接给事实。
#[must_use]
pub fn is_open_with(id: i32, f: &PlayerFacts) -> bool {
    cache().is_open(id, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_day_before_open_time_is_zero() {
        assert_eq!(open_server_day(1_000, 999), 0);
    }

    #[test]
    fn open_day_counts_from_one() {
        let open = 1_700_000_000_000; // 2023-11-14T22:13:20Z = 11-15 06:13 CST
        assert_eq!(open_server_day(open, open), 1);
        // 同一「东八区日」内的稍晚时刻仍是 1
        assert_eq!(open_server_day(open, open + 3_600_000), 1);
    }

    #[test]
    fn open_day_advances_after_midnight_cst() {
        // 2023-11-15 23:59:59.999 CST
        let open = 1_700_063_999_999;
        // 2023-11-16 00:00:00.000 CST ← 刚跨过东八区零点
        let after = 1_700_064_000_000;
        assert_eq!(open_server_day(open, after), 2);
    }

    #[test]
    fn empty_cache_is_closed() {
        assert!(!cache().is_open(120, &PlayerFacts::default()));
    }

    #[test]
    fn facts_carry_level_and_open_day_only() {
        let b = PlayerBox {
            level: 7,
            ..PlayerBox::default()
        };
        let f = facts_of(&b, ServerFacts { open_time: 0 }, 0);
        assert_eq!(f.level, 7);
        assert_eq!(f.open_server_day, 1, "open_time=0 视为早已开服");
        // 未接模块的字段全默认
        assert_eq!(f.vip_lv, 0);
        assert_eq!(f.talent_count, 0);
        assert!(f.talent_levels.is_empty());
        assert!(f.family_lv.is_none());
    }

    #[test]
    fn real_open_time_row_120_opens_on_day_two() {
        // 实测 `bf_server.dbId=1` 的 open_time = 1733356835000
        // = 2024-12-05 03:00:35 CST
        let open = 1_733_356_835_000;
        let cache = SystemOpenCache::new(vec![crate::system_open::SystemOpenTemplate {
            id: 120,
            unlock_type: vec![13],
            value: vec!["2".to_owned()],
            name: "change_word_607".to_owned(),
        }]);
        let b = PlayerBox::default();
        // 开服当天 → 1 天 < 2 天 → 关闭
        assert!(!cache.is_open(120, &facts_of(&b, ServerFacts { open_time: open }, open)));
        // 第二天 → 2 天 → 开放
        let next_day = open + 86_400_000;
        assert!(cache.is_open(
            120,
            &facts_of(&b, ServerFacts { open_time: open }, next_day)
        ));
    }
}
