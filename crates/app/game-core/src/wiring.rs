//! 依赖装配
//!
//! 原系统：XML 里写类名 → 反射 new → 字段注入 → 生命周期回调。
//! 这里：显式结构体，字段就是依赖，构造即装配。

use crate::modules::ModuleKind;
use ddj_cache::RedisPool;
use ddj_config::Config;
use ddj_db::DbPools;
use ddj_registry::HandlerTable;
use ddj_session::SessionManager;
use ddj_template::TemplateStore;
use std::sync::Arc;

/// 全部已装配的组件。
#[derive(Clone)]
pub struct Components {
    /// 配置。
    pub cfg: Arc<Config>,
    /// 数据库。
    pub db: Arc<DbPools>,
    /// 缓存。
    pub cache: Arc<RedisPool>,
    /// 配置表。
    pub templates: Arc<TemplateStore>,
    /// 会话。
    pub sessions: Arc<SessionManager>,
    /// 消息注册表。
    pub handlers: Arc<HandlerTable>,
}

impl std::fmt::Debug for Components {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Components")
            .field("server", &self.cfg.service.name)
            .field("db", &self.db)
            .field("handlers", &self.handlers.len())
            .field("templates", &self.templates.loaded())
            .finish()
    }
}

impl Components {
    /// 按模块清单决定是否需要某个组件。
    #[must_use]
    pub fn wants(kinds: &[ModuleKind], k: ModuleKind) -> bool {
        kinds.contains(&k)
    }
}
