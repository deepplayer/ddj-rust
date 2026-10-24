//! 连接池
//!
//! 对应 Java：`mysql-connector-java` + `druid` + `proxool`（三个池共存，配置混乱）。
//! 这里统一为 sqlx 的 `PgPool`。
//!
//! ## 为什么是 PostgreSQL
//! 原系统用 MySQL，但业务里大量依赖「行级锁 + 事务内读一致性」的场景
//! （扣道具、结算、跨服快照），PostgreSQL 的 MVCC 与 `ON CONFLICT` 语义更干净，
//! 且不受 MySQL 那种「DDL 隐式提交」的坑。迁移代价集中在一处：
//! 占位符 `?` → `$n`、`ON DUPLICATE KEY` → `ON CONFLICT`、`LAST_INSERT_ID()` → `RETURNING`。

use ddj_config::schema::DbCfg;
use std::time::Duration;

/// 各库连接池。
#[derive(Clone)]
pub struct DbPools {
    /// 账号库（原 `platform`，277 表）。
    pub platform: sqlx::PgPool,
    /// 本服游戏库（原 `game_7010544`，173 表）。
    pub game: sqlx::PgPool,
}

impl DbPools {
    /// 按配置建立两个池。
    ///
    /// # Errors
    /// 连接串非法或数据库不可达。
    pub async fn connect(cfg: &DbCfg) -> Result<Self, sqlx::Error> {
        let platform = sqlx::postgres::PgPoolOptions::new()
            .max_connections(cfg.max_connections)
            .acquire_timeout(Duration::from_secs(cfg.acquire_timeout_secs))
            .connect(&cfg.platform_url)
            .await?;

        let game = sqlx::postgres::PgPoolOptions::new()
            .max_connections(cfg.max_connections)
            .acquire_timeout(Duration::from_secs(cfg.acquire_timeout_secs))
            .connect(&cfg.game_url)
            .await?;

        Ok(Self { platform, game })
    }

    /// 健康检查。
    ///
    /// # Errors
    /// 任一库不可用。
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT 1").execute(&self.platform).await?;
        sqlx::query("SELECT 1").execute(&self.game).await?;
        Ok(())
    }
}

impl std::fmt::Debug for DbPools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbPools")
            .field("platform_size", &self.platform.size())
            .field("game_size", &self.game.size())
            .finish()
    }
}
