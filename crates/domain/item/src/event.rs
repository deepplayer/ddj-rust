//! 领域事件
//!
//! 用于把「副作用」从主流程解耦：使用道具后要推送 `itemUpdate`、
//! 记任务进度、触发活动计数。原 Java 在这些点直接硬编码调用，
//! 新增一个系统就要改 `ItemSet`；这里改成事件订阅。

use ddj_core::{PlayerId, TemplateId};

/// 道具领域事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemEvent {
    /// 获得道具。
    Gained {
        /// 玩家。
        player: PlayerId,
        /// 模板。
        template: TemplateId,
        /// 数量。
        num: i64,
    },
    /// 消耗道具。
    Consumed {
        /// 玩家。
        player: PlayerId,
        /// 模板。
        template: TemplateId,
        /// 数量。
        num: i64,
        /// 剩余。
        left: i64,
    },
    /// 使用道具成功（本次逆向的核心）。
    Used {
        /// 玩家。
        player: PlayerId,
        /// 模板。
        template: TemplateId,
        /// 使用次数。
        num: i64,
        /// 扣除数量。
        removed: i64,
    },
}

impl ItemEvent {
    /// 事件名，用于日志与指标。
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Gained { .. } => "item.gained",
            Self::Consumed { .. } => "item.consumed",
            Self::Used { .. } => "item.used",
        }
    }
}
