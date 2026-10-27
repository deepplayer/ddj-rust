//! # 分叉记录（原版行为 vs 本工程行为）
//!
//! 按 `AGENTS.md` §3：只有当原版行为会让 Rust 进程**挂死 / panic / 无限占用**时，
//! 才允许偏离原版。目前有 3 处，全部在这里登记并统一记录。
//!
//! ## 为什么需要这个模块
//! 这三处的分叉**只在畸形数据或极端数值下触发**，正常跑不会出现。但一旦真实客户端
//! 连上来，表现会是「某个操作没反应 / 回了个错误码」，从日志里看不出是模板数据坏了
//! 还是代码写错了。所以每次**真的触发**分叉时，打一条带完整上下文的结构化日志：
//!
//! ```text
//! WARN target=ddj::divergence code=P1 player=10001 detail="compound 链超过 64 层，原版会死循环"
//! ```
//!
//! 日志里带 `original` 字段说明「原版在这种情况下会怎样」，便于对照
//! `data_d/` 反编译源码判断这是原版缺陷还是移植错误。
//!
//! ## 计数
//! 每处分叉维护一个 `AtomicU64`，进程内累计。`ddj status` 与 GM 后台可以读出来
//! —— 如果某个计数器非 0，说明该分叉在真实流量下被触发过，需要拿日志里的
//! `player` / `detail` 去查模板数据。
//!
//! ## 不做的事
//! - 不改变业务返回值（护栏行为保持现状，见各分叉点的说明）
//! - 不做限流（分叉是异常路径，正常流量下次数为 0；真出问题时全量记录反而有用）

use std::sync::atomic::{AtomicU64, Ordering};

/// 已登记的分叉点。
///
/// 编号与 `docs/AUDIT.md` 的「待裁决偏离」表一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Divergence {
    /// **P1**：`ItemSet.itemCompound` 的链式追溯无环检测。
    ///
    /// 原版 `while(true)` 在 `compoundItem` 成环时**永久循环**，该玩家线程卡死。
    /// 本工程限制 64 层，超出回 `PARAM_ERROR(29)`。
    P1CompoundCycle,
    /// **P2**：`DailySignDB.rand()` 的池空。
    ///
    /// 原版 `pool.remove(randomIndex(pool.size()))` 在 `randTimes > 奖励条数` 时
    /// 池空 → `IndexOutOfBoundsException` → 被 handler 外层 catch → `ret=2`。
    /// 本工程提前 `break`，返回已抽到的部分（可能少于 `randTimes`）。
    P2RandPickShortPool,
    /// **P3**：`long` 乘法溢出。
    ///
    /// 原版 Java `long` 溢出会**回绕成负数**，随后的 `num < removeNum` 比较、
    /// `checkItem` 判定都会基于负数得出不同结论。
    /// 本工程用 `saturating_*`，饱和到 `i64::MAX` / `i64::MIN`。
    P3SaturatingArith,
}

impl Divergence {
    /// 编号（与 `docs/AUDIT.md` 一致）。
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::P1CompoundCycle => "P1",
            Self::P2RandPickShortPool => "P2",
            Self::P3SaturatingArith => "P3",
        }
    }

    /// 原版在这种情况下会怎样（写进日志的 `original` 字段）。
    #[must_use]
    pub const fn original(self) -> &'static str {
        match self {
            Self::P1CompoundCycle => {
                "ItemSet.itemCompound 的 while(true) 无环检测，会永久循环卡住玩家线程"
            }
            Self::P2RandPickShortPool => {
                "DailySignDB.rand 池空抛 IndexOutOfBoundsException，被 catch 成 ret=2"
            }
            Self::P3SaturatingArith => "Java long 乘法回绕成负数，后续比较结论不同",
        }
    }

    /// 本工程的护栏行为（写进日志的 `ours` 字段）。
    #[must_use]
    pub const fn ours(self) -> &'static str {
        match self {
            Self::P1CompoundCycle => "限制 64 层，超出回 PARAM_ERROR(29)",
            Self::P2RandPickShortPool => "提前 break，返回已抽到的部分",
            Self::P3SaturatingArith => "saturating_*，饱和到 i64 边界",
        }
    }

    /// 全部登记项（`ddj status` / GM 后台遍历用）。
    pub const ALL: [Self; 3] = [
        Self::P1CompoundCycle,
        Self::P2RandPickShortPool,
        Self::P3SaturatingArith,
    ];

    /// 该分叉在进程内被触发的次数。
    #[must_use]
    pub fn count(self) -> u64 {
        counter(self).load(Ordering::Relaxed)
    }
}

/// 每处分叉的计数器。
static P1: AtomicU64 = AtomicU64::new(0);
static P2: AtomicU64 = AtomicU64::new(0);
static P3: AtomicU64 = AtomicU64::new(0);

fn counter(d: Divergence) -> &'static AtomicU64 {
    match d {
        Divergence::P1CompoundCycle => &P1,
        Divergence::P2RandPickShortPool => &P2,
        Divergence::P3SaturatingArith => &P3,
    }
}

/// 记录一次分叉：计数器 +1，并打一条 `WARN`。
///
/// `detail` 由调用方给出本次触发的具体上下文（模板 id、玩家、数值等），
/// 会原样进日志的 `detail` 字段。
///
/// 用 `WARN` 而不是 `ERROR`：分叉本身不代表服务出错（护栏已经兜住），
/// 但需要运维知道「有畸形数据被碰上了」。
pub fn record(d: Divergence, detail: impl std::fmt::Display) {
    let n = counter(d).fetch_add(1, Ordering::Relaxed) + 1;
    tracing::warn!(
        target: "ddj::divergence",
        code = d.code(),
        original = d.original(),
        ours = d.ours(),
        occurrences = n,
        detail = %detail,
        "偏离原版行为（AGENTS.md §3 护栏），原版会在此处异常"
    );
}

/// 一次分叉的只读快照（给 `ddj status` / GM 后台）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DivergenceCount {
    /// 分叉点。
    pub divergence: Divergence,
    /// 触发次数。
    pub count: u64,
}

/// 全部计数（含 0 的项，便于前端固定展示三行）。
#[must_use]
pub fn snapshot() -> Vec<DivergenceCount> {
    Divergence::ALL
        .iter()
        .map(|&d| DivergenceCount {
            divergence: d,
            count: d.count(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable() {
        // 编号写进了日志与 AUDIT 台账，不能改
        assert_eq!(Divergence::P1CompoundCycle.code(), "P1");
        assert_eq!(Divergence::P2RandPickShortPool.code(), "P2");
        assert_eq!(Divergence::P3SaturatingArith.code(), "P3");
    }

    #[test]
    fn all_contains_every_variant() {
        assert_eq!(Divergence::ALL.len(), 3);
        for d in Divergence::ALL {
            assert!(!d.original().is_empty());
            assert!(!d.ours().is_empty());
        }
    }

    #[test]
    fn record_increments_counter() {
        // 注意：计数器是进程级静态，这里只验证「调用后变大」
        let before = Divergence::P1CompoundCycle.count();
        record(Divergence::P1CompoundCycle, "test");
        assert_eq!(Divergence::P1CompoundCycle.count(), before + 1);
    }

    #[test]
    fn snapshot_has_three_entries() {
        let s = snapshot();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].divergence, Divergence::P1CompoundCycle);
        assert_eq!(s[2].divergence, Divergence::P3SaturatingArith);
    }
}
