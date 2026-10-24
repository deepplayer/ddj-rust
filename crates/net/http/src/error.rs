//! 错误 → HTTP 响应
//!
//! 原 PHP 版直接 `echo json_encode(["status"=>500,...])`，HTTP 状态码永远是 200，
//! 客户端只能靠 body 里的 `status` 判断。这里保留兼容（`code` 字段），
//! 同时给出语义正确的 HTTP 状态码。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// 统一错误码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 参数不合法。
    BadRequest,
    /// 未登录 / 会话失效。
    Unauthorized,
    /// 无权限。
    Forbidden,
    /// 资源不存在。
    NotFound,
    /// 冲突（重复登录 / 重复领取）。
    Conflict,
    /// 请求过于频繁。
    TooManyRequests,
    /// 内部错误。
    Internal,
}

impl ErrorCode {
    /// 对应 HTTP 状态码。
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 兼容原 PHP 的 `status` 字段取值。
    #[must_use]
    pub const fn legacy_status(self) -> i32 {
        match self {
            Self::BadRequest => 400,
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::Conflict => 409,
            Self::TooManyRequests => 429,
            Self::Internal => 500,
        }
    }
}

/// HTTP 层错误。
#[derive(Debug)]
pub struct HttpError {
    /// 错误码。
    pub code: ErrorCode,
    /// 对外消息（不泄露内部细节）。
    pub message: String,
}

impl HttpError {
    /// 构造。
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 400。
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, msg)
    }

    /// 401。
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthorized, msg)
    }

    /// 404。
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, msg)
    }

    /// 500。
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, msg)
    }
}

/// 响应体。
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// 兼容原 PHP 的 `status`。
    pub status: i32,
    /// 新的机器可读码。
    pub code: ErrorCode,
    /// 人类可读消息。
    pub message: String,
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        if self.code == ErrorCode::Internal {
            tracing::error!(message = %self.message, "HTTP 500");
        }
        let body = ErrorBody {
            status: self.code.legacy_status(),
            code: self.code,
            message: self.message,
        };
        (self.code.status(), Json(body)).into_response()
    }
}

impl From<sqlx::Error> for HttpError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = %e, "数据库错误");
        Self::internal("数据库错误")
    }
}

impl From<ddj_core::AppError> for HttpError {
    fn from(e: ddj_core::AppError) -> Self {
        use ddj_core::AppError;
        match e {
            AppError::BadRequest(m) => Self::bad_request(m),
            AppError::Unauthorized => Self::unauthorized("未登录"),
            AppError::RateLimited => Self::new(ErrorCode::TooManyRequests, "请求过于频繁"),
            AppError::Domain(d) => Self::new(ErrorCode::Conflict, d.to_string()),
            AppError::Service(s) => {
                tracing::error!(error = %s, "服务层错误");
                Self::internal("服务暂时不可用")
            }
        }
    }
}
