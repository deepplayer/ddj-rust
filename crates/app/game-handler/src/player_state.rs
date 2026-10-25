//! 玩家侧状态的**存储抽象**。
//!
//! ## 为什么需要这一层
//! 1103 / 1111 / 1112 / 1010 / 1024 这类「小功能」都读写**同一份**玩家态
//! （`playerGuideInfo`、`clientCacheMap`、`cityId`、`resetAddressTimes`）。
//! 在原版 Java 里这份态就是 `Player` 对象上的字段，随便哪个模块都能拿到；
//! 但 Rust 侧如果让每个 handler 各自持一个 repo，就会出现
//! **同一个玩家被多个 handler 复制成多份内存态**的问题。
//!
//! 所以这里定义一个**对象安全**的窄接口：只暴露「读一块 / 写一块」，
//! 真正的分片容器（带脏标记与批量落库）在 `ddj-game-core` 里实现。
//!
//! ## 为什么不直接 `fn with(&self, f: &mut dyn FnMut(&mut PlayerState))`
//! 那样虽然通用，但要跨 `await` 持有锁、还不好做「写后置脏」。
//! 按块开方法反而更清楚，也让每个方法能独立做权限/限流。

use async_trait::async_trait;
use ddj_core::{PlayerId, Result};

use crate::player_misc::GuideInfo;

/// 一个玩家在「小功能」维度上的全部可持久化状态。
///
/// 字段与原版 `Player` / `PlayerCommon` 的对应关系写在每个字段上。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerState {
    /// `Player.playerGuideInfo`（键 `id`/`time`/`nextId`）。
    pub guide: GuideInfo,
    /// `PlayerCommon.clientCacheMap`（`ConcurrentHashMap<Integer,String>`）。
    ///
    /// Rust 侧用 `BTreeMap` 而不是 `HashMap`：原版遍历顺序不保证，
    /// 但测试要能断言，故固定成有序。
    pub cache: std::collections::BTreeMap<i32, String>,
    /// `Player.cityId`。
    pub city_id: i32,
    /// `Player.resetAddressTimes`。
    pub reset_address_times: i32,
}

/// 玩家态存储。实现方负责分片、加锁、脏标记与落库。
#[async_trait]
pub trait PlayerStateStore: Send + Sync {
    /// 读引导存档。
    async fn guide(&self, player: PlayerId) -> Result<GuideInfo>;

    /// 写引导存档（原版整体替换三个键）。
    async fn save_guide(&self, player: PlayerId, info: GuideInfo) -> Result<()>;

    /// 逐键合并客户端缓存（**不是**整表替换）。
    async fn cache_put(&self, player: PlayerId, items: Vec<(i32, String)>) -> Result<()>;

    /// 读客户端缓存：`keys` 为空回整表；否则按键取，缺失键回空串。
    async fn cache_get(&self, player: PlayerId, keys: Vec<i32>) -> Result<Vec<(i32, String)>>;

    /// 读定位相关态（`cityId` + `resetAddressTimes`）。
    async fn location(&self, player: PlayerId) -> Result<(i32, i32)>;

    /// 写定位成功后落地（`cityId` 覆盖、次数 +1）。
    async fn save_location(&self, player: PlayerId, city_id: i32, times: i32) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_all_zero() {
        let st = PlayerState::default();
        assert_eq!(st.guide, GuideInfo::default());
        assert!(st.cache.is_empty());
        assert_eq!(st.city_id, 0);
        assert_eq!(st.reset_address_times, 0);
    }

    #[test]
    fn cache_map_is_ordered() {
        let mut st = PlayerState::default();
        st.cache.insert(9, "nine".into());
        st.cache.insert(2, "two".into());
        let keys: Vec<i32> = st.cache.keys().copied().collect();
        assert_eq!(keys, vec![2, 9], "BTreeMap 让遍历顺序可断言");
    }
}
