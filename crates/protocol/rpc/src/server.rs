//! RPC 服务端
//!
//! 监听跨服端口，收帧 → 按 `cmdId` 分发 → 回帧。
//! 原 Java 用 Hessian Overlay 方式把 `CrossService` 暴露成对象；
//! 这里显式注册路由，未注册的 `cmdId` 直接返回错误。

use crate::service::RpcResult;
use crate::wire::{self, RpcEnvelope, ServiceTag};
use dashmap::DashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// 处理函数类型。
pub type RpcHandler = Arc<
    dyn Fn(
            RpcEnvelope,
            Vec<u8>,
        ) -> std::pin::Pin<Box<dyn Future<Output = RpcResult<Vec<u8>>> + Send>>
        + Send
        + Sync,
>;

/// 跨服服务端。
#[derive(Clone)]
pub struct RpcServer {
    tag: ServiceTag,
    routes: Arc<DashMap<i32, RpcHandler>>,
}

impl std::fmt::Debug for RpcServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcServer")
            .field("tag", &self.tag)
            .field("routes", &self.routes.len())
            .finish()
    }
}

impl RpcServer {
    /// 新建。
    #[must_use]
    pub fn new(tag: ServiceTag) -> Self {
        Self {
            tag,
            routes: Arc::new(DashMap::new()),
        }
    }

    /// 注册一条消息处理。
    pub fn route<F, Fut>(&self, cmd_id: i32, f: F)
    where
        F: Fn(RpcEnvelope, Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = RpcResult<Vec<u8>>> + Send + 'static,
    {
        self.routes
            .insert(cmd_id, Arc::new(move |env, body| Box::pin(f(env, body))));
    }

    /// 已注册消息数。
    #[must_use]
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// 监听并服务。
    ///
    /// # Errors
    /// 端口占用 / IO 错误。
    pub async fn serve(
        &self,
        addr: SocketAddr,
        mut shutdown: watch::Receiver<bool>,
    ) -> std::io::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!(%addr, tag = ?self.tag, routes = self.route_count(), "跨服端口就绪");

        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("跨服服务收到停止信号");
                        return Ok(());
                    }
                }
                accepted = listener.accept() => {
                    let (stream, peer) = accepted?;
                    let srv = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = srv.handle_conn(stream).await {
                            tracing::debug!(%peer, error = %e, "跨服连接结束");
                        }
                    });
                }
            }
        }
    }

    async fn handle_conn(&self, mut stream: TcpStream) -> std::io::Result<()> {
        loop {
            let mut head = [0u8; ddj_codec::HEADER_LEN];
            if stream.read_exact(&mut head).await.is_err() {
                return Ok(());
            }
            let total = i32::from_be_bytes([head[2], head[3], head[4], head[5]]);
            let body_len =
                usize::try_from(total.saturating_sub(ddj_codec::HEADER_LEN as i32)).unwrap_or(0);
            let mut body = vec![0u8; body_len];
            stream.read_exact(&mut body).await?;

            let frame = ddj_codec::Frame {
                cmd_id: i32::from_be_bytes([head[6], head[7], head[8], head[9]]),
                body: bytes::Bytes::from(body),
            };
            let Ok((env, payload)) = wire::decode(&frame) else {
                return Ok(());
            };

            let Some(h) = self.routes.get(&env.cmd_id).map(|v| Arc::clone(v.value())) else {
                tracing::warn!(cmd = env.cmd_id, "未注册的跨服消息");
                continue;
            };
            let payload = payload.to_vec();
            let out = h(env, payload).await.unwrap_or_default();
            let resp = wire::encode(
                RpcEnvelope {
                    from: self.tag,
                    to: ServiceTag::Game,
                    cmd_id: frame.cmd_id,
                },
                &out,
            );
            stream.write_all(&resp).await?;
        }
    }
}
