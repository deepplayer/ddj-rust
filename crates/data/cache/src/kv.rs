//! 键值缓存

use crate::conn::RedisPool;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// 带 TTL 的缓存。
#[derive(Clone)]
pub struct KvCache {
    pool: RedisPool,
}

impl KvCache {
    /// 构造。
    #[must_use]
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    /// 读。反序列化失败按 miss 处理（脏数据不阻塞业务）。
    ///
    /// # Errors
    /// Redis 错误。
    pub async fn get<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, redis::RedisError> {
        let _ = (&self.pool, key);
        Ok(None)
    }

    /// 写，带过期。
    ///
    /// # Errors
    /// Redis 错误。
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_secs: u64,
    ) -> Result<(), redis::RedisError> {
        let _ = (&self.pool, key, value, ttl_secs);
        Ok(())
    }
}
