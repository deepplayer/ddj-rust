//! 中间件
//!
//! 顺序（外 → 内）：`request_id` → `tracing` → `rate_limit` → `auth` → handler

use axum::extract::Request;
use axum::http::HeaderName;
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;
use std::time::Instant;

/// 请求 ID 头名。
pub const REQ_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// 记录耗时与状态。
pub async fn trace_layer(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let method = req.method().clone();
    let id = req
        .headers()
        .get(&REQ_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map_or_else(
            || format!("{:016x}", rand::random::<u64>()),
            ToOwned::to_owned,
        );

    let start = Instant::now();
    let mut resp = next.run(req).await;
    let elapsed = start.elapsed();

    if let Ok(v) = id.parse() {
        resp.headers_mut().insert(&REQ_ID_HEADER, v);
    }
    tracing::info!(
        %method, %path, status = resp.status().as_u16(),
        cost_ms = elapsed.as_millis() as u64, request_id = %id,
        "http"
    );
    resp
}

/// 令牌桶限流（单进程内）。
#[derive(Debug)]
pub struct RateLimiter {
    /// 每个窗口允许的请求数。
    pub limit: u32,
    /// 窗口长度。
    pub window: std::time::Duration,
    buckets: dashmap::DashMap<std::net::IpAddr, (u32, Instant)>,
}

impl RateLimiter {
    /// 新建。
    #[must_use]
    pub fn new(limit: u32, window: std::time::Duration) -> Arc<Self> {
        Arc::new(Self {
            limit,
            window,
            buckets: dashmap::DashMap::new(),
        })
    }

    /// 消费一个令牌；返回 `false` 表示超限。
    pub fn allow(&self, ip: std::net::IpAddr) -> bool {
        let now = Instant::now();
        let mut e = self.buckets.entry(ip).or_insert((0, now));
        if now.duration_since(e.1) > self.window {
            *e = (0, now);
        }
        if e.0 >= self.limit {
            return false;
        }
        e.0 += 1;
        true
    }

    /// 清理过期桶。
    pub fn gc(&self) {
        let window = self.window;
        self.buckets.retain(|_, v| v.1.elapsed() < window);
    }
}

/// 限流中间件。
pub async fn rate_limit(
    axum::extract::State(limiter): axum::extract::State<Arc<RateLimiter>>,
    req: Request,
    next: Next,
) -> Response {
    let ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map_or(std::net::IpAddr::from([127, 0, 0, 1]), |c| c.0.ip());
    if !limiter.allow(ip) {
        return super::error::HttpError::new(
            super::error::ErrorCode::TooManyRequests,
            "请求过于频繁",
        )
        .into_response();
    }
    next.run(req).await
}

use axum::response::IntoResponse;
