//! 三层错误
//!
//! ```text
//! DomainError   领域不变量被破坏（"道具不足" / "等级不够"）
//!     ↓ From
//! ServiceError  服务级失败（DB 超时 / Redis 断连 / RPC 失败）
//!     ↓ From
//! AppError      进程级，能映射成协议错误码或 HTTP 状态
//! ```
//!
//! 分层的目的：领域层不认识 `sqlx::Error`，数据层不认识"道具不足"。

use thiserror::Error;

/// 错误大类，决定回包的错误码段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// 客户端参数错误（原服务端返回 `ret != 0`）。
    BadRequest,
    /// 未登录 / 会话失效。
    Unauthorized,
    /// 业务规则拒绝（道具不足、CD 中…）。
    Rejected,
    /// 服务端内部错误（DB / RPC / panic 兜底）。
    Internal,
    /// 限流 / 过载。
    Unavailable,
}

/// 领域层错误：业务规则。
#[derive(Debug, Error)]
pub enum DomainError {
    /// 前置资源不足。
    #[error("资源不足: {0}")]
    Insufficient(&'static str),
    /// 状态不允许该操作。
    #[error("状态非法: {0}")]
    IllegalState(&'static str),
    /// 模板不存在。
    #[error("模板不存在: {0}")]
    TemplateMissing(i32),
    /// 触发上限。
    #[error("超出上限: {0}")]
    LimitExceeded(&'static str),
}

/// 服务层错误：基础设施。
#[derive(Debug, Error)]
pub enum ServiceError {
    /// 数据访问失败。
    #[error("数据访问失败: {0}")]
    Db(#[from] sqlx_error::SqlxError),
    /// 缓存访问失败。
    #[error("缓存访问失败: {0}")]
    Cache(String),
    /// 跨服调用失败。
    #[error("跨服调用失败: {0}")]
    Rpc(String),
    /// 超时。
    #[error("操作超时: {0}")]
    Timeout(&'static str),
}

/// 应用层错误。
#[derive(Debug, Error)]
pub enum AppError {
    /// 领域错误。
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// 服务错误。
    #[error(transparent)]
    Service(#[from] ServiceError),
    /// 参数校验失败。
    #[error("参数错误: {0}")]
    BadRequest(String),
    /// 未认证。
    #[error("未认证")]
    Unauthorized,
    /// 限流。
    #[error("请求过频")]
    RateLimited,
}

impl AppError {
    /// 映射到协议层的错误码（对齐原 `ret` 字段约定：0 = 成功）。
    #[must_use]
    pub fn code(&self) -> i32 {
        match self {
            Self::Domain(DomainError::Insufficient(_) | DomainError::LimitExceeded(_)) => 1001,
            Self::Domain(DomainError::IllegalState(_)) => 1002,
            Self::Domain(DomainError::TemplateMissing(_)) => 1003,
            Self::BadRequest(_) => 400,
            Self::Unauthorized => 401,
            Self::RateLimited => 429,
            Self::Service(_) => 500,
        }
    }

    /// 映射到大类。
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Domain(_) => ErrorKind::Rejected,
            Self::BadRequest(_) => ErrorKind::BadRequest,
            Self::Unauthorized => ErrorKind::Unauthorized,
            Self::RateLimited => ErrorKind::Unavailable,
            Self::Service(_) => ErrorKind::Internal,
        }
    }
}

/// 占位：真实实现里换成 `sqlx::Error`。
/// 这里避免 `ddj-core` 反向依赖 sqlx（地基不碰具体驱动）。
pub mod sqlx_error {
    /// 数据驱动错误的最小投影。
    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    pub struct SqlxError(pub String);
}
