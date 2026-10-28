//! sqlx + PostgreSQL 实现
//!
//! 与原 MySQL 实现的差异只有三处语法：
//! - 占位符 `?` → `$1`/`$2`
//! - `ON DUPLICATE KEY UPDATE` → `ON CONFLICT (...) DO UPDATE SET`
//! - `LAST_INSERT_ID()` → `RETURNING id`

use async_trait::async_trait;
use ddj_core::{PlayerId, Result};
use ddj_db::pool::DbPools;

use crate::entity::{ItemRow, PlayerRow};
use crate::traits::{ItemRepo, PlayerRepo};

/// PostgreSQL 角色仓储。
pub struct PgPlayerRepo {
    pool: DbPools,
}

impl PgPlayerRepo {
    /// 构造。
    #[must_use]
    pub fn new(pool: DbPools) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PlayerRepo for PgPlayerRepo {
    async fn find(&self, id: PlayerId) -> Result<Option<PlayerRow>> {
        let row = sqlx::query_as::<sqlx::Postgres, PlayerRow>(
            "SELECT player_id, user_id, name, level, exp, vip, last_login, open_id, server_id, create_time, \
                    channel_id, cid, money::text, total_money::text, max_total_money::text \
             FROM player WHERE player_id = $1",
        )
        .bind(id.get())
        .fetch_optional(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("查角色: {e}")))?;
        Ok(row)
    }

    async fn save(&self, row: &PlayerRow) -> Result<()> {
        sqlx::query::<sqlx::Postgres>(
            "UPDATE player SET name = $1, level = $2, exp = $3, vip = $4, last_login = $5, \
                    money = $6::numeric, total_money = $7::numeric, max_total_money = $8::numeric \
             WHERE player_id = $9",
        )
        .bind(&row.name)
        .bind(row.level)
        .bind(row.exp)
        .bind(row.vip)
        .bind(row.last_login)
        .bind(&row.money)
        .bind(&row.total_money)
        .bind(&row.max_total_money)
        .bind(row.player_id)
        .execute(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("存角色: {e}")))?;
        Ok(())
    }

    async fn find_by_open_id(&self, open_id: &str, server_id: i32) -> Result<Option<PlayerRow>> {
        let row = sqlx::query_as::<sqlx::Postgres, PlayerRow>(
            "SELECT player_id, user_id, name, level, exp, vip, last_login, open_id, server_id, create_time, \
                    channel_id, cid, money::text, total_money::text, max_total_money::text \
             FROM player WHERE open_id = $1 AND server_id = $2",
        )
        .bind(open_id)
        .bind(server_id)
        .fetch_optional(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("按账号查角色: {e}")))?;
        Ok(row)
    }

    async fn create(&self, row: &PlayerRow) -> Result<()> {
        sqlx::query::<sqlx::Postgres>(
            "INSERT INTO player (player_id, user_id, name, level, exp, vip, last_login, open_id, server_id, \
                                create_time, channel_id, cid, money, total_money, max_total_money) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13::numeric, $14::numeric, $15::numeric)",
        )
        .bind(row.player_id)
        .bind(row.user_id)
        .bind(&row.name)
        .bind(row.level)
        .bind(row.exp)
        .bind(row.vip)
        .bind(row.last_login)
        .bind(&row.open_id)
        .bind(row.server_id)
        .bind(row.create_time)
        .bind(row.channel_id)
        .bind(&row.cid)
        .bind(&row.money)
        .bind(&row.total_money)
        .bind(&row.max_total_money)
        .execute(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("建角色: {e}")))?;
        Ok(())
    }
}

/// PostgreSQL 道具仓储。
pub struct PgItemRepo {
    pool: DbPools,
}

impl PgItemRepo {
    /// 构造。
    #[must_use]
    pub fn new(pool: DbPools) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ItemRepo for PgItemRepo {
    async fn count(&self, player: PlayerId, template_id: i32) -> Result<i64> {
        let n: Option<i64> = sqlx::query_scalar::<sqlx::Postgres, i64>(
            "SELECT num FROM item WHERE player_id = $1 AND template_id = ?",
        )
        .bind(player.get())
        .bind(template_id)
        .fetch_optional(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("查道具: {e}")))?;
        Ok(n.unwrap_or(0))
    }

    async fn add(&self, player: PlayerId, template_id: i32, delta: i64) -> Result<()> {
        // 注意：不能照搬 MySQL 的 `VALUES(num)`。
        // `item.num` 上有 CHECK (num >= 0)，PostgreSQL 会先校验待插入元组，
        // 所以负数 delta 必须在这里就夹一次；而 DO UPDATE 中要用**原始** $3
        // 参与累加（用 EXCLUDED.num 会把扣减变成 0，语义错）。
        sqlx::query::<sqlx::Postgres>(
            "INSERT INTO item (player_id, template_id, num) VALUES ($1, $2, GREATEST(0, $3)) \
             ON CONFLICT (player_id, template_id) \
             DO UPDATE SET num = GREATEST(0, item.num + $3)",
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
        sqlx::query_as::<sqlx::Postgres, ItemRow>(
            "SELECT id, player_id, template_id, num FROM item WHERE player_id = $1",
        )
        .bind(player.get())
        .fetch_all(&self.pool.game)
        .await
        .map_err(|e| ddj_core::AppError::BadRequest(format!("列道具: {e}")))
    }
}
