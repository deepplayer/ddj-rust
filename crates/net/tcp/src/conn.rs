//! 单连接驱动
//!
//! 原 Netty 一条 `Channel` 的等价物：读循环 + 写队列 + 空闲超时。

use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

/// 一条连接的句柄。
pub struct ConnHandle {
    /// 会话 ID。
    pub session_id: u64,
    /// 对端地址。
    pub peer: std::net::SocketAddr,
    /// 下行发送队列（有界，形成背压）。
    pub tx: mpsc::Sender<bytes::Bytes>,
}

/// 连接配置。
#[derive(Debug, Clone, Copy)]
pub struct ConnCfg {
    /// 空闲超时。
    pub idle: Duration,
    /// 下行队列容量（对应 Netty 的 `WriteBufferWaterMark` 高水位）。
    pub queue_cap: usize,
}

impl Default for ConnCfg {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(120),
            queue_cap: 256,
        }
    }
}

/// 带空闲超时的等待。
///
/// # Errors
/// 超时或通道关闭。
pub async fn recv_with_idle<T>(
    rx: &mut mpsc::Receiver<T>,
    idle: Duration,
) -> Result<T, &'static str> {
    timeout(idle, rx.recv())
        .await
        .map_err(|_| "idle timeout")?
        .ok_or("channel closed")
}
