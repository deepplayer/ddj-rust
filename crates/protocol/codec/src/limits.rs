//! 协议侧硬限制
//!
//! 全部来自原服务端 `NettyServerHandler` 的实际代码，不是猜的。

/// 单帧最大字节数。
pub const MAX_FRAME_BYTES: usize = crate::MAX_BODY;
/// 单连接积压消息上限（超出直接 `Channel.close()`）。
pub const MAX_PENDING_MESSAGES: usize = crate::MAX_QUEUE;

/// 一条连接的用量计数。
#[derive(Debug, Default)]
pub struct Quota {
    pending: usize,
}

impl Quota {
    /// 记账一条消息，返回是否超限。
    pub fn on_message(&mut self) -> bool {
        self.pending += 1;
        self.pending > MAX_PENDING_MESSAGES
    }
    /// 处理完一条，归还额度。
    pub fn on_done(&mut self) {
        self.pending = self.pending.saturating_sub(1);
    }
}
