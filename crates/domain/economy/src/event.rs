//! 经济事件

use crate::model::PayChannel;
use ddj_core::PlayerId;

/// 经济事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EconomyEvent {
    /// 下单。
    OrderCreated {
        /// 订单号。
        order: String,
        /// 玩家。
        player: PlayerId,
        /// 金额（分）。
        amount_cent: i64,
    },
    /// 支付成功。
    Paid {
        /// 订单号。
        order: String,
        /// 渠道。
        channel: PayChannel,
    },
    /// 发货完成。
    Delivered {
        /// 订单号。
        order: String,
        /// 玩家。
        player: PlayerId,
    },
    /// 资源变动。
    ResourceChanged {
        /// 玩家。
        player: PlayerId,
        /// 类型。
        kind: i32,
        /// 变化。
        delta: i64,
    },
}
