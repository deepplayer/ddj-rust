//! 分布式追踪
//!
//! 跨服 RPC 传播 W3C `traceparent`。同一个玩家请求在 6 个服务间的链路可串起来。

use std::fmt;

/// 追踪上下文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceCtx {
    /// 128 bit trace id。
    pub trace_id: u128,
    /// 64 bit span id。
    pub span_id: u64,
}

impl TraceCtx {
    /// 生成新的根上下文。
    #[must_use]
    pub fn new_root() -> Self {
        let mut buf = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut buf);
        let trace_id = u128::from_be_bytes(buf);
        let span_id = (trace_id as u64) ^ 0xA5A5_A5A5_A5A5_A5A5;
        Self { trace_id, span_id }
    }

    /// 编码为 W3C `traceparent` 头值。
    #[must_use]
    pub fn to_header(self) -> String {
        format!("00-{:032x}-{:016x}-01", self.trace_id, self.span_id)
    }
}

impl fmt::Display for TraceCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.trace_id)
    }
}
