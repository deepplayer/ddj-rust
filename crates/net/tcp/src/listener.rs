//! TCP 监听
//!
//! 对应 Java：`netty-all-4.1.17` 的 `ServerBootstrap` + `ChannelGroup`。

use ddj_codec::{Frame, FrameDecoder, FrameEncoder, FrameError};
use ddj_config::schema::NetCfg;
use ddj_session::SessionManager;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_util::codec::Framed;

use ddj_core::ServerId;
use ddj_registry::{GuardChain, HandlerCtx, HandlerTable};

/// 每条连接出站队列容量（对齐 Netty 默认写水位 64 KB / 消息数近似）。
const OUTBOUND_CAPACITY: usize = 64;

/// TCP 服务器。
pub struct TcpServer {
    cfg: NetCfg,
    sessions: Arc<SessionManager>,
    handlers: Arc<HandlerTable>,
    guards: Arc<GuardChain>,
    server_id: i32,
}

impl TcpServer {
    /// 构造。
    #[must_use]
    pub fn new(
        cfg: NetCfg,
        sessions: Arc<SessionManager>,
        handlers: Arc<HandlerTable>,
        guards: Arc<GuardChain>,
        server_id: i32,
    ) -> Self {
        Self {
            cfg,
            sessions,
            handlers,
            guards,
            server_id,
        }
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

                    // 出站队列：服务端主动推送（`ItemUpdateResp_11004` 等）先入队，
                    // 由连接任务写回。容量对齐原来的 Netty `WriteBufferWaterMark`。
                    let (tx, rx) = mpsc::channel::<Frame>(OUTBOUND_CAPACITY);
                    let session = self
                        .sessions
                        .register(peer, ServerId::new(self.server_id), tx);
                    let sid = session.id;
                    tracing::debug!(peer = %peer, session = sid, "新连接");

                    let handlers = Arc::clone(&self.handlers);
                    let guards = Arc::clone(&self.guards);
                    tokio::spawn(async move {
                        serve_conn(stream, peer, sid, handlers, guards, rx).await;
                    });
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

/// 客户端编解码器：把 `FrameDecoder` 与 `FrameEncoder` 合成一个
/// `tokio_util::codec::Framed` 要求的类型（`Framed` 的编解码器必须同时实现两者）。
pub struct ClientCodec {
    dec: FrameDecoder,
    enc: FrameEncoder,
}

impl ClientCodec {
    /// 默认上限。
    #[must_use]
    pub fn new() -> Self {
        Self {
            dec: FrameDecoder::new(),
            enc: FrameEncoder,
        }
    }
}

impl Default for ClientCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl tokio_util::codec::Decoder for ClientCodec {
    type Item = Frame;
    type Error = FrameError;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.dec.decode(src)
    }
}

impl tokio_util::codec::Encoder<Frame> for ClientCodec {
    type Error = FrameError;

    fn encode(&mut self, item: Frame, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        self.enc.encode(item, dst)
    }
}

/// 一条连接的生命周期：读帧 → 守卫 → 处理器 → 写回。
///
/// ## 回包的消息号
/// 与请求**同号**（`LoginResp_4` 的 cmdId 就是 4），客户端按 cmdId 配请求，
/// 原服务端 `player.sendMessage(resp)` 也是这样做的。
///
/// ## 错误策略（与原版对齐）
/// - 帧解码错误 → 断连（协议已不同步，继续读只会读到垃圾）。
/// - 守卫拒绝 `Close` → 断连。
/// - 处理器返回错误 → 只记日志、**不断连**；原版 `catch(Exception)` 之后仍然可以继续收包。
async fn serve_conn(
    stream: tokio::net::TcpStream,
    peer: std::net::SocketAddr,
    session_id: u64,
    handlers: Arc<HandlerTable>,
    guards: Arc<GuardChain>,
    mut outbound: mpsc::Receiver<Frame>,
) {
    use futures::{SinkExt as _, StreamExt as _};

    let mut framed = Framed::new(stream, ClientCodec::new());
    let mut ctx = HandlerCtx {
        session_id,
        player_id: None,
        peer_ip: peer.ip(),
        cmd_id: 0,
    };

    loop {
        // 入站优先；出站队列非空时也要能写回。
        let next = framed.next();
        tokio::pin!(next);
        let frame = tokio::select! {
            f = &mut next => match f {
                Some(Ok(f)) => f,
                Some(Err(e)) => {
                    tracing::debug!(peer = %peer, error = %e, "帧解码失败，断连");
                    return;
                }
                None => return,
            },
            Some(out) = outbound.recv() => {
                let _ = framed.send(out).await;
                continue;
            }
        };

        ctx.cmd_id = frame.cmd_id;
        match crate::pipeline::dispatch(&handlers, &guards, &ctx, frame).await {
            Ok(Some(body)) => {
                let resp = Frame::new(ctx.cmd_id, body);
                if let Err(e) = framed.send(resp).await {
                    tracing::debug!(peer = %peer, error = %e, "写回失败，断连");
                    return;
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(peer = %peer, cmd_id = ctx.cmd_id, error = %e, "处理器返回错误");
            }
        }
    }
}
