//! # ddj-http — HTTP 服务
//!
//! 对应 Java：`jetty-runner-7.6`（端口 1544 的 GM 后台）+ `data/web` 的 11 个 PHP。
//!
//! ## 路由分组
//! | 组 | 路径前缀 | 说明 |
//! |---|---|---|
//! | 运维 | `/healthz` `/readyz` `/metrics` | 无鉴权，仅内网 |
//! | GM | `/gm/v1/*` | JWT + 角色 RBAC |
//! | SDK | `/sdk/v1/*` | 兼容原 `i.php` / `l.php` 的加密契约 |
//! | 回调 | `/callback/v1/*` | 充值 / CDK |
//!
//! ## 子模块
//! - [`router`]    路由装配
//! - [`middleware`] 追踪 / 限流 / 鉴权 / 错误映射
//! - [`error`]     `AppError` → HTTP 响应

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod error;
pub mod middleware;
pub mod router;

pub use error::{ErrorCode, HttpError};
pub use middleware::RateLimiter;
pub use router::{build_router, HttpState};
