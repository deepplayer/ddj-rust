//! 工作单元
//!
//! 原 Java 靠 `JDBCRepository` 内部的 `Connection` 传递 + 手动 commit。
//! 这里显式建模：一个 `UnitOfWork` 就是一个事务，drop 时未提交即回滚。
//!
//! 注意：`sqlx::Transaction` 自身实现了 `Drop`，不能在 `commit(self)` 里
//! 直接搬出字段（E0509）。这里用 `Option` 包一层，`take()` 之后再提交。

/// 事务内的操作序列。
pub struct UnitOfWork<'a> {
    tx: Option<sqlx::Transaction<'a, sqlx::MySql>>,
}

impl<'a> UnitOfWork<'a> {
    /// 开始事务。
    ///
    /// # Errors
    /// 取连接失败。
    pub async fn begin(pool: &'a sqlx::MySqlPool) -> Result<Self, sqlx::Error> {
        Ok(Self {
            tx: Some(pool.begin().await?),
        })
    }

    /// 访问底层事务。
    ///
    /// 已提交后返回 `None`——调用方不应再复用同一事务。
    pub fn tx(&mut self) -> Option<&mut sqlx::Transaction<'a, sqlx::MySql>> {
        self.tx.as_mut()
    }

    /// 提交。
    ///
    /// # Errors
    /// 事务已被提交过，或提交失败。
    pub async fn commit(mut self) -> Result<(), sqlx::Error> {
        let tx = self.tx.take().ok_or(sqlx::Error::PoolClosed)?;
        tx.commit().await
    }

    /// 显式回滚。
    ///
    /// # Errors
    /// 事务已被消费，或回滚失败。
    pub async fn rollback(mut self) -> Result<(), sqlx::Error> {
        let tx = self.tx.take().ok_or(sqlx::Error::PoolClosed)?;
        tx.rollback().await
    }
}

impl Drop for UnitOfWork<'_> {
    fn drop(&mut self) {
        if self.tx.is_some() {
            tracing::debug!("UnitOfWork 未提交，sqlx 会按 Drop 语义回滚");
        }
    }
}
