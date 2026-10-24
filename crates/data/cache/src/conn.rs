//! Redis 连接
//!
//! 对应 Java：`jedis-2.9.0`（单连接 + 手动归还）。这里用 `ConnectionManager`
//! 自动重连，断线后恢复不需要重启进程。

use ddj_config::schema::RedisCfg;

/// Redis 句柄。
#[derive(Clone)]
pub struct RedisPool {
    /// 连接管理器。
    pub(crate) mgr: redis::aio::ConnectionManager,
    prefix: String,
}

impl RedisPool {
    /// 建连。
    ///
    /// # Errors
    /// 连接串非法或不可达。
    pub async fn connect(cfg: &RedisCfg) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(cfg.url.as_str())?;
        let mgr = client.get_connection_manager().await?;
        Ok(Self {
            mgr,
            prefix: cfg.key_prefix.clone(),
        })
    }

    /// 底层连接管理器（业务层拼 pipeline / 事务时用）。
    ///
    /// 克隆成本极低：`ConnectionManager` 内部是 `Arc`，多任务共享同一连接。
    pub fn mgr(&self) -> &redis::aio::ConnectionManager {
        &self.mgr
    }

    /// 拼 key 前缀，避免多服共用一个 Redis 时撞车。
    #[must_use]
    pub fn key(&self, parts: &[&str]) -> String {
        let mut s = self.prefix.clone();
        for p in parts {
            s.push(':');
            s.push_str(p);
        }
        s
    }
}

impl std::fmt::Debug for RedisPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisPool")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}
