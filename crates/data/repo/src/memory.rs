//! 内存实现（测试用）

use async_trait::async_trait;
use ddj_core::{PlayerId, Result};
use std::collections::HashMap;
use std::sync::Arc;

use crate::entity::{ItemRow, MailRow, PlayerRow};
use crate::traits::{ItemRepo, MailRepo, PlayerRepo};

/// 内存角色仓储。
#[derive(Default, Clone)]
pub struct MemPlayerRepo {
    rows: Arc<parking_lot::RwLock<HashMap<i64, PlayerRow>>>,
}

#[async_trait]
impl PlayerRepo for MemPlayerRepo {
    async fn find(&self, id: PlayerId) -> Result<Option<PlayerRow>> {
        Ok(self.rows.read().get(&id.get()).cloned())
    }
    async fn save(&self, row: &PlayerRow) -> Result<()> {
        self.rows.write().insert(row.player_id, row.clone());
        Ok(())
    }
    async fn find_by_open_id(&self, open_id: &str, server_id: i32) -> Result<Option<PlayerRow>> {
        Ok(self
            .rows
            .read()
            .values()
            .find(|r| r.open_id.as_deref() == Some(open_id) && r.server_id == server_id)
            .cloned())
    }
    async fn create(&self, row: &PlayerRow) -> Result<()> {
        self.rows.write().insert(row.player_id, row.clone());
        Ok(())
    }
}

/// 内存道具仓储。
#[derive(Default, Clone)]
pub struct MemItemRepo {
    rows: Arc<parking_lot::RwLock<HashMap<(i64, i32), i64>>>,
}

#[async_trait]
impl ItemRepo for MemItemRepo {
    async fn count(&self, player: PlayerId, template_id: i32) -> Result<i64> {
        Ok(*self
            .rows
            .read()
            .get(&(player.get(), template_id))
            .unwrap_or(&0))
    }
    async fn add(&self, player: PlayerId, template_id: i32, delta: i64) -> Result<()> {
        let mut w = self.rows.write();
        let e = w.entry((player.get(), template_id)).or_insert(0);
        *e = (*e + delta).max(0);
        Ok(())
    }
    async fn list(&self, player: PlayerId) -> Result<Vec<ItemRow>> {
        Ok(self
            .rows
            .read()
            .iter()
            .filter(|((p, _), _)| *p == player.get())
            .map(|((p, t), n)| ItemRow {
                id: 0,
                player_id: *p,
                template_id: *t,
                num: *n,
            })
            .collect())
    }
}

/// 内存邮件仓储。
#[derive(Default, Clone)]
pub struct MemMailRepo {
    rows: Arc<parking_lot::RwLock<HashMap<i64, MailRow>>>,
}

#[async_trait]
impl MailRepo for MemMailRepo {
    async fn unclaimed(&self, player: PlayerId) -> Result<Vec<MailRow>> {
        Ok(self
            .rows
            .read()
            .values()
            .filter(|m| m.player_id == player.get() && m.claimed == 0)
            .cloned()
            .collect())
    }
    async fn mark_claimed(&self, mail_id: i64) -> Result<()> {
        if let Some(m) = self.rows.write().get_mut(&mail_id) {
            m.claimed = 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn item_add_and_count() {
        let repo = MemItemRepo::default();
        let p = PlayerId::new(1001);
        repo.add(p, 400001, 3).await.expect("add");
        assert_eq!(repo.count(p, 400001).await.expect("count"), 3);
        repo.add(p, 400001, -1).await.expect("sub");
        assert_eq!(repo.count(p, 400001).await.expect("count"), 2);
    }

    #[tokio::test]
    async fn item_never_negative() {
        let repo = MemItemRepo::default();
        let p = PlayerId::new(1);
        repo.add(p, 1, -100).await.expect("sub");
        assert_eq!(repo.count(p, 1).await.expect("count"), 0);
    }
}
