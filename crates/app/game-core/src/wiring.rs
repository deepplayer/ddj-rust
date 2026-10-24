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
    /// 缓存。`ServiceKind::needs(Cache) == false` 的服务（如 GM）为 `None`，
    /// 不会有连不上的 Redis 拖住启动。
    pub cache: Option<Arc<RedisPool>>,
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

// ---------------------------------------------------------------------------
// 启动期注入的进程内单例
// ---------------------------------------------------------------------------
//
// `Module` 的工厂签名是 `fn() -> Box<dyn Module>`（无参），所以模块拿不到
// 配置和连接池。原系统靠 Spring 式的 XML + 反射 + 全局静态持有，这里改成显式注入：
// `Bootstrap::run()` 装配完 `Components` 后写进 `OnceLock`，模块在生命周期钩子里读。
//
// 这是**有意的进程内单例**，不是隐藏依赖：注入点在 `bootstrap.rs` 一处，
// 读取点全部在 `lifecycle_modules.rs`。

static COMPONENTS: std::sync::OnceLock<Components> = std::sync::OnceLock::new();
static SHUTDOWN: std::sync::OnceLock<tokio::sync::watch::Receiver<bool>> =
    std::sync::OnceLock::new();

/// 注入运行时组件与停机信号（只能调用一次）。
pub fn install(components: Components, shutdown: tokio::sync::watch::Receiver<bool>) {
    let _ = COMPONENTS.set(components);
    let _ = SHUTDOWN.set(shutdown);
}

/// 取运行时组件。生命周期开始前调用会返回 `None`。
#[must_use]
pub fn components() -> Option<&'static Components> {
    COMPONENTS.get()
}

/// 取停机信号接收端（克隆）。
#[must_use]
pub fn shutdown() -> Option<tokio::sync::watch::Receiver<bool>> {
    SHUTDOWN.get().cloned()
}
