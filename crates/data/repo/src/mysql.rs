//! sqlx 实现

use async_trait::async_trait;
use ddj_core::{PlayerId, Result};
use ddj_db::pool::DbPools;

use crate::entity::{ItemRow, PlayerRow};
use crate::traits::{ItemRepo, PlayerRepo};

/// MySQL 角色仓储。
pub struct MyPlayerRepo {
    pool: DbPools,
}

impl MyPlayerRepo {
    /// 构造。
    #[must_use]
    pub fn new(pool: DbPools) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PlayerRepo for MyPlayerRepo {
    async fn find(&self, id: PlayerId) -> Result<Option<PlayerRow>> {
        let row = sqlx::query_as::<_, PlayerRow>(
            "SELECT player_id, user_id, name, level, exp, vip, last_login \
             FROM player WHERE player_id = ?",
        )
        .bind(id.get())
        .fetch_optional(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("查角色: {e}")))?;
        Ok(row)
    }

    async fn save(&self, row: &PlayerRow) -> Result<()> {
        sqlx::query(
            "UPDATE player SET name = ?, level = ?, exp = ?, vip = ?, last_login = ? \
             WHERE player_id = ?",
        )
        .bind(&row.name)
        .bind(row.level)
        .bind(row.exp)
        .bind(row.vip)
        .bind(row.last_login)
        .bind(row.player_id)
        .execute(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("存角色: {e}")))?;
        Ok(())
    }
}

/// MySQL 道具仓储。
pub struct MyItemRepo {
    pool: DbPools,
}

impl MyItemRepo {
    /// 构造。
    #[must_use]
    pub fn new(pool: DbPools) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ItemRepo for MyItemRepo {
    async fn count(&self, player: PlayerId, template_id: i32) -> Result<i64> {
        let n: Option<i64> =
            sqlx::query_scalar("SELECT num FROM item WHERE player_id = ? AND template_id = ?")
                .bind(player.get())
                .bind(template_id)
                .fetch_optional(&self.pool.game)
                .await
                .map_err(|e| ddj_core::AppError::BadRequest(format!("查道具: {e}")))?;
        Ok(n.unwrap_or(0))
    }

    async fn add(&self, player: PlayerId, template_id: i32, delta: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO item (player_id, template_id, num) VALUES (?, ?, ?) \
             ON DUPLICATE KEY UPDATE num = GREATEST(0, num + VALUES(num))",
        )
        .bind(player.get())
        .bind(template_id)
        .bind(delta)
        .execute(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("改道具: {e}")))?;
        Ok(())
    }

    async fn list(&self, player: PlayerId) -> Result<Vec<ItemRow>> {
        sqlx::query_as::<_, ItemRow>(
            "SELECT id, player_id, template_id, num FROM item WHERE player_id = ?",
        )
        .bind(player.get())
        .fetch_all(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("列道具: {e}")))
    }
}
