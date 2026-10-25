//! 1008/1020 的本服角色查询（查 PG `player` 表）。

use std::sync::Arc;

use ddj_core::PlayerId;
use ddj_game_handler::install_view::ViewRepo;
use ddj_repo::{PgPlayerRepo, PlayerRepo};

/// `PgPlayerRepo` 的视图适配。
pub struct PgViewRepo {
    repo: Arc<PgPlayerRepo>,
}

impl PgViewRepo {
    /// 用已有仓储构造。
    #[must_use]
    pub fn new(repo: Arc<PgPlayerRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait::async_trait]
impl ViewRepo for PgViewRepo {
    async fn find(&self, id: i64) -> Option<(String, i64, i32)> {
        match self.repo.find(PlayerId::new(id)).await {
            Ok(Some(row)) => Some((row.name, i64::from(row.level), row.vip)),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(error = %e, player_id = id, "看他人：查库失败，按不存在处理");
                None
            }
        }
    }
}
