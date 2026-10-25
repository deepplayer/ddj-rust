//! 玩家侧状态的**分片容器**。
//!
//! `ddj-game-handler` 只声明 [`PlayerStateStore`] 这个窄接口（它要保持零 IO），
//! 真正的持有、加锁、脏标记、落库放在这里。
//!
//! ## 并发模型
//! 每个玩家一把 `std::sync::Mutex`，**临界区里不 `await`**：
//! 先在锁内改完并拍一份快照，解锁后再去 `await` 落库。
//! 这样既不会跨 await 持锁（`MutexGuard` 不是 `Send`），
//! 也不会把不同玩家的写操作串行化。
//!
//! ## 与原版 `GameReadWriteLock` 的差异
//! 原版是「一把大锁 + 超时」。这里按键分段，玩家之间互不阻塞 ——
//! 属于 m04429 允许的架构优化，不改变任何可观察行为。

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use dashmap::DashMap;
use ddj_core::{PlayerId, Result};

use ddj_game_handler::player_misc::GuideInfo;
use ddj_game_handler::player_state::{PlayerState, PlayerStateStore};

/// 脏标记：记录哪些块自上次落库后被改过。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dirty(u8);

impl Dirty {
    /// `playerGuideInfo`。
    pub const GUIDE: u8 = 1 << 0;
    /// `clientCacheMap`。
    pub const CACHE: u8 = 1 << 1;
    /// `cityId` + `resetAddressTimes`。
    pub const LOCATION: u8 = 1 << 2;

    /// 置位。
    pub fn set(&mut self, bit: u8) {
        self.0 |= bit;
    }

    /// 是否含某块。
    #[must_use]
    pub fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    /// 是否有任意脏块。
    #[must_use]
    pub fn any(self) -> bool {
        self.0 != 0
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.0 = 0;
    }

    /// 原始位。
    #[must_use]
    pub fn bits(self) -> u8 {
        self.0
    }
}

/// 玩家态的持久化后端。
#[async_trait]
pub trait PlayerStateRepo: Send + Sync {
    /// 冷加载。没有记录时返回默认值（原版新号也是空 map）。
    async fn load(&self, player: PlayerId) -> Result<PlayerState>;

    /// 按脏块落库。
    async fn save(&self, player: PlayerId, state: &PlayerState, dirty: Dirty) -> Result<()>;
}

/// 一个玩家的槽位：状态 + 脏位。
struct Slot {
    state: PlayerState,
    dirty: Dirty,
}

/// 分片容器。
pub struct PlayerStores {
    map: DashMap<PlayerId, Arc<Mutex<Slot>>>,
    repo: Arc<dyn PlayerStateRepo>,
}

impl std::fmt::Debug for PlayerStores {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerStores")
            .field("resident", &self.map.len())
            .finish()
    }
}

impl PlayerStores {
    /// 用给定后端构造（启动时调一次）。
    #[must_use]
    pub fn new(repo: Arc<dyn PlayerStateRepo>) -> Self {
        Self {
            map: DashMap::new(),
            repo,
        }
    }

    /// 当前常驻内存的玩家数。
    #[must_use]
    pub fn resident(&self) -> usize {
        self.map.len()
    }

    /// 取（或冷加载）某玩家的槽位。
    async fn slot(&self, player: PlayerId) -> Result<Arc<Mutex<Slot>>> {
        if let Some(slot) = self.map.get(&player) {
            return Ok(Arc::clone(slot.value()));
        }
        let loaded = self.repo.load(player).await?;
        let slot = Arc::new(Mutex::new(Slot {
            state: loaded,
            dirty: Dirty::default(),
        }));
        // 并发冷加载时后到者覆盖前者：两者内容相同，无害。
        self.map.insert(player, Arc::clone(&slot));
        Ok(slot)
    }

    /// 把脏块落库并清标记；无脏块时是空操作。
    pub async fn flush(&self, player: PlayerId) -> Result<()> {
        let Some(slot) = self.map.get(&player).map(|s| Arc::clone(s.value())) else {
            return Ok(());
        };
        let (snapshot, dirty) = {
            let g = lock(&slot);
            (g.state.clone(), g.dirty)
        };
        if !dirty.any() {
            return Ok(());
        }
        self.repo.save(player, &snapshot, dirty).await?;
        lock(&slot).dirty.clear();
        Ok(())
    }

    /// 下线时调用：落库并卸载。
    pub async fn release(&self, player: PlayerId) -> Result<()> {
        self.flush(player).await?;
        self.map.remove(&player);
        Ok(())
    }

    /// 当前脏位（测试与运维用）。
    #[must_use]
    pub fn dirty(&self, player: PlayerId) -> Dirty {
        self.map
            .get(&player)
            .map_or(Dirty::default(), |s| lock(s.value()).dirty)
    }

    /// 读一份状态副本（测试与运维用）。
    #[must_use]
    pub fn snapshot(&self, player: PlayerId) -> Option<PlayerState> {
        self.map.get(&player).map(|s| lock(s.value()).state.clone())
    }
}

/// 毒化锁的兜底：状态是纯数据，不会半更新，直接取回内部值。
fn lock(slot: &Arc<Mutex<Slot>>) -> MutexGuard<'_, Slot> {
    match slot.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[async_trait]
impl PlayerStateStore for PlayerStores {
    async fn guide(&self, player: PlayerId) -> Result<GuideInfo> {
        let slot = self.slot(player).await?;
        let info = lock(&slot).state.guide;
        Ok(info)
    }

    async fn save_guide(&self, player: PlayerId, info: GuideInfo) -> Result<()> {
        let slot = self.slot(player).await?;
        {
            let mut g = lock(&slot);
            g.state.guide = info;
            g.dirty.set(Dirty::GUIDE);
        }
        // 原版 `setPlayerGuideInfo` 末尾就调 `save()`，这里同样立即落库。
        self.flush(player).await
    }

    async fn cache_put(&self, player: PlayerId, items: Vec<(i32, String)>) -> Result<()> {
        let slot = self.slot(player).await?;
        {
            let mut g = lock(&slot);
            for (k, v) in items {
                g.state.cache.insert(k, v);
            }
            g.dirty.set(Dirty::CACHE);
        }
        self.flush(player).await
    }

    async fn cache_get(&self, player: PlayerId, keys: Vec<i32>) -> Result<Vec<(i32, String)>> {
        let slot = self.slot(player).await?;
        let g = lock(&slot);
        let out = if keys.is_empty() {
            g.state.cache.iter().map(|(k, v)| (*k, v.clone())).collect()
        } else {
            keys.iter()
                .map(|k| (*k, g.state.cache.get(k).cloned().unwrap_or_default()))
                .collect()
        };
        Ok(out)
    }

    async fn location(&self, player: PlayerId) -> Result<(i32, i32)> {
        let slot = self.slot(player).await?;
        let g = lock(&slot);
        Ok((g.state.city_id, g.state.reset_address_times))
    }

    async fn save_location(&self, player: PlayerId, city_id: i32, times: i32) -> Result<()> {
        let slot = self.slot(player).await?;
        {
            let mut g = lock(&slot);
            g.state.city_id = city_id;
            g.state.reset_address_times = times;
            g.dirty.set(Dirty::LOCATION);
        }
        self.flush(player).await
    }
}

/// 纯内存后端：可跑通整条链路，重启即丢（PG 版随后补）。
#[derive(Debug, Default)]
pub struct MemoryPlayerStateRepo;

#[async_trait]
impl PlayerStateRepo for MemoryPlayerStateRepo {
    async fn load(&self, _player: PlayerId) -> Result<PlayerState> {
        Ok(PlayerState::default())
    }

    async fn save(&self, _player: PlayerId, _state: &PlayerState, _dirty: Dirty) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingRepo {
        saves: AtomicUsize,
    }

    #[async_trait]
    impl PlayerStateRepo for CountingRepo {
        async fn load(&self, _p: PlayerId) -> Result<PlayerState> {
            Ok(PlayerState::default())
        }
        async fn save(&self, _p: PlayerId, _s: &PlayerState, _d: Dirty) -> Result<()> {
            self.saves.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    fn pid(n: i64) -> PlayerId {
        PlayerId::new(n)
    }

    #[tokio::test]
    async fn cold_load_returns_default() {
        let stores = PlayerStores::new(Arc::new(MemoryPlayerStateRepo));
        let g = stores.guide(pid(1)).await.expect("读");
        assert_eq!(g, GuideInfo::default());
    }

    #[tokio::test]
    async fn save_guide_persists_and_clears_dirty() {
        let repo = Arc::new(CountingRepo::default());
        let stores = PlayerStores::new(repo.clone());
        let info = GuideInfo {
            db_id: 7,
            next_id: 9,
            time: 11,
        };
        stores.save_guide(pid(1), info).await.expect("写");
        assert_eq!(stores.guide(pid(1)).await.expect("读"), info);
        assert!(!stores.dirty(pid(1)).any(), "落库后脏位必须清零");
        assert_eq!(repo.saves.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn cache_put_merges() {
        let stores = PlayerStores::new(Arc::new(MemoryPlayerStateRepo));
        stores
            .cache_put(pid(1), vec![(1, "a".into())])
            .await
            .expect("写");
        stores
            .cache_put(pid(1), vec![(2, "b".into())])
            .await
            .expect("写");
        let all = stores.cache_get(pid(1), vec![]).await.expect("读");
        assert_eq!(all, vec![(1, "a".to_string()), (2, "b".to_string())]);
    }

    #[tokio::test]
    async fn cache_get_missing_key_yields_empty_string() {
        let stores = PlayerStores::new(Arc::new(MemoryPlayerStateRepo));
        stores
            .cache_put(pid(1), vec![(1, "a".into())])
            .await
            .expect("写");
        let got = stores.cache_get(pid(1), vec![1, 99]).await.expect("读");
        assert_eq!(got, vec![(1, "a".to_string()), (99, String::new())]);
    }

    #[tokio::test]
    async fn two_players_are_isolated() {
        let stores = PlayerStores::new(Arc::new(MemoryPlayerStateRepo));
        stores
            .cache_put(pid(1), vec![(1, "one".into())])
            .await
            .expect("写");
        stores
            .cache_put(pid(2), vec![(1, "two".into())])
            .await
            .expect("写");
        assert_eq!(
            stores.cache_get(pid(1), vec![1]).await.expect("读"),
            vec![(1, "one".to_string())]
        );
        assert_eq!(
            stores.cache_get(pid(2), vec![1]).await.expect("读"),
            vec![(1, "two".to_string())]
        );
    }

    #[tokio::test]
    async fn release_unloads_slot() {
        let stores = PlayerStores::new(Arc::new(MemoryPlayerStateRepo));
        stores
            .cache_put(pid(1), vec![(1, "a".into())])
            .await
            .expect("写");
        assert_eq!(stores.resident(), 1);
        stores.release(pid(1)).await.expect("下线");
        assert_eq!(stores.resident(), 0);
    }

    #[tokio::test]
    async fn flush_without_changes_does_not_hit_repo() {
        let repo = Arc::new(CountingRepo::default());
        let stores = PlayerStores::new(repo.clone());
        stores.flush(pid(1)).await.expect("空 flush");
        assert_eq!(repo.saves.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn dirty_bits_are_independent() {
        let mut d = Dirty::default();
        d.set(Dirty::GUIDE);
        assert!(d.has(Dirty::GUIDE));
        assert!(!d.has(Dirty::CACHE));
        d.set(Dirty::CACHE);
        assert_eq!(d.bits(), Dirty::GUIDE | Dirty::CACHE);
        d.clear();
        assert!(!d.any());
    }
}
