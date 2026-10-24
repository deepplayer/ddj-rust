//! 模块实现：把 `ModuleKind` 的声明变成真正会干活的启动单元
//!
//! 对应 Java 的 `serverModuleListConfig.xml` + 各 `XxxModule.init/start`。
//!
//! ## 为什么工厂无参
//! `ModuleFactory = fn() -> Box<dyn Module>`，模块拿不到配置与连接池，
//! 所以启动期由 `Bootstrap::run()` 通过 [`crate::wiring::install`] 注入
//! `Components` 与停机信号。读取点只在本文件，便于审计。
//!
//! ## 阶段归属
//! 每个模块只实现自己该干的那一段，其余留空（trait 默认实现即 no-op）：
//! - `pre_init`：验连接（数据库 / 缓存已由 bootstrap 建好，这里只探活）
//! - `init`：校验已就绪的资源（模板表、注册表由 bootstrap 填充）
//! - `start`：真正开始对外服务（accept / 起定时器）
//! - `stop`：发停机信号，幂等

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ddj_core::Result;
use ddj_runtime::Module;

use crate::wiring;

/// 取注入的组件；未注入说明启动顺序错了，必须早暴露。
///
/// 这里的 `expect` 是有意的：模块在 `Bootstrap` 安装组件之前被启动属于
/// **编程错误**，静默降级只会让服务带病运行。
#[allow(clippy::expect_used, reason = "启动顺序错误必须立即中止，不能静默降级")]
fn comps() -> &'static wiring::Components {
    wiring::components().expect("模块在 Components 注入之前被启动")
}

// ---------------------------------------------------------------------------
// database
// ---------------------------------------------------------------------------

/// 数据库探活。连接池由 `Bootstrap` 建好，这里只确认可用。
struct DatabaseModule;

#[async_trait]
impl Module for DatabaseModule {
    fn name(&self) -> &'static str {
        "database"
    }

    async fn pre_init(&mut self) -> Result<()> {
        comps()
            .db
            .ping()
            .await
            .map_err(|e| ddj_core::AppError::BadRequest(format!("数据库不可用: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// cache
// ---------------------------------------------------------------------------

/// 缓存探活。不需要缓存的服务（GM）不会注册本模块。
struct CacheModule;

#[async_trait]
impl Module for CacheModule {
    fn name(&self) -> &'static str {
        "cache"
    }

    fn depends_on(&self) -> &'static [&'static str] {
        &["database"]
    }

    async fn pre_init(&mut self) -> Result<()> {
        // `cache` 为 None 时不注册本模块，故这里必有值。
        if comps().cache.is_none() {
            tracing::warn!("该服务声明了 cache 模块但没有缓存连接，跳过");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// templates
// ---------------------------------------------------------------------------

/// 配置表。加载由 `Bootstrap` 完成，这里只报告规模并做非空校验。
struct TemplatesModule;

#[async_trait]
impl Module for TemplatesModule {
    fn name(&self) -> &'static str {
        "templates"
    }

    fn depends_on(&self) -> &'static [&'static str] {
        &["database"]
    }

    async fn init(&mut self) -> Result<()> {
        let c = comps();
        let loaded = c.templates.loaded();
        if loaded == 0 && c.cfg.template.load_on_start {
            tracing::warn!(dir = %c.cfg.template.dir, "配置表为空");
        }
        tracing::info!(tables = loaded, "配置表就绪");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// registry
// ---------------------------------------------------------------------------

/// 消息注册表。处理器由 `wiring` 装好，这里只报告条数。
struct RegistryModule;

#[async_trait]
impl Module for RegistryModule {
    fn name(&self) -> &'static str {
        "registry"
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!(handlers = comps().handlers.len(), "消息注册表就绪");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// session
// ---------------------------------------------------------------------------

/// 会话表。`SessionManager` 由 `Bootstrap` 建好，这里只记录初始容量。
struct SessionModule;

#[async_trait]
impl Module for SessionModule {
    fn name(&self) -> &'static str {
        "session"
    }

    async fn start(&mut self) -> Result<()> {
        tracing::info!(online = comps().sessions.online(), "会话管理就绪");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        // 会话的关闭由 TCP 模块的停机信号驱动；这里只记录。
        tracing::info!(online = comps().sessions.online(), "会话管理停止");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// scheduler
// ---------------------------------------------------------------------------

/// 定时任务。当前只做心跳，具体任务在领域模块里注册。
struct SchedulerModule {
    task: Option<tokio::task::JoinHandle<()>>,
}

#[async_trait]
impl Module for SchedulerModule {
    fn name(&self) -> &'static str {
        "scheduler"
    }

    fn depends_on(&self) -> &'static [&'static str] {
        &["registry"]
    }

    async fn start(&mut self) -> Result<()> {
        let Some(mut shutdown) = wiring::shutdown() else {
            return Ok(());
        };
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        self.task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = shutdown.changed() => break,
                }
            }
            tracing::debug!("定时任务已停止");
        }));
        tracing::info!("定时任务已启动（1s 心跳）");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(h) = self.task.take() {
            h.abort();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// metrics
// ---------------------------------------------------------------------------

/// 指标。Prometheus 记录器在 `ddj_telemetry::init` 里装好，这里校验可渲染。
struct MetricsModule;

#[async_trait]
impl Module for MetricsModule {
    fn name(&self) -> &'static str {
        "metrics"
    }

    async fn start(&mut self) -> Result<()> {
        if comps().cfg.telemetry.metrics {
            let snapshot = ddj_telemetry::metrics::render();
            tracing::info!(bytes = snapshot.len(), "指标已就绪");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// http
// ---------------------------------------------------------------------------

/// HTTP 监听。原 `jetty 1544` / `data/web` 的 11 个 PHP 全部收敛到这里。
struct HttpListenerModule {
    task: Option<tokio::task::JoinHandle<()>>,
}

#[async_trait]
impl Module for HttpListenerModule {
    fn name(&self) -> &'static str {
        "http"
    }

    fn depends_on(&self) -> &'static [&'static str] {
        &["database"]
    }

    async fn start(&mut self) -> Result<()> {
        let c = comps();
        let addr = c.cfg.net.http_listen;
        let state = ddj_http::HttpState {
            version: env!("CARGO_PKG_VERSION"),
            server_id: c.cfg.service.server_id,
            limiter: ddj_http::RateLimiter::new(240, Duration::from_secs(60)),
        };
        let app = ddj_http::build_router(state);
        let mut shutdown = wiring::shutdown().unwrap_or_else(|| {
            let (_tx, rx) = tokio::sync::watch::channel(false);
            rx
        });

        // 绑定失败必须让启动失败，而不是悄悄跑一个没有端口的进程。
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| ddj_core::AppError::BadRequest(format!("绑定 {addr} 失败: {e}")))?;
        tracing::info!(%addr, "HTTP 监听已启动");

        self.task = Some(tokio::spawn(async move {
            let served = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown.changed().await;
                })
                .await;
            if let Err(e) = served {
                tracing::error!(error = %e, "HTTP 服务退出");
            }
        }));
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(h) = self.task.take() {
            h.abort();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// tcp
// ---------------------------------------------------------------------------

/// TCP 长连。原 `GameServer` 的 10544 等端口。
struct TcpListenerModule {
    task: Option<tokio::task::JoinHandle<()>>,
}

#[async_trait]
impl Module for TcpListenerModule {
    fn name(&self) -> &'static str {
        "tcp"
    }

    fn depends_on(&self) -> &'static [&'static str] {
        &["registry", "session"]
    }

    async fn start(&mut self) -> Result<()> {
        let c = comps();
        let server = ddj_tcp::TcpServer::new(c.cfg.net.clone(), Arc::clone(&c.sessions));
        let shutdown = wiring::shutdown().unwrap_or_else(|| {
            let (_tx, rx) = tokio::sync::watch::channel(false);
            rx
        });
        self.task = Some(tokio::spawn(async move {
            if let Err(e) = server.serve(shutdown).await {
                tracing::error!(error = %e, "TCP 监听退出");
            }
        }));
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(h) = self.task.take() {
            h.abort();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// rpc
// ---------------------------------------------------------------------------

/// 跨服 RPC。原 Hessian 的 4544 / 4106。
struct RpcListenerModule {
    task: Option<tokio::task::JoinHandle<()>>,
}

#[async_trait]
impl Module for RpcListenerModule {
    fn name(&self) -> &'static str {
        "rpc"
    }

    fn depends_on(&self) -> &'static [&'static str] {
        &["registry"]
    }

    async fn start(&mut self) -> Result<()> {
        let c = comps();
        let Some(addr) = c.cfg.net.rpc_listen else {
            tracing::info!("未配置 rpc_listen，跳过跨服端口");
            return Ok(());
        };
        let tag = match c.cfg.service.name.as_str() {
            "cross" => ddj_rpc::ServiceTag::Cross,
            "hall" => ddj_rpc::ServiceTag::Hall,
            "battle" => ddj_rpc::ServiceTag::Battle,
            "world" => ddj_rpc::ServiceTag::World,
            "platform" => ddj_rpc::ServiceTag::Platform,
            _ => ddj_rpc::ServiceTag::Game,
        };
        let server = ddj_rpc::RpcServer::new(tag);
        let shutdown = wiring::shutdown().unwrap_or_else(|| {
            let (_tx, rx) = tokio::sync::watch::channel(false);
            rx
        });
        self.task = Some(tokio::spawn(async move {
            if let Err(e) = server.serve(addr, shutdown).await {
                tracing::error!(error = %e, "跨服端口退出");
            }
        }));
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(h) = self.task.take() {
            h.abort();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 注册
// ---------------------------------------------------------------------------

ddj_runtime::submit_module!("database", || Box::new(DatabaseModule));
ddj_runtime::submit_module!("cache", || Box::new(CacheModule));
ddj_runtime::submit_module!("templates", || Box::new(TemplatesModule));
ddj_runtime::submit_module!("registry", || Box::new(RegistryModule));
ddj_runtime::submit_module!("session", || Box::new(SessionModule));
ddj_runtime::submit_module!("scheduler", || Box::new(SchedulerModule { task: None }));
ddj_runtime::submit_module!("metrics", || Box::new(MetricsModule));
ddj_runtime::submit_module!("http", || Box::new(HttpListenerModule { task: None }));
ddj_runtime::submit_module!("tcp", || Box::new(TcpListenerModule { task: None }));
ddj_runtime::submit_module!("rpc", || Box::new(RpcListenerModule { task: None }));
