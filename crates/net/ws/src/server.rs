//! WebSocket 服务端
//!
//! 对应原版 `WebSocketServerPipelineInitializer`：
//! ```text
//! HttpServerCodec → HttpObjectAggregator(65536) → WebSocketServerProtocolHandler("/", null, true)
//!   → WebSocketMsgDecoder → IdleStateHandler(3×idle) → WebSocketHandler → WebSocketMsgEncoder
//! ```
//! 关键点：
//! - 路径检查是 `checkStartsWith = true`，所以任意以 `/` 开头的路径都会升级，这里用 fallback 路由复刻。
//! - `AllowedIdelTime` 同时设给 reader/writer/all，**任一方向**安静超时就触发 `connectIdle`。
//! - 协议消息只走 `BinaryWebSocketFrame`；文本帧不参与业务。

use crate::codec::MessageCodec;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use ddj_codec::Frame;
use ddj_config::schema::NetCfg;
use ddj_core::ServerId;
use ddj_registry::{GuardChain, HandlerCtx, HandlerTable};
use ddj_session::SessionManager;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

/// 每条连接出站队列容量。
const OUTBOUND_CAPACITY: usize = 64;

/// WebSocket 服务器。
#[derive(Clone)]
pub struct WsServer {
    inner: Arc<Inner>,
}

struct Inner {
    cfg: NetCfg,
    sessions: Arc<SessionManager>,
    handlers: Arc<HandlerTable>,
    guards: Arc<GuardChain>,
    server_id: i32,
}

impl WsServer {
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
            inner: Arc::new(Inner {
                cfg,
                sessions,
                handlers,
                guards,
                server_id,
            }),
        }
    }

    /// 监听地址（未配置 `ws_listen` 时为 `None`）。
    #[must_use]
    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.inner.cfg.ws_listen
    }

    /// 开始服务，直到 `shutdown` 变 true。
    ///
    /// # Errors
    /// 端口占用 / 权限不足。
    pub async fn serve(&self, mut shutdown: watch::Receiver<bool>) -> std::io::Result<()> {
        let Some(addr) = self.inner.cfg.ws_listen else {
            tracing::warn!("未配置 net.ws_listen，WebSocket 监听跳过");
            return Ok(());
        };
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(addr = %addr, "WebSocket 监听已启动（对齐原版 10544）");

        let app = Router::new()
            .route("/", get(upgrade))
            .fallback(get(upgrade))
            .with_state(self.clone());

        let stop = CancellationToken::new();
        let stop2 = stop.clone();
        tokio::spawn(async move {
            loop {
                if shutdown.changed().await.is_err() {
                    break;
                }
                if *shutdown.borrow() {
                    break;
                }
            }
            stop2.cancel();
        });

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move { stop.cancelled().await })
        .await
    }
}

/// 升级入口。原版 `checkStartsWith=true` → 任意路径都升级。
async fn upgrade(
    ws: WebSocketUpgrade,
    State(srv): State<WsServer>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| conn(socket, peer, srv))
}

/// 一条连接的生命周期。
async fn conn(mut socket: WebSocket, peer: SocketAddr, srv: WsServer) {
    let inner = &srv.inner;
    if inner.sessions.online() >= inner.cfg.max_conn_total {
        tracing::warn!(peer = %peer, "连接数已达上限，拒绝");
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    let (tx, mut outbound) = mpsc::channel::<Frame>(OUTBOUND_CAPACITY);
    let session = inner
        .sessions
        .register(peer, ServerId::new(inner.server_id), tx);
    let sid = session.id;
    tracing::debug!(peer = %peer, session = sid, "WebSocket 新连接");

    let mut ctx = HandlerCtx {
        session_id: sid,
        player_id: session.player(),
        peer_ip: peer.ip(),
        cmd_id: 0,
        session: Some(Arc::new(ConnSession(Arc::clone(&session)))),
    };
    let idle = std::time::Duration::from_secs(inner.cfg.idle_timeout_secs.max(1));
    let mut codec = MessageCodec::new();

    loop {
        let msg = tokio::select! {
            r = tokio::time::timeout(idle, socket.recv()) => match r {
                Err(_elapsed) => {
                    tracing::info!(peer = %peer, session = sid, "空闲超时，断开（对齐 IdleStateHandler）");
                    break;
                }
                Ok(None) => break,
                Ok(Some(Err(e))) => {
                    tracing::debug!(peer = %peer, error = %e, "WebSocket 读失败");
                    break;
                }
                Ok(Some(Ok(m))) => m,
            },
            Some(out) = outbound.recv() => {
                if send_frame(&mut socket, &out).await.is_err() {
                    break;
                }
                continue;
            }
        };

        match msg {
            Message::Binary(data) => {
                let mut frames = Vec::new();
                if let Err(e) = codec.feed(&data, &mut frames) {
                    tracing::debug!(peer = %peer, error = %e, "帧解码失败，断连");
                    break;
                }
                for frame in frames {
                    ctx.cmd_id = frame.cmd_id;
                    match ddj_tcp::pipeline::dispatch(&inner.handlers, &inner.guards, &ctx, frame)
                        .await
                    {
                        Ok(Some(reply)) => {
                            let resp = Frame::new(reply.cmd.unwrap_or(ctx.cmd_id), reply.body);
                            if send_frame(&mut socket, &resp).await.is_err() {
                                return;
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            tracing::warn!(
                                peer = %peer, cmd_id = ctx.cmd_id, error = %e,
                                "处理器返回错误"
                            );
                        }
                    }
                    // 登录成功后 `SessionControl::bind_player` 已写会话，这里每帧同步。
                    ctx.player_id = session.player();
                }
            }
            Message::Ping(p) => {
                if socket.send(Message::Pong(p)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            // 原版不处理文本帧（业务处理器只接 `IoMessage`），这里保持一致：忽略。
            Message::Text(_) | Message::Pong(_) => {}
        }
    }

    tracing::debug!(peer = %peer, session = sid, "WebSocket 连接结束");
}

/// 把协议帧编码成二进制并写回。
async fn send_frame(socket: &mut WebSocket, frame: &Frame) -> Result<(), ()> {
    let mut buf = bytes::BytesMut::new();
    frame.encode(&mut buf);
    socket
        .send(Message::Binary(buf.to_vec()))
        .await
        .map_err(|_| ())
}

/// 会话桥接（与 TCP 版同一实现）。
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
        if let Err(e) = self.0.try_push(ddj_codec::Frame::new(cmd, body)) {
            tracing::debug!(error = %e, cmd, "推送失败");
        }
    }
}
