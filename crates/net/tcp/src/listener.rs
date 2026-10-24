//! TCP 监听
//!
//! 对应 Java：`netty-all-4.1.17` 的 `ServerBootstrap` + `ChannelGroup`。

use ddj_codec::{Frame, FrameDecoder};
use ddj_config::schema::NetCfg;
use ddj_session::SessionManager;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::codec::Framed;

/// TCP 服务器。
pub struct TcpServer {
    cfg: NetCfg,
    sessions: Arc<SessionManager>,
}

impl TcpServer {
    /// 构造。
    #[must_use]
    pub fn new(cfg: NetCfg, sessions: Arc<SessionManager>) -> Self {
        Self { cfg, sessions }
    }

    /// 开始 accept，直到 `shutdown` 变为 true。
    ///
    /// # Errors
    /// 端口占用 / 权限不足。
    pub async fn serve(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.cfg.tcp_listen).await?;
        tracing::info!(addr = %self.cfg.tcp_listen, "TCP 监听已启动");

        loop {
            tokio::select! {
                r = listener.accept() => {
                    let (stream, peer) = match r {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(error = %e, "accept 失败");
                            continue;
                        }
                    };

                    if self.sessions.online() >= self.cfg.max_conn_total {
                        tracing::warn!(peer = %peer, "连接数已达上限，拒绝");
                        continue;
                    }

                    let framed = Framed::new(stream, FrameDecoder::new());
                    let _ = framed; // 真实实现：交给 conn::spawn
                    tracing::debug!(peer = %peer, "新连接");
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("收到停机信号，停止 accept");
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// 帧类型别名。
pub type ClientFramed = Framed<tokio::net::TcpStream, FrameDecoder>;
/// 帧别名。
pub type ClientFrame = Frame;
