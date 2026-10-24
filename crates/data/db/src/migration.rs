//! 内嵌迁移
//!
//! 原系统靠 `platform.sql`（277 表）+ 一堆散装 `.sql` 手工导入。
//! 这里用 sqlx 的内嵌迁移，启动时自动补齐 schema 版本。
//!
//! 注意：`sqlx::migrate!` 的目录参数必须是**字面量**（宏的匹配规则是
//! `$dir:literal`），所以 `MIGRATION_DIR` 只能作为文档与 xtask 的约定，
//! 真正展开时必须写字符串字面量。

/// 迁移目录（相对 crate 根），与下方 `migrate!("./migrations")` 保持一致。
pub const MIGRATION_DIR: &str = "./migrations";

/// 执行迁移。
///
/// # Errors
/// 目录缺失或 SQL 执行失败。
pub async fn run(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
