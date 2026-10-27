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
                    let sess = Arc::clone(&session);
                    tokio::spawn(async move {
                        serve_conn(stream, peer, sid, handlers, guards, sess, rx).await;
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

/// 把 `Session` 桥接成协议层能用的最小接口。
struct ConnSession(Arc<ddj_session::Session>);

impl ddj_registry::SessionControl for ConnSession {
    fn bind_player(&self, player: ddj_core::PlayerId) {
        if let Err(e) = self.0.authenticate(player) {
            tracing::warn!(error = e, "绑定角色失败");
            return;
        }
        if let Err(e) = self.0.enter_game() {
            tracing::warn!(error = e, "进入游戏状态失败");
        }
        tracing::info!(player = player.get(), "会话已绑定角色");
    }

    fn push(&self, cmd: i32, body: bytes::Bytes) {
        // 原版 `MessageUtil.sendToPlayerId` 是同步发；这里同样不等下游。
        if let Err(e) = self.0.try_push(Frame::new(cmd, body)) {
            tracing::debug!(error = %e, cmd, "推送失败");
        }
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
    session: Arc<ddj_session::Session>,
    mut outbound: mpsc::Receiver<Frame>,
) {
    use futures::{SinkExt as _, StreamExt as _};

    let mut framed = Framed::new(stream, ClientCodec::new());
    let mut ctx = HandlerCtx {
        session_id,
        player_id: session.player(),
        peer_ip: peer.ip(),
        cmd_id: 0,
        session: Some(Arc::new(ConnSession(Arc::clone(&session)))),
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
            Ok(Some(reply)) => {
                // ⚠ 先把**已入队**的推送帧全部写出，再写回包。
                //
                // 原版是「handler 里先 `sendToPlayerId(推送)`，再 `return 回包`」，
                // 两者走的是同一个 socket 写通道，所以顺序天然是「推送在前」。
                // 这里的推送走 `mpsc`、回包走 `framed.send`，两条通道各自独立，
                // 若直接写回包，`select!` 可能先处理回包分支 → 客户端看到
                // 「先回包、后推送」，与原版相反。
                //
                // 典型例子：17071 焚香祈愿先推 `MsgPrayDataResp_17074`、
                // 再回 `PrayResp_17071`；16004 签到先推 16001 再回 16004。
                while let Ok(pushed) = outbound.try_recv() {
                    if let Err(e) = framed.send(pushed).await {
                        tracing::debug!(peer = %peer, error = %e, "推送写回失败，断连");
                        return;
                    }
                }
                // `Reply::cmd` 为 `Some` 时换号组帧（`ErrorResp_1` 走这条）。
                let resp = Frame::new(reply.cmd.unwrap_or(ctx.cmd_id), reply.body);
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
        // 登录成功会通过 `SessionControl::bind_player` 把角色写进会话，
        // 这里每帧同步一次，后续需要登录的消息（如握手回执 16/17/18）才放行。
        ctx.player_id = session.player();
    }
}
