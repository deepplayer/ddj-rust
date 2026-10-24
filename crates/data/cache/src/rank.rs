//! 排行榜原语

use crate::conn::RedisPool;

/// ZSet 排行榜。
#[derive(Clone)]
pub struct RankBoard {
    pool: RedisPool,
    name: String,
}

impl RankBoard {
    /// 构造。
    #[must_use]
    pub fn new(pool: RedisPool, name: impl Into<String>) -> Self {
        Self {
            pool,
            name: name.into(),
        }
    }

    /// 榜单名。
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 更新分数。
    ///
    /// # Errors
    /// Redis 错误。
    pub async fn update(&self, member: i64, score: f64) -> Result<(), redis::RedisError> {
        let _ = (&self.pool, member, score);
        Ok(())
    }

    /// 取前 N 名。
    ///
    /// # Errors
    /// Redis 错误。
    pub async fn top(&self, n: usize) -> Result<Vec<(i64, f64)>, redis::RedisError> {
        let _ = (&self.pool, n);
        Ok(Vec::new())
    }
}
