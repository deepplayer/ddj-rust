//! RPC 客户端
//!
//! 连接池 + 超时 + 重连。原 Java 用 Hessian 的长连接，断线后整个进程要重启；
//! 这里每次调用从池里取连接，失败自动重连。

use crate::service::{RpcError, RpcResult};
use crate::wire::{self, RpcEnvelope, ServiceTag};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

/// 单个目标服务的连接。
#[derive(Debug)]
struct Conn {
    stream: TcpStream,
}

/// RPC 客户端。
#[derive(Debug)]
pub struct RpcClient {
    tag: ServiceTag,
    /// 目标 → 地址。
    targets: Arc<DashMap<ServiceTag, String>>,
    /// 目标 → 连接。
    pool: Arc<DashMap<ServiceTag, Arc<Mutex<Conn>>>>,
    /// 单次调用超时。
    timeout: Duration,
}

impl RpcClient {
    /// 新建。
    #[must_use]
    pub fn new(tag: ServiceTag, timeout: Duration) -> Self {
        Self {
            tag,
            targets: Arc::new(DashMap::new()),
            pool: Arc::new(DashMap::new()),
            timeout,
        }
    }

    /// 注册目标地址，如 `127.0.0.1:4106`。
    pub fn register(&self, tag: ServiceTag, addr: impl Into<String>) {
        self.targets.insert(tag, addr.into());
    }

    /// 取连接（首次或断线后重建）。
    async fn conn(&self, to: ServiceTag) -> RpcResult<Arc<Mutex<Conn>>> {
        if let Some(c) = self.pool.get(&to) {
            return Ok(Arc::clone(c.value()));
        }
        let addr = self.targets.get(&to).map(|v| v.value().clone());
        let addr = addr.ok_or(RpcError::Unreachable(to))?;
        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|_| RpcError::Unreachable(to))?;
        let conn = Arc::new(Mutex::new(Conn { stream }));
        self.pool.insert(to, Arc::clone(&conn));
        Ok(conn)
    }

    /// 发起一次调用。
    ///
    /// # Errors
    /// 见 [`RpcError`]。
    pub async fn call(&self, to: ServiceTag, cmd_id: i32, body: &[u8]) -> RpcResult<Vec<u8>> {
        let env = RpcEnvelope {
            from: self.tag,
            to,
            cmd_id,
        };
        let raw = wire::encode(env, body);
        let conn = self.conn(to).await?;
        // 闭包必须显式返回 `RpcResult<Vec<u8>>`，否则 `?` 无处展开。
        let fut = async {
            let mut guard = conn.lock().await;
            guard
                .stream
                .write_all(&raw)
                .await
                .map_err(|e| RpcError::Codec(e.to_string()))?;
            let total = read_frame_len(&mut guard.stream)
                .await
                .map_err(|e| RpcError::Codec(e.to_string()))?;
            let mut buf = vec![0u8; total];
            guard
                .stream
                .read_exact(&mut buf)
                .await
                .map_err(|e| RpcError::Codec(e.to_string()))?;
            Ok::<Vec<u8>, RpcError>(extra_body(&buf).to_vec())
        };
        #[allow(clippy::cast_possible_truncation)]
        let timeout_ms = self.timeout.as_millis() as u64;
        match tokio::time::timeout(self.timeout, fut).await {
            Ok(Ok(b)) => Ok(b),
            Ok(Err(e)) => {
                self.pool.remove(&to);
                Err(e)
            }
            Err(_) => {
                self.pool.remove(&to);
                Err(RpcError::Timeout(timeout_ms))
            }
        }
    }
}

async fn read_frame_len(s: &mut TcpStream) -> std::io::Result<usize> {
    let mut head = [0u8; ddj_codec::HEADER_LEN];
    s.read_exact(&mut head).await?;
    let len = i32::from_be_bytes([head[2], head[3], head[4], head[5]]);
    // 跨服帧体 = total - 头长；头后再带 2 字节 from/to，读的时候要一起读进来。
    let body = usize::try_from(len.saturating_sub(ddj_codec::HEADER_LEN as i32)).unwrap_or(0);
    Ok(body)
}

fn extra_body(buf: &[u8]) -> &[u8] {
    buf.get(2..).unwrap_or(&[])
}
