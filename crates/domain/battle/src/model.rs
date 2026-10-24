//! 战斗聚合
//!
//! 对应 Java：`bf.game.server.domain.battle.*`（战报 / 战斗服返回校验）。
//!
//! ## 关键设计
//! 原系统的战斗在独立进程（`BattleServer`）里算，结果回传给游戏服。
//! 客户端也能上报战报 —— 这是最大的作弊入口。这里把「战报」建模为
//! **不可信输入**，必须经过 `replay::verify` 才能采信。

use ddj_core::PlayerId;

/// 战斗结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 胜利。
    Win = 1,
    /// 失败。
    Lose = 2,
    /// 平局。
    Draw = 3,
}

/// 一次战斗的回放数据。
#[derive(Debug, Clone)]
pub struct BattleReport {
    /// 战斗 ID。
    pub battle_id: u64,
    /// 发起者。
    pub attacker: PlayerId,
    /// 防守者。
    pub defender: PlayerId,
    /// 关卡 / 玩法 ID。
    pub stage: i32,
    /// 结果。
    pub outcome: Outcome,
    /// 回合序列（每步：技能 ID + 目标索引）。
    pub steps: Vec<(i32, u8)>,
    /// 客户端声称的伤害总和。
    pub claimed_damage: i64,
    /// 结束时间。
    pub ended_ms: i64,
}

impl BattleReport {
    /// 步数。
    #[must_use]
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 是否可疑：步数超过理论上限。
    #[must_use]
    pub fn suspicious(&self, max_steps: usize) -> bool {
        self.steps.len() > max_steps
    }
}

/// 单场战斗最大回合数（原 `t_stage.max_round` 上限）。
pub const MAX_STEPS: usize = 500;
