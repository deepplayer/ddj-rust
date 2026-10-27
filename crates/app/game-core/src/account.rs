//! 账号查询的 PostgreSQL 实现。
//!
//! 把 `ddj_repo::PlayerRepo::find_by_open_id` 适配成 `login::AccountLookup`。
//! 放在 `game-core` 而不是 `game-handler`：后者要保持零 IO，
//! 前者才有数据库连接池。

use std::sync::Arc;

use async_trait::async_trait;
use ddj_core::PlayerId;
use ddj_game_handler::login::{Account, AccountLookup};
use ddj_repo::{PgPlayerRepo, PlayerRepo};

/// PG 支撑的账号查询。
pub struct PgAccountLookup {
    repo: Arc<PgPlayerRepo>,
}

impl PgAccountLookup {
    /// 构造。
    #[must_use]
    pub fn new(repo: Arc<PgPlayerRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl AccountLookup for PgAccountLookup {
    async fn find(&self, open_id: &str, server_id: i32) -> Option<Account> {
        match self.repo.find_by_open_id(open_id, server_id).await {
            Ok(Some(row)) => Some(Account {
                player_id: PlayerId::new(row.player_id),
                create_player_time: row.create_time.timestamp_millis(),
                banned_time: 0,
                channel_id: row.channel_id,
                cid: row.cid,
            }),
            Ok(None) => None,
            Err(e) => {
                // 查库失败按「没有角色」处理：客户端会去创角，
                // 而创角第 14 步的 `repeated` 检查会把它挡住。
                tracing::error!(error = %e, open_id, "账号查询失败");
                None
            }
        }
    }
}
