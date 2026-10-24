//! 分服路由
//!
//! 原系统库名硬编码在 6 个启动脚本里（`game_7010544` / `cross_7010106` /
//! `world7010002`）。这里改为规则推导 + 显式覆盖。

use ddj_core::ServerId;
use std::collections::HashMap;

/// 分服路由表。
#[derive(Debug, Default)]
pub struct ShardRouter {
    /// 显式覆盖：区服 ID → 库名。
    overrides: HashMap<i32, String>,
    /// 默认库名前缀。
    game_prefix: String,
}

impl ShardRouter {
    /// 默认规则：`game_{server_id}`。
    #[must_use]
    pub fn new(game_prefix: impl Into<String>) -> Self {
        Self {
            overrides: HashMap::new(),
            game_prefix: game_prefix.into(),
        }
    }

    /// 指定某区服的库名。
    pub fn set(&mut self, server: ServerId, db: impl Into<String>) {
        self.overrides.insert(server.get(), db.into());
    }

    /// 解析库名。
    #[must_use]
    pub fn db_for(&self, server: ServerId) -> String {
        self.overrides
            .get(&server.get())
            .cloned()
            .unwrap_or_else(|| format!("{}{}", self.game_prefix, server.get()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rule() {
        let r = ShardRouter::new("game_");
        assert_eq!(r.db_for(ServerId::new(7010544)), "game_7010544");
    }

    #[test]
    fn override_wins() {
        let mut r = ShardRouter::new("game_");
        r.set(ServerId::new(1), "custom_db");
        assert_eq!(r.db_for(ServerId::new(1)), "custom_db");
    }
}
