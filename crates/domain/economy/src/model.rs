//! 经济聚合
//!
//! 对应 Java：`bf.game.server.domain.pay.*`（充值订单 / 首充 / 月卡）+ 消耗流水。

use ddj_core::PlayerId;

/// 支付渠道。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PayChannel {
    /// 官方直充。
    Official = 1,
    /// 渠道 SDK（原 `i.php` 对应的聚合 SDK）。
    Sdk = 2,
    /// 后台补单。
    Manual = 3,
}

/// 订单状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderState {
    /// 已创建未支付。
    Created,
    /// 已支付待发货。
    Paid,
    /// 已发货。
    Delivered,
    /// 已退款。
    Refunded,
    /// 已关闭。
    Closed,
}

/// 一笔充值订单。
#[derive(Debug, Clone)]
pub struct Order {
    /// 订单号（业务主键）。
    pub id: String,
    /// 玩家。
    pub player: PlayerId,
    /// 商品 ID（`t_shop` 行）。
    pub product_id: i32,
    /// 金额（分）。
    pub amount_cent: i64,
    /// 渠道。
    pub channel: PayChannel,
    /// 状态。
    pub state: OrderState,
    /// 创建时间（毫秒）。
    pub created_ms: i64,
}

impl Order {
    /// 状态迁移是否合法。
    ///
    /// 复刻原 PHP 的关键约束：**已发货不可回退**，防止重复发货刷元宝。
    #[must_use]
    pub const fn can_transit(&self, next: OrderState) -> bool {
        matches!(
            (self.state, next),
            (OrderState::Created, OrderState::Paid | OrderState::Closed)
                | (
                    OrderState::Paid,
                    OrderState::Delivered | OrderState::Refunded
                )
                | (OrderState::Delivered, OrderState::Refunded)
        )
    }
}

/// 一条资源流水。
#[derive(Debug, Clone)]
pub struct Flow {
    /// 玩家。
    pub player: PlayerId,
    /// 资源类型（`TypeModule` 编号，如 501 铜钱 / 11002 道具）。
    pub kind: i32,
    /// 变化量。
    pub delta: i64,
    /// 变化后。
    pub after: i64,
    /// 来源描述（"使用道具" / "任务奖励"…）。
    pub reason: &'static str,
    /// 时间。
    pub at_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivered_cannot_go_back_to_paid() {
        let o = Order {
            id: "X1".into(),
            player: PlayerId::new(1),
            product_id: 1,
            amount_cent: 600,
            channel: PayChannel::Sdk,
            state: OrderState::Delivered,
            created_ms: 0,
        };
        assert!(!o.can_transit(OrderState::Paid));
        assert!(!o.can_transit(OrderState::Created));
        assert!(o.can_transit(OrderState::Refunded));
    }
}
