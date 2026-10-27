//! # 防骗手册（17151 / 17152 / 17153）
//!
//! 对应 Java：
//! - `SysActivityMessageHandler.antiCheatReward`（行 119-142）→ 17151
//! - `SysActivityMessageHandler.antiCheatRead`（行 143-153）→ 17152
//! - `SysActivityMessageHandler.antiCheat`（行 154-162）→ 17153
//! - `bf/game/server/module/sysActivity/model/AntiCheatModel.java`（75 行）
//! - `bf/template/config/AntiCheatRewardJson.java`（配置）
//!
//! ## 三条消息
//! | cmd | 作用 | 回包 |
//! |---|---|---|
//! | 17151 | 领取某条防骗奖励 | `AntiCheatRewardResp_17151{index, ret}` |
//! | 17152 | 标记某条已阅读 | `AntiCheatReadResp_17152{ret=0}` |
//! | 17153 | 查询全部状态 | `AntiCheatResp_17153{info}` |
//!
//! ## ⚠ 17151 的三种失败码都在 Resp 里，不是 ErrorResp_1
//! ```text
//! !canReward(index)                              → PARAM_ERROR(29)
//! !read.contains(index)                          → ANTICHEAT_READ(6451) 未读完
//! !addReward(index)（已领过）                     → ACTIVITY_RANK_LIST_ERROR_REPEATED(1501)
//! 否则 → 发奖 + ret=0
//! ```
//! 且 **`ret=0` 与失败共用同一个 `sendMessage`**，帧号恒为 17151。
//!
//! ## ⚠ 17152 的静默分支
//! `!canReward(index)` 时**直接 return，不发包**。客户端会超时。
//! 这是原版写法，照抄。
//!
//! ## ⚠ 17151 的 catch 也不发包
//! 外层 `catch (Exception)` 只记日志。所以任何异常（例如模型未加载）
//! 都表现为「客户端收不到回包」。
//!
//! ## `AntiCheatRewardJson` 的配置格式
//! `reward` 是 `List<List<ResourcesJson>>`，由 `param.split("\\|")` 得到外层，
//! 每段再用 `ResourcesJson.toResourcesJsons` 解析。`canReward(index)` 只判
//! `index < reward.size()`。

use ddj_pb::RetCode;

/// 一条奖励的 `(资源 id, 数量)` 列表。
pub type RewardGroup = Vec<(i32, i64)>;

/// `AntiCheatRewardJson`：配置表里的一串奖励组。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AntiCheatRewardConfig {
    /// `reward`：每个 index 对应一组资源。
    pub reward: Vec<RewardGroup>,
}

impl AntiCheatRewardConfig {
    /// `canReward(index)`：`index < reward.size()`。
    ///
    /// ⚠ 原版是 `reward.size() <= index → false`，**负数 index 会越界**
    /// （`size() <= -1` 为 false，然后 `rewardList(-1)` 抛 IndexOutOfBounds）。
    /// 这里显式拦负数，返回 false。
    #[must_use]
    pub fn can_reward(&self, index: i32) -> bool {
        if index < 0 {
            return false;
        }
        usize::try_from(index).is_ok_and(|i| i < self.reward.len())
    }

    /// `rewardList(index)`。
    #[must_use]
    pub fn reward_list(&self, index: i32) -> RewardGroup {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.reward.get(i))
            .cloned()
            .unwrap_or_default()
    }
}

/// 防骗手册状态（`AntiCheatModel` 的两个集合）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AntiCheatState {
    /// `read`：已阅读的 index。
    pub read: Vec<i32>,
    /// `reward`：已领取的 index。
    pub reward: Vec<i32>,
}

impl AntiCheatState {
    /// `isRead(index)`。
    #[must_use]
    pub fn is_read(&self, index: i32) -> bool {
        self.read.contains(&index)
    }

    /// `addRead(index)`：已存在则不动。
    #[must_use]
    pub fn add_read(&self, index: i32) -> Self {
        if self.read.contains(&index) {
            return self.clone();
        }
        let mut next = self.clone();
        next.read.push(index);
        next
    }

    /// `addReward(index)`：返回 `None` 表示已领过（原版返回 `false`）。
    #[must_use]
    pub fn add_reward(&self, index: i32) -> Option<Self> {
        if self.reward.contains(&index) {
            return None;
        }
        let mut next = self.clone();
        next.reward.push(index);
        Some(next)
    }
}

/// 17151 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntiCheatRewardOutcome {
    /// 回 `AntiCheatRewardResp_17151{ret=code}`（**不是** ErrorResp_1）。
    Reject(RetCode),
    /// 通过：发 `rewards` 并记录 index 已领。
    Accept {
        /// 要发的奖励。
        rewards: RewardGroup,
    },
}

impl AntiCheatRewardOutcome {
    /// 协议上的 `ret`。
    #[must_use]
    pub const fn resolve_ret(&self) -> i32 {
        match self {
            Self::Reject(code) => code.as_i32(),
            Self::Accept { .. } => 0,
        }
    }
}

/// `antiCheatReward`（17151）的纯逻辑。
#[must_use]
pub fn reward_verdict(
    state: &AntiCheatState,
    cfg: &AntiCheatRewardConfig,
    index: i32,
) -> AntiCheatRewardOutcome {
    // 1. index 越界
    if !cfg.can_reward(index) {
        return AntiCheatRewardOutcome::Reject(RetCode::ParamError);
    }
    // 2. 未阅读
    if !state.is_read(index) {
        return AntiCheatRewardOutcome::Reject(RetCode::AnticheatRead);
    }
    // 3. 已领过（addReward 返回 false）
    if state.reward.contains(&index) {
        return AntiCheatRewardOutcome::Reject(RetCode::ActivityRankListErrorRepeated);
    }
    AntiCheatRewardOutcome::Accept {
        rewards: cfg.reward_list(index),
    }
}

/// 17152 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntiCheatReadOutcome {
    /// **不发包**（原版 `return`，客户端超时）。
    Silent,
    /// 回 `AntiCheatReadResp_17152{ret=0}`，并把 index 写入 read。
    Accept,
}

/// `antiCheatRead`（17152）的纯逻辑。
#[must_use]
pub fn read_verdict(cfg: &AntiCheatRewardConfig, index: i32) -> AntiCheatReadOutcome {
    if !cfg.can_reward(index) {
        // 原版直接 return，不发包
        return AntiCheatReadOutcome::Silent;
    }
    AntiCheatReadOutcome::Accept
}

/// 17153 下发的状态快照（`AntiCheatInfo` 的两个列表）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AntiCheatInfo {
    /// `antiCheatRead`。
    pub read: Vec<i32>,
    /// `antiCheatReward`。
    pub reward: Vec<i32>,
}

impl AntiCheatState {
    /// `AntiCheatModel.toMsg()`。
    #[must_use]
    pub fn to_info(&self) -> AntiCheatInfo {
        AntiCheatInfo {
            read: self.read.clone(),
            reward: self.reward.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AntiCheatRewardConfig {
        AntiCheatRewardConfig {
            reward: vec![
                vec![(1, 100)],
                vec![(2, 200), (3, 300)],
                vec![(4, 400)],
            ],
        }
    }

    fn empty() -> AntiCheatState {
        AntiCheatState::default()
    }

    // ── 配置 ──

    #[test]
    fn can_reward_checks_index_range() {
        let c = cfg();
        assert!(c.can_reward(0));
        assert!(c.can_reward(2));
        assert!(!c.can_reward(3), "size=3，index 3 越界");
        assert!(!c.can_reward(-1), "负数原版会越界，这里拦掉");
    }

    #[test]
    fn reward_list_returns_group() {
        let c = cfg();
        assert_eq!(c.reward_list(1), vec![(2, 200), (3, 300)]);
        assert_eq!(c.reward_list(99), Vec::new(), "越界返回空");
    }

    // ── 17151 ──

    #[test]
    fn reward_out_of_range_is_param_error() {
        let out = reward_verdict(&empty(), &cfg(), 5);
        assert_eq!(
            out,
            AntiCheatRewardOutcome::Reject(RetCode::ParamError)
        );
        assert_eq!(out.resolve_ret(), RetCode::ParamError.as_i32());
    }

    #[test]
    fn reward_not_read_is_6451() {
        let out = reward_verdict(&empty(), &cfg(), 0);
        assert_eq!(
            out,
            AntiCheatRewardOutcome::Reject(RetCode::AnticheatRead)
        );
    }

    #[test]
    fn reward_already_got_is_1501() {
        let s = AntiCheatState {
            read: vec![0],
            reward: vec![0],
        };
        let out = reward_verdict(&s, &cfg(), 0);
        assert_eq!(
            out,
            AntiCheatRewardOutcome::Reject(RetCode::ActivityRankListErrorRepeated)
        );
    }

    #[test]
    fn reward_accept_carries_group() {
        let s = AntiCheatState {
            read: vec![1],
            reward: vec![],
        };
        match reward_verdict(&s, &cfg(), 1) {
            AntiCheatRewardOutcome::Accept { rewards } => {
                assert_eq!(rewards, vec![(2, 200), (3, 300)]);
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn reward_branch_order_range_before_read() {
        // 越界优先于未阅读
        assert_eq!(
            reward_verdict(&empty(), &cfg(), 99),
            AntiCheatRewardOutcome::Reject(RetCode::ParamError)
        );
    }

    #[test]
    fn reward_branch_order_read_before_duplicate() {
        // 未阅读优先于已领（即使 reward 里已有该 index）
        let s = AntiCheatState {
            read: vec![],
            reward: vec![0],
        };
        assert_eq!(
            reward_verdict(&s, &cfg(), 0),
            AntiCheatRewardOutcome::Reject(RetCode::AnticheatRead)
        );
    }

    // ── 17152 ──

    #[test]
    fn read_out_of_range_is_silent() {
        // ⚠ 原版直接 return，不发包
        assert_eq!(read_verdict(&cfg(), 99), AntiCheatReadOutcome::Silent);
        assert_eq!(read_verdict(&cfg(), -1), AntiCheatReadOutcome::Silent);
    }

    #[test]
    fn read_in_range_accepts() {
        assert_eq!(read_verdict(&cfg(), 0), AntiCheatReadOutcome::Accept);
    }

    // ── 状态操作 ──

    #[test]
    fn add_read_is_idempotent() {
        let s = empty().add_read(1);
        assert_eq!(s.read, vec![1]);
        let s2 = s.add_read(1);
        assert_eq!(s2.read, vec![1], "重复添加不增长");
    }

    #[test]
    fn add_reward_returns_none_when_duplicate() {
        let s = AntiCheatState {
            read: vec![0],
            reward: vec![],
        };
        let s2 = s.add_reward(0).expect("首次应成功");
        assert_eq!(s2.reward, vec![0]);
        assert!(s2.add_reward(0).is_none(), "重复领取返回 None");
    }

    #[test]
    fn is_read_reflects_state() {
        let s = AntiCheatState {
            read: vec![1, 2],
            reward: vec![],
        };
        assert!(s.is_read(1));
        assert!(!s.is_read(3));
    }

    #[test]
    fn to_info_mirrors_both_lists() {
        let s = AntiCheatState {
            read: vec![1, 2],
            reward: vec![3],
        };
        let info = s.to_info();
        assert_eq!(info.read, vec![1, 2]);
        assert_eq!(info.reward, vec![3]);
    }

    #[test]
    fn to_info_of_empty() {
        let info = empty().to_info();
        assert!(info.read.is_empty());
        assert!(info.reward.is_empty());
    }
}
