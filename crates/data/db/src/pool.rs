//! 连接池
//!
//! 对应 Java：`mysql-connector-java` + `druid` + `proxool`（三个池共存，配置混乱）。
//! 这里统一为 sqlx 的 `MySqlPool`。

use ddj_config::schema::DbCfg;
use std::time::Duration;

/// 各库连接池。
#[derive(Clone)]
pub struct DbPools {
    /// 账号库（原 `platform`，277 表）。
    pub platform: sqlx::MySqlPool,
    /// 本服游戏库（原 `game_7010544`，173 表）。
    pub game: sqlx::MySqlPool,
}

impl DbPools {
    /// 按配置建立两个池。
    ///
    /// # Errors
    /// 连接串非法或数据库不可达。
    pub async fn connect(cfg: &DbCfg) -> Result<Self, sqlx::Error> {
        let platform = sqlx::mysql::MySqlPoolOptions::new()
            .max_connections(cfg.max_connections)
            .acquire_timeout(Duration::from_secs(cfg.acquire_timeout_secs))
            .connect(&cfg.platform_url)
            .await?;

        let game = sqlx::mysql::MySqlPoolOptions::new()
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
