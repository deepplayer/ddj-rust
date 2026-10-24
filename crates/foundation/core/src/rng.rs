//! 可播种随机源
//!
//! 战斗、掉落、洗练都需要**可复现**：
//! 客户端上报的结果服务端要能验算，出问题时能按种子重放。

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// 随机源抽象。
pub trait RngSource: Send + Sync {
    /// 生成 `[lo, hi]` 闭区间整数。
    fn gen_range_i64(&self, lo: i64, hi: i64) -> i64;
    /// 生成 `[0, n)` 的索引。
    fn gen_index(&self, n: usize) -> usize;
    /// 按万分比判定（`per_10000` = 10000 表示必中）。
    fn chance(&self, per_10000: u32) -> bool {
        self.gen_range_i64(1, 10_000) <= i64::from(per_10000)
    }
}

/// 确定性随机源（单线程）。
pub struct DeterministicRng {
    inner: parking_lot::Mutex<StdRng>,
    seed: u64,
}

impl DeterministicRng {
    /// 用固定种子构造。
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            inner: parking_lot::Mutex::new(StdRng::seed_from_u64(seed)),
            seed,
        }
    }
    /// 取种子（用于战报回放）。
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }
}

impl RngSource for DeterministicRng {
    fn gen_range_i64(&self, lo: i64, hi: i64) -> i64 {
        self.inner.lock().gen_range(lo..=hi)
    }
    fn gen_index(&self, n: usize) -> usize {
        self.inner.lock().gen_range(0..n)
    }
}

impl std::fmt::Debug for DeterministicRng {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeterministicRng")
            .field("seed", &self.seed)
            .finish()
    }
}
