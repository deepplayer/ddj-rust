//! 创角落库的 PostgreSQL 实现。
//!
//! 与原版的对应关系：
//! - `name_exists`     → `DBPlayerModule.repeated(name)`
//! - `register_count`  → `getRegisterPlayerNum()`
//! - `next_player_id`  → `PlayerServerModule` 的发号器（本实现用序列）
//! - `create`          → `PlayerServerModule.createPlayer(...)`，返回 null 表示重名

use async_trait::async_trait;
use ddj_game_handler::player::{CreatePlayerRepo, NewPlayerRow};
use sqlx::PgPool;

/// PG 支撑的创角仓储。
pub struct PgCreatePlayerRepo {
    pool: PgPool,
}

impl PgCreatePlayerRepo {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CreatePlayerRepo for PgCreatePlayerRepo {
    async fn name_exists(&self, name: &str) -> bool {
        match sqlx::query_scalar::<_, i64>("SELECT count(*) FROM player WHERE name = $1")
            .bind(name)
            .fetch_one(&self.pool)
            .await
        {
            Ok(n) => n > 0,
            Err(e) => {
                // 查不到就当作没重名：真正的兜底是插入时的唯一键。
                tracing::error!(error = %e, "重名查询失败");
                false
            }
        }
    }

    async fn register_count(&self) -> i32 {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM player")
            .fetch_one(&self.pool)
            .await
            .map_or(0, |n| i32::try_from(n).unwrap_or(i32::MAX))
    }

    async fn next_player_id(&self, server_id: i32) -> i64 {
        // 原版发号规则不在打包产物里，这里用「服号前缀 + 序列」保证唯一且可读。
        match sqlx::query_scalar::<_, i64>("SELECT nextval('player_id_seq')")
            .fetch_one(&self.pool)
            .await
        {
            Ok(n) => i64::from(server_id) * 1_000_000 + n,
            Err(e) => {
                tracing::error!(error = %e, "发号失败，退化为时间戳");
                chrono::Utc::now().timestamp_millis()
            }
        }
    }

    async fn create(&self, row: &NewPlayerRow) -> bool {
        let ok = sqlx::query(
            "INSERT INTO player \
             (player_id, user_id, name, level, exp, vip, open_id, server_id, create_time, \
              channel_id, cid, money) \
             VALUES ($1, 0, $2, 1, 0, 0, $3, $4, to_timestamp($5 / 1000.0), $6, $7, $8::numeric)",
        )
        .bind(row.player_id)
        .bind(&row.name)
        .bind(&row.open_id)
        .bind(row.server_id)
        .bind(row.now_ms)
        .bind(row.channel_id)
        .bind(&row.cid)
        // 原版 `PlayerCached.java:91` 创角时把 `ConfigDBCache.InitializeSilverNum`
        // 传进 `new Player(...)` 的 `str4`（即 `this.money`）。
        // `B-变量表-Config.csv:7` 实测值 = 20000。
        .bind("20000")
        .execute(&self.pool)
        .await;
        match ok {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(error = %e, name = %row.name, "插入角色失败");
                false
            }
        }
    }
}

/// 空的扮装表：`faceId` 全部走「默认补齐」分支（对应原版白名单补齐逻辑）。
pub struct EmptyFashion;

impl ddj_game_handler::player::FashionLookup for EmptyFashion {
    fn get(&self, _id: i32) -> ddj_core::Result<Option<ddj_game_handler::player::FashionInfo>> {
        Ok(None)
    }
}
