//! 项目级 `Result` 别名。

/// 领域层 / 服务层统一返回类型。
pub type Result<T, E = crate::error::AppError> = std::result::Result<T, E>;
